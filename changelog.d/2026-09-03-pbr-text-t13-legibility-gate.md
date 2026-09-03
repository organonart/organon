### PBR text T13 — the legibility gate on a real render: §9's number, in `verify.sh`, re-runnable by anyone

T2 (organon#217) made `doc/pbr_text_engine.md` §9's two laws a number and wired it nowhere;
this is where it lives. `native/verify.sh --legibility-only` builds the text producer and
the judge, starts `organon-glyphs` on the Omarchy logo in the harness's private namespace
(`--effect expand --seed 217 --once --dwell 600`, the input **derived from the fixture** by
`legibility-gate --emit-text` so there is no second copy of the logo to drift), drives
`native/verify/legibility/faceplate.scene` through the CLI, waits for the producer to say the
effect settled, lets the held frame accumulate, snaps it twice, and runs
`legibility-gate` over the pair against `native/verify/legibility/thresholds.toml`. Exit 0
readable · 1 a threshold not met, the term named · 2 could not measure — and the third is
kept apart from the second on purpose, because "the producer drew the wrong text" and "the
text bled" must never read as the same number. `--legibility` runs it after the standing
suite; `--effect`, `--seed`, `--converge` move the knobs. The judge is a new
`organon-render` `[[bin]]` over `organon-render/src/legibility_gate.rs`: a thresholds file
whose four keys are all required and whose unknown keys are errors, `locate` (a centred
scale sweep then an offset refinement maximising Pearson), the fixture **from the ring**,
a two-frame **spread**, and the exit code. No clap and no new dependency — eight options do
not need a parser, and `cargo tree -p organon-render` is an acceptance test.

📌 **Three decisions the brief left open, and what settled each.** *The fixture's colours
come from the wire.* §9's law 2 is "the cell's brightness tracks what TTE said that cell
was", and what TTE said is the effect's own `final_gradient`, which `omarchy-logo.txt` —
one colour, deliberately, a shape census — cannot know; estimated against a plausible
three-stop gradient, scoring the real frame against the one-colour file would sit at
about 0.92, a hair over the 0.90 line, for reasons that have nothing to do with
legibility. So `--ring <ns>` reads the settled grid, cross-checks its **shape** against the
file cell for cell (the message names the first cell that disagrees), and scores against
the ring's colours; an unsettled ring is refused outright. *`GridGeom::fit` is not quite the
held camera's case.* The rig fits the tiles' **bounds** at their centre plane and the front
faces are nearer, so even at `glyph_margin 0` the projected grid is a fraction of a percent
larger than the fit — a fifth of a cell at the edge of an 81-column logo. `locate` absorbs
it and prints the geometry it found, so a re-run can pin it with `--geom X,Y,W`; the
synthetic test proves a grid padded on both axes, which `fit` misplaces, is recovered to
within a tenth of a pixel, and that `locate` does not wander when `fit` was already right.
*Determinism is measured, not asserted.* The settled ring hands the frame to the path
tracer (T5's `pathtrace_active`, no CLI toggle and no environment variable turns it off;
`RtContext::new` gates only on `EXPERIMENTAL_RAY_QUERY`, which the 5090 has), so the held
frame accumulates and two snaps are two noise realisations of one picture. Gating the
raster instead is not reachable: every effect animates until it settles, and the settle
is what makes the trace. So the gate snaps twice, seconds apart, and `spread` — the largest
change over the three judged numbers — is judged against `max_spread`. The per-cell box
filter averages hundreds of pixels per cell, which is why the numbers agree while the
pixels do not.

⚠️ **Two limits, stated on the report rather than in a footnote.** *The frame is the
display frame, not the HDR buffer T2 asked for.* `organon snap` writes the production
texture through `snap.rs`: `Rgba16Float` → Reinhard → sRGB8. A gain above 1 (`faceplate`'s
`glyph_gain` is 3) is therefore **compressed, not clipped** — a monotone map, so the
ranking of cells survives and Pearson on a one-colour fixture barely moves, but the
gradient inside the text is squashed and `correlation_lit` reads low for it. The gate's
`frame:` line says so every run; a float snap is `snap.rs`'s to add and that is the world's
file. *It is not the whole `faceplate` rung.* The harness runs the visual with no writer,
so no preset can be recalled, and only ids on the CLI vocabulary can be set: nine of the
rung's fields are not on it — `atmos_enabled`, `bg_visible`, `fx_enabled`, `hal_amount`,
`ml_enabled`, `ml_intensity`, `ml_radius`, `ml_count`, `ml_restir` — so the gate render
has no halation, no glyph pools, and the sky behind the grid (outside the cells, so the
gate ignores it). It also departs from the rung in three places on purpose:
`glyph_cam_tilt 0` (the geometry is axis-aligned), `glyph_margin 0` (so the grid rather
than the backplane fills the frame), and the clock frozen. `faceplate.scene`'s header is
the authority for the list. Registering the nine (seven sites, per the CLI-vocab change)
or a harness leg that runs the standalone would close it.

📌 **`verify.sh` runs on Windows now, which it did not.** Git Bash reports `MINGW64_NT`,
`DISPLAY` is unset, and the script's Linux branch reached for `xvfb-run` and exited 2 —
on the one machine with the GPU this gate is for. A `WINDOWS` case on `uname -s` skips
that branch and the binary check accepts `foo.exe`. Not exercised here; named so the
first Windows run knows what changed if it fails somewhere new.

🚨 **No GPU touched this** — green and ready to try. What is verified: `cargo test -p
organon-render` runs `tests/legibility_gate.rs`, which drives the module *and the built
binary* over T2's synthetic painter and a glyph ring written to a temp file — the
thresholds file and every way it can be wrong, the producer input reproducing the
fixture's ragged widths (20, 80, 81 …, 47), the ring fixture carrying the wire's colours
and the file's shape, the unsettled-ring refusal, `locate` on padded and unpadded grids,
`spread`, and the exit codes: a clean render passes; a slight blur against
`min_correlation 0.9995` fails naming **only** `correlation 0.99… < 1.00`; a blur past a
quarter cell fails naming **only** `bleed 0.… > 0.25`; the same frame twice spreads by
exactly `0.0000`; a noisy twin against `max_spread 0.0001` fails as "the two frames do not
agree"; a missing frame, a fixture that is not one, a second frame of another size and a
bad option all exit 2. Mutation-tested, each broken on purpose and the message read: with
the exit code forced to 0 the binary test fails on the correlation case with the report
in the message (`legibility-gate: FAIL — correlation 0.9988 < 1.00`, exit 0 where 1 was
asserted); with `SGR_HAS_FG` ignored the ring test fails on the first lit cell — `left:
[225, 225, 225] right: [61, 238, 200]`, the default where the wire's colour should be;
with the offset refinement removed, `locate` on a grid pasted 5 px left and 9 px up of
centre fails with `off-centre grid found: corr 0.8233` against the 0.999 a found grid
scores. What no test can say is what a real frame scores. The coordinator's one command
is `cd native && ./verify.sh --legibility-only`; `native/verify/README.md` says what a
pass and a fail look like, and the thresholds file stays at T2's defaults until that first
number tightens it.
