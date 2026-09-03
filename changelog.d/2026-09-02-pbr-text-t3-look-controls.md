### PBR text T3 — the look on the param chain, a held camera, and `faceplate`

The third tier of `doc/pbr_text_engine.md` (organon#217): the glyph ring's look was one
`const` (`GlyphLook::DEFAULT`, T1), the camera it drew under was whatever the orbit rig
happened to be doing, and T6's capsule core was reachable only through an environment
variable. All three are parameters now, and the first preset of §10's ladder rides them.

**Three `Shared` blocks, tail-appended after `mindview_gen`; `LAYOUT_VERSION` 0x0285 →
0x0286, 8512 → 8624 bytes.** `glyph[16]` carries every field of T1's look in **cell
units** (§5.1) — cell width (the one world-unit anchor), extrusion, gap, emission gain in
SDR-white units (§4), the faceplate grey, the backplane's RGB / margin / depth, the default
foreground — plus the tiles' **own bevel** and a **face crown**. `glyph_cam[8]` is the held
camera: hold / tilt / zoom. `capsule[4]` is T6's core fraction and absorption. Sixteen
parameters, each walked through every link of `ARCHITECTURE.md` §17 — `params.rs`,
`param_table.rs`, both `to_shared()`s, `ipc.rs`, `preset.rs`'s mirror with serde defaults
and the tab partition, the world's readers, the uniform, the shader — and 🚨 **every
default is exactly the constant it replaces** (invariant #4). `glyph_look_tests::
a_default_snapshot_is_exactly_the_t1_look` pins a default `Shared` against
`GlyphLook::DEFAULT` field for field, and `every_look_slot_reaches_its_field` writes a
distinct value into every slot and reads each back at the field the contract names, so a
swapped or skipped slot is a named failure rather than a wrong tile. The layout goldens
grew the way they must — offset pins, `EXPECTED_SHARED_SIZE`, the whole-struct hash
re-pinned from the live print (11090782705610843067) — and a **prefix golden** proves the
first 8512 bytes still hash to the 0x0285 value with `layout_version` rewound, which is
what makes the re-pin a growth and not a shift.

**The camera holds, so T5 can converge.** Measured on the first GPU look: the grid arrived
small and far because it inherited the cube field's default distance, and the auto-orbit
kept moving, so `pt_moved` restarted accumulation every frame and converge-on-hold never
converged. `world.rs::glyph_camera_rig` is now a second *absolute* arm on the camera
selection — below the Console's `substrate_rig`, above rails and the orbit — `(centre,
yaw 0, pitch = tilt, distance, roll 0, fov)` with the distance **computed from the tiles'
bounds and the frame's FOV and aspect** (`fit_distance`; the bounds include the backplane,
so "fills the frame" means the backplane does), times the zoom. ⚠️ Never sized by feel
from the wheel — a notch is `distance *= 1 − dy·0.001`, which is no unit. It applies only
while a ring is live **and** the preset's hold is on; with either off, a session is on the
orbit rig it had before. `a_held_rig_lets_the_dwell_converge_where_an_orbit_cannot` walks
T5's own restart logic under a held rig (accumulates) and an advancing yaw (restarts every
frame). The hold is a **Motion** control, because it is a camera and a Look bucket must
not be able to move the viewpoint.

**Bevel and crown.** The tiles get their **own** bevel lane rather than sharing
`Shared.bevel`: that is a Generator-bucket control for the field's cubes, a Look preset
must carry the whole glyph look, and on a 1×2×0.18 tile the same number rounds a different
shape. Its default is 0, which is what T1 drew (it rode `Shared.bevel`, default 0). The
**face crown** is §5.1's curvature across the face — a per-fragment dome normal in
`cube.wgsl`'s `fs_main` (`(2·crown·x, 2·crown·y, 1)` on the dominant face, the bevel's
rounded band keeping `round_local`'s normal, transformed to world through the inverse
frame the vertex stage already passes), gated on `Uniforms.shape.y > 0`. **Normal-only**:
no vertex moves, so the `@invariant` depth prepass, the silhouette and the RT / path-trace
hit shading are untouched, and there is nothing to keep in step. `render()` now zeroes
`shape.y` beside `shape.x` off the generator cube draw, and the world writes it only for a
live ring, so every frame today is byte-identical.

**T6's capsule core reaches a parameter — and the env seed stays, as an override.**
`Shared.capsule` travels as a field on the render frame's `Surface` (constructed in exactly
one place, so the compiler names the site) to `ParticleSystem::set_capsule_core` before the
uploads it affects. `ORGANON_CAPSULE_CORE` **wins when set**, on purpose: every link from
`params.rs` to the setter is pinned on the CPU, but the last hop — the GPU draw reading the
uniform — is not provable without a GPU, and retiring the only knob that has been looked at
on the strength of one that has not would be the wrong order. `the_param_reaches_the_lanes_
unless_the_seed_overrides_it` pins both directions; a GPU session that sees the param move
the core deletes `capsule_env` and the resolver collapses to `lanes`.

**`faceplate`**, seeded once into the store like the Rails rides and named literally so
`preset load faceplate` needs no guessing: T1's look with bevel 0.12 and crown 0.35, a
Clearcoat faceplate at roughness 0.22, the held camera at 6° with the orbit path off, the
atmosphere off and the background hidden (the default sky behind the grid read as fog over
terrain), the IBL at 0.15 so dark cells still show the room (§4.1), and halation on.
⚠️ **What it cannot carry, and why**: TAA is `temporal[0]`, a **param-only** block
(`pack_temporal` declares one packer) that no preset captures — §8's "do not use TAA" is met
by TAA's default of OFF, not by this file, and a session that turned it on keeps it on
through a recall. MSAA is Settings too. The path tracer's toggle is a session state; the
dwell is traced by T5's handover, not by the preset asking.

Not done here, with reasons. `GLYPH_SILENCE_S` and the unknown-symbol stand-in's emission
and depth stay constants: the first is a policy about a producer that exited, the second is
T7's placeholder whose whole point is to be replaced. CRT post authored nothing — halation
was already a full parameter block (`finishing`), so the preset sets it and no knob was
lifted. The **legibility thresholds** are not on the chain (T2's `native/verify/` proposal
stands). And ⚠️ **the reference did not change**: `doc/reference/parameters.md` lists only
the CLI-settable ids, and none of these sixteen is one — the regeneration ran and confirmed
no drift rather than producing a diff. 🚨 **No GPU touched any of this**: green and ready to
try. A GPU session must load `faceplate` with a producer running and see the grid fill the
frame, the camera hold through a dwell so the trace visibly sharpens, the bevel and crown
sliders move the light across a tile, and `ORGANON_CAPSULE_CORE` unset with the capsule
core slider lighting a Glass capsule's wire — that last one is what retires the seed.
