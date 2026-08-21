### The procedural material bake can be read by the renderer that asked for it

`Renderer::baked_material_views()` and `Renderer::baked_material_resolution()`. The six channel
textures the bake writes were reachable only from inside `organon-render`: they live in a private
field of the private `MaterialBaker`, on a private field of `Renderer`.

🚨 **The half that already worked is what makes this small.** `Renderer::new(device, queue,
format)` takes **no surface and no window**, and `bake_material` is already public — so another
wgpu application can construct a `Renderer` on **its own** device and drive the bake today. It
simply could not reach the result, which left a downstream renderer no option but to reimplement
the bake. That is the fork `LICENSING.md`'s split exists to make unnecessary.

📌 **Views, not pixels.** A caller on the same `wgpu::Device` binds these directly; nothing is
copied and no readback happens. That is why the bake targets still need no `COPY_SRC`, and why
this is an accessor rather than a transfer path. ⚠️ A caller wanting the bytes on the CPU would
need `COPY_SRC` on `make_target` — deliberately **not** added, because nothing in this repo needs
it and an unused usage flag costs every target its fast paths.

⚠️ **Inert by construction.** Nothing inside this workspace calls either method, no pipeline or
bind group changed, and the bake writes exactly what it wrote before. Invariant 4 is satisfied
without a flag, because two `&self` accessors cannot alter a frame.

📌 **Four tests, on the part a GPU is not needed for.** `baked_material_views` documents its order
as `MaterialTextures::CHANNELS`', while the *count* comes from `MAT_SLOTS` and the present bits
come from `MaterialBaker::channel_slot` — three tables that must agree and that nothing checked.
They now pin: one target per declared channel, every baked channel landing in a slot that exists,
no two channels sharing a slot, and the present bits being distinct powers of two. ⚠️ Verified to
fire rather than assumed: pointing `channel_slot` at a slot past the end fails one, and making two
channels share a slot fails two.

⚠️ **What is still not reachable, and is the next tier rather than an oversight:** `MAX_LAYERS` is
2 and each `bake_material` call **replaces** the previous result, so a material of more than two
layers cannot be accumulated by a caller that plans it as a sequence of passes. That is the same
defect as the non-accumulating `present_mask`, seen from outside. Fixing it changes bake
*semantics* — the visual calls `bake_material` every frame and depends on replacement — so it
needs an opt-in and is deliberately not folded in here.

📌 Asked for by **Ascent**, a 6DOF game built on this render stack, which consumes these crates
and may never fork them.
