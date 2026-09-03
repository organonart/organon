### PBR text T12 — sub-cell rendering: slide on a path, cut on a teleport, behind a motion switch that defaults to today

`doc/pbr_text_engine.md` §7's rule — a character on a path **slides**, a character placed
by `set_coordinate` **cuts** — turned out, on reading, to be already true of the lowering.
`lower_grid` has gated its lerp on `SGR_ACTIVE_PATH` since T1 and pinned it in
`active_path_slides_and_a_cut_does_not` (*"a cut is drawn where it is, never between"*),
and the brief's second half — a path character at exactly `centre + sub` once `blend`
reaches 1 — was already enforced by the `blend < 1.0` gate on the origin map. So this tier
is not a fix. It is the switch §7 wanted for the taste question it left open, the pins that
prove the two smoothings do not stack, and a two-tick fixture that says all of it in one
test. `glyph_ring::LowerOptions` gains `motion: Motion`, an enum of three: **`Slide`** (the
default — today's lowering exactly: a path character at `lerp(prev_exact, exact, blend)`,
both ends `centre + sub`; a teleport, a trail and a dark tile drawn where they are),
**`Exact`** (no inter-tick interpolation at all — every tile at its exact position *this*
tick, the producer's sub-cell path the only source of smoothness, zero latency), and
**`Cells`** (cell centres, the remainder ignored, nothing sliding — the terminal's own
picture, which is the other side of the A/B §7 names). **A teleport cuts under every
variant.** Default `Slide`, and `Slide` is byte-identical to `lower_grid` (invariant #4):
the T9 pin lowers its asymmetric fixture through both entry points at three blends and
compares every instance, tint, emit and the bounds, and now also asserts the default by
value, which is the assertion that caught the default-flipped mutation when the
byte-identity comparison alone could not (both entry points go through `Default`).

📌 **An enum, not the two bools the brief offered, and the reason is what the bools could
express.** `slide_paths` × `cut_teleports` has four states: today, "nothing moves", the
first draft's smear (`cut_teleports: false` — a teleporting effect interpolated across the
grid, §7's named failure), and a fourth that means nothing. None of them is the quantised
terminal picture the A/B needs. The enum makes the smear **unrepresentable** — the same
argument the lighting layer made when it gave an agent a scene *name* and no way to say
"strobe" — and adds the variant the question actually needs. It also fits one lane. The
proposed wire is **`Shared.glyph[15]`** through `Motion::from_lane`: `0` / `1` / `2` to
the nearest integer, and **anything else is `Slide`** — a negative, a `7`, a NaN, a lane a
snapshot never wrote — so an old preset draws today's picture. Lane accounting after this:
`[0..12]` are T3's look controls on the chain, `[13]` the profile strength (#233), `[14]`
the dark tiles (T9), `[15]` this — **`glyph[16]` is full**; the next glyph lane needs a
`Shared` append and a `LAYOUT_VERSION` move.

**Checked, not reasoned: `Slide` and `Exact` are not a smoothing stack.** `Slide` is one
linear reconstruction between two exact samples; nothing filters the remainder and nothing
interpolates on top of the interpolation. Pinned: on the asymmetric fixture, `Slide` at
blend 1 with a previous grid, `Slide` at blend 1 without one, `Exact` at blend 0.5, `Exact`
at blend 0 and `Exact` at blend 1 are five byte-identical lowerings, and `Slide` at blend
0.5 differs from all of them (the fixture has a path character with a remainder — the
`assert_ne!` is what proves the switch does something). A tick that arrives late clamps
`blend` at 1 and holds at `exact`; the next tick starts a fresh `prev → cur` pair; a tile is
never smoothed twice. ⚠️ **What `Slide` costs is latency, not blur, and more of it than the
name suggests.** Read from `world.rs`'s blend clock (not seen — no GPU here): `blend` is
`(now − seen_at) × tick_hz`, where `seen_at` is the instant the *world read* the grid, not
the instant the producer published it (the ring header carries no publish time, so the
world cannot phase-lock to the producer). At blend 0 the tile is where the character *was*,
one tick behind. And when the render rate is **below** the tick rate — the producer's
default is `--fps 120` with `tick_hz = fps`, on a 60 Hz display — the world reads a fresh
grid on every frame with `since ≈ 0`, so `blend ≈ 0` on every frame and the tile is always
drawn at the *previously read* grid: `Slide` degenerates to "`Exact`, one read late", two
ticks behind, and never shows an in-between position at all. `Slide` only bridges anything
when the display outruns the producer. That is the finding nobody anticipated, and it makes
the GPU A/B sharper than "does it look smoother": under `slide` at 120/60, `Exact` should
look identical to `Slide` minus 16 ms of lag, and only at a lower `--tick-hz` (or a 240 Hz
panel) should `Slide` earn its keep. `scattered` / `unstable` must look like cuts under
both, which is the invariant, not the taste.

**Every claim mutation-tested, and two mutations survived on purpose.** Removing the
`ACTIVE_PATH` gate (teleports slide) fails four tests, the two-tick fixture with *"a
teleport (no ACTIVE_PATH) is drawn at its NEW cell at blend 0, never between: it would
smear the scatter across the grid — left: (-1.5, -1.0), right: (1.5, -1.0)"*. Removing the
motion gate on the origin map (`Exact` interpolates) fails the `Exact` pin and the `Cells`
pin; making `Cells` honour the remainder fails the `Cells` pin; flipping `#[default]` to
`Exact` fails seven, including *"lit tiles are byte-identical with the switch on"* and
*"half-way at blend 0.5: 1"*; swapping lanes 1 and 2 fails the lane test with `left: Cells`;
removing T11's trail exclusion fails both trail tests. ⚠️ **Two survived alone and died
together**: dropping the `clamp(0, 1)` on `blend` changes nothing while the `blend < 1.0`
gate skips the origin map, and dropping the gate changes nothing while the clamp makes the
lerp land at exactly 1 (bit-exact on this fixture). Removing both fails with *"blend 1.5
with prev: the path character sits at centre + sub: (-0.65000004, 1) vs (-0.8, 1)"*. So
"blend ≥ 1 is exact" has two guards, either sufficient; a future edit that removes one will
see green, which is fine, and one that removes both will not, which is the point.

🚨 **A merge hazard for whoever lands the `glyph[14]` wire beside this.** A new field on
`LowerOptions` means a bare struct literal — `LowerOptions { dark_tiles: s.glyph[14] > 0.5
}` — **stops compiling** the moment both branches are in `main`, and each branch is green
on its own. The wire must be written `LowerOptions { dark_tiles: …, ..Default::default() }`
(or name `motion: Motion::from_lane(s.glyph[15])` outright, which is the whole wire in one
line). The tests' own `DARK` constant hit exactly this and now names both fields. Green and
ready to try; no frame has been rendered under `Exact` or `Cells`. The world still calls
`lower_grid`; nothing on screen changes until the lane is wired, and with it wired and at
`0` nothing changes either.
