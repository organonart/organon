#!/usr/bin/env python3
"""Train / export a Neural Cellular Automaton (NCA) for Organon's Field Engine
"Neural CA (learned surrogate)" source (#407 Tier B).

The runtime (native/src/math.rs :: NcaWeights / NeuralCA) rolls out a tiny NCA on a
periodic CPU grid: each cell gathers a fixed perception per channel —
[identity, sobel_x, sobel_y, laplacian] (dim 4*channels) — then a two-layer MLP
(relu hidden -> linear delta), integrates, and tanh-squashes every channel so the
rollout is provably bounded. This script exports weights in that exact schema:

    { "channels": C, "hidden": H,
      "w1": [H * (4*C)]   (row-major, perception -> hidden),
      "b1": [H],
      "w2": [C * H]       (row-major, hidden -> channel deltas),
      "b2": [C] }

Two modes:

  --demo   stdlib-only (NO torch / NO numpy). Emits the analytic built-in default
           weights (a lambda-omega reaction-diffusion oscillator), byte-for-byte the
           same rule as `NcaWeights::builtin_default()`. This always produces a valid,
           loadable JSON — used to seed the shipped gallery file `nca-default.json`.

  (real)   the default path. Trains an NCA to imitate a physics simulation from
           PolymathicAI's *The Well* (https://github.com/PolymathicAI/the_well,
           CC-BY-4.0) — e.g. `active_matter` or a Gray-Scott run — by fitting the
           per-step residual so the learned surrogate reproduces the reference
           rollout. Requires torch + h5py + the Well data; DO NOT run in CI. This is
           documented rather than executed here; the wiring below shows the shape of
           the training loop and the export.

Usage:
    python3 train.py --demo -o nca-default.json
    python3 train.py --dataset active_matter --data /path/to/the_well -o nca-well.json
"""

import argparse
import json
import sys


# --- perception layout (must match math.rs) --------------------------------------
NCA_PERCEPT = 4  # identity, sobel_x, sobel_y, laplacian


def builtin_default_weights():
    """The analytic lambda-omega oscillator — identical to
    `NcaWeights::builtin_default()` in native/src/math.rs. Two coupled oscillator
    channels (rotation w + mild growth a + diffusion D) plus two decaying auxiliary
    channels; the tanh squash in the runtime caps the growth into a limit cycle, so
    the field stays alive (never a dead fixed point) yet provably bounded.

    Construction: each channel's target delta is a LINEAR form z_c over the
    perception. A ReLU pair reproduces a linear form exactly (relu(z) - relu(-z) = z),
    so hidden unit 2c carries relu(z_c), 2c+1 carries relu(-z_c), and w2 differences
    them -> delta_c = z_c. The "learned" default therefore IS the operator, exactly.
    """
    channels = 4
    hidden = 2 * channels          # one ReLU pair per channel
    pin = NCA_PERCEPT * channels   # 16

    # lambda-omega / decay coefficients (grid-time units, dx = 1).
    d_diff = 0.20   # diffusion of the oscillator channels
    a_grow = 0.10   # linear growth (tanh caps it -> limit cycle)
    w_rot = 0.60    # rotation rate between ch0 and ch1
    d_aux = 0.10    # auxiliary-channel diffusion
    decay = 0.50    # auxiliary-channel relaxation to 0

    def pidx(j, t):
        return j * NCA_PERCEPT + t

    # per-channel linear form z_c as a coefficient row over perception
    z = [[0.0] * pin for _ in range(channels)]
    # z0 = a*id0 - w*id1 + D*lap0
    z[0][pidx(0, 0)] = a_grow
    z[0][pidx(1, 0)] = -w_rot
    z[0][pidx(0, 3)] = d_diff
    # z1 = w*id0 + a*id1 + D*lap1
    z[1][pidx(0, 0)] = w_rot
    z[1][pidx(1, 0)] = a_grow
    z[1][pidx(1, 3)] = d_diff
    # z2 = -decay*id2 + Daux*lap2
    z[2][pidx(2, 0)] = -decay
    z[2][pidx(2, 3)] = d_aux
    # z3 = -decay*id3 + Daux*lap3
    z[3][pidx(3, 0)] = -decay
    z[3][pidx(3, 3)] = d_aux

    # w1: rows [2c] = +z_c, [2c+1] = -z_c ; b1 = 0
    w1 = [0.0] * (hidden * pin)
    for c in range(channels):
        for k in range(pin):
            w1[(2 * c) * pin + k] = z[c][k]
            w1[(2 * c + 1) * pin + k] = -z[c][k]
    b1 = [0.0] * hidden
    # w2: row c = +1 at col 2c, -1 at col 2c+1 -> delta_c = z_c ; b2 = 0
    w2 = [0.0] * (channels * hidden)
    for c in range(channels):
        w2[c * hidden + 2 * c] = 1.0
        w2[c * hidden + 2 * c + 1] = -1.0
    b2 = [0.0] * channels

    return {"channels": channels, "hidden": hidden,
            "w1": w1, "b1": b1, "w2": w2, "b2": b2}


