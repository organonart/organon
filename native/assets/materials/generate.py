#!/usr/bin/env python3
"""#472 Tier 1 — sample material generator (pure stdlib, no deps).

Writes procedural PBR material folders that the renderer's "Load Material…" folder
picker can consume: each folder holds `albedo/normal/roughness/metallic/ao/height`
PNGs (8-bit). The maps are *derived from one height field* (the #472 correlation
principle — crevices are dark, rough, occluded), so a hand-made set already reads
like a real material before the Tier-2 noise composer exists.

This mirrors the #226 network gallery's `generate.py`. These are **Tier-1 test
assets** — the deploy.sh install + "Load Material" gallery dialog are #472 Tier 4;
for now, load a folder directly from this directory in the editor.

Run:  python3 native/assets/materials/generate.py
"""

import math
import os
import struct
import zlib

SIZE = 256
HERE = os.path.dirname(os.path.abspath(__file__))


def write_png(path, size, pixel_fn):
    """Write an 8-bit RGBA PNG. `pixel_fn(x, y) -> (r, g, b)` in 0..255."""
    raw = bytearray()
    for y in range(size):
        raw.append(0)  # filter type 0 (None) per scanline
        for x in range(size):
            r, g, b = pixel_fn(x, y)
            raw += bytes((r & 255, g & 255, b & 255, 255))

    def chunk(typ, data):
        return (
            struct.pack(">I", len(data))
            + typ
            + data
            + struct.pack(">I", zlib.crc32(typ + data) & 0xFFFFFFFF)
        )

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)  # 8-bit, colour type 6 (RGBA)
    idat = zlib.compress(bytes(raw), 9)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", ihdr))
        f.write(chunk(b"IDAT", idat))
        f.write(chunk(b"IEND", b""))


def clampb(v):
    return max(0, min(255, int(round(v))))


def build_height(size, height_fn):
    """Sample a height field into a 2-D list of floats in 0..1 (tileable if `height_fn` is)."""
    return [[height_fn(x, y) for x in range(size)] for y in range(size)]


def emit_material(name, height_fn, albedo_fn, metallic=0.0):
    """Derive the full correlated map set from one height field + an albedo tint."""
    folder = os.path.join(HERE, name)
    os.makedirs(folder, exist_ok=True)
    h = build_height(SIZE, height_fn)

    def hg(x, y):
        return h[y % SIZE][x % SIZE]

    # Height (grayscale).
    write_png(os.path.join(folder, "height.png"), SIZE,
              lambda x, y: (clampb(hg(x, y) * 255),) * 3)

    # Normal ← Sobel gradient of height (tangent-space, OpenGL green-up).
    strength = 2.5
    def normal_px(x, y):
        gx = (hg(x + 1, y) - hg(x - 1, y)) * strength
        gy = (hg(x, y + 1) - hg(x, y - 1)) * strength
        nx, ny, nz = -gx, -gy, 1.0
        inv = 1.0 / math.sqrt(nx * nx + ny * ny + nz * nz)
        nx, ny, nz = nx * inv, ny * inv, nz * inv
        return (clampb((nx * 0.5 + 0.5) * 255),
                clampb((ny * 0.5 + 0.5) * 255),
                clampb((nz * 0.5 + 0.5) * 255))
    write_png(os.path.join(folder, "normal.png"), SIZE, normal_px)

    # Roughness ← rougher in the crevices (low height), smoother on the peaks.
    write_png(os.path.join(folder, "roughness.png"), SIZE,
              lambda x, y: (clampb((0.85 - 0.5 * hg(x, y)) * 255),) * 3)

    # AO ← cheap cavity: darker where the local height sits below its neighbourhood.
    def ao_px(x, y):
        c = hg(x, y)
        nb = (hg(x + 2, y) + hg(x - 2, y) + hg(x, y + 2) + hg(x, y - 2)) * 0.25
        occ = max(0.0, nb - c)
        return (clampb((1.0 - 0.8 * occ) * 255),) * 3
    write_png(os.path.join(folder, "ao.png"), SIZE, ao_px)

    # Metallic (flat).
    write_png(os.path.join(folder, "metallic.png"), SIZE,
              lambda x, y: (clampb(metallic * 255),) * 3)

    # Albedo (tinted, darkened in the crevices so it agrees with the height).
    write_png(os.path.join(folder, "albedo.png"), SIZE,
              lambda x, y: albedo_fn(x, y, hg(x, y)))

    with open(os.path.join(folder, "material.json"), "w") as f:
        f.write('{\n  "name": "%s",\n  "maps": ["albedo","normal","roughness","metallic","ao","height"],\n  "tiling": true\n}\n' % name)
    print("wrote", folder)


def brick_height(x, y):
    """Running-bond brick: raised bricks, recessed mortar lines (tileable)."""
    bw, bh, mortar = 64, 32, 5
    row = (y // bh)
    xx = (x + (bw // 2 if row % 2 else 0)) % bw
    yy = y % bh
    in_mortar = xx < mortar or yy < mortar
    if in_mortar:
        return 0.15
    # Slight per-brick height variation + a soft bevel toward the mortar.
    edge = min(xx - mortar, bw - 1 - xx, yy - mortar, bh - 1 - yy)
    bevel = min(1.0, edge / 8.0)
    jitter = 0.08 * math.sin((x // bw) * 12.9898 + (y // bh) * 78.233)
    return 0.55 + 0.35 * bevel + jitter


def brick_albedo(x, y, h):
    if h < 0.2:  # mortar
        return (150, 148, 143)
    base = (150, 66, 48)
    tint = 0.6 + 0.4 * h
    jitter = 1.0 + 0.10 * math.sin((x // 64) * 3.1 + (y // 32) * 5.7)
    return (clampb(base[0] * tint * jitter),
            clampb(base[1] * tint * jitter),
            clampb(base[2] * tint * jitter))


def main():
    emit_material("brick", brick_height, brick_albedo, metallic=0.0)


if __name__ == "__main__":
    main()
