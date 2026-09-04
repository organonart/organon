### PBR text T15 — the scatter: motion streaks keyed to the picture's own measured velocity

`doc/pbr_text_engine.md` §15's last open row, and the only one that was new rendering work
rather than wiring: a velocity-keyed streak with an RGB split, in the post-composite FX
pass. Three parameters on the full §17 chain and the CLI vocabulary — `scatter_amount`
(0 = off), `scatter_length` (in **cell widths**), `scatter_split` — riding a new
`Shared.scatter[4]`, tail-appended after `capsule` (LAYOUT_VERSION 0x0286 → 0x0287,
`Shared` 8624 → 8640 bytes).

**The velocity is measured, not reprojected.** §8 already forbids TAA for the text path,
and the reason generalises past TAA: `temporal.rs` reconstructs velocity by reprojecting
the previous camera, which describes how the *world* moved under a moving eye — and under
T3's **held** camera the glyphs move while the eye does not, teleporting cell to cell. So
the flow is solved per pixel from the image itself, by the normal-flow relation
`dI/dt + v·∇I = 0` → `v = −(dI/dt)·∇I / |∇I|²`. That recovers only the component along the
local gradient (the aperture problem), which is exactly the component a streak wants: a
smeared edge smears across itself. Below a gradient floor there is no recoverable
direction and no streak is drawn, which is also what keeps a flat region from turning
noise into a long streak pointing anywhere.

📌 **The previous frame was already there, unread.** `fx.wgsl` has written its feedback
history as `vec4(col, 1.0)` since #152 and the trail samples `.rgb` — so the alpha lane has
been dead the whole time. It now carries last frame's un-streaked `luma(base)`. No new
attachment, no ping-pong of its own, no second pipeline, and no extra bandwidth: with the
scatter off the shader writes the same literal `1.0` it always did, so the history texture
is byte-identical and the feedback trail cannot tell either way. ⚠️ **The reference is
taken *before* the streak is mixed in.** After it, the estimate would see its own smear as
motion and grow it every frame. ⚠️ And the first frame after the scatter is switched on
(or after a resize, which clears the histories to black) has no reference at all —
`Fx::scatter_primed` holds the effect off for exactly that frame rather than letting a
difference against `1.0` streak the whole screen once.

**How §9's first law is honoured — twice, and the second is the real answer.** A streak
takes energy out of a cell by definition, which is the tension the tier opens with.
(1) The reach is expressed in **cell widths** rather than pixels: `params.rs`'s range stops
at one cell, `fx::scatter_max_px` clamps again past it, and the cell's on-screen width is
*measured* by `world::glyph_cell_px`, which projects one cell width through this frame's
own view-projection (the trick the lens-flare anchor already uses). So the bound means the
same thing at every zoom and under the orbit rig as well as the held one — a bound in
pixels would only have held at whatever distance somebody tuned at. With no ring live there
is no cell, and it falls back to a fraction of the frame's short side.
(2) **It is transient by construction.** `dI/dt` is zero where nothing changed, so on a
settled frame the streak is identically absent — and the settled frame is the one §9's
harness scores, the one T13's gate snaps, and the one T5's tracer converges to. The
legibility claim is about the picture the effect leaves behind; the scatter exists only
while the effect is still running.

The gather is convex, never additive — a weighted mean of source samples — so it moves a
cell's light without changing how much of it there is (§9 law 2, which an additive streak
would break by brightening whatever moves). The **RGB split** is per-channel tap weights
along the streak, red pulled toward the tail and blue toward the head, symmetric about the
middle so the mean travel does not move; at `scatter_split = 0` every weight is exactly 1,
so the dispersion cannot leak in at the off position and the result is a plain achromatic
directional blur.

⚠️ **`post.rs` / `post.wgsl` turned out not to be involved**, though §15's row named them:
they are the HDR bloom chain and the composite, which run *before* the tonemap on the
linear scene buffer. A streak belongs after the picture is a picture, which is `fx.wgsl`.

**Also here, closing §3 item 8:** `bottled` and `cathode` drop the `glyph_faceplate = 0.55`
that W17 (#243) made inert when the Plexus pass started colouring nodes from emission
rather than tint. ⚠️ Dropping the value is not enough on its own — the text seed is
one-shot, so the marker bumps to **`seeded_text_v4`** and every name `v3` wrote joins
`TEXT_RUNGS_SEEDED_BEFORE`, or the amendment never reaches a store that already seeded and
the miss is completely silent. ⚠️ That makes the "a name no earlier marker wrote is never
replaced" guard untestable against the const (every rung name is now on the list, so the
test would pass with the guard deleted — #133's failure exactly), so `seed_text_into` gained
`seed_text_into_given`, which takes the list as a parameter and keeps the guard
mutation-testable at every future bump.

**Tests.** Leg 7 (`cargo test -p organic-math-native --lib --features console-edition`)
**354 → 357**; `cargo test -p organon-render --lib` **81 → 85**; `cargo test -p organon-world
--lib --features world` **189 → 191**. New: the one-cell clamp and its fallbacks
(`the_streak_can_never_reach_past_one_cell`, `with_no_cell_the_cap_is_a_fraction_of_the_short_side`,
`a_zero_or_broken_length_is_no_streak`, `the_cap_is_always_finite_and_non_negative`), the
cell measurement (`a_cell_is_measured_through_the_frames_own_projection`,
`nothing_to_measure_reads_zero_not_a_guess`), the append guard
(`the_scatter_append_leaves_every_pre_append_byte_where_it_was`), invariant #4
(`the_scatter_is_inert_until_its_amount_is_raised`) and the seed bump
(`every_rung_the_current_marker_amends_may_be_replaced_in_place`). Four mutation-tested,
quoted in the PR. `fx.wgsl` validates under naga (`organon-render --test wgsl`, 50 passed).

🚨 **No GPU touched this: green and ready to try.** Two things a CPU cannot answer and a
GPU session must — whether the temporal deadband actually clears film grain at the
amplitudes a rung uses (grain is added before the history is written, so it is on both
sides of the difference), and whether ten taps read as a smear rather than as a comb at a
full one-cell reach.
