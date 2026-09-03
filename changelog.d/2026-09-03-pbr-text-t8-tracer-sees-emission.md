### PBR text T8 — the tracer sees emission

The gap T1 named and the first GPU look measured (`doc/pbr_text_engine.md` §15, organon#217):
T5 hands a settled glyph frame to the path tracer, and the tracer renders it **dark**, because
every ray-traced pass shaded a hit from `inst_buf` + `tint_buf` and had never been handed the
per-instance emission buffer the cube pipeline reads at `@location(8)`. The three passes that
shade a hit — `rt_pathtrace`, `rt_reflect`, `rt_gi` — now bind `emit_buf` as a read-only
storage buffer beside the instance and tint buffers, index it by the instance id the hit
reports, and add **the same expression `cube.wgsl` adds**, `emit.rgb * emit.w`, into the hit's
radiance — one `instance_emission(idx)` helper per shader, pinned textually identical across
the three by test, so raster and traced agree on what a lit cell is worth (§9's second law).

**Which passes, and why not the others.** `rt_shadow` and `rt_ao` trace visibility only — a
hit is a boolean, never shaded — so an emitter is an occluder to them exactly as before and
they bind nothing new. `rt_caustic` shades hits, but for the *photon's* BSDF: a photon leaves
the key light and bounces through the specular chain, and the emission of the surface it lands
on plays no part in that transport. What emission would mean there is **emitters as photon
sources** — sampling tiles proportionally to their emitted flux — which needs a per-frame CDF
over instances and is a tier of its own; the layout comment names the binding it would take.
And the tracer's next-event estimation reaches only the two directional lights (key + fill,
one shadow ray each) — it has **no light list and no light selection**, so an emissive tile is
reached the way an area light is reached in a pure path tracer: by the cosine bounce landing on
it. That converges over the dwell (the grid fills most of the backplane's hemisphere), but it
is noisier than NEE would be; an emitter list for NEE would ride the brightest-N selection
that T10 owns in `world.rs`, and is left as a documented hook rather than half-built here.

📌 **In the path tracer an emissive hit terminates the path.** Its radiance is added
(throughput × emission) and the loop breaks — in both the RGB and the hero-wavelength
integrators; in GI-add mode the primary-hit emission is skipped like the other primary
terms, since the raster already shows it, and the path *continues* there — the tracer owes
that pixel its indirect light (the review on #232 caught the first version terminating
outside that guard). This is the "lights are emitters" simplification,
taken deliberately: a lit tile's tint is the near-black faceplate (§4), so what the
continuation would have added is ≤ albedo × incident — under 4 % — and a fullscreen grid that
terminates at the first lit tile costs one ray per pixel where it cost `bounces`. What is given
up is the faceplate's own sheen over a *lit* cell in the trace (T9's clearcoat highlight); a
dark cell has zero emission, continues, and shows the room as before. ⚠️ The gate is the
emission's *value*, never "is this a glyph instance" — a T9 dark tile with `emit == 0` must
keep bouncing, and it does.

🚨 **Inert by construction (invariant #4).** With the all-zero emit buffer every existing draw
binds — and every one still does; nothing about how the buffer is filled or zeroed changed —
`instance_emission` returns exactly `vec3(0.0)`, the added term is zero, the termination gate
(`any(le > 0)`) is false, and every pass's output and RNG stream is byte-identical to before.
`RtReflect::run`, `RtGi::run` and `PathTracer::trace` each take one more `&wgpu::Buffer`, and
the three call sites in `render.rs` pass `&self.emit_buf` — the only lines touched there.

⚠️ **The binding is a bind-group entry, not a vertex slot — and CI cannot see it fail.** wgpu
validates a bind group against its layout at *draw* time, so a layout that declares binding N
and a `create_bind_group` that omits it is a runtime panic no leg of the bar can reach. So each
pass's layout is now a pure `layout_entries()` a test can hold: it asserts the emit binding's
index (5 for reflections and GI, 7 for the tracer, after the caustic map and the cache
weights), that it is read-only storage, fragment-visible, and — the drift that actually bites —
that the shader source declares `emits` at the **same** `@binding`. Mutation-tested by dropping
the entry from one pass: the test names the pass and the missing binding. 🚨 **And the buffer
has to be created with `STORAGE`** — `make_emit_buf` made it `VERTEX | COPY_DST` only, which the
raster path never minded and wgpu would have refused at bind-group creation on the first real
GPU; the review on #232 caught it. Every buffer an RT layout binds as storage (instances,
tints, emission — in `new` and on every regrow) is now created with one `RT_HIT_BUFFER_USAGE`,
and a test walks every `BufferDescriptor` in `render.rs` and fails naming the label if one of
them is created any other way, so the class is closed rather than the instance. What no test here
can do is light the traced image; **green and ready to try**, and what a GPU session must look
at is the `faceplate` preset with `organon-glyphs` running and the camera hold on: the dwell
should converge to a *lit* photograph where at `2a06e06` it went dark, and a ray-traced
reflection of the grid on a glossy backplane should show lit glyphs rather than dark faceplates.
