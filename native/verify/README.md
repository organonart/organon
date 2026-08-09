# Frame verification — `verify.sh`

Drive Organon from the outside, capture frames, and judge them. This is the scripted
half of the deploy/verify loop: `deploy.sh` installs the plugin for a person to look
at; `verify.sh` launches the visual on its own, drives it through the `organon` CLI,
snaps frames, and diffs them against committed goldens.

```bash
cd native
./verify.sh                      # the standing suite, diffed against goldens
./verify.sh --pr                 # ALSO run verify/pr/ — this PR's own checks
./verify.sh 02-chrome            # one scene
./verify.sh --update-golden      # re-baseline: adopt this run's frames as truth
./verify.sh --strict             # a missing golden is a failure (for CI)
./verify.sh --keep-visual        # leave the visual up afterwards to poke by hand
```

## Two kinds of check, deliberately kept apart

| | `verify/scenes/` — **standing** | `verify/pr/` — **per-PR** |
|---|---|---|
| Asks | did I break something that already worked? | does the new thing actually work? |
| Lifetime | permanent, slow-growing | one PR; promoted or deleted at merge |
| Compares against | a committed golden | **another frame from the same run** |
| Works on brand-new capability | no — nothing to compare to | **yes, first time it runs** |

The second column is the one that verifies a PR, and `verify/pr/README.md` is its
protocol. The short version: `#!expect same-as` / `differs-from` compares two frames
captured seconds apart in the same run, so it needs no baseline and works immediately
on a feature that did not exist yesterday.

Artifacts land in `target/verify/`: `report.md` (a table plus every frame and diff
image inline), `summary.json` (the same verdicts, for a CI job to post), `frames/`,
`diffs/`, and `visual.log`. Exit `0` all passed · `1` a check failed · `2` the harness
could not run.

## Why it exists

