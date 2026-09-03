### PBR text T14 — the preset ladder: five more rungs, as data, each naming the knob it lacks

The fourteenth tier of `doc/pbr_text_engine.md` (organon#217, §10): `nixie`, `foundry`,
`anodized`, `bottled` and `cathode` join `faceplate` as factory presets in
`preset.rs::builtin_text_presets`, seeded under `seeded_text_v3`, named literally so
`preset load nixie` needs no guessing. Every rung shares the room the first GPU look
settled (held camera, orbit off, atmosphere off, background hidden, IBL at a sheen, FX
pass on — now one `room` closure, so the six cannot drift apart on it) and each departs
in one direction. §10.1 is the ledger: per rung, what it is built from and the knob it
is missing. The rule that shaped every one of them: **a rung sets only knobs that reach
its draw.** A value set where nothing reads it is a preset lying about what it does, and
three of the brief's assumptions turned out to be exactly that once the draw was read
rather than the vocabulary.

⚠️ **The T6 capsule core reaches one draw, and a tile is not it.** `Shared.capsule`
travels to `ParticleSystem::set_capsule_core` and is read by `particles.wgsl::fs_capsule`
alone — the capsule impostor path. A glyph tile is an instanced cube shaded by
`cube.wgsl`, whose Glass branch adds `tile_emit` at the surface. So `nixie` — §10's
"glass envelope with a filament", a *tile* rung riding Glass + §4 — does **not** set
`capsule_core`/`capsule_absorb` as the brief asked, and the test pins that it does not:
the filament *inside* the envelope is the rung's named gap (T6's idea applied to the cube
path), not a knob to turn. What `nixie` does have is the one dispersion that reaches a
tile, `glass_dispersion` (the cube glass path's RGB split), a deeper domed tile, and the
emission gathered to a filament by `glyph_profile` 0.85.

⚠️ **There is no thin-film `MaterialType`**, and the physical Airy film model
(`film_thickness`, `thin_film_physical`) is evaluated only inside `cube.wgsl`'s
Glass/Refractive branch. `anodized` is therefore the **iridescence lobe** (`u.irid`, the
view-angle model) over a `metallic 1` Standard at a mid-grey faceplate — a metal's albedo
is its F0, so the near-black default would be a black mirror — with a crown, because the
lobe is keyed on `n·v` and the crown is what rolls it across a tile. `foundry` is the
blackbody lane (`incandescence` 0.12 at 1600 K) over dark rough iron with the effect's
emission low and gathered to the slug's centre; §10's "value drives temperature" has no
lane — `blackbody(u.emit.w) * u.emit.z` is a per-draw uniform, the same ember on every
tile and on the backplane, and the per-instance `emit.w` is a gain, not a Kelvin.

📌 **`bottled` and `cathode` are honest because of an ordering nobody set out to use:
`world.rs` runs the Plexus pass *after* the grid lowers**, so while a ring is live the
tiles *are* the node cloud, and with impostors on every lit cell is a sphere impostor and
every adjacent pair a capsule — the draw the T6 core does reach. `bottled` is Glass beads
on Glass rods (`capsule_core 0.4`, `capsule_absorb 1.5`, the T6 worker's values), wired to
a stroke's four neighbours (`plexus_radius` 2.05 — a vertical neighbour is 2.0 on a 2:1
cell, a diagonal 2.24 — and four links), the camera at 55° to look along them; `cathode`
is the same web as circuitry, emissive nodes on thin wires. ⚠️ **Three gaps, all in
`world.rs`, none of them this change's to close.** The plexus pass takes the tiles'
**tints** — the faceplate grey — and drops their emission, so both webs are monochrome:
each raises `glyph_faceplate` to 0.55 so the beads have an albedo to glow with (the
core is `albedo × emissive`), and the effect's colours are lost; a hue lane cannot help,
since `apply_hsv` on a grey is a grey. Proximity has no glyph identity, so "wire the cells
*within* each glyph" is by distance, and the 2.0 that reaches a vertical neighbour also
bridges a one-column gap. And the impostors are not in the TLAS, so the T5 dwell under
either web hands the tracer no geometry — reasoned, not measured. Dark tiles are **off**
in both (a node per cell is a lattice) and glyphs-as-lights **off** (the impostor path
empties `instances`, so there is nothing to lower); the backplane instance is a node, and
above `NODE_CAP` (1400) lit cells the web sub-samples.

📌 **The seed gained one rule, because the `exposed` discriminator has a hole for a name
that has never been seeded.** #239's `seed_text_into` replaces a *factory-shaped* entry
(no stated `exposed` set) and keeps a captured one — right for `faceplate`, which `v1`
wrote. But a preset saved before organon#124 has no `exposed` set either, so a user's own
`nixie` from that era is factory-shaped by every test but history. History is what
`TEXT_RUNGS_SEEDED_BEFORE` records: only a name an earlier marker wrote is ever replaced,
so at `v3` a stale `faceplate` is amended in place and any `nixie` already in the store —
whatever its shape — is the user's. ⚠️ The list grows at every bump: when `v4` amends a
rung `v3` first wrote, that name goes on it or the amendment never reaches a `v3` store.
Pinned by `a_rung_never_seeded_before_never_replaces_an_entry_of_its_name` (mutation:
drop the guard, the first assertion fails by name) and by the widened seed test — a v2
store gets exactly the five appended, `faceplate` untouched, and a captured `faceplate`
keeps its values with the five arriving beside it.

**Tests** (leg 7, `cargo test -p organic-math-native --lib --features console-edition`):
`builtin_text_presets_are_wellformed` now walks all six — the shared room, the wire
(`glyph_cam`, slots 13/14/15), tile rungs tiling every cell and never claiming the
capsule core, web rungs never tiling every cell — and
`each_rung_is_pinned_by_the_value_that_makes_it` pins per rung the one value that makes
it that rung, in `Shared`: `nixie` Glass on `lighting[7]`, `foundry` `emit[2..3]`,
`anodized` `surface_fx[3]`, `bottled` `capsule[0..1]` + `plexus_edge_mat[0]`, `cathode`
`plexus_node_mat[7]` with no core. Mutation-tested three ways: `nixie` given `foundry`'s
material fails `nixie is the Glass-tile rung` (`left: 0 right: 2`); the seed guard dropped
fails `an unstated nixie predates every seed that could have written it`; a web rung
tiling every cell fails `bottled: a node per cell would be a lattice`.

🚨 **No GPU touched any of this: green and ready to try.** Recall each rung with
`organon-glyphs` running and look for one thing each: `nixie` — the glow pulled to a
bright core inside a domed glass tile whose rim reflects the room, and a warm bleed;
`foundry` — dull-red iron with the effect's colour only at the slug's centre, the whole
plate faintly warm (the per-draw ember — if the backplane glows as much as a cold slug,
that is the gap, not a bug); `anodized` — colour bands sweeping across each tile as the
key highlight moves over the crown, the phosphor barely there; `bottled` — the tiles gone,
grey glass rods between adjacent lit cells with a bright core visible through the shell,
seen steeply; `cathode` — glowing grey beads on thin wires spelling the text, gaps of one
column bridged. In both webs the **loss of the effect's colour** is the known gap; a
coloured web means someone has already fed `emits` to the plexus pass.
