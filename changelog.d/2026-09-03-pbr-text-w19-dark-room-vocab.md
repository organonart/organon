### PBR text — the dark room is on the CLI vocabulary, and the demo says ORGANON

A text demo is a near-black room with the words on it — never glyphs over the generated
landscape or the atmosphere sky. `faceplate` (organon#217 T3/T10) already says so, but
#240 measured that nine of its fields were off the `organon` vocabulary, so a CLI-driven
demo and the legibility gate's own render both showed the sky behind the grid. This puts
them on it — `atmos_enabled`, `bg_visible`, `fx_enabled`, `hal_amount`, `ml_enabled`,
`ml_intensity`, `ml_radius`, `ml_count`, `ml_restir` — at the seven sites #235 counted:
`ACTUATABLE_IDS`, `id_range`, `current`, `actuate` and the `param_desc` gloss in
`organon-agent`; the `engine_ranges` join in `cli.rs`; `console_catalog::slot_facts`; the
editor's apply-channel mirror in `lib.rs`. `organon set atmos_enabled 0 bg_visible 0
fx_enabled 1 hal_amount 0.35 ml_enabled 1 ml_intensity 1 ml_radius 2 ml_count 64
ml_restir 0` is now a sentence, `doc/reference/parameters.md` gained exactly nine rows from
`organon docs`, and `verify/legibility/faceplate.scene` sets all nine, so the gate scores the
dark room the preset draws rather than the tiles over a sunset.

📌 **Which packs the nine live in decided how they reach the catalog, and two of the five
were left out of the curated prompt on purpose.** `pack_manylight` is exactly the four
`ml_*` ids and `pack_restir` is `ml_restir` plus reserved slots, so both join
`agent::core_catalog` the way T3's packs did — they bring nothing the prompt has no route
for. `pack_atmosphere`, `pack_fx` and `pack_finishing` would have brought **28** ids between
them (`atmos_turbidity`, `fx_dof`, `lf_ghosts`, …) into the Performer's prompt and `organon
catalog` as "not directly settable", so `atmos_enabled`, `fx_enabled` and `hal_amount` reach
the catalog the way `mat_hue` and `bell_physical` always have — through the `ACTUATABLE_IDS`
union in `cli::catalog_entries`, with their blocks walked only in `slot_facts`'s
"outside the curated core" section. And `bg_visible` is in **no block at all**: a `u32`
scalar on `Shared` that `params.rs::to_shared` packs by hand, so it joins `scale_amp` and
`tempo` in `orphan_facts`, and its lane route thresholds (`v > 0.5`) the way the editor
mirror thresholds every flag rather than truncating — `as u32` would have read
`bg_visible 0.7` as off while the checkbox went on.

⚠️ **That union arm hard-coded `"num"` for every id outside the curated blocks, and it was
already wrong.** `bell_physical` — a `BoolParam` — shipped as `num 0 .. 1` in `organon
catalog` and in the published reference, and a test pinned the wart with "if the catalog
stopped calling bell_physical a number, say so here". Three of the nine flags and the
`IntParam` count would have gone the same way, so the arm now reads each outside id's kind
off `console_catalog::slot_facts`, the same walk the console's control facts use;
`kinds_match_the_slot_lists` pins the catalog agreeing with the facts, and
`bell_physical`'s row in `parameters.md` is the one existing row this change moves.

📌 **Pinned, and mutation-tested.** `dark_room_routes_agree_with_the_param_table_slot_lists`
(root crate) packs a marked `PresetValues` through the real `pack_*_preset` packers and
reads it back through the agent's routes, so a route on `fx[1]` fails naming `fx_enabled`;
`dark_room_ids_round_trip_through_distinct_shared_slots` (agent crate) checks the routes
are injective — ⚠️ against `Shared::default()`, not against zero, because these five blocks
default to real values (`atmosphere[0]` is 1, `manylight` is `[0, 1, 0.5, 24]`) and the T3
test's "count the non-zero slots" shape counted the defaults on first run — and that the
scalar thresholds. Dropping an id from `ACTUATABLE_IDS`, dropping a gloss, and a wrong range
each fail in the test #235 named for it. The editor mirror stays the one unpinned site.

🚨 **The demo says ORGANON.** `native/assets/text/organon.txt` — 9 lines × 82 columns of
`█` and blanks, a 5×7 pixel font at two cells per pixel so the 2:1 cell makes square
pixels, one blank row above and below — is the text a demo feeds `organon-glyphs --input`,
never the Omarchy logo. Its legibility fixture is
`organon-render/tests/fixtures/organon.txt` in the T2 format, and
`the_organon_fixture_is_the_demo_text_cell_for_cell` pins the two as one grid and pins
`emit_text(fixture)` equal to the asset minus its padding. `verify.sh --legibility` takes
`--text <file>` (or `LEG_TEXT`), feeding that file to the producer and judging against the
fixture of the same basename unless `--fixture` says otherwise, defaulting to the Omarchy
logo the shape check was written against. ⚠️ ttfx trims trailing blanks, so the file is
padded to 82 anyway and the gate pins `--cols`; `.gitattributes` pins
`native/assets/text/*.txt` to LF (the #225 lesson), and ⚠️ the Write tool of at least one
agent harness strips trailing spaces, so the two all-space rows came out empty on first
write — count widths, never trust the editor.

📌 **`organon preset <name>` was investigated and not built.** The apply channel is
`ApplyOp { Set, Generator, Surface, Material, Release }`, visual → editor, and the editor's
drain (`agent_apply_drain`) reaches only `params` and the `ParamSetter`; a recall by name
needs `enqueue_recall`, which also wants the `PresetUi` store, `apply_gen`, `hdr_gen`,
`model_gen` and the beat position, plus a scope (a name can exist as a Scene and as a tab
preset), plus a `CliOp::Preset` on the command channel and its parser in `ctl.rs`, plus a
line format that survives a name with a space. That is five files and well over the forty
lines the brief allowed, so the two-paragraph spec is in the PR and the verb stays absent;
`AgentAction::ApplyPreset` still records intent only.

🚨 **No GPU touched this**: green and ready to try. The loop a GPU session closes is
`organon set atmos_enabled 0 bg_visible 0 fx_enabled 1 hal_amount 0.35 ml_enabled 1
ml_intensity 1 ml_radius 2 ml_count 64 ml_restir 0` against a live editor and visual with
`organon-glyphs --input native/assets/text/organon.txt --cols 82 --rows 9` running — a
dark room with ORGANON in it, the editor's Environment / FX / Lights cards showing the same
numbers — and `verify.sh --legibility-only --text native/assets/text/organon.txt`, whose
thresholds in `thresholds.toml` were set before the gate's render was ever dark, and may
move once it is.
