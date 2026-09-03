//! The **host half** of the AI Performer — the two functions that could not descend.
//!
//! organon#49 T4c-i. The Performer's vocabulary, override lane, tool-call protocol and
//! localhost chat client are `organon-agent`, a crate carrying no nih-plug and, in
//! fact, nothing beyond `organon-core` + `serde`. This module re-exports all of it, so
//! **every `crate::agent::…` path in the tree resolves exactly as before**, and adds back
//! the two functions that read the plugin's own automation surface:
//!
//! | Here | Reads |
//! |---|---|
//! | [`core_catalog`] | `param_table::pack_*::catalog` — 12 references |
//! | [`scene_features`] | `preset::{PresetValues, PresetScope, EditorTab}` — 4 |
//!
//! plus the three private helpers (`cam_speed_word`, `hue_word`, `enum_name`) that
//! **nothing but `scene_features` calls**, which is why they travelled with it rather
//! than being made `pub` in the crate below to stay reachable.
//!
//! 📌 **This is the `HostFuncName` shape, for the fifth time in organon#49** (#626 T3
//! established it, Tiers 1/2/4a repeated it for fourteen enums): the semantic half lives
//! below the plugin, the host adapter lives here, and the two meet at one module path so
//! no caller has to know which side it is talking to.
//!
//! ⚠️ **Do not "simplify" this by moving `param_table` or `preset` down instead.** They
//! are 3 734 and 7 383 lines, `preset` names `param_table` 159 times, and `param_table`
//! names `agent` *back* — taking them would drag the plugin's entire automation surface
//! into the engine and turn a two-function seam into a cycle.
//!
//! ⚠️ **`world.rs` does not call [`core_catalog`] any more** — it receives a catalog
//! through `World::new`, because T4c-ii moves it into `organon-world`, which cannot see
//! this module. The three construction sites in this crate pass `core_catalog()`.

pub use organon_agent::*;

// The same four `params` names the module used before it split — `scene_features` names
// three of them and `enum_name` is generic over the fourth. All four resolve to
// `organon-core`, not to the plugin's `Host*` adapters (organon#49 T1/T2/T4a).
use crate::params::{GeneratorMode, IndexedEnum, MaterialType, SurfaceMode};

/// The curated core vocabulary for the prompt, generated from the `param_block!` slot
/// lists (see the `@catalog_struct` arm in `param_table.rs`). New params added to any of
/// these blocks appear automatically — there is no second hand-maintained list. Kept
/// focused (motion + look + camera) so the system prompt stays small; more blocks can be
/// added here without touching the params.
///
/// organon#49 T4c-i: this is the reason the module did not descend whole. `param_table`
/// is the plugin's automation surface and is not going anywhere.
pub fn core_catalog() -> Vec<CatSlot> {
    let mut out = Vec::new();
    crate::param_table::pack_loop_count::catalog(&mut out);
    crate::param_table::pack_rot_amp::catalog(&mut out);
    crate::param_table::pack_rot_mod::catalog(&mut out);
    crate::param_table::pack_trans_amp::catalog(&mut out);
    crate::param_table::pack_trans_mod::catalog(&mut out);
    crate::param_table::pack_lighting::catalog(&mut out);
    crate::param_table::pack_pbr::catalog(&mut out);
    crate::param_table::pack_surface_fx::catalog(&mut out);
    crate::param_table::pack_camera::catalog(&mut out);
    // organon#217 T3 — the PBR text look, its held camera and the capsule core. Look
    // and camera blocks, so they belong in the curated core; the tiles draw nothing
    // without a producer, but the ids must be settable before one is (a screensaver
    // launcher dresses the look first and starts the producer second).
    crate::param_table::pack_glyph::catalog(&mut out);
    crate::param_table::pack_glyph_cam::catalog(&mut out);
    crate::param_table::pack_capsule::catalog(&mut out);
    // organon#217 T13 / #240 — the glyph-lights: `pack_manylight` is exactly the four ids
    // `faceplate` sets, and `pack_restir` is `ml_restir` plus three reserved slots, so
    // neither brings an id the prompt has no route for. ⚠️ The dark room's other four are
    // NOT walked here on purpose: `atmos_enabled`, `fx_enabled` and `hal_amount` live in
    // `pack_atmosphere` / `pack_fx` / `pack_finishing`, which would put 28 ids with no
    // route (`atmos_turbidity`, `fx_dof`, `lf_ghosts`, …) into the Performer's prompt and
    // `organon catalog` as "not directly settable", and `bg_visible` has no block at all
    // (a hand-packed scalar, like `tempo`). They reach the catalog through `ACTUATABLE_IDS`
    // the way `mat_hue` and `bell_physical` do (`cli::catalog_entries`), with their kinds
    // read off the slot lists in `console_catalog::slot_facts`.
    crate::param_table::pack_manylight::catalog(&mut out);
    crate::param_table::pack_restir::catalog(&mut out);
    out
}

