### PBR text — the look, the held camera and the capsule core are on the CLI vocabulary

T3 (organon#217) put sixteen text-look parameters and two capsule-core parameters on the
param chain and registered none of them with the `organon` command, so the day after it
landed `organon set gtbv 0.12` answered *"'gtbv' is not an actuatable param"* and
`organon catalog --manual` listed nothing of the text look. This is the registration:
`pack_glyph`, `pack_glyph_cam` and `pack_capsule` join `agent::core_catalog()` (so the
Performer's prompt and `catalog` see them), the eighteen ids gain an `id_range`, a
`current` / `actuate` route into `Shared.glyph` / `glyph_cam` / `capsule`, a
`param_desc` gloss and an `ACTUATABLE_IDS` entry in `organon-agent`, the
`engine_ranges` join in `cli.rs` and the `slot_facts` walk in `console_catalog.rs` learn
the three blocks, and the editor's apply-channel mirror in `lib.rs` learns the eighteen
fields, so a slider follows what the command did. `doc/reference/parameters.md` gained
exactly eighteen rows from `organon docs`, and `generated_reference_is_current` went red
and then green around that regeneration, which is the shape it should have.

⚠️ **The ids are the parameter field names, not the four-character host ids.** The brief
that commissioned this asked for `organon set gtbv 0.12`; the vocabulary has never been
spelled that way — every existing id is the `param_block!` slot name (`glow`, `cam_path`,
`bell_physical`), the catalog is generated from those slot lists, and `gtbv` is what a
DAW sees. So it is `organon set glyph_bevel 0.12 glyph_cam_hold 1`, and the skill says
so in as many words. No alias layer was added: a second id namespace on the command line
would be a new mechanism, and this change is registration. The flag `glyph_cam_hold` is
spelled 0 / 1 on the lane like `bell_physical`, and the editor mirror sets the
`BoolParam` from `v > 0.5`.

📌 **What a new actuatable id costs, counted here because nothing else counts it.** Seven
sites in five files: `core_catalog` and `slot_facts` (each a list of packs), the five
tables in `organon-agent` (`id_range`, `current`, `actuate`, `param_desc`,
`ACTUATABLE_IDS`), the `engine_ranges` join, and the editor mirror. Six of them are
pinned — miss `ACTUATABLE_IDS` and `catalog_ids_with_a_range_are_all_actuatable` names the
id, miss the gloss and `every_actuatable_id_has_a_gloss` does, get a range wrong and the
taper gate prints both numbers, and the two new tests
(`t3_ids_round_trip_through_distinct_shared_slots` below the plugin,
`t3_routes_agree_with_the_param_table_slot_lists` above it, which packs a marked
`PresetValues` through the real slot lists and reads it back through the agent's routes)
name the id whose slot index is wrong. All of that was mutation-tested. ⚠️ **The seventh
site — the editor mirror's `match` in `lib.rs` — is pinned by nothing.** Leave an id out
of it and `organon set` still moves the picture, because the visual's override lane is
what draws; only the editor's slider stops following, and no test says so. Closing that
needs a `ParamSetter`, which is nih_plug's, and is not done here.

🚨 **No GPU touched this**: green and ready to try. The loop a GPU session closes is
`organon set glyph_cam_hold 1 glyph_bevel 0.12 glyph_crown 0.35` against a live editor
and visual with a producer running — the tiles should round, the light should move across
a face, and the editor's PBR Text card should show the same numbers.