The test suite says almost nothing about the frame. Three egui 0.33 defects shipped
against a 920/0 suite (#545) — a combo row under-filling by ~64pt, a mis-derived
grain, a dead Enter key — and all three were found by *looking*, one of them only
after merge. The depth-prepass `group(5)` crash (#519) was a runtime pipeline error
that offline naga validation cannot see by construction.

Everything in that class is one snapshot away from being caught, and each instance
currently costs a full cloud→Mac→cloud round trip to find. That is what this collapses.

## What it catches, and what it doesn't

| | |
|---|---|
| ✅ Pipeline / bind-group mismatch, shader runtime error, panic on startup | the visual never answers; `visual.log` has the reason |
| ✅ Black or flat frame — it launched but drew nothing | the `nonblack` check |
| ✅ Frozen clock or wedged redraw loop | the `animates` check — two snaps, zero input, must differ |
| ✅ Layout shifts, material regressions, tone-map/exposure drift | the `golden` check |
| ❌ macOS EDR / true-HDR headroom, `CAMetalLayer` behaviour | needs a real display and an eye |
| ❌ Ableton: VST3 hosting, MIDI, host transport, PLL lock | needs Live |
| ❌ "Is this the *intended* look?" | needs James |

The last row is the important one. A golden proves the render **did not change**. It
cannot tell you the render is **right** — a wrong look, once baselined, passes forever.
Goldens catch regressions; they do not replace judgment on new work.

## First run on a machine

There are no goldens committed yet, because they can only be generated on a machine
with a GPU. On the Mac:

```bash
cd native
./verify.sh --update-golden      # capture the baseline
./verify.sh                      # MUST pass — this is the determinism check
```

The second run is not optional. It is what proves each golden scene is actually
pixel-stable; if a scene still has a live clock in it somewhere, it fails here rather
than becoming a flaky check that cries wolf on every later PR. If one does fail, fix
the scene (find the animating parameter and zero it) rather than widening its
tolerance — a loose tolerance is a golden that has stopped watching.

Then commit `verify/golden/*.png`.

## Scene format

A scene is a file of `organon` commands, one per line, plus `#!` directives. Blank
lines and `#` comments are ignored; everything else is passed to `organon` with
shell-style quote parsing, so `generator "swept tubes"` arrives as one argument.

```
#!desc      One line, shown in the report.
#!checks    nonblack,golden        (default: nonblack,golden; also: animates)
#!tolerance 0.004                  max diff_frac before the golden check fails
#!settle    800                    ms to wait after the commands before snapping
#!expect    same-as <scene>        this frame must match another scene's frame
#!expect    differs-from <scene>   ...or must differ from it

generator "organic math"
surface original
material chrome
set metallic 1.0 roughness 0.1
```

A command that fails — a typo in a param id, an ambiguous selector — fails the scene
loudly. `resolve_enum` validates selectors CLI-side, so a bad name cannot silently
no-op.

**Every scene must set its own generator / surface / material.** The harness runs
`organon release` before each scene, which drops parameter holds **and the mode
selectors** — `agent.rs::release_all` clears `generator` / `surface` / `material` along
with the holds (`world.rs:9459` is the CLI's entry point). A scene that omits them
therefore does *not* inherit the previous scene's; it falls back to whatever the
visual's own snapshot carries, which is not a state any scene declared.

> 📌 This paragraph used to say the opposite — that selectors survive `release`, so an
> omitting scene inherits its predecessor. The rule it justifies was right and the
> justification was wrong, which is the more dangerous half to leave standing in the
> file scene authors read to learn the contract.

**A golden scene must freeze the clock.** Zero `rot_mod_*` (the rotation *speed* the
clock integrates), the `rot_amp_*` / `trans_amp_*` / `scale_amp` oscillators, and
`cam_path` / `cam_speed` / `cam_kick`. Scenes that are inherently in motion get
`#!checks nonblack,animates` and no golden — see `05-motion.scene`.

> ⚠️ **Freezing the clock is not the same as resetting it, and nothing here resets it.**
> `world.rs`'s frame step integrates `self.angle += rot_mod[0..2] * rot_mod[3] *
> speed_mult` (and `self.wind_phase` beside it) once per frame, gated on `s.animate`.
> Zeroing `rot_mod_*` stops the integration and leaves the accumulator where it stood.
> It is `World` state inside the visual, reachable by no param — neither `set` nor
> `organon release` touches it, and `release` briefly restores the *default* (non-zero)
> speed until the scene's own `set` lands, so every scene advances it a little.
>
> **Consequence: a frozen frame is a function of its parameters *and* of the phase the
> clock had reached when it stopped — which depends on every scene that ran before it.**
> Two frames of the identical scene, captured either side of an animating scene, are
> the same geometry at two different rotation phases. This is why an `#!expect` pair
> must be **adjacent**, and it is what makes the goldens for the dense hard-edged
> scenes (`01`, `02`) run hotter against tolerance than the soft ones (`03`, `04`):
> a sub-degree phase difference flips many edge pixels on opaque cubes and few on
> blended glass or smooth tubes.

`#!expect` is repeatable, is evaluated in a **second pass** once every frame is
captured (so it may reference a scene declared later — *evaluation* ordering is never
load-bearing), and uses the scene's own `#!tolerance` as the threshold in both
directions: `same-as` passes when `diff_frac <= tolerance`, `differs-from` when it
exceeds it. Both compare frames from **this run**, so no golden is involved.

⚠️ ***Capture* ordering is load-bearing, even though evaluation ordering is not** — for
the clock reason above. Put an `#!expect` pair next to each other in filename order and
give them a shared numeric prefix so they cannot drift apart. `06-fx-baseline.scene`,
`06-fx-inert.scene` and `07-fx-active.scene` are the worked example: together they
assert that Surface FX costs nothing at 0 *and* does something when turned up, which is
the full contract of an inert capability. The baseline exists **only** so the
comparison is local — it duplicates `01-original-standard`'s parameters on purpose.
Pointing the inert check at `01` instead is what made it fail its first real run
(2026-08-05) with the FX code entirely innocent. See `pr/README.md` for the per-PR
protocol.

## The metrics

`imgdiff` (`examples/imgdiff.rs`) reports three numbers because they fail differently:

- **`diff_frac`** — fraction of *pixels* differing by more than 2% on any channel.
  This is what the golden check gates on. It is sensitive to local layout shifts: an
  egui row that moves 64pt changes a lot of pixels while barely moving the average.
- **`mean_abs`** — mean channel difference. Sensitive to overall brightness and colour
  drift; reported for triage.
- **`max_abs`** — the single worst channel, for triage.

Each failing golden also writes `diffs/<scene>.png`, the absolute difference amplified
4×, so *where* it moved is visible at a glance in the report.

## Re-baselining

When a change is *meant* to move the look, `./verify.sh --update-golden` adopts the new
frames and the report says how far each one moved. **Say so in the PR** — a golden
update inside an unrelated diff is how a real regression gets laundered into the
baseline. If a PR updates goldens, the reason belongs in its description.

## Running it elsewhere

The harness talks to a **private IPC namespace** (`ORGANON_IPC_NS=organon-verify`), so
it is safe to run while Organon is open in Ableton — the two cannot see each other. It
also forces `ORGANON_VISUAL_DISPLAY=off` so it never grabs the projector.

On Linux it needs a real GPU and a display; with neither `DISPLAY` nor
`WAYLAND_DISPLAY` set it reaches for `xvfb-run` (the X server is software, the
rendering is still the GPU's). That path is what a GPU CI runner would use — but note
that a Linux/Vulkan result is not a Metal result: it catches the crash-and-layout
classes, and the look approximately, but the Mac stays the final gate.