/// A rough camera-speed word for the fingerprint.
fn cam_speed_word(s: f32) -> &'static str {
    if s < 0.4 {
        "slow"
    } else if s > 1.2 {
        "fast"
    } else {
        "steady"
    }
}

/// Build a concise, **scope-aware** list of the distinguishing settings for a saved preset
/// — the fingerprint the model names from. For a **Scene** (`PresetScope::Global`) it spans
/// generator + surface form + material/palette/look + camera + environment; for a **tab**
/// preset it emits only that tab's salient features (a Look preset names for its
/// material/palette, a Motion preset for its camera, …). Reuses the same generator/surface/
/// material identities the Performer catalog uses. Pure + headless-testable.
pub fn scene_features(v: &crate::preset::PresetValues, scope: crate::preset::PresetScope) -> Vec<String> {
    use crate::params::{CamPath, Palette};
    use crate::preset::{EditorTab, PresetScope};

    // A field's tab is in this preset iff the scope is that tab, or the whole Scene (which
    // is exactly the four in-scene tabs — Audio/Synth/Settings are never part of a Scene).
    let want = |t: EditorTab| match scope {
        PresetScope::Global => matches!(
            t,
            EditorTab::Generator | EditorTab::Motion | EditorTab::Environment | EditorTab::Look
        ),
        PresetScope::Tab(tab) => tab == t,
    };

    let mut f: Vec<String> = Vec::new();

    // Generator + surface FORM (the geometry engine + how nodes are drawn).
    if want(EditorTab::Generator) {
        f.push(format!("Generator: {}", enum_name::<GeneratorMode>(v.generator)));
        f.push(format!("Surface form: {}", enum_name::<SurfaceMode>(v.surface_mode)));
    }

    // Material + palette + standout look (the Look tab).
    if want(EditorTab::Look) {
        f.push(format!("Material: {}", enum_name::<MaterialType>(v.mat_type)));
        f.push(format!("Palette: {}", enum_name::<Palette>(v.palette)));
        f.push(format!("Base colour: {} (hue {:.2})", hue_word(v.mat_hue), v.mat_hue));
        let mut fx: Vec<&str> = Vec::new();
        if v.iridescence > 0.02 {
            fx.push("iridescent");
        }
        if v.subsurface > 0.02 {
            fx.push("subsurface glow");
        }
        if v.glow > 0.6 {
            fx.push("emissive");
        }
        if v.metallic > 0.6 && v.roughness < 0.3 {
            fx.push("polished metal");
        }
        if v.mat_hue_cycle > 0.01 {
            fx.push("hue-cycling");
        }
        if v.plexus_overlay_on {
            fx.push("plexus web overlay");
        }
        if !fx.is_empty() {
            f.push(format!("Standout look: {}", fx.join(", ")));
        }
    }

    // Camera / motion (the Motion tab).
    if want(EditorTab::Motion) {
        if v.cam_path == 0 {
            f.push("Camera: static".to_string());
        } else {
            f.push(format!(
                "Camera: {} {} move",
                cam_speed_word(v.cam_speed),
                enum_name::<CamPath>(v.cam_path)
            ));
        }
    }

    // Environment / backdrop (the Environment tab).
    if want(EditorTab::Environment) {
        if !v.hdr_path.is_empty() {
            f.push("Environment: HDR image loaded".to_string());
        }
        f.push(
            if v.bg_visible {
                "Backdrop: environment visible"
            } else {
                "Backdrop: dark"
            }
            .to_string(),
        );
        if v.env_tint_amt > 0.02 {
            f.push("Environment tint applied".to_string());
        }
    }

    f
}

