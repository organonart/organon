### PBR text — the ORGANON gate produces a number, and a harness that measured nothing says so

`verify.sh --legibility-only --text native/assets/text/organon.txt` shipped in #246 and had
**never once produced a number.** It aborted every time with *"the ring's text is not the
fixture's: 220 cell(s) differ, first at (2,1) — fixture `█`, ring ` `"*, and — the worse half
— the run then exited **1**, the code `verify.sh`'s own header reserves for *a check failed*,
which tells whoever reads it to go and look at numbers nobody took. It now scores:
**`corr 0.9230 ok · lit-only 0.912 · bleed 0.334 at (1,0) FAIL · stray 0.1317 FAIL · spread
0.0009 ok`**, exit 1, on organon-one at 1100×760 with `--effect expand --seed 217`. A second
whole run gave `corr 0.9231 · bleed 0.334 · stray 0.1317`, so the numbers reproduce across
runs and not merely across the two snaps inside one. ⚠️ **Nothing was tuned toward them** —
`verify/legibility/thresholds.toml` is untouched and the threshold question is open.

📌 **The fixture described a grid the producer cannot publish, and the abort was the honest
answer.** Measured rather than reasoned: the gate's ring cross-check runs *before* it loads
the frame, so `organon-glyphs` plus `legibility-gate <a path that does not exist>.png
<fixture> --ring <ns>` settles the question in seconds with no GPU at all. Against the real
producer at `--cols 82 --rows 9`, a candidate fixture with two blank rows **above** the word
passed the cross-check; two **below** failed at 272 cells; the shipped blank-above-and-below
failed at 220 — the reported abort. Three ttfx behaviours compose to that: trailing spaces
are stripped from every input line, so an all-space row arrives as an *empty* line; trailing
empty lines are then dropped outright; and the default `sw` text anchor resolves to
`row_delta = bottom - 1`, i.e. zero, which leaves the glyph block on the canvas floor. **So
every row of slack between the text and `--rows` surfaces at the TOP of the published grid
and none at the bottom** — and a fixture padded above *and* below is unpublishable for **any**
`--rows`, not merely for nine. `native/assets/text/organon.txt` and
`organon-render/tests/fixtures/organon.txt` are now the seven glyph rows, 82×7, 240 lit, with
nothing inert in either; `no_fixture_carries_a_blank_bottom_row` walks the whole fixtures
directory rather than a hand-kept list, so what is closed is the class and not the instance.

⚠️ **Nothing to fix in `organon-glyphs`** — it hands the text to ttfx, and `terminal_config`
takes ttfx's own defaults deliberately rather than keeping a second copy of them. ⚠️ And
**`Anchor::C` is not the tidy alternative someone will reach for** (derived, not measured —
the anchor has no runtime override): canvas top 9 gives `center_row` 5, the block's
`input_height` 7 gives `floor_div` 3, `row_delta` 2, and the glyphs move to canvas rows 3..9
— two blank rows at the **bottom** and none at the top, the mirror image of the bug.
Symmetric padding needs `row_delta` 1 and no anchor produces it. The rig's `glyph_margin` is
the knob for breathing room; padding rows cannot be one.

⚠️ **The demo changes, slightly.** Measured: the nine-line asset published an **82×8** grid,
so the trailing blank row never reached the screen at all while the leading one did — and
since `glyph_dark_tiles` is on in `faceplate`, it reached it as a rendered row of dark glass
tiles rather than as invisible padding. The word ORGANON is byte-identical; the slab is seven
rows rather than eight and the held camera refits to it, so the word sits slightly larger in
frame and the slab stops being asymmetric about it for no reason.

🚨 **A harness that cannot tell "the look leaks" from "I never scored anything" is worse than
none.** `record` set one `FAILED` flag for every non-`ok` verdict and the script ended `exit
"$FAILED"`. Three verdicts now — `ok`, `FAIL` (measured, a threshold missed) and `UNMEASURED`
(no judgement reached) — with `exit_code_for` as the whole decision: **2 outranks 1**, because
a run with a hole in it cannot honestly be summarised as "a check failed". ⚠️ The line
between the last two is *whether a number was taken*, not how bad it is: `frame is black` is
a FAIL, because the frame was scored; `snap failed` is UNMEASURED, because there was no
frame. ⚠️ A second hole beside it, the same defect at full strength: `FAILED` started at 0
and nothing required a check to have been recorded, so **a run that recorded nothing printed
"All checks passed" and exited 0.** An empty report is now 2 as well, and `summary.json`
carries `measured` and `checks_recorded` beside `passed`. `./verify.sh --self-test`
(`verify/selftest.sh`) sources the script in a define-only mode and pins every case in
milliseconds with no GPU — the one part of `verify.sh` that could always have been tested and
never was. Proven both ways on the real thing: the ORGANON gate returns **1** with numbers, a
missing fixture returns **2** under *"COULD NOT MEASURE — at least one check reached no
judgement"*.

📌 **The 17 `no live Organon snapshot detected` warnings are noise — and the harness now
holds a receipt instead of an argument.** Driving `faceplate.scene` printed one per command,
and ⚠️ **that warning cannot be right or wrong in this harness**: it fires on `is_live()`,
which polls the `Shared` mmap's seqlock for *motion*, and the harness deliberately runs no
`Shared` writer at all — the same structural fact that already makes `organon status`
impossible here. It would print the same 17 lines if every op were being dropped. The ops
travel a different road (`organon` is never an IPC writer; it appends `CliOp` lines to a
sidecar the visual drains per frame off a cursor seeded at *its* construction, and the visual
is up and has answered a snap before the look is driven) — but that is an argument, not
evidence. The evidence is `<ns>-agent-apply.txt`, where the visual appends one `set <id>
<value>` line per op it actually actuates and an id off the vocabulary produces nothing: the
harness clears it, drives the look, and **refuses to measure unless every id comes back**.
**Measured: all 37 ids of the scene's 14 `set` lines came back, on both runs.** ⚠️ The path
is derived from the producer's own `ring at …` log line rather than rebuilt from `$TMPDIR` in
bash, because `ipc::ns_file` is the one place IPC filenames are composed and a second copy of
that rule would drift silently — the failure being a file that is simply never found.

⚠️ **Unexplained, and the thing to look at next: the plate is not a dark room.** The frame
carries a light blue-grey field behind the word and a faint ghost of the text above it, and
`stray` is exactly the number a bright field inflates. It is **not** the atmosphere sky —
`atmos_enabled 1` and `atmos_enabled 0`, snapped back to back over a held producer, differ by
`mean_abs 0.0018` / `diff_frac 0.0012`, which is nothing. It is not `env_intensity` and not
the dark tiles (`glyph_dark_tiles 0` moves 2.6% of pixels). Most likely the backplane;
not confirmed, and not this tier's to change. ⚠️ Two probes on the way were **confounded**,
recorded so nobody repeats them: `--keep-visual` keeps the visual but always kills the
producer, so the ring expires and the world drops the grid — giving a black frame with a
diagonal of cubes that reads exactly like *"the toggle did that"*; and comparing every snap
against the first is worthless while the tracer is still accumulating, since the snap that
*restores* the control's setting differs from the control by `mean_abs 0.095`. Adjacent pairs
are the only honest comparison in a converging render.
