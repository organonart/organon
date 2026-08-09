#!/usr/bin/env python3
"""Bake a Field Playback clip (`.bin`) for Organon's Field Engine (#407 Tier A).

A `.bin` clip is a small, downsampled, pre-baked *visual* animation of a physical
field. The renderer (`native/src/math.rs::FieldClip`) replays it through the same
glyph lattice as the live PDE sim, stepped off the PLL beat clock — so the clip is
physics-agnostic: this tool has already mapped whatever physical field it chose
into two display channels per cell:

    c0 = density  (>= 0)      -> glyph length / brightness
    c1 = hue      (0..1 turns) -> glyph colour (when the clip is flagged "complex")

Two paths:

  --demo   Pure-Python (stdlib `struct` + `math` only — NO numpy / h5py). Generates
           a synthetic swirling/advecting field so the gallery is never empty and
           the tool runs anywhere. This is what ships in the repo.

  (real)   Convert a run from PolymathicAI's *The Well* (HDF5). Requires h5py+numpy.
           Point --input at a `.hdf5`/`.h5` file and pick a field with --field. The
           chosen physical field's magnitude -> c0, and a hue is derived from its
           sign / a second component. See README.md for The Well attribution + how
           to download runs. (Not exercised in CI — the demo path is.)

Format (little-endian): a 7-word u32 header —
    magic("OFLC"=0x4F464C43), version(=1), w, h, nframes, channels(=2), complex(0/1)
— then nframes * w * h * 2 float32 (interleaved c0,c1 per cell, row-major y*w+x).

Usage:
    python3 generate.py --demo -o welldemo-swirl.bin
    python3 generate.py --input run.hdf5 --field velocity -o myclip.bin
"""

import argparse
import math
import struct
import sys

MAGIC = 0x4F464C43  # "OFLC"
VERSION = 1
CHANNELS = 2

# Must match the caps in `math.rs::FieldClip::from_bytes`.
MAX_DIM = 512
MAX_FRAMES = 4096
MAX_CELLS = 16_000_000


def write_clip(path, w, h, frames, complex_flag):
    """Serialize frames (each a flat list of w*h*2 float32) to the .bin format."""
    nframes = len(frames)
    assert 1 <= w <= MAX_DIM and 1 <= h <= MAX_DIM, "w/h out of range"
    assert 1 <= nframes <= MAX_FRAMES, "nframes out of range"
    assert w * h * nframes <= MAX_CELLS, "clip too large (cells*frames cap)"
    with open(path, "wb") as f:
        f.write(struct.pack("<7I", MAGIC, VERSION, w, h, nframes, CHANNELS,
                            1 if complex_flag else 0))
        for frame in frames:
            assert len(frame) == w * h * CHANNELS, "frame size mismatch"
            f.write(struct.pack("<%df" % len(frame), *frame))
    total = 28 + nframes * w * h * CHANNELS * 4
    print("wrote %s  (%dx%d, %d frames, complex=%s, %d bytes)"
          % (path, w, h, nframes, bool(complex_flag), total))


def make_demo(w=96, h=96, nframes=120):
    """A synthetic swirling / advecting field: two counter-rotating vortices drag a
    couple of Gaussian density blobs around the domain. Density -> c0; the local
    flow angle -> c1 hue. Deterministic (no RNG), pure stdlib."""
    frames = []
    cx, cy = (w - 1) * 0.5, (h - 1) * 0.5
    # Two vortex centres, orbiting slowly so the whole field breathes.
    for fi in range(nframes):
        t = fi / nframes
        ang = 2.0 * math.pi * t
        # Vortex centres (rotate around the middle).
        v1 = (cx + 0.28 * w * math.cos(ang), cy + 0.28 * h * math.sin(ang))
        v2 = (cx - 0.28 * w * math.cos(ang), cy - 0.28 * h * math.sin(ang))
        # Advected density blobs follow near the vortices, phase-shifted.
        b1 = (cx + 0.22 * w * math.cos(ang + 1.7), cy + 0.22 * h * math.sin(ang + 1.7))
        b2 = (cx + 0.30 * w * math.cos(-ang + 0.4), cy + 0.30 * h * math.sin(-ang + 0.4))
        sig = w * 0.11
        frame = [0.0] * (w * h * 2)
        for y in range(h):
            for x in range(w):
                # Density: sum of two moving Gaussian blobs + a faint swirl ripple.
                d = 0.0
                for (bx, by) in (b1, b2):
                    r2 = (x - bx) ** 2 + (y - by) ** 2
                    d += math.exp(-r2 / (2.0 * sig * sig))
                # Rotational flow angle from the two vortices (for the hue channel).
                vx = vy = 0.0
                for (px, py, s) in ((v1[0], v1[1], 1.0), (v2[0], v2[1], -1.0)):
                    dx, dy = x - px, y - py
                    r2 = dx * dx + dy * dy + 4.0
                    # Tangential (curl) velocity ~ perpendicular / r.
                    vx += s * (-dy) / r2
                    vy += s * (dx) / r2
                ripple = 0.15 * (0.5 + 0.5 * math.sin(0.35 * x + 0.35 * y - 3.0 * ang))
                d = d + ripple * d
                hue = (math.atan2(vy, vx) / (2.0 * math.pi) + 0.5)
                hue = (hue + 0.15 * t) % 1.0  # slow global hue drift
                i = (y * w + x) * 2
                frame[i] = d
                frame[i + 1] = hue
        frames.append(frame)
    return w, h, frames, True


