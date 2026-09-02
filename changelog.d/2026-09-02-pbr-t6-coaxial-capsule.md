### The coaxial glass capsule: a Glass capsule impostor can show the glowing wire inside it

PBR text T6 (#217, `doc/pbr_text_engine.md` §11 route 1). A Glass/Refractive capsule
impostor used to refract the **environment** — `particles.wgsl`'s Glass branch sampled
the prefiltered IBL through the refracted direction, so a glass tube showed you the sky
rather than the thing suspended in it. It can now show a **coaxial emissive core**: the
view ray is traced to the outer capsule exactly as before, refracted at that surface,
solved against an inner capsule of the same endpoints and a fraction of the radius that
carries the instance's emission, attenuated by Beer–Lambert in the instance's colour over
the glass path, and Fresnel-composed with today's environment reflection. This is what
`bottled` and `cathode` in the preset ladder ride; it does not author either.

🚨 **Inert by default, and the reduction is a gate rather than an algebraic limit.** The
knob is a render-side uniform, `DrawU.capsule` — appended at the **tail** of the particle
draw uniform (a struct `particles.rs` owns and uploads, not `Shared`; no `params.rs`,
`param_table.rs`, `to_shared()` or preset field, since look controls are T3). `x` is the
core fraction and `y` the absorption density; both default to 0. With `x == 0`,
`fs_capsule` calls `shade_bead` exactly as before, so the frame is pixel-identical — and
the bead and spark paths never read the lane at all. `ParticleSystem::set_capsule_core`
is the API a later tier wires; until then `ORGANON_CAPSULE_CORE="<frac>[,<density>]"`
seeds it so a GPU session can look at it.

📌 **No extra march.** The inner hit and the outer exit are analytic: a capsule is a
convex union of a finite cylinder and two spheres, so its ray interval is the min of the
pieces' entries and the max of their exits — three quadratics each, no loop, beside the
96-step sphere trace that found the outer surface. `capsule_trace`, and so the depth the
FX prepass sees, is untouched.

⚠️ **Two traps the arithmetic guards against.** WGSL `refract()` returns the zero vector
on total internal reflection, and a zero direction solved against a capsule is silent
garbage — so the shader falls back to the reflection-only expression on it. But air→glass
entry (η = 1/ior ≤ 1) **cannot** TIR, which means the pre-existing `// total internal
reflection` branch in the bead Glass path has been unreachable since #298; the guard
here is against a bad normal, not a physical case. And a near-black tint over a long
chord underflows to exactly zero and reads as a black tube rather than a dark one, so
optical depth is clamped at 6 per channel in `capsule_transmittance`.

A CPU twin, `particles.rs::capsule_core`, mirrors the interval solve, the transmittance
clamp, the inert gate, the `refract()` contract and the env-seed parser, and pins each
with a test under `cargo test -p organon-render` (the shader itself is validated offline
by that crate's `tests/wgsl.rs`, not the root crate's). ⚠️ **No GPU touched this:** green
and ready to try. A GPU session must look at three things — the inert default against a
pre-change frame, a Glass capsule with `ORGANON_CAPSULE_CORE=0.4,1.5` showing its core,
and a grazing view where Fresnel takes the tube to mirror rather than any TIR artefact.