/// A rough colour word for a 0..1 hue, so a small model can reason about mood.
fn hue_word(h: f32) -> &'static str {
    match (h.rem_euclid(1.0) * 12.0).floor() as i32 {
        0 => "red",
        1 => "orange",
        2 => "amber",
        3 => "yellow-green",
        4 => "green",
        5 => "teal",
        6 => "cyan",
        7 => "azure",
        8 => "blue",
        9 => "violet",
        10 => "magenta",
        _ => "rose",
    }
}

/// Display name of an ordinal, clamped to the last variant.
///
/// organon#49 T2: over [`IndexedEnum`] rather than nih-plug's `Enum`. The clamp is kept
/// rather than replaced by `from_index`'s fallback, because the two differ and the
/// difference is visible here: `from_index` sends an out-of-range value to the type's
/// *first* variant, which would print "Original" for a corrupt ordinal. Clamping to the
/// last variant keeps an out-of-range value looking out-of-range in the prompt.
fn enum_name<T: IndexedEnum>(i: u32) -> &'static str {
    let all = T::all();
    match all.len() {
        0 => "",
        n => all[(i as usize).min(n - 1)].label(),
    }
}


// ===========================================================================
// Tests — the ones that need the plugin's params, and only those
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_generated_from_param_table() {
        let cat = core_catalog();
        // A param that lives in a core block must appear mechanically.
        assert!(cat.iter().any(|c| c.id == "loop_count_x" && c.kind == SlotKind::Int));
        assert!(cat.iter().any(|c| c.id == "glow" && c.kind == SlotKind::Num));
        assert!(cat.iter().any(|c| c.id == "metallic"));
        assert!(cat.iter().any(|c| c.id == "iridescence"));
        // Enum + reserved/expr slots: mat_type is an enum in the lighting block; the
        // inc_scale expr slot in rot_mod contributes no vocab (no field name).
        assert!(cat.iter().any(|c| c.id == "mat_type" && c.kind == SlotKind::Enum));
        assert!(!cat.iter().any(|c| c.id == "inc_scale"));
        // The prompt lists actuatable ranges.
        let prompt = catalog_prompt(&cat);
        assert!(prompt.contains("glow : 0 .. 2"));
    }


    #[test]
    fn scene_features_are_scope_aware_and_clamp() {
        use crate::params::OrganicMathParams;
        use crate::preset::{EditorTab, PresetScope, PresetValues};
        let mut v = PresetValues::capture_params_only(&OrganicMathParams::default());
        v.generator = GeneratorMode::Frenet as u32;
        v.surface_mode = SurfaceMode::Metaball as u32;
        v.mat_type = MaterialType::Chrome as u32;
        v.cam_path = 1; // a camera move
        v.bg_visible = true;

        // Scene = the full fingerprint: generator + surface + material + camera + backdrop.
        let scene = scene_features(&v, PresetScope::Global);
        let joined = scene.join(" | ");
        assert!(joined.contains("Frenet"), "generator in scene: {joined}");
        assert!(joined.contains("Metaball"), "surface in scene");
        assert!(joined.contains("Chrome"), "material in scene");
        assert!(scene.iter().any(|l| l.starts_with("Camera")), "camera in scene");
        assert!(scene.iter().any(|l| l.starts_with("Backdrop")), "backdrop in scene");

        // A Look-tab preset names only from its own fields (no generator/camera line).
        let look = scene_features(&v, PresetScope::Tab(EditorTab::Look));
        assert!(look.iter().any(|l| l.contains("Chrome")), "material in Look");
        assert!(!look.iter().any(|l| l.starts_with("Generator")), "no generator in Look");
        assert!(!look.iter().any(|l| l.starts_with("Camera")), "no camera in Look");

        // Out-of-range ordinals clamp instead of panicking (via enum_name).
        v.generator = 9999;
        v.surface_mode = 9999;
        let _ = scene_features(&v, PresetScope::Global); // no panic == pass
    }

    /// The reverse direction of `organon-agent`'s `actuatable_ids_is_exactly_the_id_range_set`:
    /// every catalog id that HAS a range must be listed in `ACTUATABLE_IDS`, so a new
    /// actuation route stays CLI-visible.
    ///
    /// ⚠️ organon#49 T4c-i split this off because it is the only half that needs
    /// `core_catalog()`, which reads `param_table` and could not descend. The forward
    /// direction (every listed id has a range and a read route, and none repeat) stays in
    /// the crate, where both `ACTUATABLE_IDS` and `id_range` live. Two assertions, two
    /// reasons to fail, two homes — the same split T3 made for `substrate_materials`.
    /// organon#217 T3: the hand-written slot indices in `organon-agent`'s `current` /
    /// `actuate` name the same slots as `param_table.rs`'s `pack_glyph` / `pack_glyph_cam`
    /// / `pack_capsule` lists. The preset packers are the slot lists made executable, so a
    /// `PresetValues` with a distinct marker in every T3 field, packed, must read back
    /// field-for-field through the agent's routes — a swapped index would hand `organon
    /// set glyph_gap` to the gain, silently. (The crate below cannot see the lists, so its
    /// own test checks only that the routes are injective.)
    #[test]
    fn t3_routes_agree_with_the_param_table_slot_lists() {
        use crate::ipc::Shared;
        use crate::params::OrganicMathParams;
        use crate::preset::PresetValues;
        let mut pv = PresetValues::capture_params_only(&OrganicMathParams::default());
        pv.glyph_cell_w = 0.11;
        pv.glyph_depth = 0.12;
        pv.glyph_gap = 0.13;
        pv.glyph_gain = 0.14;
        pv.glyph_faceplate = 0.15;
        pv.glyph_back_r = 0.16;
        pv.glyph_back_g = 0.17;
        pv.glyph_back_b = 0.18;
        pv.glyph_margin = 0.19;
        pv.glyph_back_depth = 0.21;
        pv.glyph_default_fg = 0.22;
        pv.glyph_bevel = 0.23;
        pv.glyph_crown = 0.24;
        // organon#217 T9 — slots 13 and 14. The flag packs as 1.0, so its marker IS 1.0.
        pv.glyph_profile = 0.31;
        pv.glyph_dark_tiles = true;
        pv.glyph_cam_hold = true;
        pv.glyph_cam_tilt = 0.26;
        pv.glyph_cam_zoom = 0.27;
        pv.capsule_core = 0.28;
        pv.capsule_absorb = 0.29;

        let mut s = Shared::default();
        s.glyph = crate::param_table::pack_glyph_preset(&pv);
        s.glyph_cam = crate::param_table::pack_glyph_cam_preset(&pv);
        s.capsule = crate::param_table::pack_capsule_preset(&pv);

        let want: [(&str, f32); 20] = [
            ("glyph_cell_w", 0.11), ("glyph_depth", 0.12), ("glyph_gap", 0.13),
            ("glyph_gain", 0.14), ("glyph_faceplate", 0.15), ("glyph_back_r", 0.16),
            ("glyph_back_g", 0.17), ("glyph_back_b", 0.18), ("glyph_margin", 0.19),
            ("glyph_back_depth", 0.21), ("glyph_default_fg", 0.22), ("glyph_bevel", 0.23),
            ("glyph_crown", 0.24), ("glyph_profile", 0.31), ("glyph_dark_tiles", 1.0),
            ("glyph_cam_hold", 1.0), ("glyph_cam_tilt", 0.26),
            ("glyph_cam_zoom", 0.27), ("capsule_core", 0.28), ("capsule_absorb", 0.29),
        ];
        // The two T9 slots are exactly where the table says; slot 15 (T12's proposed
        // motion lane) is not marked here, so it packs at its inert default.
        assert_eq!(s.glyph[13], 0.31, "glyph_profile packs to slot 13");
        assert_eq!(s.glyph[14], 1.0, "glyph_dark_tiles packs to slot 14 as a 0/1 flag");
        assert_eq!(s.glyph[15], 0.0, "slot 15 is not one of these two and packs 0 unmarked");
        for (id, v) in want {
            assert_eq!(
                current(&s, id),
                Some(v),
                "{id}: the agent's read route names a different slot than param_table packs"
            );
            let mut w = Shared::default();
            assert!(actuate(&mut w, id, v), "{id} has no write route");
            assert_eq!(
                current(&w, id),
                Some(v),
                "{id}: write and read routes disagree"
            );
        }
    }

    /// organon#217 T13 / #240 — the same pin for the nine dark-room ids: the agent's
    /// hand-written routes name the slots `param_table.rs`'s `pack_atmosphere` / `pack_fx`
    /// / `pack_finishing` / `pack_manylight` / `pack_restir` lists pack, and `bg_visible`
    /// reads the hand-packed `u32` scalar. A `PresetValues` with a distinct marker in every
    /// field, packed through the real preset packers, must read back field-for-field —
    /// a route on `fx[1]` would hand `organon set fx_enabled` to the style selector.
    #[test]
    fn dark_room_routes_agree_with_the_param_table_slot_lists() {
        use crate::ipc::Shared;
        use crate::params::OrganicMathParams;
        use crate::preset::PresetValues;
        let mut pv = PresetValues::capture_params_only(&OrganicMathParams::default());
        pv.atmos_enabled = true;
        pv.bg_visible = true;
        pv.fx_enabled = true;
        pv.hal_amount = 0.35;
        pv.ml_enabled = true;
        pv.ml_intensity = 0.41;
        pv.ml_radius = 0.52;
        pv.ml_count = 7;
        pv.ml_restir = true;
        // The neighbours of each slot 0 carry their own defaults so a route off by one
        // reads a real value, not a coincidental zero: turbidity, fx_style, hal_threshold.

        let mut s = Shared::default();
        s.atmosphere = crate::param_table::pack_atmosphere_preset(&pv);
        s.fx = crate::param_table::pack_fx_preset(&pv);
        s.finishing = crate::param_table::pack_finishing_preset(&pv);
        s.manylight = crate::param_table::pack_manylight_preset(&pv);
        s.restir = crate::param_table::pack_restir_preset(&pv);
        // `bg_visible` has no block: `params.rs::to_shared` writes `self.bg_visible.value()
        // as u32`, and this is that line for a preset.
        s.bg_visible = pv.bg_visible as u32;

        let want: [(&str, f32); 9] = [
            ("atmos_enabled", 1.0), ("bg_visible", 1.0), ("fx_enabled", 1.0),
            ("hal_amount", 0.35), ("ml_enabled", 1.0), ("ml_intensity", 0.41),
            ("ml_radius", 0.52), ("ml_count", 7.0), ("ml_restir", 1.0),
        ];
        assert_eq!(s.manylight, [1.0, 0.41, 0.52, 7.0], "pack_manylight is exactly the four");
        assert_eq!(s.restir[0], 1.0, "ml_restir packs to restir[0]");
        for (id, v) in want {
            assert_eq!(
                current(&s, id),
                Some(v),
                "{id}: the agent's read route names a different slot than param_table packs"
            );
            let mut w = Shared::default();
            assert!(actuate(&mut w, id, v), "{id} has no write route");
            assert_eq!(current(&w, id), Some(v), "{id}: write and read routes disagree");
        }
        // The catalog walks the two blocks that bring nothing extra, and only those.
        let cat = core_catalog();
        for id in ["ml_enabled", "ml_intensity", "ml_radius", "ml_count", "ml_restir"] {
            assert!(cat.iter().any(|c| c.id == id), "{id} is in the curated catalog");
        }
        assert!(cat.iter().any(|c| c.id == "ml_restir" && c.kind == SlotKind::Flag));
        assert!(cat.iter().any(|c| c.id == "ml_count" && c.kind == SlotKind::Int));
        for id in ["atmos_turbidity", "fx_dof", "lf_ghosts", "hal_width"] {
            assert!(!cat.iter().any(|c| c.id == id), "{id} must not reach the prompt");
        }
    }

    #[test]
    fn catalog_ids_with_a_range_are_all_actuatable() {
        for c in core_catalog() {
            if id_range(c.id).is_some() {
                assert!(
                    ACTUATABLE_IDS.contains(&c.id),
                    "{} has a range but is not in ACTUATABLE_IDS",
                    c.id
                );
            }
        }
    }
}
