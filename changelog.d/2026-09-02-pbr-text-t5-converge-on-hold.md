### PBR text T5 — converge on hold: the glyph ring's generation keys the path tracer

The fifth tier of `doc/pbr_text_engine.md` (organon#217): a rastering preset now hands a
glyph ring's **settled** frame to the path tracer, so the screensaver resolves into a
photograph — animation that comes to rest and then visibly sharpens over the dwell into a
converged still — and drops back to raster the instant the next effect moves. Two changes,
both in `organon-world`'s `world.rs`, both pure functions with the decision pinned by test.

**Accumulation restarts when the glyphs move.** The path tracer restarts its progressive
sample count on a camera move, a resize, or a change to the settings that decide what the
buffer holds (the `pt_content` key) and deliberately not on geometry change — a moving
field would smear the average. T1's ring put a counter there for exactly this: `GlyphFrame.
generation` bumps only when the cell **payload** changes and holds through the dwell's
heartbeat republish. `pt_content_key` now appends the ring's `(live, generation)`, so a
glyph frame that changed restarts accumulation and one that did not accumulates. ⚠️ `seq`
or `tick` would restart every 250 ms and never converge. The `live` bit beside it keeps
"no ring" distinct from "ring at generation 0" and makes a producer going silent a content
change of its own.

**Raster during motion, path-trace during the dwell.** The handover is one predicate,
`pathtrace_active(preset_pt, glyph)`: *the preset's toggle OR a glyph frame is drawing this
frame AND carries `FRAME_SETTLED`*. A preset that already traces is untouched; a session with
no ring reduces to the toggle and is byte-identical to before (invariant #4); every other
gate the tracer had still applies on top. The restart is keyed on that live answer rather
than the toggle, so the count is held at 0 through motion and the dwell's first traced
frame starts clean. **Silence is not settle**: T1's 3 s silence rule clears `live`, so a
stale grid whose last frame still says settled is never traced as held.

Not done here, on purpose: TAA is the preset's `temporal[0]`, not a glyph setting, and §8's
ghosting warning is T3's to act on with the look controls; and the camera is not stilled —
a preset whose auto-orbit is running restarts accumulation every frame and the hold never
converges, which is a preset / screensaver-mode matter (T3 / T4). The editor's "path
tracer: ON — N spp" line now reports the live state, so it reads true during a dwell.
Green and ready to try: what a GPU session must see is the frame sharpening over the dwell
after an effect settles, and restarting with no after-image when the next one starts.
