### PBR text — the emission profile and the dark tiles are parameters, and `faceplate` wears them

T9 landed both halves of the tile inert (organon#217, `doc/pbr_text_engine.md` §15): #233
put `tile_profile` in `cube.wgsl` keyed on `Uniforms.shape.z`, which nothing wrote, and
#236 gave `glyph_ring::lower_grid_with` a `LowerOptions { dark_tiles }` the world never
passed. This is the wire, on the whole of `ARCHITECTURE.md` §17: **`glyph_profile`**
(`gtpf`, 0..1, default 0) and **`glyph_dark_tiles`** (`gtdt`, a flag, default off) in
`params.rs`; `pack_glyph` slots **13** and **14**, the two lanes T3 reserved — `[15]` stays
reserved, no `Shared` field moves and `LAYOUT_VERSION` does not; `PresetValues` fields with
serde defaults in the Look bucket; `world.rs::glyph_shape` lifting `glyph[13]` into
`shape.z` while a ring is live, and a pure `glyph_lower_options` reading `glyph[14] > 0.5`
into the `lower_grid_with` call that replaces `lower_grid`; the seven vocabulary sites
#235 counted (`ACTUATABLE_IDS`, `id_range`, `current`, `actuate`, the gloss, the
`engine_ranges` join, the editor mirror in `lib.rs`), so `organon set glyph_profile 0.5
glyph_dark_tiles 1` is a sentence; two rows in the PBR Text card; and two rows in
`doc/reference/parameters.md` from `organon docs`. 🚨 **Both default to exactly what T1
drew** (invariant #4): `tile_profile` is bit-for-bit `1.0` at zero strength and
`lower_grid_with` at `LowerOptions::default()` *is* `lower_grid`, both already pinned in
their own crates; `glyph_look_tests::a_default_snapshot_is_exactly_the_t1_look` now also
pins the two lanes at 0 on a default `Shared` and that `glyph_lower_options` of it is the
default options. `the_profile_lane_rides_shape_z_and_the_dark_tile_lane_is_a_flag` is the
pure twin of each wire, so dropping either is a named failure rather than a tile that looks
wrong; the chain tests (`t3_routes_agree_with_the_param_table_slot_lists`,
`t3_ids_round_trip_through_distinct_shared_slots`, `builtin_text_presets_are_wellformed`)
grew from eighteen ids to twenty and assert slots 13/14/15 by number.

⚠️ **One line outside the brief's files, in `render.rs`, because the wire would otherwise
leak.** `render()` zeroes `shape.x` and `shape.y` off the generator's cube draw so the
bevel and crown reach only the generator's cubes — and it did not zero `shape.z`, because
nothing wrote it. Once the world writes the profile for a live ring, every other draw that
shades through the main uniform (a plexus node, a membrane sheet) would multiply *its*
per-instance emission by the same falloff. `shape[2]` is now zeroed beside the other two,
with the same scoping and the same reason.

📌 **`faceplate` carries both — `glyph_profile = 0.5`, `glyph_dark_tiles = on` — and that
needed the seed marker bumped.** T10 amended the rung under `seeded_text_v1` and found
what the seed's own doc predicted: a store that had already seeded kept the values it was
seeded with, since the marker keeps the seed from running again. So the marker is now
`seeded_text_v2`, and `seed_text_into` (the seed with the marker read out of it, so a test
can drive it) **replaces the factory-shaped `faceplate`** — the entry with no stated
`exposed` set, which is what `Preset::unstated` writes and what an editor save
(`Preset::capture`) never does — **and leaves a `faceplate` a person has captured over
alone**, with the two new lanes simply off in it. ⚠️ The rails precedent (`seeded_rails_v2`)
replaces by *name* and would have taken a user's `faceplate` with it; the `exposed`
discriminator is the one thing this adds, and it is what keeps T3's promise ("a user's own
`faceplate` is never replaced") true through an amendment. On a store already carrying the
amended rung it reports nothing to persist and the marker drops without a save. Pinned by
`the_v2_text_seed_replaces_a_factory_faceplate_and_keeps_a_captured_one`. The next
amendment bumps to `v3` and touches nothing else.

🚨 **No GPU touched any of this**: green and ready to try. The loop a GPU session closes is
`organon set glyph_profile 0.5 glyph_dark_tiles 1` against a live editor and visual with a
producer running — a lit cell's core should fall off toward its edges, every dark cell
should become a low glass tile carrying the room's sheen between the lit glyphs (the
spec-sheet plate; `glyph_dark_tiles 0` is the before/after plate), the editor's PBR Text
card should show the same two numbers — and the frame time on a fullscreen producer is the
number nobody has, since with dark tiles on the draw is `cols × rows` instances.
