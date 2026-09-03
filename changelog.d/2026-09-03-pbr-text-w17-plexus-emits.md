### PBR text W17 — the web takes the emission: coloured beads and wires under `bottled` and `cathode`

T14 (#242) shipped the two web rungs of `doc/pbr_text_engine.md` §10 knowing they were
monochrome, and said why: `world.rs` runs the Plexus pass *after* the grid lowers, so while a
ring is live the tiles are the node cloud — but the loop that builds that cloud copied
`geom.tints` (the faceplate, a near-black dielectric, §4) into the node tints and dropped
`geom.emits`. Every bead and every wire was the same grey, and no lane could recolour it
(`apply_hsv` on a grey is a grey). This closes that gap — one pure function and one loop.

**The node colour is now `plexus_node_colour(ring_live, tint, emit)`**: with no ring the
generator's tint, byte for byte (invariant #4 — every field the web has ever wired is
untouched); with a ring live, the tile's **emission** as linear radiance, `emit.rgb × emit.w`,
the same term `cube.wgsl` adds past the albedo and T10's light lowering ranks by. A dark
tile is a dark node, not a faceplate-grey one; the backplane (emission zero) likewise.

📌 **It feeds `ntints`, and that is the only lane there is.** Read at the draw rather than
the vocabulary: the plexus impostor (`ArmInstance::color`) carries one colour, and
`particles.wgsl::fs_capsule` derives *both* the albedo (`apply_hsv(color, hsv)`) and the
emission (`albedo × glow`) from it — there is no separate emissive lane to route to, and
T6's coaxial core shows exactly that emission through the Glass shell. Tier 1's markers and
struts ride `tints` the same way (`math::draw_plexus`), and the node-driven systems (VXGI,
glyphs-as-lights, GI) read `node_tints_weld` from the same vector. So one value colours the
web whichever tier draws it. ⚠️ A consequence for the two rungs' data: `glyph_faceplate`
**no longer reaches a web** — the tint is not what the nodes carry any more, and Tier 2
clears `geom.tints` outright — so the 0.55 each rung raised it to "so the beads have an
albedo to glow with" is now inert there. Harmless; a later pass over `preset.rs` can drop it.

⚠️ **The gate mirrors the renderer's own.** `emits` is read only while the ring is live
*and* `emits.len() == instances.len()` — the parallel-buffer convention `render.rs` uses
(a mismatch is "no emission") and `glyph_light_candidates` already follows. `lower_grid` is
the only filler of all three buffers, so a live ring always passes; the gate exists so a
future producer that does not cannot index past the end. `geom.emits` is deliberately left
as it was after the pass rebuilds `instances`: Tier 1 leaves it mismatched (so the cube
draw adds no `tile_emit` to the markers — the pre-existing behaviour) and Tier 2 empties
`instances`; neither is changed here.

**Tests** (leg: `cargo test -p organon-world --features world`, `plexus_glyph_tests`):
`a_lit_tile_makes_its_node_the_emission_not_the_faceplate` (rgb is `emit.rgb × emit.w`,
and is not the faceplate), `a_dark_tile_makes_a_dark_node_not_a_faceplate_grey_one` (a
dark tile and the backplane both come out black), and
`with_no_ring_the_node_keeps_the_generator_tint` (invariant #4). Mutation-tested: routing
the tint instead of the emission fails the first by name (quoted in the PR).

🚨 **No GPU touched this: green and ready to try.** Recall `bottled` and `cathode` with
`organon-glyphs` running — the beads and wires should carry the effect's colours where
they were grey, a hue lane should now move them, and a dark cell (dark tiles are off in
both rungs, so only the backplane node) should be a dark bead, not a grey one.
