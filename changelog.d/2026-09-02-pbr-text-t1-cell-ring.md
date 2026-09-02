### PBR text T1 — the glyph ring, a ttfx producer, and per-instance emission

The first buildable tier of `doc/pbr_text_engine.md` (organon#217): a terminal text effect,
rendered as a grid of lit, bevelled tiles instead of blitted glyphs. Four pieces, none of which
changes a frame that does not ask for it.

**A new workspace member, `organon-glyphs`**, links `ttfx` (DHH's Rust port of
terminaltexteffects, MIT, pinned by git rev — it is not on crates.io) and runs an effect
headless: build it by name, tick `next_frame` under `Clock::Virtual` at a cadence we own, walk
the cell grid out of the engine after every tick, and publish it. When the effect returns
`None` the settled text is **held for a dwell** (`--dwell`, default 4 s, republished as a
heartbeat) before the next effect — §8 measured that every effect settles and almost none
hold, so the hold is ours, and this is where it lives. ⚠️ The grid is *not* read from
`Terminal.terminal_state`, which is public and is the formatted rows with colour already
folded into ANSI; the painter's walk is rebuilt from `arena` (`is_visible`, `motion.
current_coord` + the canvas offsets, max `(layer, character_id)` per cell), matching
`update_render_cells` step for step. ⚠️ **ttfx rows grow up**; the ring is top-down, the
flip happens once in the producer, and it is pinned on an asymmetric fixture because a
symmetric logo cannot tell a wrong flip from a right one.

**The glyph ring** — `organon_core::glyph_ring`, at `ipc::glyph_ring_path()`
(`$TMPDIR/<ns>-glyphs.bin`, with the `_in(ns)` twin) — is a separate mmap channel on the
`mind_ring`/`audio_ring` precedent: no `Shared` field, no `LAYOUT_VERSION` move. A
**double buffer with a lap guard** rather than a slot ring: two slots, the writer always fills
the one the reader is not on, and the reader re-reads `write_seq` after its copy and retries if
it advanced by two. Per cell it carries more than a terminal would — symbol, fg, bg and SGR
flags, plus `layer`, `character_id`, an `active_path` bit, and a **reserved** sub-cell offset
pair nothing writes yet. The header carries the **cell aspect** (ttfx's 2:1 — square tiles turn
every ring into an ellipse), the producer's tick rate, a layout version and the cell stride,
and the reader refuses a ring that disagrees on either of the last two: a stale writer beside
a fresh reader would not crash, it would draw plausible garbage.

**The world reads it.** When the ring is live, the frame's instances are replaced by one
rounded-box tile per non-empty cell — the block and shade glyphs mapped to sub-cell extent and
extrusion depth (`░ ▒ ▓` become 25 / 50 / 75 % depth, which is better than the stipple it
replaces), an unknown symbol drawn as a full block at reduced emission and depth (its colour
and timing read; its letterform is T7's) — plus a PBR backplane slab behind the grid. A cell's
colour goes to **emission, not albedo** (§4), decoded from sRGB first, with a near-black
faceplate as the tint. A character on an active path slides `previous → current` over the
producer's tick; one placed by `set_coordinate` cuts, because interpolating it would invent
motion the effect never authored. A producer that stops leaves the last grid for three seconds
and then hands the frame back to the generator, so yesterday's ring file in `$TMPDIR` does not
draw a frozen grid forever.

**The cube pipeline gains a fourth instance buffer** — `@location(8) emit: vec4` beside the
tint — and `cube.wgsl`'s emissive term gains `+ emit.rgb * emit.w`, bypassing albedo. 🚨
**Inert by construction (invariant #4):** every existing draw binds an all-zero emission
buffer (wgpu zero-initialises a fresh buffer; nothing writes it until a glyph frame does, and
everything a glyph frame lit is zeroed back — to a **high-water mark**, not the previous frame's
length: an effect's live-cell count shrinks as it animates, and the review caught the first
version leaving `[50, 100)` lit after a 100-then-50 pair for an 80-instance generator draw to
read; `emit_upload_plan` is pure and its tests pin that the lit set is always exactly the last
upload), so the added term is exactly `vec3(0.0)` and the expression reduces to the one it
replaced. No `Shared` change. ⚠️ A
fourth layout in a pipeline means a fourth buffer at **every** draw against it or wgpu fails
validation at draw time, and no leg of the bar has a GPU — so all 27 slot-2 binds gained a
slot-3 twin in one mechanical pass, the depth prepass takes the same four buffers and ignores
the fourth, and the all-zero buffer bound beside the scenery / plexus-overlay / membrane tints
is regrown whenever any of those could draw more instances than it covers.

Three things the design got wrong or did not say, found on the way. **`clap` is a third direct
dependency of the producer**, not the two §6.1 named: ttfx's effect configs are clap `Args`
whose defaults exist only in attributes, none implements `Default`, and ttfx does not re-export
clap — so building an effect by name cannot be written without naming it (clap 4 is already in
the workspace graph, so nothing new is compiled). **ttfx pins `clap_complete = "4.6.9"`** and
this workspace's lockfile held 4.6.7, so cargo refused to resolve the new member at all until
`cargo update -p clap_complete --precise 4.6.9` — a lockfile-only bump the root crate's `^4`
still satisfies. And **the hardware-RT and path-trace passes do not see the emission**: they
take `inst_buf`/`tint_buf` as storage and shade from the tint, so a reflection of the grid is
a reflection of dark faceplates. T1 names it; carrying `emit_buf` into the hit shading is the
same shape of change one layer down. The look — extrusion, gap, gain, faceplate, backplane,
margin — is `GlyphLook::DEFAULT`, one `const` in core, and **T3 lifts every field of it onto
the param chain**. Green and ready to try; nothing here has been looked at on a GPU.
