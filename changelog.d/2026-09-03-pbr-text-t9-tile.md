### PBR text T9 — the tile: an emission profile across the face, and the faceplate that was already there

The ninth tier of `doc/pbr_text_engine.md` (organon#217, §15's T9 row): what the plates show
that `main` did not is that every cell is a *tile* — a phosphor with a soft falloff seen
through a glossy faceplate over a near-black body. This is the shading half of that, in
`cube.wgsl`; the lowering half (a tile for every cell, dark ones too) stays a follow-up,
because `glyph_ring.rs::lower_grid` was in another worker's hands.

**The emission profile.** `fs_main`'s per-instance term is now `emit.rgb * emit.w *
tile_profile(face_uv(local_pos), shape.z)`. The rounded-box mesh has no UV attribute and
needs none: `VsOut.local_pos` is the un-rounded mesh-local position, and `face_uv` takes its
two coordinates across the dominant face — the T3 crown's own rule, now one shared function
(`face_axis`), so the crown and the profile cannot disagree about which face a fragment is
on. A 1×2 tile's face is a square in local space, so the profile is keyed on the tile's own
extent and stretches with it, never on screen space. `tile_profile` is `mix(1, (1 − s)², k)`
with `s = (2u)⁴ + (2v)⁴` clamped to 1: a p=4 squircle, flat-topped, soft-landing, `1 − k` on
the edge midlines and in the corners. ⚠️ It multiplies the **per-instance term only** — the
albedo-modulated glow, the ripple and the RD term are other generators' and do not move —
and at `k = 0` it is *exactly* `1.0`, so the expression reduces bit for bit to T1's and every
draw today is byte-identical (invariant #4). 📌 **The strength lane is `Shared.glyph[13]`,
riding `Uniforms.shape.z`** — a lane the shader never read and `build_uniforms` writes as 0.
Lifting it in the world's `glyph_shape` is a one-line change in `world.rs`, which belongs to
T10; until that lands the lane is 0 and the profile is inert. `glyph_tile.rs` is the CPU
twin (the way T6 mirrored `capsule_interval`): zero strength is exactly one everywhere, sign
and axis-swap symmetry, monotone along every ray, the curve's fixed values, `face_uv`'s tie
rule, and a source check that the shader still defines both functions with the mirrored
signatures — so the twin cannot outlive its subject unnoticed.

**The faceplate needed no code, and saying so is the finding.** The brief asked whether the
glyph draw needs its own clearcoat strength. It does not: the Standard branch already
composes `color * base_scale + coat_spec` with `emissive` inside `color` and `base_scale =
1 − fc`, so under a Clearcoat the phosphor is already transmitted through the coat's Fresnel
and the coat's environment sheen is computed without `emissive` and added after it. A tile
with `emit == 0` shades as its near-black body plus that sheen — §4.1's dark cell that still
reflects the room — and T3's `faceplate` preset already selects the Clearcoat at roughness
0.22. ⚠️ The coat is a per-draw uniform and the backplane is an instance of the same draw,
so it wears the same coat; a backplane of its own is the same "own draw" question §15
already raises for the anisotropic backplane (T10).

📌 **What does not see the profile:** the RT and path-trace hit shading, which reads the
emit flat — so a T5 dwell converges to a flat-cored tile where the raster frame showed a
falloff. `tile_profile` and `face_uv` are pure in the hit's local position, so the tracer can
apply the same two lines once it has it; named for T8 rather than done in its file.

🚨 **No GPU touched any of this**: green and ready to try. A GPU session must load
`faceplate` with a producer running and, once `glyph[13]` is wired and raised, see a lit
cell's core fall off toward its edges; with the full-grid lowering, a dark cell showing the
environment's sheen at zero emission; and the bevel highlight unchanged, since the profile
touches no normal and no vertex.
