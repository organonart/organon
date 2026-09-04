### `bg_visible 0` now blacks the path-traced dwell, not just the raster phase

The glyph demo's whole premise is a black room, and `organon set bg_visible 0` gave one — for
about forty-five seconds. Measured on organon-one (RTX 5090, `main @ 4683d72`): the raster
phase is black and the grid of dark glass tiles reads with its faint IBL sheen, and then T5's
dwell hands the held frame to the hardware path tracer and a blue-grey gradient floods it. The
flag worked; the tracer had never heard of it.

`skybox.wgsl` paints the raster backdrop as `hdr × env_intensity × bg_brightness`, where
`bg_brightness` is the **Background Brightness** dial gated by **Background Visible**. The
tracer misses to its own **analytic** sky — a horizon→zenith gradient plus a key-light disc —
and that expression carried only `env_intensity`, because `bg_brightness` lives in
`SkyUniforms` and the tracer binds no such block. 🚨 **So to the tracer, "how dark is the
room" and "how much light is on the tiles" were the same number.** `env_intensity 0` was the
only route to a black dwell, and it takes off the IBL sheen that dark cells need in order to
show the room at all — the spec-sheet plate's own premise.

**No new parameter, no preset change, no `Shared` field, no `LAYOUT_VERSION` bump.** Both
flags already existed and were already on the CLI vocabulary; a value simply failed to reach
one shader. The lane it now rides was already there and already empty:
`render::Uniforms.env.x` is where **exposure** sat before it moved to the composite, after
which `world.rs` wrote a literal `1.0` into it that nothing read — verified across every
spelling (`env.x`, `env.r`, `env[0]`, every `.x`-leading swizzle) in every shader in the
crate. `world.rs::bg_brightness` is now the one computation feeding **both** uniform blocks,
which is the defect actually being closed: the term had one consumer and silently gained a
second.

📌 **The gate applies to the PRIMARY miss only, and that asymmetry is the point.** A camera
ray that hits nothing *is* the backdrop, so it obeys the flag. A **bounce** ray reaching the
sky is indirect **illumination** — the same environment light the raster's IBL keeps
delivering while the backdrop is hidden — so it keeps `env_intensity` untouched. Gating both
would make `bg_visible 0` unlight the scene, which is exactly the behaviour this change
exists to remove. The distinction is not invented here: `rt_pathtrace.wgsl` already drew it
for GI-add composite mode ("skip the primary miss — the raster shows the background"), and
this reuses it rather than flattening it. The same reasoning covers looking *through* glass:
a refracted ray is a bounce, and the raster shows the environment through glass with the
backdrop hidden too.

⚠️ **`rt_reflect` and `rt_gi` do not have the same gap — and the reason is not symmetry, it
is that neither has a sky.** Both return `vec4(0.0)` on a miss: rt_reflect so the raster
env/IBL reflection stands through the missed fraction, rt_gi so the receiver's own ambient
stands. A mirror showing the environment is showing reflected *light*, which `bg_visible`
leaves alone on the raster path as well. Both are pinned that way by test, so if either grows
an analytic sky the question is asked again rather than answered by silence.

🚨 **Taking a dead lane makes every stale comment about it load-bearing.** A dozen shaders
mirror `Uniforms` and most still label that slot `x=exposure` — harmless while nothing reads
it, a confidently wrong number the moment something does. The authority is now
`render.rs::Uniforms`, and a guard stands over the rest:
`only_the_tracer_reads_the_background_lane` **scans `organon-render/src/*.wgsl` on disk**
rather than a hand-kept list, strips comments before scanning (or the comment explaining the
trap would trip the guard), and fails if anything but `rt_pathtrace.wgsl` reads the
component. The three Rust sites that copied `env[0]` into the chamber/particle/splat blocks
now pass a literal `1.0`, so each keeps the meaning its own comment claims.

**Byte-identical where the raster already agreed.** The gate is exactly `1.0` for every
bounce and `bg_brightness` (default `1.0`) at the camera, and 1.0 is the IEEE-754
multiplicative identity — pinned as bits over subnormals, both zeros and both infinities,
alongside the shader's own literal so a future `0.999` fails the build. ⚠️ One widening worth
naming: because the term is the raster's whole `bg_brightness` rather than a bare on/off, the
**Background Brightness** dial now reaches the tracer as well. At the shipped default that is
a no-op; at any other value the two paths previously disagreed and now agree.

Eleven mutations were run against the six new tests and all eleven were caught. **No GPU
touched this** — green and ready to try.