def validate(weights):
    """Mirror the runtime dimension check so a bad export fails loudly here."""
    c, h = weights["channels"], weights["hidden"]
    pin = NCA_PERCEPT * c
    ok = (c > 0 and h > 0
          and len(weights["w1"]) == h * pin
          and len(weights["b1"]) == h
          and len(weights["w2"]) == c * h
          and len(weights["b2"]) == c)
    if not ok:
        raise SystemExit("export failed dimension validation")
    return weights


def train_on_the_well(dataset, data_root, channels, hidden, steps):
    """Train an NCA to imitate a *The Well* physics rollout. Documented, not run in
    CI — needs torch + h5py + the downloaded dataset.

    The Well (PolymathicAI, CC-BY-4.0): a large collection of physics simulations
    (active matter, reaction-diffusion, turbulence, ...) stored as HDF5 trajectories.
    Fitting recipe:

      1. Load a trajectory tensor  U[t, C, H, W]  (h5py) and map C physical fields to
         the NCA's first channels (pad with learned auxiliary channels to `channels`).
      2. Build the SAME fixed perception the runtime uses (identity + Sobel x/y +
         Laplacian per channel) as fixed depthwise conv kernels.
      3. Learn (w1,b1,w2,b2) so one NCA step predicts the per-step residual
         U[t+1] - U[t], with the tanh squash in the loop; optimize the multi-step
         rollout loss (curriculum on horizon) for stability. Adam, MSE.
      4. Export via `dump()` below — no architecture change, the runtime loads it.
    """
    try:
        import torch  # noqa: F401
        import h5py   # noqa: F401
    except Exception as e:  # pragma: no cover - documented path
        raise SystemExit(
            "real training needs torch + h5py and a downloaded The Well dataset "
            f"({dataset} under {data_root}). Missing dependency: {e}\n"
            "Use --demo to emit the built-in default weights with no dependencies."
        )
    # The actual training loop is intentionally omitted from the shipped script (it
    # requires the multi-GB dataset). See the docstring for the recipe; wire your
    # trainer to return a dict in the same schema as `builtin_default_weights()`.
    raise SystemExit(
        "training loop not bundled — see train_on_the_well() docstring for the recipe; "
        "return weights in the runtime schema and call dump()."
    )


def dump(weights, out_path):
    validate(weights)
    with open(out_path, "w") as f:
        json.dump(weights, f)
    print(f"wrote {out_path}: channels={weights['channels']} hidden={weights['hidden']} "
          f"(w1={len(weights['w1'])} b1={len(weights['b1'])} "
          f"w2={len(weights['w2'])} b2={len(weights['b2'])})")


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--demo", action="store_true",
                    help="stdlib-only: emit the analytic built-in default weights")
    ap.add_argument("-o", "--out", default="nca-default.json", help="output JSON path")
    ap.add_argument("--dataset", default="active_matter",
                    help="The Well dataset name (real training)")
    ap.add_argument("--data", default=".", help="The Well data root (real training)")
    ap.add_argument("--channels", type=int, default=8)
    ap.add_argument("--hidden", type=int, default=32)
    ap.add_argument("--steps", type=int, default=20000)
    args = ap.parse_args(argv)

    if args.demo:
        dump(builtin_default_weights(), args.out)
        return 0
    weights = train_on_the_well(args.dataset, args.data, args.channels, args.hidden, args.steps)
    dump(weights, args.out)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