def make_from_well(input_path, field, w_out, h_out, max_frames):
    """Convert a run from *The Well* (HDF5) into a clip. Requires numpy + h5py.

    The Well stores physics fields on a grid over time. We pick one field, reduce
    it to a scalar magnitude per cell (c0) and a hue (c1) from its orientation /
    sign, downsample to (w_out, h_out), and normalize per-frame. The exact dataset
    keys vary by run; adjust `field` / the key search below to match your file."""
    try:
        import numpy as np
        import h5py
    except ImportError:
        sys.exit("The real (Well) path needs numpy + h5py: pip install numpy h5py "
                 "(the --demo path needs neither). See README.md.")

    with h5py.File(input_path, "r") as f:
        # The Well groups vary; find a dataset whose name contains `field`. This is a
        # best-effort locator — inspect your file (`h5ls -r run.hdf5`) and hard-code
        # the key if needed.
        candidates = []
        f.visititems(lambda name, obj: candidates.append(name)
                     if isinstance(obj, h5py.Dataset) else None)
        key = next((c for c in candidates if field.lower() in c.lower()), None)
        if key is None:
            sys.exit("no dataset matching %r; datasets present:\n  %s"
                     % (field, "\n  ".join(candidates)))
        data = np.asarray(f[key])  # expect [..., T, H, W] or [T, H, W, C]

    # Coerce to [T, H, W] scalar magnitude + [T, H, W] hue.
    arr = np.asarray(data, dtype=np.float64)
    while arr.ndim > 4:
        arr = arr[0]
    if arr.ndim == 4:
        # Assume trailing axis is components -> magnitude + angle of first two.
        mag = np.sqrt(np.sum(arr * arr, axis=-1))
        c0comp = arr[..., 0]
        c1comp = arr[..., 1] if arr.shape[-1] > 1 else arr[..., 0]
        hue = (np.arctan2(c1comp, c0comp) / (2.0 * math.pi) + 0.5)
    elif arr.ndim == 3:
        mag = np.abs(arr)
        hue = (np.sign(arr) * 0.5 + 0.5) * 0.6  # sign -> two-tone hue
    else:
        sys.exit("unexpected data rank %d for %r" % (arr.ndim, key))

    T = min(mag.shape[0], max_frames)
    frames = []
    for ti in range(T):
        m = _resample(mag[ti], w_out, h_out)
        hh = _resample(hue[ti], w_out, h_out)
        mmax = float(m.max()) or 1.0
        frame = [0.0] * (w_out * h_out * 2)
        for y in range(h_out):
            for x in range(w_out):
                i = (y * w_out + x) * 2
                frame[i] = max(0.0, float(m[y, x]) / mmax)
                frame[i + 1] = float(hh[y, x]) % 1.0
        frames.append(frame)
    return w_out, h_out, frames, True


def _resample(a, w_out, h_out):
    """Nearest-neighbour downsample of a 2-D numpy array to (h_out, w_out)."""
    import numpy as np
    a = np.asarray(a)
    ys = (np.linspace(0, a.shape[0] - 1, h_out)).astype(int)
    xs = (np.linspace(0, a.shape[1] - 1, w_out)).astype(int)
    return a[np.ix_(ys, xs)]


def main():
    ap = argparse.ArgumentParser(description="Bake a Field Playback (.bin) clip.")
    ap.add_argument("-o", "--output", required=True, help="output .bin path")
    ap.add_argument("--demo", action="store_true",
                    help="generate a synthetic swirl clip (pure stdlib, no deps)")
    ap.add_argument("--input", help="input HDF5 run from The Well (real path)")
    ap.add_argument("--field", default="velocity",
                    help="dataset name substring to visualize (real path)")
    ap.add_argument("--width", type=int, default=96, help="output grid width")
    ap.add_argument("--height", type=int, default=96, help="output grid height")
    ap.add_argument("--frames", type=int, default=120, help="max frames")
    args = ap.parse_args()

    if args.demo:
        w, h, frames, cx = make_demo(args.width, args.height, args.frames)
    elif args.input:
        w, h, frames, cx = make_from_well(args.input, args.field,
                                          args.width, args.height, args.frames)
    else:
        ap.error("pass --demo or --input <run.hdf5>")

    write_clip(args.output, w, h, frames, cx)


if __name__ == "__main__":
    main()
