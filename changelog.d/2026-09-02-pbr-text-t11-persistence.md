### PBR text T11 — phosphor persistence: the producer decays a cell in linear light, and the ring carries the trail as a flagged cell

`doc/pbr_text_engine.md` §15 measured the gap between the plates and the first render, and
one row of it was not in the design at all: the spec-sheet plate labels a *fading* tile
"PHOSPHOR PERSISTENCE", and nothing in the pipeline could fade. A CRT phosphor keeps emitting
after the beam leaves it; a terminal cell is lit or it is not, and `ttfx` (faithfully) has no
notion of the difference. That cut is a good part of why the first look read as a spreadsheet.
So persistence lives **in the producer**, as a pass between the walk and the ring:
`organon-glyphs --persist-ms <τ>` keeps one phosphor per cell (`src/persist.rs`), and the walk
itself — the engine's truth — never learns it exists. **Default 0 = off**, and off is not a
decay of zero but a return before a byte is touched or a phosphor allocated: invariant #4,
pinned by a test that runs `rain` to settle and through sixteen dwell heartbeats and compares
every published frame to the raw walk, and by a differential of the pre- and post-change
binaries under `--once --no-pace` on the same seed (byte-identical ring files).

**In linear light, always.** The ring's colour contract does not move — a cell carries sRGB8
and the world decodes it — so the phosphor's residual is kept linear, decays there
(`glow *= e^(−dt/τ)` per channel), and comes back through a new `linear_to_srgb8` that is
pinned to invert `srgb8_to_linear` exactly on all 256 codes. The two ways to get this wrong are
both tested: decaying the encoded byte puts a full-white trail at 93 after one τ where linear
puts it at 163, and *skipping the decode* makes a mid-grey trail come out **brighter than its
source** (128 → 188) — §4's classic gamma bug, arriving from the encode side. Mutating the
decode away fails with *"a mid-grey trail brightened to 188 > 128: the source byte was treated
as linear and re-encoded — §4's gamma bug"*.

**The rule, where the brief left it open: excitation is instant, decay is slow, and a phosphor
cannot be un-lit by a new colour.** A lit cell publishes `max(source, residual)` **per channel**.
A steadily lit cell is therefore exactly its source — `max(s, s·k) = s`, and it publishes the
byte it arrived as, never a round-tripped copy; a cell that goes bright→dim shows the bright
residual fading *into* the dim source; a cell that changes hue at equal brightness keeps the old
hue's residual under the new. That is what a phosphor does (its emission is additive) without
the runaway a literal sum would have — a constant source re-excited every tick would converge to
`s/(1−k)` and blow through white. The alternative, "the source replaces the residual", cuts
every bright→dim transition, and a mutation to it failed the lit-over-trail test with *"the
residual outshines a dim source: [40, 0, 0]"*. A cell whose source has gone dark publishes the
**last lit cell** — its symbol (the tile shape is what fades), `bg`, attributes, `layer`,
`character_id` and sub-cell offset — with `fg` decayed and **`SGR_PERSIST`** (bit 11) set and
`ACTIVE_PATH` cleared, so the renderer can tell a trail from a lit cell (T9 may draw one without
a faceplate highlight). Below a floor of linear `1e-3` (≈ 3/255 encoded; ~6.9τ from full white)
the phosphor is spent and the cell reverts to its source. ⚠️ A lit cell with **no colour of its
own** leaves no trail: it draws in `GlyphLook::default_fg`, a look constant of the renderer's
that T3 is lifting onto the param chain, and the producer must not bake a copy of it into the
ring. ⚠️ **No header field and no `layout_version` move.** The colour arrives already decayed, so
the world needs no τ; the flag is a bit in an existing word, and a reader that predates it draws
a dimmer tile — the right picture, just without knowing why.

**Time is the producer's published time, nominal.** `1/tick_hz` per motion tick, the heartbeat
interval per dwell beat, and zero for the settle publish (it is the same instant as the last
motion frame, so with persistence off it stays byte-identical to before). Nominal rather than
measured so a seed reproduces a run; published rather than effect time because persistence is a
property of the *display* — `--tick-hz` below `--fps` slows the effect, not the phosphor. The
phosphors outlive an effect and reset only on a grid of another size, so one effect's settled
text fades under the opening of the next.

**The settle rule: the effect has settled when the *source* has.** `FRAME_SETTLED` is set
whatever the phosphors are doing, so a trail can never hold the settle off. But the trails keep
decaying through the dwell — each heartbeat re-walks the settled source and advances them — so
the payload keeps changing, `generation` keeps moving, and T5's path-trace accumulation restarts
every heartbeat until the last trail crosses the floor. That is the right order: the tracer
converges once the picture has stopped changing, and the world learns that from the counter it
already watches, with no new field. It is also why the floor is not lower: the tail from 3/255
down to the encoder's own 1/255 would hold `generation` moving for another ~2τ for nothing
visible.

**One consumer-side change, in the crate this tier owns.** A trail keeps the `character_id` of
the character that left it, and that character is usually *also* live elsewhere in the same
grid — so `lower_grid` now skips `SGR_PERSIST` cells when it builds the map of where each
character was last tick. Without that the later index wins the map and a sliding character
starts every tick from its own trail; the mutation fails with *"the slide starts at the LIVE
cell (x=-1), not the trail (x=1): 1"*. A trail carries no path bit, so it never slides itself.

⚠️ **Two things the brief assumed that the measurement did not bear out.** The GPU check it
names — *"a `--persist-ms 300` run of `decrypt` showing tails behind the resolving
characters"* — cannot show tails: measured over a 24×2 fixture at 120 fps, `decrypt` runs 752
frames and **never lets a lit cell go dark** (neither do `wipe`, `expand`, `slice` or
`middleout`), so what persistence does there is the bright→dim fade of each resolving
character, which is the max rule at work and the thing "source replaces" would have cut. Tails
behind moving characters are `rain` (55 frames, 301 trail cells on that fixture), `pour`,
`print`, `beams`, `swarm`, `bubbles`, `crumble`. And the first draft of the whole-effect test
used `overflow`, under which the only difference persistence makes is that same rule — found
because the "source replaces" mutation failed *that* test too, which is what a fixture that
never leaves a trail looks like from the inside. The test now demands real trails of its
fixture, or fails saying so. Green and ready to try; no trail has been rendered.
