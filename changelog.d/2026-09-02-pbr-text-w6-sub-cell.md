### PBR text W6 — sub-cell motion: ttfx carries the pre-rounded path point, and the ring cell carries the remainder

`doc/pbr_text_engine.md` §7 named this "the highest-risk unknown": ttfx's `Coord` is
`{column: i64, row: i64}`, `path_step` computes a float position and rounds it (banker's, to
match Python) before it becomes `motion.current_coord`, and in a terminal that is invisible
because the cell is the atom. Rendered as tiles under a camera it reads as stepping, and
interpolating cell-to-cell on the Organon side is wrong for every effect that teleports. The
fix was always going to be upstream, and it is: **organonart/ttfx#1** adds
`Motion.current_pos: (f64, f64)` — the same point before rounding — kept in step with
`current_coord` at every write (`path_step` returns the float pair, `motion_move` rounds it
through the new `Motion::set_position`, `set_coordinate` sets it to the integer coordinate's
exact value, and the two direct writes in `matrix.rs` plus the `SetCoordinate` event action go
through `set_coordinate`), with `Motion::sub_cell()` as the remainder. 🚨 **The rendered output
is unchanged** — same banker's rounding at the same moment, so `round_half_even(current_pos)
== current_coord` holds by construction — which is ttfx's whole promise and what makes it an
upstream PR rather than a fork. Checked two ways rather than asserted: ttfx's Python-traced
engine golden (`engine_traces_match_python`, which logs `current_coord` every tick of every
motion scenario) passes and **failed with 317 mismatches** when the new rounding site was
deliberately given swapped axes; and every case in `tools/parity/cases.txt` at both suite seeds
was dumped with `--parity-dump` from a binary built at the previous commit and one built at
this one, then compared by SHA-256 and exit code: **354 of 354 identical** (177 cases × 2 seeds — the
same 354 ttfx's README counts for the suite; one case, `laseretch-group-quirk`, is a 48-byte
dump on both sides and proves nothing about motion). ⚠️ The parity suite *proper* is
Linux/glibc-pinned and needs a Rust toolchain inside WSL, which this machine does not have; the
differential run is the stronger claim for this change anyway, since it measures "unchanged"
directly rather than through Python.

**On the Organon side** the producer (`organon-glyphs`, ttfx bumped to that commit) fills the
ring's reserved `sub_x`/`sub_y` with `current_pos − current_coord`: cells, each axis in
`-0.5..=0.5`, stored as the `f32` pair it is (no quantisation — the fields were already `f32`),
`f64→f32` the only loss. ⚠️ **No flip, unlike the row index.** ttfx's row grows up and the
ring's `sub_y` is "+y up from the cell's centre", so the remainder is carried as it is; only
the cell *index* is flipped top-down. A character placed by `set_coordinate` has no remainder
and encodes as exactly `(0.0, 0.0)`, so a cut is still a cut. **No `layout_version` move**:
reserved-to-defined is additive — the bytes were in the cell and were zero, and a reader that
ignores them sees exactly what it saw.

⚠️ **The consumer had to change too, and the brief said it would not.** T1's `lower_grid`
already added `sub_x`/`sub_y` — *after* lerping the previous and current cell **centres** —
so the moment a producer filled them, a character at 0.3 cells/tick would have jumped *back*
toward the cell boundary at the start of every tick and then slid forward past where it was:
`lerp(centre_prev, centre_cur, blend) + sub_cur` is not a position anything ever occupied.
`lower_grid` now slides between the two **exact** positions (`centre + sub` on both ends);
`lerp(a, a, t)` is `a`, so the "did it move" gate went with it, and a producer writing zeros
lowers byte-identically to before. `world.rs` is untouched — T3 owns it — this is the pure
lowering in `organon-core`, where the field was already being read.

Every invariant here was mutation-tested, not just asserted: swapping the consumer's axes fails
`the_sub_cell_offset_places_the_tile_and_the_slide_runs_between_exact_positions` with *"sub_x
moves the tile RIGHT by a quarter cell: [0, 0, 0.09] -> [-0.5, 0.5, 0.09]"*; swapping the
producer helper's axes fails the real-engine remainder test with the pair reversed
(`(0.259, 0.481)` against `(0.481, 0.259)` — the expected pair is computed inline from the
motion, never through the helper, which is what makes the swap visible); and the ring
round-trip pins the pair exact and a whole-cell position zero. ⚠️ A fixture trap worth one
line: the first draft moved the *first* character, which after one tick rounded onto the
*second* character's cell and lost the painter's `(layer, character_id)` contest — it vanished
from the walk and read as "the producer does not emit it". Move the last character; it wins
any cell it shares. Green and ready to try; no frame has been rendered with a non-zero
remainder, and "smoother" is reasoned from the arithmetic.
