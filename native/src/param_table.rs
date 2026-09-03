//! Single-source-of-truth param packing (GitHub #103, infra + pilot).
//!
//! Every logical param is hand-maintained in ~7 places today; the most fragile
//! is the **indexed `[f32; N]` packing** into the `Shared` IPC snapshot — done
//! by hand in *two* `to_shared()`s (`params.rs` and `preset.rs`), where a wrong
//! or reordered slot is a silent corruption risk.
//!
//! `param_block!` collapses that: one ordered slot list per `Shared` array block
//! generates *both* packers (params → `Shared` and `PresetValues` → `Shared`), so
//! the slot layout lives in exactly one place. The two packers can no longer drift
//! from each other, and a field renamed out from under the table becomes a
//! **compile error**, not a silent zero.
//!
//! This is the pilot (#103 step 1): the `bell` block is migrated here and proven
//! byte-identical (see the tests). The remaining blocks migrate one at a time
//! behind the layout-golden + round-trip tests below. (Generating the struct
//! fields / `capture` / `apply` is a later increment — see the issue.)

use crate::params::OrganicMathParams;
use crate::params::TerrainRes; // #354: the terrain-res divisor expr's preset variant
use crate::preset::PresetValues;

/// Generate the two `Shared`-array packers for one param block from a single
/// ordered slot list. Each slot is one of:
/// - `_`                      → a reserved `0.0` slot
/// - `(f32, ident)`           → `FloatParam` / `f32`
/// - `(i32, ident)`           → `IntParam` / `i32`
/// - `(bool, ident)`          → `BoolParam` / `bool`
/// - `(enum, ident)`          → `EnumParam<T>` / `u32` (via `to_u32`)
/// - `(lit, value)`           → a fixed non-zero literal in *both* packers
/// - `(expr, {p_expr}, {pv_expr})` → an arbitrary computed slot (the binders are
///   `p` for the param packer and `pv` for the preset packer); use the 2-arg
///   `(expr, {p_expr})` form in a param-only block
///
/// The param-side field (`OrganicMathParams`) and the preset-side field
/// (`PresetValues`) are assumed to share the same identifier (the project's
/// convention), so one name drives both packers. A symmetric block declares both
/// packer names; a **param-only** block (e.g. `terrain`/`stars`, which presets
/// don't capture) declares just the one.
macro_rules! param_block {
    // symmetric: both packers
    (
        $packer:ident, $preset_packer:ident, [$n:expr];
        $( $slot:tt ),* $(,)?
    ) => {
        /// Pack this block's params (live `OrganicMathParams`) into its `Shared` slots.
        pub(crate) fn $packer(p: &OrganicMathParams) -> [f32; $n] {
            let _ = p; // silence unused when a block is all-reserved
            [ $( param_block!(@from_param p, $slot) ),* ]
        }
        /// Pack this block's params (serialized `PresetValues`) into its `Shared` slots.
        pub(crate) fn $preset_packer(pv: &PresetValues) -> [f32; $n] {
            let _ = pv;
            [ $( param_block!(@from_preset pv, $slot) ),* ]
        }
        param_block!(@catalog_struct $packer; $( $slot ),*);
    };

    // param-only: presets don't capture this block (e.g. terrain/stars)
    (
        $packer:ident, [$n:expr];
        $( $slot:tt ),* $(,)?
    ) => {
        pub(crate) fn $packer(p: &OrganicMathParams) -> [f32; $n] {
            let _ = p;
            [ $( param_block!(@from_param p, $slot) ),* ]
        }
        param_block!(@catalog_struct $packer; $( $slot ),*);
    };

    // --- AI-agent action catalog (#317): a read-only, macro-generated vocabulary
    // walked from the SAME slot list that drives the packers, so a param added to a
    // block automatically appears in the agent's vocabulary — there is no second
    // hand-maintained list. Emits a braced marker type (type namespace only, so it
    // never collides with the same-named packer fn in the value namespace) carrying a
    // `catalog()` walker. The prompt-side aggregator (`core_catalog`) calls it for the
    // curated core blocks. `_` / `(lit, …)` / `(expr, …)` slots contribute no vocab.
    (@catalog_struct $packer:ident; $( $slot:tt ),* $(,)?) => {
        #[allow(non_camel_case_types, dead_code)]
        pub(crate) struct $packer {}
        impl $packer {
            pub(crate) fn catalog(out: &mut ::std::vec::Vec<crate::agent::CatSlot>) {
                $( param_block!(@cat out, $slot); )*
            }

            /// Walk this block's slots for the **console control facts** (#4 Tier 3) —
            /// the same slot list again, read through the *live param objects* rather
            /// than through their names alone.
            ///
            /// This is what makes a control descriptor generated rather than
            /// hand-written (`doc/console_discover_schema.md` I2). Each arm touches
            /// `p.<field>` at its declared type, exactly as `@from_param` does, so a
            /// renamed field or a retyped param is a **build error** and never a
            /// descriptor that quietly disagrees with the engine. Ranges, defaults and
            /// labels are read off the parameter itself in `console_catalog`; nothing is
            /// restated here. `_` / `(lit, …)` / `(expr, …)` slots contribute no facts,
            /// for the same reason they contribute no vocabulary.
            #[allow(dead_code)]
            pub(crate) fn facts(
                p: &OrganicMathParams,
                out: &mut ::std::vec::Vec<crate::console_catalog::SlotFacts>,
            ) {
                let _ = p; // silence unused when a block is all-reserved
                let _ = &out;
                $( param_block!(@facts out, p, $slot); )*
            }
        }
    };
    (@cat $out:ident, _) => {};
    (@cat $out:ident, (f32, $f:ident)) => { $out.push(crate::agent::CatSlot::num(stringify!($f))); };
    (@cat $out:ident, (i32, $f:ident)) => { $out.push(crate::agent::CatSlot::int(stringify!($f))); };
    (@cat $out:ident, (bool, $f:ident)) => { $out.push(crate::agent::CatSlot::flag(stringify!($f))); };
    (@cat $out:ident, (enum, $f:ident)) => { $out.push(crate::agent::CatSlot::enm(stringify!($f))); };
    (@cat $out:ident, (lit, $v:expr)) => {};
    (@cat $out:ident, (expr, |$a:ident| $e:expr)) => {};
    (@cat $out:ident, (expr, |$a:ident| $e:expr, |$b:ident| $e2:expr)) => {};

    // --- console control facts (#4 Tier 3): the slot list walked at the DECLARED TYPE.
    // Same shape as `@from_param` on purpose — the type annotation is the check.
    (@facts $out:ident, $p:ident, _) => {};
    (@facts $out:ident, $p:ident, (f32, $f:ident)) => {
        $out.push(crate::console_catalog::float_facts(stringify!($f), &$p.$f));
    };
    (@facts $out:ident, $p:ident, (i32, $f:ident)) => {
        $out.push(crate::console_catalog::int_facts(stringify!($f), &$p.$f));
    };
    (@facts $out:ident, $p:ident, (bool, $f:ident)) => {
        $out.push(crate::console_catalog::bool_facts(stringify!($f), &$p.$f));
    };
    (@facts $out:ident, $p:ident, (enum, $f:ident)) => {
        $out.push(crate::console_catalog::enum_facts(stringify!($f), &$p.$f));
    };
    (@facts $out:ident, $p:ident, (lit, $v:expr)) => {};
    (@facts $out:ident, $p:ident, (expr, |$a:ident| $e:expr)) => {};
    (@facts $out:ident, $p:ident, (expr, |$a:ident| $e:expr, |$b:ident| $e2:expr)) => {};

    // --- per-slot extraction (live params) ---
    (@from_param $p:ident, _) => { 0.0 };
    (@from_param $p:ident, (f32, $f:ident)) => { $p.$f.value() };
    (@from_param $p:ident, (i32, $f:ident)) => { $p.$f.value() as f32 };
    (@from_param $p:ident, (bool, $f:ident)) => { $p.$f.value() as u32 as f32 };
    (@from_param $p:ident, (enum, $f:ident)) => { $p.$f.value().to_u32() as f32 };
    (@from_param $p:ident, (lit, $v:expr)) => { $v };
    // `(expr, |binder| ...)` binds the caller's name to the packer's argument so
    // the expression resolves under macro hygiene. 1-arg = param-only block.
    (@from_param $p:ident, (expr, |$a:ident| $e:expr)) => {{ let $a = $p; $e }};
    (@from_param $p:ident, (expr, |$a:ident| $e:expr, |$b:ident| $e2:expr)) => {{
        let $a = $p;
        $e
    }};

    // --- per-slot extraction (preset mirror) ---
    (@from_preset $pv:ident, _) => { 0.0 };
    (@from_preset $pv:ident, (f32, $f:ident)) => { $pv.$f };
    (@from_preset $pv:ident, (i32, $f:ident)) => { $pv.$f as f32 };
    (@from_preset $pv:ident, (bool, $f:ident)) => { $pv.$f as u32 as f32 };
    (@from_preset $pv:ident, (enum, $f:ident)) => { $pv.$f as f32 };
    (@from_preset $pv:ident, (lit, $v:expr)) => { $v };
    (@from_preset $pv:ident, (expr, |$a:ident| $e:expr, |$b:ident| $e2:expr)) => {{
        let $b = $pv;
        $e2
    }};
}

// ===========================================================================
// Migrated blocks
// ===========================================================================

// Soft-body bell (#99): `Shared.bell[8]` = [physical, stroke_depth, stiffness,
// damping, openness, stroke_rate, _, _]. Pilot block for #103 (PR 1).
param_block! {
    pack_bell, pack_bell_preset, [8];
    (bool, bell_physical),
    (f32, bell_stroke_depth),
    (i32, bell_stiffness),
    (f32, bell_damping),
    (f32, bell_open),
    (f32, bell_speed),
    _,
    _,
}

// ---------------------------------------------------------------------------
// PR 2 — look / render blocks (symmetric: captured by presets too).
// ---------------------------------------------------------------------------

param_block! {
    pack_loop_count, pack_loop_count_preset, [4];
    (i32, loop_count_x), (i32, loop_count_y), (i32, loop_count_z), (i32, loop_count_q),
}

// w = continuous-rotation flag.
param_block! {
    pack_rot_amp, pack_rot_amp_preset, [4];
    (f32, rot_amp_x), (f32, rot_amp_y), (f32, rot_amp_z), (bool, continuous),
}

// w = effective global speed (inc_scale dial × 10^speed_exp).
param_block! {
    pack_rot_mod, pack_rot_mod_preset, [4];
    (f32, rot_mod_x),
    (f32, rot_mod_y),
    (f32, rot_mod_z),
    (expr, |p| p.inc_scale.value() * 10f32.powi(p.speed_exp.value()),
           |pv| pv.inc_scale * 10f32.powi(pv.speed_exp)),
}

param_block! {
    pack_trans_amp, pack_trans_amp_preset, [4];
    (f32, trans_amp_x), (f32, trans_amp_y), (f32, trans_amp_z), _,
}

param_block! {
    pack_trans_mod, pack_trans_mod_preset, [4];
    (f32, trans_mod_x), (f32, trans_mod_y), (f32, trans_mod_z), _,
}

// [7] = material type (spare slot).
param_block! {
    pack_lighting, pack_lighting_preset, [8];
    (f32, ambient),
    (f32, key_intensity),
    (f32, fill_intensity),
    (f32, elevation),
    (f32, azimuth),
    (f32, glow),
    (f32, opacity),
    (enum, mat_type),
}

// [7] = glass IOR (spare slot).
param_block! {
    pack_pbr, pack_pbr_preset, [8];
    (f32, metallic),
    (f32, roughness),
    (f32, exposure),
    (f32, env_intensity),
    (f32, env_rotation),
    (f32, bloom_intensity),
    (f32, bloom_threshold),
    (f32, ior),
}

param_block! {
    pack_camera, pack_camera_preset, [4];
    (enum, cam_path), (f32, cam_speed), (f32, cam_kick), (f32, cam_damping),
}

// Camera shot sequencer (#307 Tier 1): [enabled, bars_per_shot, order, transition].
param_block! {
    pack_cam_seq, pack_cam_seq_preset, [4];
    (bool, cam_seq_enabled),
    (expr, |p| p.cam_bars_per_shot.value().bars(),
           |pv| crate::params::BarPeriod::from_u32(pv.cam_bars_per_shot).bars()),
    (enum, cam_seq_order),
    (enum, cam_transition),
}

// Decoupled dolly (#307 Tier 1): [period_bars, depth, wave, beats_per_bar]. The
// beats-per-bar shares this block so the visual has the bar clock in one place.
param_block! {
    pack_cam_dolly, pack_cam_dolly_preset, [4];
    (f32, cam_dolly_period),
    (f32, cam_dolly_depth),
    (enum, cam_dolly_wave),
    (i32, beats_per_bar),
}

// Beat/bar clock feel (#307 Tier 1): [tempo_source, beat_momentum, transition_bars, _].
param_block! {
    pack_cam_clock, pack_cam_clock_preset, [4];
    (enum, tempo_source),
    (bool, cam_beat_momentum),
    (f32, cam_transition_bars),
    _,
}

// Camera framing axes + Tier 2 sequencer richness (#307 Tier 2):
// [roll_deg, fov_deg, fov_dolly, hold_prob, phrase_lock, seq_mix, _, _].
param_block! {
    pack_cam_frame, pack_cam_frame_preset, [8];
    (f32, cam_roll),
    (f32, cam_fov),
    (f32, cam_fov_dolly),
    (f32, cam_hold_prob),
    (bool, cam_phrase_lock),
    (f32, cam_seq_mix),
    _,
    _,
}

// Camera storyboard (#307 Tier 3): a header + 4 shot slots.
// Header[0..8] = [enabled, count, mode, seed, next_gen(process-filled), _, _, _];
// each shot slot (8 + k*4) = [path, bars, radius, _].
param_block! {
    pack_cam_story, pack_cam_story_preset, [24];
    (bool, cam_story_enabled),
    (i32, cam_story_count),
    (enum, cam_story_mode),
    (i32, cam_story_seed),
    _, // [4] next-shot trigger (filled by process())
    _,
    _,
    _,
    // shot 0
    (enum, cam_shot0_path),
    (expr, |p| p.cam_shot0_bars.value().bars(),
           |pv| crate::params::BarPeriod::from_u32(pv.cam_shot0_bars).bars()),
    (f32, cam_shot0_radius),
    _,
    // shot 1
    (enum, cam_shot1_path),
    (expr, |p| p.cam_shot1_bars.value().bars(),
           |pv| crate::params::BarPeriod::from_u32(pv.cam_shot1_bars).bars()),
    (f32, cam_shot1_radius),
    _,
    // shot 2
    (enum, cam_shot2_path),
    (expr, |p| p.cam_shot2_bars.value().bars(),
           |pv| crate::params::BarPeriod::from_u32(pv.cam_shot2_bars).bars()),
    (f32, cam_shot2_radius),
    _,
    // shot 3
    (enum, cam_shot3_path),
    (expr, |p| p.cam_shot3_bars.value().bars(),
           |pv| crate::params::BarPeriod::from_u32(pv.cam_shot3_bars).bars()),
    (f32, cam_shot3_radius),
    _,
}

param_block! {
    pack_routing, pack_routing_preset, [4];
    (enum, mod_a_target), (f32, mod_a_depth), (enum, mod_b_target), (f32, mod_b_depth),
}

// [6] = palette id (spare slot).
param_block! {
    pack_surface_fx, pack_surface_fx_preset, [8];
    (f32, subsurface),
    (f32, sss_distortion),
    (f32, sss_power),
    (f32, iridescence),
    (f32, irid_scale),
    (f32, irid_shift),
    (enum, palette),
    _,
}

param_block! {
    pack_ssao, pack_ssao_preset, [4];
    (bool, ssao), (f32, ssao_radius), (f32, ssao_intensity), (f32, ssao_bias),
}

param_block! {
    pack_speed_pulse, pack_speed_pulse_preset, [4];
    (f32, speed_pulse_amount), (f32, speed_pulse_attack), (f32, speed_pulse_decay), _,
}

param_block! {
    pack_metaball, pack_metaball_preset, [4];
    (f32, metaball_radius), (f32, metaball_threshold), (f32, metaball_smooth), _,
}

// Contiguous (welded) Swept Tubes: [weld, end_cap, cap_round, cap_bevel].
param_block! {
    pack_tube, pack_tube_preset, [4];
    (bool, tube_weld), (bool, tube_end_cap), (f32, tube_cap_round), (f32, tube_cap_bevel),
}

// [3] reserved (was fill mode); [9..11] reserved (trails follow-up).
param_block! {
    pack_voxel, pack_voxel_preset, [12];
    (f32, voxel_res),
    (f32, voxel_threshold),
    (f32, voxel_radius),
    _,
    (f32, voxel_emission),
    (f32, voxel_ao),
    (f32, voxel_shadow),
    (f32, voxel_quantize),
    (f32, voxel_beat),
    _, _, _,
}

param_block! {
    pack_voxel_gi, pack_voxel_gi_preset, [4];
    (bool, voxel_gi), (f32, voxel_gi_strength), (f32, voxel_gi_distance), (f32, voxel_gi_sky),
}

param_block! {
    pack_bio, pack_bio_preset, [8];
    (f32, color_cycle),
    (f32, ripple_intensity),
    (f32, ripple_speed),
    (f32, ripple_freq),
    (f32, ripple_sharp),
    (enum, ripple_geom),
    _, _,
}

param_block! {
    pack_membrane, pack_membrane_preset, [4];
    (enum, membrane_weave), (bool, membrane_show_strands),
    (bool, membrane_arms), (bool, membrane_close),
}

param_block! {
    pack_rd, pack_rd_preset, [8];
    (f32, rd_feed),
    (f32, rd_kill),
    (f32, rd_scale),
    (f32, rd_intensity),
    (f32, rd_albedo_mix),
    _, _, _,
}

param_block! {
    pack_breath, pack_breath_preset, [4];
    (f32, breath_amount), (f32, breath_attack), (f32, breath_decay), _,
}

param_block! {
    pack_particles, pack_particles_preset, [16];
    (enum, particles_tier),
    (i32, particles_count_k),
    (i32, particles_grid_res),
    (f32, particles_speed),
    (f32, particles_lifetime),
    (f32, particles_spawn_radius),
    (f32, particles_size),
    (f32, particles_emissive),
    (bool, particles_ribbon),
    (f32, particles_ribbon_stretch),
    (f32, particles_hue_shift),
    (f32, particles_beat_burst),
    (f32, particles_drag),
    (f32, particles_turbulence),
    (f32, particles_alpha),
    (bool, particles_hide_generator),
}

param_block! {
    pack_fluid, pack_fluid_preset, [8];
    (f32, fluid_force),
    (f32, fluid_vorticity),
    (f32, fluid_dissipation),
    (i32, fluid_iters),
    (f32, fluid_inflow_decay),
    _, _, _,
}

// [5] = stride (reserved perf dial; fixed at 2.0 for now).
param_block! {
    pack_ssr, pack_ssr_preset, [8];
    (bool, ssr),
    (f32, ssr_intensity),
    (f32, ssr_max_roughness),
    (f32, ssr_thickness),
    (i32, ssr_steps),
    (lit, 2.0),
    _, _,
}

param_block! {
    pack_gi, pack_gi_preset, [4];
    (bool, gi), (f32, gi_intensity), (f32, gi_falloff), _,
}

// [3] = spectral samples (reserved; the shader uses 3-tap RGB).
param_block! {
    pack_glass_spec, pack_glass_spec_preset, [4];
    (f32, glass_dispersion), (f32, glass_caustic), (f32, glass_thin_film), (lit, 3.0),
}

// ---------------------------------------------------------------------------
// PR 2 — param-only blocks (presets don't capture these; `..Default` covers them).
// ---------------------------------------------------------------------------

// [16] = render divisor (TerrainRes::divisor()); [27..31] reserved.
param_block! {
    pack_terrain, pack_terrain_preset, [32];
    (bool, terrain_enabled),
    (f32, terrain_height),
    (f32, terrain_snow),
    (f32, terrain_fog),
    (f32, terrain_sun_elev),
    (f32, terrain_sun_azim),
    (f32, terrain_sun_int),
    (f32, terrain_scroll),
    (f32, terrain_ride),
    (enum, terrain_noise),
    (i32, terrain_seed),
    (bool, terrain_ridged),
    (f32, terrain_brightness),
    (f32, terrain_haze),
    (i32, terrain_steps),
    (i32, terrain_octaves),
    (expr, |p| p.terrain_res.value().divisor() as f32, |pv| TerrainRes::from_u32(pv.terrain_res).divisor() as f32),
    (enum, terrain_palette),
    (f32, terrain_emissive),
    (f32, terrain_day_speed),
    (bool, terrain_water),
    (f32, terrain_water_level),
    (f32, terrain_water_hue),
    (f32, terrain_water_ripple),
    (f32, terrain_scatter),
    (f32, terrain_godray),
    (bool, terrain_sun_scene),
    _, _, _, _, _,
}

param_block! {
    pack_stars, pack_stars_preset, [16];
    (bool, stars_enabled),
    (f32, stars_brightness),
    (f32, stars_twinkle),
    (f32, stars_twinkle_speed),
    (f32, stars_size),
    (f32, stars_latitude),
    (f32, stars_sky_speed),
    (f32, stars_mag_limit),
    (f32, stars_saturation),
    (bool, stars_sun),
    (f32, stars_sun_bright),
    (f32, stars_sun_size),
    (f32, stars_sun_warmth),
    _, _, _,
}

// Physically based atmosphere (#100): `Shared.atmosphere[8]` = [enabled, turbidity,
// mie_g, sun_intensity, ground_albedo, exposure, aerial_strength, rayleigh]. A
// global world layer — param-only (presets don't capture it, like terrain/stars).
param_block! {
    pack_atmosphere, pack_atmosphere_preset, [8];
    (bool, atmos_enabled),
    (f32, atmos_turbidity),
    (f32, atmos_mie_g),
    (f32, atmos_sun_int),
    (f32, atmos_ground_albedo),
    (f32, atmos_exposure),
    (f32, atmos_aerial),
    (f32, atmos_rayleigh),
}

// Volumetric clouds (#102, Part A): `Shared.clouds[12]`. Param-only (a global world
// layer; not preset-captured, like terrain/stars/atmosphere).
param_block! {
    pack_clouds, pack_clouds_preset, [12];
    (bool, clouds_enabled),
    (f32, clouds_coverage),
    (f32, clouds_density),
    (f32, clouds_base),
    (f32, clouds_thickness),
    (i32, clouds_steps),
    (f32, clouds_detail),
    (f32, clouds_drift),
    (f32, clouds_hg),
    (f32, clouds_absorption),
    (f32, clouds_shadow),
    (f32, clouds_ambient),
}

// FFT (Tessendorf) ocean (#102, Part B): `Shared.ocean[12]`. Param-only (a global
// world layer; not preset-captured).
param_block! {
    pack_ocean, pack_ocean_preset, [12];
    (bool, ocean_enabled),
    (f32, ocean_level),
    (f32, ocean_wind_speed),
    (f32, ocean_wind_dir),
    (f32, ocean_amplitude),
    (f32, ocean_choppiness),
    (f32, ocean_tile_size),
    (f32, ocean_foam),
    (f32, ocean_glitter),
    (f32, ocean_hue),
    (f32, ocean_depth),
    _,
}

// ---------------------------------------------------------------------------
// PR 3 — generator blocks (symmetric: captured by presets).
// ---------------------------------------------------------------------------

param_block! {
    pack_frenet, pack_frenet_preset, [12];
    (i32, frenet_strands),
    (i32, frenet_nodes),
    (f32, frenet_step),
    (f32, frenet_kappa),
    (f32, frenet_kappa_amp),
    (f32, frenet_kappa_freq),
    (f32, frenet_tau),
    (f32, frenet_tau_amp),
    (f32, frenet_tau_freq),
    (f32, frenet_spread),
    (f32, frenet_thickness),
    (enum, frenet_func),
}

param_block! {
    pack_dna, pack_dna_preset, [16];
    (enum, dna_form),
    (i32, dna_bp),
    (f32, dna_bp_per_turn),
    (f32, dna_rise),
    (f32, dna_radius),
    (f32, dna_groove),
    (bool, dna_left),
    (f32, dna_sigma),
    (f32, dna_super_radius),
    (i32, dna_seed),
    (f32, dna_thickness),
    (f32, dna_twist_breathe),
    _, _, _, _,
}

param_block! {
    pack_attr, pack_attr_preset, [12];
    (enum, attr_field),
    (i32, attr_seeds),
    (i32, attr_seed),
    (f32, attr_spread),
    (f32, attr_dt),
    (i32, attr_trail),
    (f32, attr_speed),
    (f32, attr_scale),
    (f32, attr_thickness),
    _, _, _,
}

// [15] form, [16] size, [17] banking (creature forms, #52).
param_block! {
    pack_boids, pack_boids_preset, [24];
    (i32, boids_count),
    (f32, boids_perception),
    (f32, boids_separation),
    (f32, boids_sep),
    (f32, boids_align),
    (f32, boids_cohere),
    (f32, boids_max_speed),
    (f32, boids_max_force),
    (i32, boids_trail),
    (f32, boids_bounds),
    (f32, boids_goal),
    (f32, boids_thickness),
    (i32, boids_seed),
    (f32, boids_speed),
    (f32, boids_scale),
    (enum, boids_form),
    (f32, boids_size),
    (f32, boids_bank),
    _, _, _, _, _, _,
}

param_block! {
    pack_harm, pack_harm_preset, [16];
    (i32, harm_mode0),
    (f32, harm_amp0),
    (f32, harm_freq0),
    (i32, harm_mode1),
    (f32, harm_amp1),
    (f32, harm_freq1),
    (i32, harm_mode2),
    (f32, harm_amp2),
    (f32, harm_freq2),
    (f32, harm_radius),
    (i32, harm_theta),
    (i32, harm_phi),
    (f32, harm_thickness),
    _, _, _,
}

param_block! {
    pack_ls, pack_ls_preset, [12];
    (enum, ls_system),
    (i32, ls_depth),
    (f32, ls_angle),
    (f32, ls_step),
    (f32, ls_sway_amp),
    (f32, ls_sway_freq),
    (f32, ls_grow),
    (f32, ls_thickness),
    _, _, _, _,
}

param_block! {
    pack_cn, pack_cn_preset, [12];
    (i32, cn_seeds),
    (i32, cn_seed),
    (f32, cn_spread),
    (f32, cn_scale),
    (i32, cn_steps),
    (f32, cn_dt),
    (f32, cn_flow),
    (f32, cn_bound),
    (f32, cn_thickness),
    _, _, _,
}

param_block! {
    pack_pol, pack_pol_preset, [16];
    (i32, pol_rings),
    (i32, pol_spokes),
    (i32, pol_samples),
    (f32, pol_len),
    (f32, pol_k),
    (f32, pol_amp),
    (f32, pol_falloff),
    (bool, pol_handed),
    (f32, pol_spread),
    (f32, pol_swirl),
    (bool, pol_show_b),
    (f32, pol_thickness),
    _, _, _, _,
}

param_block! {
    pack_maxwell, pack_maxwell_preset, [24];
    (bool, mx_lines),
    (f32, mx_gen_blend),
    (i32, mx_sources),
    (bool, mx_dipoles),
    (f32, mx_separation),
    (f32, mx_phase),
    (f32, mx_swirl),
    (f32, mx_near),
    (f32, mx_k),
    (f32, mx_amp),
    (f32, mx_rmin),
    (f32, mx_thickness),
    (i32, mx_rings),
    (i32, mx_spokes),
    (i32, mx_samples),
    (f32, mx_raylen),
    (f32, mx_spread),
    (i32, mx_seeds),
    (i32, mx_steps),
    (f32, mx_ds),
    (f32, mx_bound),
    (bool, mx_norm_field),
    (bool, mx_osc_sync),  // [22] tempo-sync the dipole oscillation (LFO)
    (enum, mx_osc_div),   // [23] the synced note division
}

// FDTD Maxwell solver (#412 Tier 3, Phase 0): `Shared.fdtd[8]`. Captured
// **Generator**; `fdtd_on = 0` (default) → the analytic Maxwell path is untouched.
param_block! {
    pack_fdtd, pack_fdtd_preset, [8];
    (bool, fdtd_on),        // [0] run the solver (Maxwell generator only)
    (f32, fdtd_res),        // [1] grid cells per axis
    (enum, fdtd_source),    // [2] FdtdSource (0 Pulse / 1 CW)
    (f32, fdtd_freq),       // [3] source frequency ω
    (f32, fdtd_drive),      // [4] source drive amplitude
    (f32, fdtd_substeps),   // [5] CFL sub-steps per frame
    (f32, fdtd_boundary),   // [6] absorbing-sponge thickness (cells)
    (f32, fdtd_extent),     // [7] domain half-extent (world units)
}

// Acoustic-field generator (#325, Duo-Field N1): `Shared.acoustic[16]`.
param_block! {
    pack_acoustic, pack_acoustic_preset, [16];
    (enum, ac_source),      // [0] multipole order (AcousticSource)
    (f32, ac_k),            // [1] wavenumber
    (f32, ac_near),         // [2] near-field weight
    (f32, ac_amp),          // [3] geometry amplitude
    (f32, ac_separation),   // [4] multipole array extent
    (f32, ac_rmin),         // [5] near-source clamp
    (f32, ac_blend),        // [6] geometry pressure↔velocity
    (bool, ac_norm_field),  // [7] raw vs unit displacement
    (i32, ac_rings),        // [8] θ rings
    (i32, ac_spokes),       // [9] φ spokes
    (i32, ac_samples),      // [10] samples / ray
    (f32, ac_raylen),       // [11] ray length
    (f32, ac_spread),       // [12] cone half-angle (°)
    (f32, ac_thickness),    // [13] node thickness
    (f32, ac_aura_blend),   // [14] aura pressure↔velocity
    (f32, ac_beat_pump),    // [15] beat → source amplitude pump
}

// Acoustic Tier 4 (#325): cavity modes + intensity flux — `Shared.acoustic2[8]`.
param_block! {
    pack_acoustic2, pack_acoustic2_preset, [8];
    (enum, ac2_model),      // [0] source model (AcousticModel: Radiating / Cavity)
    (i32, ac2_nx),          // [1] cavity mode nx
    (i32, ac2_ny),          // [2] cavity mode ny
    (i32, ac2_nz),          // [3] cavity mode nz
    (f32, ac2_morph),       // [4] cavity beat morph
    (f32, ac2_cav_scale),   // [5] cavity box half-extent
    (f32, ac2_intensity),   // [6] intensity-flux aura channel
    _,                      // [7] reserved
}

// Acoustic Tier 5 (#325): cavity 3-D tween + per-axis audio breathe — `Shared.acoustic3[8]`.
param_block! {
    pack_acoustic3, pack_acoustic3_preset, [8];
    (f32, ac2_tween),       // [0] beat mode-walk tween (0 = snap, 1 = glide)
    (f32, ac2_audio_x),     // [1] audio → mode nx gain
    (f32, ac2_audio_y),     // [2] audio → mode ny gain
    (f32, ac2_audio_z),     // [3] audio → mode nz gain
    _, _, _, _,             // [4..8] reserved
}

// Analyzer / Calibrated instrument mode (#333 Tier 3) — `Shared.analytical[8]`.
param_block! {
    pack_analytical, pack_analytical_preset, [8];
    (enum, analytical_mode),// [0] Expressive / Calibrated
    (f32, an_target_lufs),  // [1] delivery loudness target (LUFS) + horizon
    (f32, an_floor_lufs),   // [2] calibrated drive floor (LUFS)
    (f32, an_tp_ceiling),   // [3] true-peak alarm ceiling (dBTP)
    (f32, an_corr_alarm),   // [4] correlation alarm threshold
    (bool, an_reference_hud),// [5] show the instrument HUD
    _, _,                   // [6..8] reserved
}

// Field Volume (#348) — density-cloud source + exposure for SurfaceMode::Volume:
// `Shared.fieldvol[8]`.
param_block! {
    pack_fieldvol, pack_fieldvol_preset, [8];
    (enum, fv_source),      // [0] FieldVolSource (Legacy / Auto / FieldBaked / SmoothedNode)
    (f32, fv_smooth),       // [1] smoothing-kernel width scale (× metaball radius)
    (f32, fv_exposure_db),  // [2] Tier-2 exposure (dB)
    (bool, fv_calibrate),   // [3] key brightness to calibrated loudness
    (f32, fv_gain),         // [4] extra density/emission gain
    (bool, fv_lines),       // [5] render volumetric FIELD-LINES (flow) instead of the cloud
    (f32, fv_line_density), // [6] streamlines traced per channel (line density)
    (f32, fv_line_thickness),// [7] filament thickness
}

// Calibrated colour (#349) — the cross-cutting colour-means-a-level tint:
// `Shared.colour[8]`.
param_block! {
    pack_colour, pack_colour_preset, [8];
    (enum, col_mode),       // [0] ColourMode (Aesthetic / Calibrated)
    (f32, col_lo_db),       // [1] LUT low dB
    (f32, col_hi_db),       // [2] LUT high dB
    (enum, col_lut),        // [3] CalLut (Turbo / Viridis / Inferno / Magma)
    (enum, col_source),     // [4] CalColourSource (Auto / Band / Lufs)
    (f32, col_amount),      // [5] calibrated-over-aesthetic blend
    _, _,                   // [6..8] reserved
}

// #339 Duo-Field synthesis Sound card: `Shared.sonify[16]` — only what the visual
// needs to draw the listener gizmos + place the played voices. The DSP reads the
// params directly in `process()`; this block is not the synth's own state.
param_block! {
    pack_sonify, pack_sonify_preset, [16];
    (bool, sn_on),          // [0] synth enable
    (enum, sn_play_mode),   // [1] Generative / Instrument / Duet
    _,                      // [2] reserved (was sn_source; the bed follows the generator)
    (f32, sn_vis_pivot),    // [3] lens fixed-point note (Hz)
    (f32, sn_vis_anchor),   // [4] lens visual rate at pivot (Hz)
    (f32, sn_vis_slope),    // [5] time-lens slope
    (f32, sn_vis_k_slope),  // [6] space-lens slope
    (enum, sn_vis_quantize),// [7] Free / Octave / Beat
    (f32, sn_probe_lx),     // [8] probe L x
    (f32, sn_probe_ly),     // [9] probe L y
    (f32, sn_probe_lz),     // [10] probe L z
    (f32, sn_probe_rx),     // [11] probe R x
    (f32, sn_probe_ry),     // [12] probe R y
    (f32, sn_probe_rz),     // [13] probe R z
    (bool, sn_probe_cam),   // [14] probe 0 rides the camera
    _,                      // [15] reserved
}

// Field Chamber (#346 Tier 1): the analyzer-panel look — `Shared.chamber[16]`. Only
// what the VISUAL needs to draw the panels; the wall-scope TIME/trigger/channel are
// separate plugin-side params (they drive the `scopewave` publish). Captured **Look**.
param_block! {
    pack_chamber, pack_chamber_preset, [16];
    (bool, panels_on),      // [0] master on/off (off → byte-identical)
    (enum, panel_style),    // [1] PanelStyle: Flat / Impostor
    (bool, panel_rear),     // [2] rear −Z wall = oscilloscope (time)
    (bool, panel_right),    // [3] right +X wall = spectrum (frequency)
    (f32, panel_opacity),   // [4] whole-panel alpha
    (f32, panel_fill),      // [5] wall inset (0..1 of the wall face)
    (f32, panel_scope_amp), // [6] scope vertical gain
    (f32, panel_db_floor),  // [7] spectrum dBFS floor
    (enum, panel_material), // [8] MaterialType (Tier 2 impostor shading)
    (f32, panel_metallic),  // [9] impostor metallic
    (f32, panel_roughness), // [10] impostor roughness
    (bool, panel_wall_rel), // [11] 0 fixed world axes / 1 camera-relative back walls
    (f32, panel_thickness), // [12] ribbon / bar radius (world units)
    (f32, panel_emissive),  // [13] trace/bar emissive glow
    (f32, panel_db_top),    // [14] spectrum dBFS ceiling
    _,                      // [15] reserved
}

// Material Emissive (HDR self-emission in the surface's own colour): `Shared.emissive[4]`
// = [mat_emissive, sc_emissive, pbead_emissive, _]. Captured **Look**; all 0 → byte-identical.
param_block! {
    pack_emissive, pack_emissive_preset, [4];
    (f32, mat_emissive),          // [0] generator material (main)
    (f32, sc_emissive),           // [1] scenery / zone material
    (f32, particles_bead_emissive), // [2] solid particle impostor (bead) material
    _,                            // [3] reserved
}

// Gaussian Splatting surface (SurfaceMode::Splat = 8): `Shared.splat[8]` =
// [radius, opacity, falloff, mode(SplatMode: 0 Additive / 1 Lit), cutoff, aniso, _, _].
// Captured **Generator** (surface-shape block); only consumed when surface_mode == 8 →
// any other mode byte-identical.
param_block! {
    pack_splat, pack_splat_preset, [8];
    (f32, splat_radius),   // [0] world × on the node covariance axes
    (f32, splat_opacity),  // [1] peak Gaussian weight (0..1)
    (f32, splat_falloff),  // [2] exponent scale (tightness)
    (enum, splat_mode),    // [3] 0 = additive/unlit (Tier 1), 1 = lit 2DGS (Tier 2)
    (f32, splat_cutoff),   // [4] discard weight
    (f32, splat_aniso),    // [5] extra axis stretch (1 = as-is)
    (i32, splat_scatter),  // [6] Tier 3: sub-splats per node (1 = one/node)
    (f32, splat_jitter),   // [7] Tier 3: sub-splat spread (fraction of node size)
}

// Plexus surface mode (ordinal 9): proximity-web controls. All multipliers of the
// field's characteristic node spacing (scale-invariant across generators).
param_block! {
    pack_plexus, pack_plexus_preset, [4];
    (f32, plexus_radius),  // [0] link radius (× node spacing)
    (f32, plexus_links),   // [1] max neighbours per node (floored)
    (f32, plexus_strut),   // [2] strut thickness (× node spacing)
    (f32, plexus_marker),  // [3] node marker size (× node spacing)
}

// Plexus Tier 2 impostor controls.
param_block! {
    pack_plexus2, pack_plexus2_preset, [4];
    (bool, plexus_impostor),   // [0] impostors on (else Tier 1 cubes)
    (bool, plexus_edges),      // [1] draw the edges (capsule impostors)
    (f32, plexus_node_radius), // [2] node sphere radius (× node spacing)
    (f32, plexus_edge_radius), // [3] edge tube radius (× node spacing)
}

// Independent node/edge impostor materials: [mat_type, metallic, roughness, ior,
// hue, saturation, value, emissive].
param_block! {
    pack_plexus_node_mat, pack_plexus_node_mat_preset, [8];
    (enum, plexus_node_type),
    (f32, plexus_node_metallic),
    (f32, plexus_node_rough),
    (f32, plexus_node_ior),
    (f32, plexus_node_hue),
    (f32, plexus_node_sat),
    (f32, plexus_node_val),
    (f32, plexus_node_emissive),
}
param_block! {
    pack_plexus_edge_mat, pack_plexus_edge_mat_preset, [8];
    (enum, plexus_edge_type),
    (f32, plexus_edge_metallic),
    (f32, plexus_edge_rough),
    (f32, plexus_edge_ior),
    (f32, plexus_edge_hue),
    (f32, plexus_edge_sat),
    (f32, plexus_edge_val),
    (f32, plexus_edge_emissive),
}

// Plexus Tier 3: beat-driven signal propagation.
param_block! {
    pack_plexus3, pack_plexus3_preset, [4];
    (bool, plexus_signal),      // [0] signal propagation on
    (f32, plexus_signal_speed), // [1] shells per beat
    (f32, plexus_signal_gain),  // [2] emissive boost at the wavefront
    (f32, plexus_signal_width), // [3] shell width (fraction of radius)
}

// Plexus Tier-1 shape morph: node cube→sphere + edge square→circle.
param_block! {
    pack_plexus4, pack_plexus4_preset, [4];
    (f32, plexus_node_shape), // [0] 0 cube → 1 sphere
    (f32, plexus_edge_shape), // [1] 0 square → 1 circle
    _,                        // [2] reserved
    _,                        // [3] reserved
}

// Splat Tier 3 look extension: `Shared.splat2[4]` = [solid, _, _, _]. Captured
// **Generator** (surface-shape); only used when surface_mode == 8, solid 0 → byte-identical.
param_block! {
    pack_splat2, pack_splat2_preset, [4];
    (f32, splat_solid), // [0] soft Gaussian (0) → opaque disc (1)
    _,                  // [1] reserved
    _,                  // [2] reserved
    _,                  // [3] reserved
}

// Maxwell E↔B phase (near↔far induction dial): `Shared.mx_eb[4]`.
param_block! {
    pack_mx_eb, pack_mx_eb_preset, [4];
    (f32, mx_eb_phase), // [0] B-swirl phase offset (degrees) vs the E clock
    _, _, _,
}

// Plexus overlay: the web wrapped as an outer shell around ANOTHER surface.
param_block! {
    pack_plexus_overlay, pack_plexus_overlay_preset, [4];
    (bool, plexus_overlay_on),   // [0] overlay on (reads the node cloud, doesn't replace it)
    (f32, plexus_shell_scale),   // [1] grow the shell outward from the centroid
    (f32, plexus_shell_depth),   // [2] radial-band depth kept per directional cell (fraction)
    (f32, plexus_shell_bins),    // [3] directional-bin resolution
}

// Field Engine (#381 Tier 1) live coefficients: `Shared.field[10]`. Captured
// **Generator**; only used when generator == 24 (byte-identical otherwise).
param_block! {
    pack_field, pack_field_preset, [10];
    (enum, field_kind),      // [0] FieldKind: 0 Auto / 1 Scalar / 2 Vector / 3 Complex
    (enum, field_preset),    // [1] FieldPreset gallery code (7 = Custom = sidecar; 8/9/10 = operators)
    (f32, field_scale),      // [2] domain scale k
    (f32, field_extent),     // [3] box half-extent
    (f32, field_a),          // [4] live coefficient a
    (f32, field_b),          // [5] live coefficient b
    (i32, field_density),    // [6] field-line seeds / lattice resolution
    (f32, field_gain),       // [7] display gain (glyph / displacement)
    (f32, field_thickness),  // [8] line / marker thickness
    _,                       // [9] reserved (Tier-2 coefficient)
}

// Density-Map Attractor (#380 Tier 1): `Shared.mapattractor[10]` = [kind, a, b,
// points_k, warmup, scale, size, intensity, a_drive, b_drive]. Captured **Generator**.
param_block! {
    pack_mapattractor, pack_mapattractor_preset, [10];
    (enum, ma_kind),    // [0] MapKind (0 = Complexus)
    (f32, ma_a),        // [1] map parameter a
    (f32, ma_b),        // [2] map parameter b
    (i32, ma_points_k), // [3] points per frame, thousands
    (i32, ma_warmup),   // [4] warm-up iterations discarded per restart orbit
    (f32, ma_scale),    // [5] world half-extent of the [-1,1] box
    (f32, ma_size),     // [6] per-point marker size
    (f32, ma_intensity),// [7] emissive tint gain
    (f32, ma_a_drive),  // [8] animation → parameter a (0..1)
    (f32, ma_b_drive),  // [9] animation → parameter b (0..1)
}

// Density-Map Attractor parameter orbit (#380 Tier 2): `Shared.maporbit[8]` =
// [mode, loop_beats, Ra, Rb, fa, fb, psi, free_rate]. Captured **Generator**.
param_block! {
    pack_maporbit, pack_maporbit_preset, [8];
    (enum, ma_orbit),      // [0] MapOrbitMode (0 Off / 1 Linear / 2 Lissajous)
    (f32, ma_loop_beats),  // [1] loop length in beats (beat-locked φ = beat_pos/this)
    (f32, ma_orbit_ra),    // [2] Lissajous radius on a
    (f32, ma_orbit_rb),    // [3] Lissajous radius on b
    (i32, ma_orbit_fa),    // [4] frequency ratio on a (integer → closed loop)
    (i32, ma_orbit_fb),    // [5] frequency ratio on b
    (f32, ma_orbit_psi),   // [6] phase offset ψ on b (radians)
    (f32, ma_orbit_free),  // [7] free-run rate (loops per gen_phase unit)
}

// Density-Map Attractor Tier 3 (#380): `Shared.mapattractor2[4]` = [c, d, color, _].
// `c`/`d` are the extra map coefficients (Clifford / de Jong / Pickover); `color` is
// the MapColor mode (0 StepSpeed / 1 IterIndex / 2 JacobianStretch). Captured **Generator**.
param_block! {
    pack_mapattractor2, pack_mapattractor2_preset, [4];
    (f32, ma_c),      // [0] map parameter c
    (f32, ma_d),      // [1] map parameter d
    (enum, ma_color), // [2] MapColor (colour-by-dynamics mode)
    _,                // [3] reserved
}

// Field Engine Tier 3 (#381): time-marched PDE sim — `Shared.fieldsim[8]` =
// [preset, D, time_scale, feed, kill, potential, forcing, res]. Captured
// **Generator**; only consumed when generator == 24 && preset != Off.
param_block! {
    pack_fieldsim, pack_fieldsim_preset, [8];
    (enum, pde_preset),      // [0] PdePreset (0 Off / 1 Heat / 2 Wave / 3 Schrödinger / 4 Gray-Scott)
    (f32, sim_diffusion),    // [1] D (diffusion / wave-speed / kinetic)
    (f32, sim_time_scale),   // [2] beat-delta -> sim-time multiplier
    (f32, sim_feed),         // [3] Gray-Scott feed F
    (f32, sim_kill),         // [4] Gray-Scott kill k
    (f32, sim_potential),    // [5] Schrödinger harmonic-trap strength
    (f32, sim_forcing),      // [6] audio/source forcing amplitude
    (i32, sim_res),          // [7] grid resolution (16..128)
}

// Scene Kaleidoscope (#361 Tier 1): `Shared.kaleido[16]` — a post-stage
// kaleidoscopic fold of the resolved HDR scene. Captured **Look**; `kal_on = 0`
// → the HDR buffer is untouched (byte-identical default).
param_block! {
    pack_kaleido, pack_kaleido_preset, [16];
    (bool, kal_on),        // [0] master on/off
    (f32, kal_sectors),    // [1] N-fold symmetry
    (enum, kal_mode),      // [2] KaleidoMode: FullFrame / Wedge
    (f32, kal_spin),       // [3] rotation speed (× the animation clock)
    (f32, kal_roll),       // [4] static rotation offset (turns)
    (f32, kal_zoom),       // [5] source sample zoom
    (f32, kal_center_x),   // [6] source sample centre X
    (f32, kal_center_y),   // [7] source sample centre Y
    (f32, kal_mix),        // [8] scene ↔ folded crossfade
    (f32, kal_twist),      // [9] log-polar spiral twist
    (f32, kal_tint_hue),   // [10] hue grade (deg)
    (f32, kal_tint_amt),   // [11] hue grade amount
    (f32, kal_seam),       // [12] mirror-seam supersample softening
    _, _, _,               // [13..15] reserved
}

// Quantitative instrumentation (#391 Tier 1): `Shared.instrument[16]` — placeable
// field probes + energy ledger + Poynting-flux surface + CSV export. Captured
// **Look**; `instr_hud = 0` (default) → the HUD is not drawn and render is
// byte-identical.
param_block! {
    pack_instrument, pack_instrument_preset, [16];
    (bool, instr_hud),        // [0] draw the instrumentation HUD (master gate)
    (bool, instr_probe_on),   // [1] read the point probe
    (f32, instr_probe_x),     // [2] probe world X
    (f32, instr_probe_y),     // [3] probe world Y
    (f32, instr_probe_z),     // [4] probe world Z
    (bool, instr_ledger_on),  // [5] integrate the energy ledger
    (f32, instr_ledger_half), // [6] ledger box half-extent
    (f32, instr_ledger_res),  // [7] ledger sample resolution (n/axis)
    (bool, instr_flux_on),    // [8] integrate the Poynting-flux patch
    (f32, instr_flux_x),      // [9] flux patch centre X
    (f32, instr_flux_y),      // [10] flux patch centre Y
    (f32, instr_flux_z),      // [11] flux patch centre Z
    (f32, instr_flux_size),   // [12] flux patch half-size
    (enum, instr_flux_axis),  // [13] FluxAxis (0 X / 1 Y / 2 Z / 3 Radial)
    (f32, instr_flux_res),    // [14] flux patch sample resolution (n/side)
    (bool, instr_csv_log),    // [15] append a probe-trace CSV row each frame
}

// Quantitative instrumentation HUD presentation (#391 Tier 1): `Shared.instrument2[8]`
// — the rounded backing panel + overall size + dock corner. Captured **Look**.
param_block! {
    pack_instrument2, pack_instrument2_preset, [8];
    (f32, instr_panel_opacity), // [0] backing-panel opacity (0 = none)
    (f32, instr_panel_bevel),   // [1] panel corner rounding (0 square … 1 pill)
    (f32, instr_hud_scale),     // [2] overall HUD size (font + panel)
    (enum, instr_hud_dock),     // [3] HudDock (0 TL / 1 BL / 2 TR / 3 BR)
    _, _, _, _,                 // [4..7] reserved
}

// Axon Waveguide (#218 Tier 1): `Shared.axon[24]` = [count, length, bundle_radius,
// samples, thickness, node_spacing, node_dip, pulse_speed, pulse_width, stagger,
// splay, seed, …12 reserved for Tiers 2–4].
param_block! {
    pack_axon, pack_axon_preset, [24];
    (i32, ax_count),
    (f32, ax_length),
    (f32, ax_bundle),
    (i32, ax_samples),
    (f32, ax_thickness),
    (f32, ax_node_spacing),
    (f32, ax_node_dip),
    (f32, ax_pulse_speed),
    (f32, ax_pulse_width),
    (f32, ax_stagger),
    (f32, ax_splay),
    (i32, ax_seed),
    (enum, ax_mode),        // [12] Tier 2: guided mode
    (f32, ax_mode_amount),  // [13] Tier 2: mode intensity amount
    (f32, ax_bend),         // [14] Tier 3: bend-degradation (mode leak / node flare)
    (f32, ax_curve),        // [15] brain tract: C-arc curvature
    (f32, ax_tortuosity),   // [16] brain tract: per-fibre undulation
    (f32, ax_dti),          // [17] brain tract: DTI tractography colour blend
    (f32, ax_dispersion),   // [18] Tier 4: chromatic pulse chirp
    (f32, ax_polarization), // [19] Tier 4: polarization coherence shimmer
    _, _, _, _,
}

param_block! {
    pack_phyl, pack_phyl_preset, [16];
    (enum, phyl_surface),
    (i32, phyl_count),
    (f32, phyl_divergence),
    (f32, phyl_radius),
    (i32, phyl_parastichy),
    (f32, phyl_height),
    (f32, phyl_growth),
    (f32, phyl_breathe_amp),
    (f32, phyl_breathe_freq),
    (f32, phyl_rot),
    (f32, phyl_thickness),
    _, _, _, _, _,
}

param_block! {
    pack_tessellation, pack_tessellation_preset, [16];
    (enum, tess_family),
    (i32, tess_depth),
    (f32, tess_scale),
    (f32, tess_thickness),
    (enum, tess_view),        // [4] Phase 2: edges / filled / extruded
    (f32, tess_height),       // [5] extrusion height (fraction of size)
    (enum, tess_height_mode), // [6] uniform / by-type / radial
    (f32, tess_beat_infl),    // [7] Phase 3: beat inflation-breathe amount
    (f32, tess_ripple_amt),   // [8] Phase 3: per-tile beat ripple amount
    (f32, tess_ripple_freq),  // [9] Phase 3: ripple spatial frequency
    (enum, tess_construct),   // [10] Phase 4: inflation / cut-and-project
    (f32, tess_phason),       // [11] Phase 4: phason window amount
    (i32, tess_grid_n),       // [12] Phase 4: multigrid grid range
    (f32, tess_ammann),       // [13] follow-up: Ammann bars overlay
    (i32, tess_hyp_p),        // [14] follow-up: hyperbolic {p,q} — p
    (i32, tess_hyp_q),        // [15] follow-up: hyperbolic {p,q} — q
}

param_block! {
    pack_mandelbulb, pack_mandelbulb_preset, [8];
    (f32, mb_power),
    (i32, mb_iter),
    (f32, mb_scale),
    (i32, mb_detail),
    (f32, mb_spin),
    (f32, mb_morph),
    (f32, mb_color),
    (f32, mb_bailout),
}

// #476 Tier 1 — Creature Engine params → Shared.creature[8]. Order matches the
// visual's unpack: [form, scale, detail(steps), swim_rate, warp_amp, warp_freq,
// rim, glow_scale].
param_block! {
    pack_creature, pack_creature_preset, [8];
    (i32, cr_form),
    (f32, cr_scale),
    (i32, cr_detail),
    (f32, cr_swim),
    (f32, cr_warp_amp),
    (f32, cr_warp_freq),
    (f32, cr_rim),
    (f32, cr_glow),
}

// #476 Tier 2a — the metachronal wave → Shared.creature2[8]. Order matches the
// visual's unpack: [wave_speed, wave_freq, wave_sharp, wave_amount, _, _, _, _].
param_block! {
    pack_creature2, pack_creature2_preset, [8];
    (f32, cr_wave_speed),
    (f32, cr_wave_freq),
    (f32, cr_wave_sharp),
    (f32, cr_wave_amt),
    _, _, _, _,
}

// #476 Tier 2c — the anatomy overlay → Shared.creature3[4]: [on, opacity, brightness, _].
param_block! {
    pack_creature3, pack_creature3_preset, [4];
    (bool, cr_overlay),
    (f32, cr_overlay_opacity),
    (f32, cr_overlay_bright),
    _,
}

// #472 Tier 1 — procedural/texture material controls → Shared.material[8]. Order
// matches the visual's unpack in build_uniforms: [on, projection, scale,
// normal_strength, ao_strength, rough_scale, metal_scale, _]. on = 0 → the
// scalar-uniform PBR path is byte-identical.
param_block! {
    pack_material, pack_material_preset, [8];
    (bool, mat_enable),      // [0] 0 off (scalar PBR) / 1 on (texture set)
    (enum, mat_projection),  // [1] 0 triplanar / 1 world-planar XZ / 2 object-planar
    (f32, mat_scale),        // [2] world→UV frequency
    _,                       // [3] reserved (was normal strength — maps feed the pipeline directly)
    _,                       // [4] reserved (was AO strength)
    _,                       // [5] reserved (was roughness mult)
    _,                       // [6] reserved (was metallic mult)
    _,                       // [7] reserved (Tier 5 height→displacement headroom)
}

// #472 Tier 2 — procedural single-layer noise graph → Shared.material_layer[18].
// Order matches the visual's unpack + material_bake.wgsl's LayerU. [17] bake_res is
// packed as PIXELS (BakeRes::px), not the enum ordinal, so the wire carries the size.
param_block! {
    pack_material_layer, pack_material_layer_preset, [18];
    (enum, mp_noise),        // [0] MatNoise kind
    (enum, mp_channel),      // [1] MatChannel route
    (f32, mp_scale),         // [2] tiles across the bake
    (f32, mp_rotation),      // [3] field rotation (rad)
    (f32, mp_offset_x),      // [4] field offset X
    (f32, mp_offset_y),      // [5] field offset Y
    (i32, mp_octaves),       // [6] fractal octaves
    (f32, mp_lacunarity),    // [7] fractal lacunarity
    (f32, mp_gain),          // [8] fractal gain
    (f32, mp_warp),          // [9] domain-warp amount
    (f32, mp_contrast),      // [10] output contrast
    (f32, mp_gamma),         // [11] output gamma
    (f32, mp_remap_lo),      // [12] input remap low
    (f32, mp_remap_hi),      // [13] input remap high
    (bool, mp_invert),       // [14] invert field
    (i32, mp_seed),          // [15] noise seed
    (bool, mp_enable),       // [16] procedural on (0 → byte-identical)
    (expr, |p| p.mp_res.value().px(),
           |pv| crate::params::BakeRes::from_u32(pv.mp_res).px()), // [17] bake px
}

// #472 Tier 2 — albedo gradient stops → Shared.material_grad[8].
param_block! {
    pack_material_grad, pack_material_grad_preset, [8];
    (f32, mp_lo_r), (f32, mp_lo_g), (f32, mp_lo_b), _,
    (f32, mp_hi_r), (f32, mp_hi_g), (f32, mp_hi_b), _,
}

// #472 Tier 3 — overlay layer 2 → Shared.material_layer2[18]. Same 16 base slots as
// material_layer, but [16] = enabled and [17] = blend_mode.
param_block! {
    pack_material_layer2, pack_material_layer2_preset, [18];
    (enum, mp2_noise),       // [0]
    (enum, mp2_channel),     // [1]
    (f32, mp2_scale),        // [2]
    (f32, mp2_rotation),     // [3]
    (f32, mp2_offset_x),     // [4]
    (f32, mp2_offset_y),     // [5]
    (i32, mp2_octaves),      // [6]
    (f32, mp2_lacunarity),   // [7]
    (f32, mp2_gain),         // [8]
    (f32, mp2_warp),         // [9]
    (f32, mp2_contrast),     // [10]
    (f32, mp2_gamma),        // [11]
    (f32, mp2_remap_lo),     // [12]
    (f32, mp2_remap_hi),     // [13]
    (bool, mp2_invert),      // [14]
    (i32, mp2_seed),         // [15]
    (bool, mp2_enable),      // [16] enabled (0 → Tier-2 path)
    (enum, mp2_blend),       // [17] BlendMode
}

// #472 Tier 3 — overlay layer 2 albedo gradient → Shared.material_grad2[8].
param_block! {
    pack_material_grad2, pack_material_grad2_preset, [8];
    (f32, mp2_lo_r), (f32, mp2_lo_g), (f32, mp2_lo_b), _,
    (f32, mp2_hi_r), (f32, mp2_hi_g), (f32, mp2_hi_b), _,
}

// #472 Tier 3 — derived maps → Shared.material_derive[8].
param_block! {
    pack_material_derive, pack_material_derive_preset, [8];
    (bool, mat_derive_normal),          // [0]
    (bool, mat_derive_ao),              // [1]
    (bool, mat_normal_source_albedo),   // [2] 0 height / 1 albedo luminance
    (f32, mat_derive_normal_strength),  // [3]
    (f32, mat_derive_ao_strength),      // [4]
    (f32, mat_derive_ao_radius),        // [5]
    _, _,                               // [6..7] reserved
}

// #472 Tier 5 — live animation + displacement → Shared.material_live[8].
param_block! {
    pack_material_live, pack_material_live_preset, [8];
    (bool, mat_anim_enable), // [0]
    (f32, mat_anim_speed),   // [1]
    (enum, mat_anim_mode),   // [2] AnimMode
    (f32, mat_flow_x),       // [3]
    (f32, mat_flow_y),       // [4]
    (f32, mat_displace),     // [5] height→vertex displacement
    _,                       // [6] audio_drive — RESERVED (deferred audio hook)
    _,                       // [7] reserved
}

// organon#217 T3 — the glyph ring's look → Shared.glyph[16]. Every slot is a field of
// `glyph_ring::GlyphLook` (cell units, §5.1) plus the tiles' own bevel and the face
// crown; `world.rs::glyph_look_from` is the reader and its test pins that a default
// `Shared` reproduces `GlyphLook::DEFAULT` exactly.
param_block! {
    pack_glyph, pack_glyph_preset, [16];
    (f32, glyph_cell_w),     // [0] world units per column
    (f32, glyph_depth),      // [1] extrusion, in column widths
    (f32, glyph_gap),        // [2] tile → backplane gap, in column widths
    (f32, glyph_gain),       // [3] emission gain, SDR-white units (§4)
    (f32, glyph_faceplate),  // [4] faceplate grey
    (f32, glyph_back_r),     // [5] backplane tint R
    (f32, glyph_back_g),     // [6] backplane tint G
    (f32, glyph_back_b),     // [7] backplane tint B
    (f32, glyph_margin),     // [8] backplane margin, in column widths
    (f32, glyph_back_depth), // [9] backplane thickness, in column widths
    (f32, glyph_default_fg), // [10] grey for a cell with no fg colour
    (f32, glyph_bevel),      // [11] the tiles' rounded-box morph (own lane, not `bevel`)
    (f32, glyph_crown),      // [12] face crown — per-fragment dome normal
    (f32, glyph_profile),    // [13] T9 emission profile strength → `Uniforms.shape.z`
    (bool, glyph_dark_tiles),// [14] T9 every cell a tile → `LowerOptions::dark_tiles`
    _,                       // [15] reserved
}

// organon#217 T3 — the held camera for a live ring → Shared.glyph_cam[8].
param_block! {
    pack_glyph_cam, pack_glyph_cam_preset, [8];
    (bool, glyph_cam_hold),  // [0] absolute fitted rig while the ring is live
    (f32, glyph_cam_tilt),   // [1] pitch, degrees
    (f32, glyph_cam_zoom),   // [2] multiplier on the fitted distance
    _,                       // [3] reserved
    _,                       // [4] reserved
    _,                       // [5] reserved
    _,                       // [6] reserved
    _,                       // [7] reserved
}

// organon#217 T6/T3 — the coaxial capsule core → Shared.capsule[4]. Reaches
// `ParticleSystem::set_capsule_core` through the render frame's `Surface.capsule_core`.
param_block! {
    pack_capsule, pack_capsule_preset, [4];
    (f32, capsule_core),     // [0] inner emissive radius / outer radius (0 = off)
    (f32, capsule_absorb),   // [1] Beer–Lambert density per outer radius
    _,                       // [2] reserved
    _,                       // [3] reserved
}

param_block! {
    pack_minimal_surface, pack_minimal_surface_preset, [16];
    (enum, ms_family),     // [0] 0 Gyroid / 1 Schwarz P / 2 Schwarz D
    (f32, ms_scale),       // [1] world radius
    (f32, ms_cells),       // [2] surface periods across the structure
    (f32, ms_iso),         // [3] isolevel (the surface is F = iso)
    (f32, ms_thickness),   // [4] soap-film wall half-width
    (f32, ms_twist),       // [5] domain twist (radians / unit height)
    (i32, ms_detail),      // [6] raymarch step budget
    (f32, ms_color),       // [7] channel-band colour intensity
    (f32, ms_beat_iso),    // [8] beat → isolevel breathe amount
    (f32, ms_bend),        // [9] P2: associate-family bend speed (catenoid↔helicoid)
    (i32, ms_uv_res),      // [10] P2: parametric (u,v) grid resolution
    (f32, ms_extent),      // [11] P2: parametric domain half-extent
    (f32, ms_bend_phase),  // [12] P2: static associate-bend position
    (f32, ms_turns),       // [13] P2: u-domain turns (multi-turn helicoid)
    (f32, ms_form_res),    // [14] P3: raymarch form-resolution divisor (perf)
    _,                     // [15] reserved (Phase 4 headroom)
}

param_block! {
    pack_synchrotron, pack_synchrotron_preset, [24];
    (f32, sy_radius),       // [0] orbit radius R
    (f32, sy_beta),         // [1] orbital speed β = v/c
    (i32, sy_charges),      // [2] bunched charges on the ring
    (i32, sy_grid),         // [3] samples / plane axis
    (f32, sy_extent),       // [4] plane half-extent
    (f32, sy_near),         // [5] velocity-term weight (0 = radiation only)
    (f32, sy_amp),          // [6] arrow length gain
    (f32, sy_thickness),    // [7] arrow rod radius
    (f32, sy_rmin),         // [8] source clamp
    (bool, sy_perp),        // [9] sample the plane ⟂ the orbit
    (enum, sy_view),        // [10] 0 arrows / 1 field lines / 2 volume (#150 P3/P4)
    (i32, sy_line_seeds),   // [11] field-line seeds
    (i32, sy_line_steps),   // [12] field-line max steps
    (f32, sy_line_ds),      // [13] field-line step ds
    (f32, sy_line_bound),   // [14] field-line bound
    (i32, sy_vol_layers),   // [15] volume depth slices (#150 P4)
    (f32, sy_reveal),       // [16] cull arrows below this |E| (#150 P5)
    (bool, sy_invert),      // [17] sphere-invert the volume (inside-out)
    (f32, sy_invert_radius), // [18] inversion sphere radius c
    (f32, sy_tilt),         // [19] orbit-plane tilt (deg, #150 P6)
    (f32, sy_precess),      // [20] orbital-plane precession rate
    _, _, _,                // [21..23] reserved
}

// Vector-field plotter (#173): `Shared.vecfield[24]`. Tier 1 = slots 0–12
// (arrows), Tier 2 = slots 13–22 (field lines); slot 23 is reserved (Tier 3
// gets its own block — the builder needs more room than one slot).
param_block! {
    pack_vecfield, pack_vecfield_preset, [24];
    (enum, vf_preset),      // [0] function bank entry (math::vecfield_eval)
    (i32, vf_grid_x),       // [1] lattice samples / X (1 = plane)
    (i32, vf_grid_y),       // [2] lattice samples / Y
    (i32, vf_grid_z),       // [3] lattice samples / Z (1 = the 2-D plot)
    (f32, vf_extent),       // [4] box half-extent
    (f32, vf_field_scale),  // [5] domain scale k in F(k·p)
    (f32, vf_amp),          // [6] arrow length gain
    (f32, vf_thickness),    // [7] arrow rod radius
    (enum, vf_mag_map),     // [8] |F| → length: 0 soft / 1 log / 2 uniform
    (enum, vf_tint_mode),   // [9] tint: 0 magnitude / 1 direction
    (f32, vf_evolve),       // [10] domain-rotation animation speed
    (f32, vf_z_lift),       // [11] planar-preset 3-D lift (Fz += lift·sin z)
    (f32, vf_reveal),       // [12] cull arrows below this soft |F|
    (enum, vf_view),        // [13] T2: 0 arrows / 1 field lines / 2 both
    (enum, vf_seed_mode),   // [14] T2: lattice/random/ring/plane/|F|-weighted
    (i32, vf_line_seeds),   // [15] T2: traced lines
    (i32, vf_line_steps),   // [16] T2: max RK4 steps per line
    (f32, vf_line_ds),      // [17] T2: RK4 step length
    (bool, vf_bidir),       // [18] T2: trace both directions from each seed
    (enum, vf_line_color),  // [19] T2: 0 |F| / 1 sweep along the line
    (f32, vf_flow),         // [20] T2: flow-pulse amount (0 = off)
    (f32, vf_flow_speed),   // [21] T2: flow-pulse speed (cycles/clock unit)
    (f32, vf_line_thickness), // [22] T2: line rod thickness
    _,                      // [23] reserved
}

// Vector-field function builder (#173, Tier 3): `Shared.vecbuild[64]` — 9 terms
// x 6 slots (func, gain, a, b, c, phase; x1 x2 x3 y1 ... z3), then the field
// operator + Helmholtz mix. Slots 56-63 reserved.
param_block! {
    pack_vecbuild, pack_vecbuild_preset, [64];
    (enum, vb_x1_func),   // [0] Fx term 1: func, gain, a, b, c, phase
    (f32, vb_x1_gain), (f32, vb_x1_a), (f32, vb_x1_b), (f32, vb_x1_c), (f32, vb_x1_phase),
    (enum, vb_x2_func),   // [6] Fx term 2: func, gain, a, b, c, phase
    (f32, vb_x2_gain), (f32, vb_x2_a), (f32, vb_x2_b), (f32, vb_x2_c), (f32, vb_x2_phase),
    (enum, vb_x3_func),   // [12] Fx term 3: func, gain, a, b, c, phase
    (f32, vb_x3_gain), (f32, vb_x3_a), (f32, vb_x3_b), (f32, vb_x3_c), (f32, vb_x3_phase),
    (enum, vb_y1_func),   // [18] Fy term 1: func, gain, a, b, c, phase
    (f32, vb_y1_gain), (f32, vb_y1_a), (f32, vb_y1_b), (f32, vb_y1_c), (f32, vb_y1_phase),
    (enum, vb_y2_func),   // [24] Fy term 2: func, gain, a, b, c, phase
    (f32, vb_y2_gain), (f32, vb_y2_a), (f32, vb_y2_b), (f32, vb_y2_c), (f32, vb_y2_phase),
    (enum, vb_y3_func),   // [30] Fy term 3: func, gain, a, b, c, phase
    (f32, vb_y3_gain), (f32, vb_y3_a), (f32, vb_y3_b), (f32, vb_y3_c), (f32, vb_y3_phase),
    (enum, vb_z1_func),   // [36] Fz term 1: func, gain, a, b, c, phase
    (f32, vb_z1_gain), (f32, vb_z1_a), (f32, vb_z1_b), (f32, vb_z1_c), (f32, vb_z1_phase),
    (enum, vb_z2_func),   // [42] Fz term 2: func, gain, a, b, c, phase
    (f32, vb_z2_gain), (f32, vb_z2_a), (f32, vb_z2_b), (f32, vb_z2_c), (f32, vb_z2_phase),
    (enum, vb_z3_func),   // [48] Fz term 3: func, gain, a, b, c, phase
    (f32, vb_z3_gain), (f32, vb_z3_a), (f32, vb_z3_b), (f32, vb_z3_c), (f32, vb_z3_phase),
    (enum, vb_op),        // [54] 0 direct / 1 gradient / 2 curl / 3 Helmholtz
    (f32, vb_mix),        // [55] Helmholtz blend (0 grad ... 1 curl)
    _, _, _, _, _, _, _, _, // [56..63] reserved
}

// Fluid Ink (#182 Tier 1): `Shared.fluidvis[12]` = [enabled, rate, radius,
// extinction, scatter, emissive, anisotropy, dissipation, steps, maccormack,
// half_res, reveal].
param_block! {
    pack_fluidvis, pack_fluidvis_preset, [12];
    (bool, ink_enabled),
    (f32, ink_rate),
    (f32, ink_radius),
    (f32, ink_extinction),
    (f32, ink_scatter),
    (f32, ink_emissive),
    (f32, ink_anisotropy),
    (f32, ink_dissipation),
    (f32, ink_steps),
    (bool, ink_maccormack),
    (bool, ink_half_res),
    (f32, ink_reveal),
}

// Fluid medium Tier 2 (#182): `Shared.fluid2[8]` = [boundaries, buoyancy,
// heat_decay, detail, splash, dye_gate, res, substeps].
param_block! {
    pack_fluid2, pack_fluid2_preset, [8];
    (bool, fl2_boundaries),
    (f32, fl2_buoyancy),
    (f32, fl2_heat_decay),
    (f32, fl2_detail),
    (f32, fl2_splash),
    (f32, fl2_dye_gate),
    (i32, fl2_res),
    (i32, fl2_substeps),
}

// MLS-MPM liquid (#182 Tier 3a): `Shared.liquid[16]` = [enabled, count_k,
// grid_res, gravity, stiffness, viscosity, container, open_top, collide,
// stir, density, threshold, hue, sat, _, substeps].
param_block! {
    pack_liquid, pack_liquid_preset, [16];
    (bool, liq_enabled),
    (i32, liq_count),
    (i32, liq_res),
    (f32, liq_gravity),
    (f32, liq_stiffness),
    (f32, liq_viscosity),
    (f32, liq_container),
    (bool, liq_open_top),
    (bool, liq_collide),
    (f32, liq_stir),
    (f32, liq_density),
    (f32, liq_threshold),
    (f32, liq_hue),
    (f32, liq_sat),
    _,
    (i32, liq_substeps),
}

// Fluid light coupling (#182 T4): `Shared.fluidgi[4]` = [gi, shadow, receive,
// sway] and `Shared.caustic[4]` = [amount, sharpness, _, _].
param_block! {
    pack_fluidgi, pack_fluidgi_preset, [4];
    (f32, fgi_gi),
    (f32, fgi_shadow),
    (bool, fgi_receive),
    (f32, fgi_sway),
}

param_block! {
    pack_caustic, pack_caustic_preset, [4];
    (f32, ca_amount),
    (f32, ca_sharpness),
    _,
    _,
}

// Liquid material + ghost light (#182 T4 follow-up): `Shared.liqmat[8]` =
// [material (0 = use scene), metallic, roughness, ior, ghost, _, _, _].
param_block! {
    pack_liqmat, pack_liqmat_preset, [8];
    (enum, liq_material),
    (f32, liq_metallic),
    (f32, liq_roughness),
    (f32, liq_ior),
    (bool, ghost_light),
    (enum, liq_render),
    (f32, liq_absorb),
    (f32, liq_glow),
}

// Liquid material block 2 (#182 T4 follow-up — the scene material's fine
// dials, liquid-local): `Shared.liqmat2[8]`.
param_block! {
    pack_liqmat2, pack_liqmat2_preset, [8];
    (f32, liq_chrome_purity),
    (f32, liq_glass_clarity),
    (f32, liq_f0),
    (f32, liq_dispersion),
    (f32, liq_gcaustic),
    (f32, liq_thin_film),
    _,
    _,
}

// Hardware ray tracing (#195): `Shared.rt[8]` = [enable, debug_view,
// shadows, shadow_soft, shadow_strength, shadow_fill, _, _]. Everything is a
// captured Look except `debug_view`, which is per-display (like HDR/MSAA —
// absent from `PresetValues`), so the preset packer writes 0 for it.
param_block! {
    pack_rt, pack_rt_preset, [8];
    (bool, rt_enable),
    (expr, |p| p.rt_debug.value().to_u32() as f32, |_pv| 0.0),
    (bool, rt_shadows),
    (f32, rt_shadow_soft),
    (f32, rt_shadow_strength),
    (bool, rt_shadow_fill),
    _,
    _,
}

// Hardware-RT reflections + AO (#195 Tiers 2+3): `Shared.rt2[8]` = [enable,
// intensity, max_roughness, reach, hit_shadows, ao_source, ao_rays, _].
// All captured Looks.
param_block! {
    pack_rt2, pack_rt2_preset, [8];
    (bool, rt_reflect),
    (f32, rt_reflect_intensity),
    (f32, rt_reflect_rough),
    (f32, rt_reflect_reach),
    (bool, rt_reflect_shadows),
    (enum, ao_source),
    (i32, rt_ao_rays),
    (i32, rt_reflect_rays),
}

// Liquid follow-ups (#182 T3a block 2): `Shared.liquid2[4]` = [offset_y,
// container shape, render reveal, _].
param_block! {
    pack_liquid2, pack_liquid2_preset, [4];
    (f32, liq_offset_y),
    (enum, liq_shape),
    (f32, liq_reveal),
    _,
}

// Z0NE rails (#187): `Shared.rails[24]`. Slot [3] is reserved for the Tier 3
// change-every (sequence transitions). Tier 2 filled [17..20] (archetype +
// the phyllo/tissue dials).
param_block! {
    pack_rails, pack_rails_preset, [24];
    (f32, rl_speed),      // [0] world units per beat
    (f32, rl_bore),       // [1] clear flight-channel radius
    (enum, rl_cell_len),  // [2] RailCellLen ordinal (1/2/4/8/16 beats)
    (enum, rl_change_every), // [3] T3: RailChangeEvery ordinal (4..64 beats)
    (f32, rl_variance),   // [4] per-cell morph depth
    (i32, rl_seed),       // [5] world seed
    (i32, rl_ring_n),     // [6] elements around the ring
    (i32, rl_rows_beat),  // [7] rows per beat along the rail
    (f32, rl_horizon),    // [8] beats visible ahead (perf dial)
    (f32, rl_rib_gain),   // [9] integer-beat rib emphasis
    (f32, rl_thickness),  // [10] element size fraction
    (i32, rl_lobes),      // [11] max superformula lobe count
    (f32, rl_spike),      // [12] profile exponent spread
    (f32, rl_twist),      // [13] turns per beat
    (f32, rl_swell),      // [14] radial profile amplitude
    (f32, rl_fade),       // [15] horizon fade-in beats
    (f32, rl_color_flow), // [16] palette cycles per beat
    (enum, rl_archetype), // [17] T2: Throat / Phyllo Wall / Gates / Tissue
    (f32, rl_diverge),    // [18] T2: phyllo divergence angle (°)
    (i32, rl_shells),     // [19] T2: tissue concentric shells
    (i32, rl_parastichy), // [20] T2: phyllo parastichy strand families
    (f32, rl_evolve),     // [21] T3: per-phrase re-roll depth
    _, _,                 // [22..23] reserved
}

// Refractive generator material: `Shared.refrmat[4]` = [absorption, overlay,
// blend, _]. `absorption` = the Beer–Lambert σ scale the cube shader applies
// when `mat_type` = Refractive (3) — the surviving colour is the node's own
// albedo, mirroring the liquid's `liq_absorb` convention. `overlay` (0/1) +
// `blend` (0..1) weave the same refracted transmission into the OTHER material
// types on top of their own shading (the refraction-overlay checkbox); both
// inert while overlay = 0.
param_block! {
    pack_refrmat, pack_refrmat_preset, [4];
    (f32, mat_absorb),
    (bool, refr_overlay),
    (f32, refr_blend),
    _,
}

// Scenery layer (#187 pivot): `Shared.scenery[16]` — the concurrent scenery
// category's mode/surface + its OWN material/FX (patched into a second
// Uniforms for the scenery draw).
param_block! {
    pack_scenery, pack_scenery_preset, [16];
    (enum, sc_mode),       // [0] 0 None / 1 Zone (the corridor)
    (enum, sc_surface),    // [1] 0 cubes / 1 rods / 2 tubes
    (enum, sc_mat),        // [2] Standard / Chrome / Glass
    (f32, sc_metallic),    // [3]
    (f32, sc_roughness),   // [4]
    (f32, sc_glow),        // [5] emissive (blooms in HDR)
    (f32, sc_opacity),     // [6]
    (f32, sc_ior),         // [7]
    (enum, sc_palette),    // [8] scenery colour LUT
    (f32, sc_sss),         // [9] translucency amount
    (f32, sc_sss_dist),    // [10]
    (f32, sc_sss_pow),     // [11]
    (f32, sc_irid),        // [12] iridescence amount
    (f32, sc_irid_scale),  // [13]
    (f32, sc_irid_shift),  // [14]
    _,                     // [15] reserved
}

// Terra scenery landform (#206 Tier 2): `Shared.terra[16]` — the landscape
// shape (timing/window shared with the rails block).
param_block! {
    pack_terra, pack_terra_preset, [16];
    (enum, terra_form),        // [0] Fjord / River / Canyon
    (f32, terra_ridge),        // [1] wall/ridge height (× scale)
    (f32, terra_channel),      // [2] channel half-width (× scale)
    (f32, terra_width),        // [3] valley half-width (× channel)
    (f32, terra_steep),        // [4] wall steepness
    (f32, terra_terrace),      // [5] canyon strata amount
    (f32, terra_rough),        // [6] fBm roughness
    (f32, terra_meander),      // [7] lateral meander amplitude
    (f32, terra_water_level),  // [8] water surface height
    (bool, terra_water_on),    // [9] water present
    (f32, terra_clearance),    // [10] navigable clearance
    (f32, terra_noise_freq),   // [11] fBm frequency
    _, _, _, _,                // [12..15] reserved
}

// Terra water surface (#206 Tier 3): `Shared.water[8]` — the channel water
// floor's own material + ripple (a Look, not latched).
param_block! {
    pack_water, pack_water_preset, [8];
    (enum, wt_mat),        // [0] Standard / Chrome / Glass
    (f32, wt_roughness),   // [1]
    (f32, wt_ior),         // [2]
    (f32, wt_opacity),     // [3]
    (f32, wt_glow),        // [4]
    (f32, wt_ripple),      // [5] ripple amplitude
    (f32, wt_ripple_freq), // [6] ripple frequency
    _,                     // [7] reserved
}

// Terra water PHYSICS (#206): `Shared.water2[8]` — the physically-real water
// shading dials (depth absorption / sun glitter / grazing reflectivity). A Look.
param_block! {
    pack_water2, pack_water2_preset, [8];
    (f32, wt_absorb),      // [0] Beer–Lambert depth darkening
    (f32, wt_glitter),     // [1] sun-sparkle on the ripples
    (f32, wt_reflect),     // [2] extra grazing reflectivity
    _, _, _, _, _,
}

// Neural Network signal propagation (#226 Tier 2): `Shared.neural_net2[8]` — the
// activation-cascade sim dials. Off (mode 0) → byte-identical to Tier 1. A Look.
param_block! {
    pack_neural_net2, pack_neural_net2_preset, [8];
    (enum, nw_fire_mode), // [0] 0 off / 1 wavefront / 2 oscillation / 3 stimulus
    (f32, nw_threshold),  // [1]
    (f32, nw_conduction), // [2] world units per beat
    (f32, nw_refractory), // [3] beats
    (f32, nw_decay),      // [4] leak per beat
    (f32, nw_deposit),    // [5] activation per arriving pulse
    (f32, nw_stim_rate),  // [6] injections per beat
    (f32, nw_motes),      // [7] signal motes (#81)
}

// Neural Network MLP look (#226 Tier 4): `Shared.neural_mlp[8]` — the loaded-weight
// MLP display dials. A Look.
param_block! {
    pack_neural_mlp, pack_neural_mlp_preset, [8];
    (f32, nw_sign_colour), // [0] signed-weight edge tint
    (f32, nw_sparsify),    // [1] drop |w| below this fraction of max
    (f32, nw_layer_gap),   // [2] layer spacing
    (f32, nw_mlp_drive),   // [3] live-input beat drive (0 = static)
    _, _, _, _,            // [4-7] reserved
}

// Neural Network attention look (#226 Tier 5): `Shared.neural_attn[8]` — the
// transformer self-attention display dials. A Look.
param_block! {
    pack_neural_attn, pack_neural_attn_preset, [8];
    (f32, nw_attn_layer),     // [0] which layer's attention
    (f32, nw_attn_head),      // [1] which head
    (f32, nw_attn_threshold), // [2] hide edges below this A_ij
    (f32, nw_attn_tokens),    // [3] synthesized token count
    (f32, nw_attn_reveal),    // [4] reveal rate (query tokens/beat)
    (f32, nw_attn_sweep),     // [5] head/layer sweep rate (steps/beat)
    (f32, nw_attn_ring),      // [6] row (0) vs ring (1) layout
    _,                        // [7] reserved
}

// Neural Tissue surface (#260 Tiers 1–4): `Shared.neural_surface[16]` — the anatomical
// surface dials. Tier 1 fills [0..5], Tier 2 [5..10], Tier 3 [10..13], Tier 4 (final)
// [13..16] (synapse). All captured Looks.
param_block! {
    pack_neural_surface, pack_neural_surface_preset, [16];
    (f32, nt_soma_size),     // [0] soma (cell-body) size multiplier
    (f32, nt_soma_shape),    // [1] round → teardrop/pyramidal anisotropy hint
    (f32, nt_bouton_size),   // [2] synaptic bouton bulb size
    (f32, nt_membrane_sss),  // [3] membrane translucency / SSS (0 = inert)
    (f32, nt_membrane_irid), // [4] membrane iridescence (0 = inert)
    (f32, nt_dendrite_density), // [5] T2 dendrite density (0 = no arbor, inert)
    (f32, nt_dendrite_length),  // [6] T2 dendrite reach (soma radii)
    (f32, nt_dendrite_taper),   // [7] T2 child/parent radius ratio (Rall taper)
    (enum, nt_neuron_type),     // [8] T2 morphology class (0 pyramidal / 1 stellate / 2 by-degree)
    (f32, nt_spines),           // [9] T2 dendritic spines (0 = off)
    (f32, nt_myelin_amount),    // [10] T3 myelin master (0 = plain capsule edges, inert)
    (f32, nt_ranvier_spacing),  // [11] T3 internodal length between Ranvier nodes
    (f32, nt_sheath_scale),     // [12] T3 fatty-myelin internode bulge vs edge thickness
    (f32, nt_synapse_cleft),    // [13] T4 synaptic cleft gap (0 = inert)
    (f32, nt_synapse_glow),     // [14] T4 cytoplasmic interior glow (0 = inert)
    (f32, nt_synapse_vesicles), // [15] T4 neurotransmitter vesicle burst (0 = off)
}

// Neural Tissue tissue-context (#260 Tier 4, final): `Shared.neural_surface2[8]` =
// [glia, capillary, _, _, _, _, _, _]. Tail block. All captured Looks.
param_block! {
    pack_neural_surface2, pack_neural_surface2_preset, [8];
    (f32, nt_glia),       // [0] astrocyte scaffolding density (0 = off)
    (f32, nt_capillary),  // [1] capillary threads (0 = off)
    _, _, _, _, _, _,     // [2..8] reserved (extracellular fog / fresnel rim follow-ups)
}

// Brain model (#275): `Shared.brain[16]` — the `NeuralTopology::Brain` layout dials.
// Tier 1 fills [0..5]; [5..16] reserved for Tiers 2–4 (tracts / callosum / parcellation
// / stimulation). Tail block after `neural_surface2`. All captured Looks.
param_block! {
    pack_brain, pack_brain_preset, [16];
    (f32, br_fold_depth),  // [0] gyri/sulci amplitude (0 = smooth ellipsoid)
    (f32, br_fold_freq),   // [1] fold frequency (~gyri count)
    (f32, br_hemi_gap),    // [2] longitudinal-fissure width
    (i32, br_local_k),     // [3] local cortical connectivity (k nearest)
    (f32, br_cerebellum),  // [4] cerebellum+brainstem fraction (0 = cerebrum only)
    (f32, br_assoc),       // [5] T2 long-range association tracts
    (f32, br_callosum),    // [6] T2 corpus callosum density
    (f32, br_subcortical), // [7] T2 subcortical nuclei + projections
    (f32, br_region_hi),   // [8] T3 target-region highlight amount (0 = off)
    (i32, br_target),      // [9] T3 target-region id (0..7)
    (f32, br_stim_amount), // [10] T4 focal-stimulation strength (0 = off)
    (f32, br_stim_rate),   // [11] T4 stimulation pulses per beat
    (f32, br_signal_swell),// [12] activation→soma-size swell (0 = glow-only, anatomy still)
    _, _, _,               // [13..16] reserved
}

// Physical thin-film interference (#258 Tier 1): `Shared.thinfilm[4]` — the real
// soap-film / bubble iridescence dials. `film_thickness` 0 → the model is OFF
// (the shader keeps the existing cosine-hack path), so the default is inert. A Look.
param_block! {
    pack_thinfilm, pack_thinfilm_preset, [4];
    (f32, film_thickness),     // [0] base film thickness (nm); 0 = model disabled
    (f32, film_thickness_var), // [1] noise-marbling amount on the thickness
    (f32, film_ior),           // [2] film refractive index
    (f32, film_drainage),      // [3] gravity-drainage gradient (top thin → bottom thick)
}

// Lens (#258 Tier 3): `Shared.lens[8]` — an analytic double-convex / plano-convex
// lens SDF's shape. All captured Looks.
param_block! {
    pack_lens, pack_lens_preset, [8];
    (f32, lens_focal),     // [0] curvature dial → sphere radius R = focal · scale
    (f32, lens_aperture),  // [1] clear-aperture radius (fraction of scale)
    (f32, lens_thickness), // [2] centre half-thickness (fraction of scale)
    (bool, lens_plano),    // [3] 0 biconvex / 1 plano-convex
    (f32, lens_scale),     // [4] world size
    (i32, lens_detail),    // [5] sphere-trace step budget
    _, _,                  // [6-7] reserved
}

// Demo scene bench (#288): `Shared.demo[8]` — the hand-authored reference-scene
// dials, live only when generator = Demo. A `DemoScene` enum + a few scalar knobs.
// Captured Generator.
param_block! {
    pack_demo, pack_demo_preset, [8];
    (enum, demo_scene),   // [0] DemoScene discriminant (Cornell / pyramid / … / light stage)
    (f32, demo_size),     // [1] overall scene scale
    (bool, demo_objects), // [2] draw the hero objects inside the box
    (bool, demo_static_cam), // [3] hold the fixed reference framing (gate orbit off)
    (f32, demo_light),    // [4] emitter / key intensity
    (f32, demo_roughness),// [5] smooth-material roughness
    (i32, demo_count),    // [6] pyramid rows / grid side / light count
    (f32, demo_spin),     // [7] turntable spin on the beat clock
}

// Hardware-RT diffuse GI (#195 Tier 4): `Shared.rt3[8]` = [enable, intensity,
// rays, reach, hit_shadows, _, _, _]. All captured Looks.
param_block! {
    pack_rt3, pack_rt3_preset, [8];
    (bool, rt_gi),
    (f32, rt_gi_intensity),
    (i32, rt_gi_rays),
    (f32, rt_gi_reach),
    (bool, rt_gi_shadows),
    (bool, rt_denoise),         // [5] #200 Tier 4½ part 2
    (f32, rt_denoise_amount),   // [6]
    _,
}

// Beat-aware temporal accumulator (#200 Tier 4½ parts 3 + 4): `Shared.rt4[8]` =
// [enable, feedback, beat_relax, variance_on, max_accum, clamp_gamma, _, _].
// All captured Looks.
param_block! {
    pack_rt4, pack_rt4_preset, [8];
    (bool, rt_temporal),
    (f32, rt_temporal_feedback),
    (f32, rt_temporal_beat),
    (bool, rt_temporal_variance),
    (f32, rt_temporal_accum),
    (f32, rt_temporal_clamp),
    _,
    _,
}

// Anisotropy (#214 Tier 1): `Shared.aniso[4]` = [amount, rotation_deg,
// overlay_enable, overlay_blend]. Drives the elliptical-GGX lobe for the
// Anisotropic material + the Standard/Chrome overlay. All captured Looks.
param_block! {
    pack_aniso, pack_aniso_preset, [4];
    (f32, anisotropy),
    (f32, aniso_rotation),
    (bool, aniso_overlay),
    (f32, aniso_blend),
}

// Surface lobes (#214 Tier 2): `Shared.coat[8]` = [clearcoat, clearcoat_rough,
// clearcoat_overlay, sheen_overlay, sheen, sheen_rough, sheen_tint, _]. Drives the
// Clearcoat/Velvet materials + the Standard/Chrome overlays. All captured Looks.
param_block! {
    pack_coat, pack_coat_preset, [8];
    (f32, clearcoat),
    (f32, clearcoat_rough),
    (bool, clearcoat_overlay),
    (bool, sheen_overlay),
    (f32, sheen),
    (f32, sheen_rough),
    (f32, sheen_tint),
    _,
}

// Neural shading foundation (#200 Tier 0): `Shared.neural[8]` = [enable, seed_a,
// seed_b, walk, omega, _, _, _]. Ships dark; all captured Looks.
param_block! {
    pack_neural, pack_neural_preset, [8];
    (bool, neural_enable),
    (i32, neural_seed_a),
    (i32, neural_seed_b),
    (f32, neural_walk),
    (f32, neural_omega),
    _,
    _,
    _,
}

// Neural field generator (#200 Tier 1): `Shared.neural2[8]` = [world_scale,
// coord_scale, iso, steps, march_relax, color_intensity, walk_rate, _].
// All captured Looks.
param_block! {
    pack_neural2, pack_neural2_preset, [8];
    (f32, neural_scale),
    (f32, neural_coord),
    (f32, neural_iso),
    (i32, neural_steps),
    (f32, neural_march),
    (f32, neural_color),
    (f32, neural_walk_rate),
    _,
}

// Neural field strand form (#200 Tier 1b): `Shared.neural3[8]` = [strands_mode,
// strands, nodes, extent, displace, _, _, _]. All captured Looks.
param_block! {
    pack_neural3, pack_neural3_preset, [8];
    (bool, neural_strands_mode),
    (i32, neural_strands_cols),
    (i32, neural_strands_rows),
    (f32, neural_strands_extent),
    (f32, neural_strands_displace),
    _,
    _,
    _,
}

// Body optics (#214 Tier 3): `Shared.body[4]` = [sss_thickness, sss_radius,
// interior_scatter, _]. Real-thickness translucency + the Glass/Refractive interior
// in-scatter glow + the Subsurface material. All captured Looks.
param_block! {
    pack_body, pack_body_preset, [4];
    (f32, sss_thickness),
    (f32, sss_radius),
    (f32, interior_scatter),
    _,
}

// Microstructure (#214 Tier 4): `Shared.micro[8]` = [glitter, glitter_density,
// glitter_sharpness, diffraction, diffraction_freq, retro, _, _]. Glitter + grating
// diffraction + retroreflection, woven into Standard/Chrome. All captured Looks.
param_block! {
    pack_micro, pack_micro_preset, [8];
    (f32, glitter),
    (f32, glitter_density),
    (f32, glitter_sharpness),
    (f32, diffraction),
    (f32, diffraction_freq),
    (f32, retro),
    _, _,
}

// Neural denoiser (#200 Tier 5a): `Shared.ndenoise[8]` = [enable, net_strength,
// seed, omega, _, _, _, _]. The kernel-predicting RT denoiser; off = the
// classical à-trous (Tier 4½). All captured Looks.
param_block! {
    pack_ndenoise, pack_ndenoise_preset, [8];
    (bool, nd_enable),
    (f32, nd_strength),
    (i32, nd_seed),
    (f32, nd_omega),
    _,
    _,
    _,
    _,
}

// Spectral emission (#214 Tier 5 pt 1): `Shared.emit[4]` = [fluorescence, fluor_hue,
// incandescence, temperature_K]. Additive emissive on every material. Captured Looks.
param_block! {
    pack_emit, pack_emit_preset, [4];
    (f32, fluorescence),
    (f32, fluor_hue),
    (f32, incandescence),
    (f32, temperature),
}

// Screen-space refraction (#214 Tier 5 pt 2): `Shared.ssrefr[4]` = [strength,
// displace, _, _]. The post-pass see-through of the scene behind the Refractive
// material. Captured Looks.
param_block! {
    pack_ssrefr, pack_ssrefr_preset, [4];
    (f32, refract_ss),
    (f32, refract_dist),
    _, _,
}

// Learned upscaler (#200 Tier 5c): `Shared.upscale[8]` = [enable, sharpen, seed,
// _, _, _, _, _]. The composite's content-adaptive-sharpen DRS upscale; off = the
// plain bilinear fetch. All captured Looks.
param_block! {
    pack_upscale, pack_upscale_preset, [8];
    (bool, up_enable),
    (f32, up_sharpen),
    (i32, up_seed),
    _,
    _,
    _,
    _,
    _,
}

// ReSTIR many-lights (#200 Tier 5d): `Shared.restir[4]` = [enable, _, _, _].
// Reservoir importance sampling of the emissive-cube light set; off = brightest-N.
// A captured Look.
param_block! {
    pack_restir, pack_restir_preset, [4];
    (bool, ml_restir),
    _,
    _,
    _,
}

// Neural Network generator (#226 Tier 1): `Shared.neural_net[16]` — graph +
// geometry controls. topology / nodes / connectivity / rewire-or-radius / layers /
// seed / extent, then the node + edge + pulse look. A captured Look. Slots 14–15
// reserved for Tier 2 (signal propagation).
param_block! {
    pack_neural_net, pack_neural_net_preset, [16];
    (enum, nw_topology),      // [0] 0 RGG / 1 layered / 2 ring / 3 small-world
    (i32, nw_nodes),          // [1]
    (i32, nw_connectivity),   // [2] neighbours (×2 = k) or layered fan-out
    (f32, nw_rewire),         // [3] rewire prob / connection radius
    (i32, nw_layers),         // [4] feed-forward layers
    (i32, nw_seed),           // [5]
    (f32, nw_extent),         // [6]
    (f32, nw_node_size),      // [7]
    (f32, nw_node_glow),      // [8]
    (f32, nw_edge_thickness), // [9]
    (f32, nw_edge_bow),       // [10]
    (i32, nw_edge_samples),   // [11]
    (f32, nw_pulse_speed),    // [12]
    (f32, nw_pulse_width),    // [13]
    _, _,                     // [14-15] reserved (Tier 2 signal propagation)
}

// Neural Network edges/somas (#226 Tier 1.5): `Shared.neural_edge[8]` — the axon-
// bundle edge + dendritic-soma controls. `[edge_fibres, bundle_radius, node_dip,
// ranvier_nodes, dendrite, dendrite_count, _, _]`. A captured Look.
param_block! {
    pack_neural_edge, pack_neural_edge_preset, [8];
    (i32, nw_edge_fibres),    // [0] 1 = single tube; >1 = myelinated bundle
    (f32, nw_bundle_radius),  // [1]
    (f32, nw_edge_node_dip),  // [2] Ranvier constriction depth
    (i32, nw_ranvier),        // [3] Ranvier nodes per edge
    (f32, nw_dendrite),       // [4] dendrite length (0 = plain soma)
    (i32, nw_dendrite_count), // [5] sprouts per soma
    _, _,                     // [6-7] reserved
}

// Maxwell field energization (#247): `Shared.maxenergy[8]` = [energize, gain, knee,
// hue, antenna_len(T2), antenna(T2), dye_inject(T3), _]. Lights the Particle Aura by
// the field's energy density. Captured Looks.
param_block! {
    pack_maxenergy, pack_maxenergy_preset, [8];
    (bool, mn_energize),
    (f32, mn_gain),
    (f32, mn_knee),
    (f32, mn_hue),
    (f32, mn_antenna_len),
    (bool, mn_antenna),
    (f32, mn_dye_inject),
    (f32, mx_aura_blend),
}

// Audio-driven dipole radiation (#248): `Shared.audiodip[8]` = [drive_on, amount,
// floor, multipole(T2), spread(T2), band_hue(T2), _, _]. Tier 1: the broadband RMS
// envelope (`audio[5]`) scales the Maxwell source's drive amplitude — the energy
// cloud breathes with the music. Tier 2: the five band envelopes drive distinct
// multipole moments (spectrum → spatial mode structure) with a per-band wavelength
// spread + colour-by-band. [6..8] reserved for Tier 3 (stereo/pitch/beat).
// Captured Motion (an audio-reactivity coupling).
param_block! {
    pack_audiodip, pack_audiodip_preset, [8];
    (bool, ad_drive),
    (f32, ad_amount),
    (f32, ad_floor),
    (bool, ad_multipole),
    (f32, ad_spread),
    (f32, ad_band_hue),
    (f32, ad_stereo),
    (f32, ad_pitch),
}

// Audio-dipole Tier 3 (#248): `Shared.audiodip2[4]` = [wave, _, _, _]. The waveform
// shells (recent loudness history → radial energy modulation). Captured Motion.
param_block! {
    pack_audiodip2, pack_audiodip2_preset, [4];
    (f32, ad_wave),
    _, _, _,
}

// Field-force particle drive (#248): `Shared.mxforce[4]` = [force_on, force_gain,
// energy_contrast, _]. The Maxwell energization drives the aura/fluid with the E
// field as a real body force (magnitude + sign) — stirred by the force, strong near
// the core — instead of following field lines at constant speed; contrast sharpens
// the near-core glow. Captured Look (an energy-cloud look, like the tone-map dials).
param_block! {
    pack_mxforce, pack_mxforce_preset, [4];
    (bool, mn_force),
    (f32, mn_force_gain),
    (f32, mn_energy_contrast),
    (f32, mn_stir_rate),
}

// Acoustic pump + beat coupling (#248): `Shared.mxforce2[4]` = [pump_amount, swirl_beat,
// pump_scale, _]. The beat drives an axial pump (speaker-pushing-air) + the swirl's
// momentum on top of the force drive. Captured Look.
param_block! {
    pack_mxforce2, pack_mxforce2_preset, [4];
    (f32, mn_pump),
    (f32, mn_swirl_beat),
    (f32, mn_pump_scale),
    (f32, mn_swirl_decay),
}

// Beat-mode crossfade + hue cycle (#248): `Shared.mxforce3[4]` = [mode_mix, ring_freq,
// hue_cycle, _]. `mode_mix` blends turbine↔dynamo; `hue_cycle` advances the energized
// motes' hue per beat pulse (pump-driven colour cycle). Captured Look.
param_block! {
    pack_mxforce3, pack_mxforce3_preset, [4];
    (f32, mn_mode_mix),
    (f32, mn_ring_freq),
    (f32, mn_hue_cycle),
    _,
}

// Shaded particle beads (#298 Tier 1): `Shared.pbeads[8]` = [beads, metallic,
// roughness, _, _, _, _, _]. `beads` swaps the additive spark motes for opaque
// sphere-impostor droplets shaded by the shared IBL + key/fill; metallic/roughness
// are their PBR material. beads 0 → additive sparks (byte-identical). Captured Look.
param_block! {
    pack_pbeads, pack_pbeads_preset, [8];
    (bool, particles_beads),
    (f32, particles_metallic),
    (f32, particles_roughness),
    (enum, particles_material),   // #298 Tier 2
    (enum, particles_shape),      // #298 Tier 2
    (f32, particles_ior),         // #298 Tier 2
    (f32, particles_shape_param), // #298 Tier 2
    (bool, particles_beads_rt),   // #298 Tier 4
}

// Per-material Hue/Saturation/Value (#305 Tier 1): `Shared.matcol[8]` = generator
// [hue, hue_cycle, saturation, value] ++ scenery [hue, hue_cycle, saturation, value].
// Identity ([0,0,1,1] each) → byte-identical. Captured Look.
param_block! {
    pack_matcol, pack_matcol_preset, [8];
    (f32, mat_hue),
    (f32, mat_hue_cycle),
    (f32, mat_saturation),
    (f32, mat_value),
    (f32, scen_hue),
    (f32, scen_hue_cycle),
    (f32, scen_saturation),
    (f32, scen_value),
}

// Bead Hue/Saturation/Value (#305 Tier 1): `Shared.pbeads2[4]` = [hue, hue_cycle,
// saturation, value] for the shaded particle beads. Identity [0,0,1,1]. Captured Look.
param_block! {
    pack_pbeads2, pack_pbeads2_preset, [4];
    (f32, particles_bead_hue),
    (f32, particles_bead_hue_cycle),
    (f32, particles_bead_sat),
    (f32, particles_bead_val),
}

// Live-sky cloud reflections (#305 Tier 2): `Shared.skyrefl[4]` = [enable, cover,
// speed, strength]. enable 0 → byte-identical. Captured Look.
param_block! {
    pack_skyrefl, pack_skyrefl_preset, [4];
    (bool, sky_reflect_clouds),
    (f32, sky_cloud_cover),
    (f32, sky_cloud_speed),
    (f32, sky_cloud_strength),
}

// Neural radiance cache — live (#256 Tier 0): `Shared.nrc[8]` = [enable, confidence,
// learn_rate, omega, terminate_bounce, train_samples, seed, _]. enable 0 →
// byte-identical (no query, no upload). Captured Look.
param_block! {
    pack_nrc, pack_nrc_preset, [8];
    (bool, nrc_enable),
    (f32, nrc_confidence),
    (f32, nrc_learn_rate),
    (f32, nrc_omega),
    (i32, nrc_terminate),
    (i32, nrc_train_samples),
    (i32, nrc_seed),
    _,
}

// Neural radiance cache — RT-stack synergies (#256 Tier 1): `Shared.nrc2[4]` =
// [guide_on, guide_candidates, firefly_on, firefly_clamp]. guide/firefly off →
// byte-identical. Captured Look.
param_block! {
    pack_nrc2, pack_nrc2_preset, [4];
    (bool, nrc_guide),
    (i32, nrc_guide_candidates),
    (bool, nrc_firefly),
    (f32, nrc_firefly_clamp),
}

// Neural radiance cache — light-field uses (#256 Tier 2): `Shared.nrc3[4]` =
// [gi_on, gi_strength, reflect_terminate, _]. All off → byte-identical. Captured Look.
param_block! {
    pack_nrc3, pack_nrc3_preset, [4];
    (bool, nrc_gi),
    (f32, nrc_gi_strength),
    (bool, nrc_reflect),
    _,
}

// Neural radiance cache — hard transport + volumetrics (#256 Tier 3): `Shared.nrc4[8]`
// = [volume_on, volume_density, volume_steps, volume_strength, caustic_on,
// caustic_gain, _, _]. All off → byte-identical. Captured Look.
param_block! {
    pack_nrc4, pack_nrc4_preset, [8];
    (bool, nrc_volume),
    (f32, nrc_volume_density),
    (i32, nrc_volume_steps),
    (f32, nrc_volume_strength),
    (bool, nrc_caustic),
    (f32, nrc_caustic_gain),
    _,
    _,
}

param_block! {
    pack_kifs, pack_kifs_preset, [30];
    (f32, kf_sectors),
    (f32, kf_fold),
    (i32, kf_iter),
    (f32, kf_iter_rot),
    (f32, kf_spin),
    (f32, kf_breathe),
    (f32, kf_zoom),
    (bool, kf_tunnel),
    (i32, kf_rays),
    (f32, kf_ring),
    (f32, kf_glow),
    (f32, kf_hue),
    (enum, kf_pattern),
    (enum, kf_palette),
    (f32, kf_color_speed),
    (f32, kf_warp),
    (f32, kf_flow),
    (f32, kf_churn),
    (i32, kf_petals),
    (f32, kf_contrast),
    (f32, kf_sharp),
    (f32, kf_invert),
    (f32, kf_dispersion),
    (enum, kf_space),
    (f32, kf_e8_flow),
    (f32, kf_relief),
    (f32, kf_relief_elev),
    (i32, kf_relief_steps),
    (f32, kf_relief_shine),
    (enum, kf_view),
}

// Capture / production frame (#135 Phase 1): `Shared.capture[12]`. Param-only —
// per-display, so presets neither capture nor apply it (like terrain/stars); a
// held-key preset keeps the live value (lib.rs restores `snapshot.capture`).
param_block! {
    pack_capture, pack_capture_preset, [12];
    (enum, aspect_preset), // [0] 0 Native / 1 9:16 / 2 16:9 / 3 1:1 / 4 4:5 / 5 21:9 / 6 Custom
    (i32, out_long_edge),  // [1] output long-edge px (short edge from the aspect)
    (i32, out_custom_w),   // [2] Custom width px
    (i32, out_custom_h),   // [3] Custom height px
    (f32, letterbox_r),    // [4] letterbox bar colour (linear)
    (f32, letterbox_g),    // [5]
    (f32, letterbox_b),    // [6]
    (bool, frame_guide),   // [7] safe-area border
    (bool, lock_window),   // [8] window inner-size = output size
    _, _, _,               // [9..12] reserved
}

// Capture overlay style (#135 Phase 2): `Shared.overlay[16]`. Param-only / per-display,
// like `capture` — presets neither capture nor apply it; a held-key preset keeps the live
// value (lib.rs restores `snapshot.overlay`).
param_block! {
    pack_overlay, [16];
    (bool, overlay_enabled),       // [0] master on/off
    (f32, overlay_opacity),        // [1] whole-overlay alpha
    (f32, overlay_scale),          // [2] font/zone scale
    (bool, overlay_title),         // [3] zone toggles
    (bool, overlay_desc),          // [4]
    (bool, overlay_formula),       // [5]
    (bool, overlay_readouts),      // [6]
    (bool, overlay_handle),        // [7]
    (f32, overlay_panel_r),        // [8] readout-panel fill colour
    (f32, overlay_panel_g),        // [9]
    (f32, overlay_panel_b),        // [10]
    (f32, overlay_panel_opacity),  // [11]
    (f32, overlay_text_r),         // [12] default text colour
    (f32, overlay_text_g),         // [13]
    (f32, overlay_text_b),         // [14]
    _,                             // [15] reserved
}

// Capture decoration: 3-D axes + wireframe box (#135 Phase 5): `Shared.axes[16]`.
// Param-only / per-display, like `capture`/`overlay`.
param_block! {
    pack_axes, [16];
    (bool, axes_on),     // [0] master XYZ axes
    (f32, axes_len),     // [1] axis length (world units)
    (bool, axes_ticks),  // [2] tick marks
    (bool, axes_labels), // [3] projected X/Y/Z labels
    (f32, axes_opacity), // [4] axis line alpha
    (bool, box_on),      // [5] wireframe bounding box
    (f32, box_extent),   // [6] box half-size
    (i32, box_subdiv),   // [7] grid subdivisions per edge
    (f32, box_r),        // [8] box/grid colour
    (f32, box_g),        // [9]
    (f32, box_b),        // [10]
    (f32, box_opacity),  // [11]
    (f32, axes_thick),   // [12] axis tube radius (world units)
    _, _, _,             // [13..16] reserved
}

// Post-composite creative FX (#152, Tier 1): `Shared.fx[16]`. A Look — preset-
// captured (symmetric packers), applied on the composited image by `fx.wgsl`.
param_block! {
    pack_fx, pack_fx_preset, [16];
    (bool, fx_enabled),       // [0] master on/off
    (enum, fx_style),         // [1] None / Toon / Outline / Halftone / Dither / Pixelate
    (f32, fx_style_amt),      // [2] style strength (bands / edge / dot / pixel size)
    (f32, fx_dof),            // [3] depth-of-field amount
    (f32, fx_dof_focus),      // [4] focus plane (0..1 raw depth)
    (f32, fx_dof_range),      // [5] in-focus band width
    (f32, fx_chroma),         // [6] chromatic aberration
    (f32, fx_vignette),       // [7] vignette
    (f32, fx_grain),          // [8] film grain
    (f32, fx_grade_sat),      // [9] saturation (1 = neutral)
    (f32, fx_grade_contrast), // [10] contrast (1 = neutral)
    (f32, fx_grade_temp),     // [11] temperature (0 = neutral)
    (f32, fx_grade_gain),     // [12] gain (1 = neutral)
    (f32, fx_feedback),       // [13] echo-trail persistence
    (f32, fx_outline),        // [14] outline edge threshold
    _,                        // [15] reserved
}

// Emissive volume surface mode (#152, Tier 1): `Shared.volume[8]`. A Surface look
// (preset-captured, Generator tab). Reuses the metaball field bake.
param_block! {
    pack_volume, pack_volume_preset, [8];
    (f32, volume_radius),     // [0] field blob radius (reuses the metaball bake)
    (f32, volume_density),    // [1] density multiplier
    (f32, volume_emission),   // [2] emissive glow strength
    (f32, volume_absorption), // [3] extinction (Beer–Lambert)
    (i32, volume_steps),      // [4] raymarch step budget
    _, _, _,                  // [5..8] reserved
}

// Temporal pass (#152 Tier 2): `Shared.temporal[8]`. Param-only — a per-display /
// quality setting (like MSAA / capture), so presets neither capture nor apply it.
param_block! {
    pack_temporal, [8];
    (bool, taa_enabled),     // [0]
    (f32, taa_blend),        // [1] current-frame weight
    (f32, taa_sharpen),      // [2]
    (bool, motion_blur),     // [3]
    (f32, mb_amount),        // [4] shutter strength
    (i32, mb_samples),       // [5] taps
    (bool, stochastic_glass),// [6] dither-discard OIT (needs TAA)
    _,                       // [7] reserved
}

// Screen-space GI (#152 Tier 2): `Shared.ssgi[4]`. A Look — preset-captured.
param_block! {
    pack_ssgi, pack_ssgi_preset, [4];
    (bool, ssgi),            // [0] enabled
    (f32, ssgi_intensity),   // [1]
    (f32, ssgi_radius),      // [2] view-space gather radius
    (i32, ssgi_rays),        // [3]
}

// Cast shadows (#152 Tier 3): `Shadow.shadow[4]`. A Look — preset-captured.
param_block! {
    pack_shadow, pack_shadow_preset, [4];
    (bool, shadow_enabled), // [0]
    (f32, shadow_bias),     // [1] depth bias (kills acne)
    (f32, shadow_strength), // [2] 0..1 shadow darkness
    _,                      // [3] reserved
}

// Voxel GI (#152 Tier 3, #10): `Shared.vxgi[4]`. A Look — preset-captured.
param_block! {
    pack_vxgi, pack_vxgi_preset, [4];
    (bool, vxgi_enabled),   // [0]
    (f32, vxgi_intensity),  // [1]
    (i32, vxgi_rays),       // [2] hemisphere rays per pixel
    (i32, vxgi_steps),      // [3] march steps per ray
}

// Reflection controls (#163 Tier 1): `Shared.reflect[4]` = [reflect_tint,
// chrome_purity, glass_clarity, f0_override]. A Look — preset-captured. All 0 →
// today's chrome/glass/standard look byte-identical.
param_block! {
    pack_reflect, pack_reflect_preset, [4];
    (f32, reflect_tint),   // [0] palette influence on the reflection
    (f32, chrome_purity),  // [1] Chrome → pure neutral mirror
    (f32, glass_clarity),  // [2] Glass → colourless clear glass
    (f32, f0_override),    // [3] Standard reflectance lift (mirror w/o metallic)
}

// Reflection probe / parallax (#163 Tier 2): `Shared.refl_probe[4]` = [source_id,
// box_scale, box_height_scale, blend]. A Look — preset-captured. source 0 (EnvOnly) →
// today's look. The visual turns box_scale/height + the live field AABB into the box.
param_block! {
    pack_refl_probe, pack_refl_probe_preset, [4];
    (enum, refl_source),      // [0] 0 EnvOnly / 1 Parallax
    (f32, refl_box_scale),    // [1] XZ AABB half-extent multiplier
    (f32, refl_box_height),   // [2] Y AABB half-extent multiplier
    (f32, refl_blend),        // [3] 0..1 corrected↔infinite mix
}

// VXGI specular cone tracing (#163 Tier 3): `Shared.vxgi_spec[4]` = [strength,
// aperture, reach_frac, steps]. A Look — preset-captured. strength 0 → off (today).
param_block! {
    pack_vxgi_spec, pack_vxgi_spec_preset, [4];
    (f32, vxgi_spec_strength), // [0] reflection strength (0 = off)
    (f32, vxgi_spec_aperture), // [1] cone widening (glossiness)
    (f32, vxgi_spec_reach),    // [2] march reach as a fraction of the scene diagonal
    (i32, vxgi_spec_steps),    // [3] cone march steps (perf/quality)
}

// Membrane screen-space FX opt-in: `Shared.membrane_fx[4]` = [enabled, _, _, _]. A Look
// — preset-captured. enabled 0 → membrane skips the depth prepass (today's look).
param_block! {
    pack_membrane_fx, pack_membrane_fx_preset, [4];
    (bool, membrane_fx),          // [0] draw membrane into the depth prepass (screen-space FX on)
    (enum, membrane_arm_build),   // [1] Skin-Arms build: 0 Impostor (capsules) / 1 Mesh (welded)
    (f32, membrane_arm_radius),   // [2] Skin-Arms capsule radius (0 = auto per-node thickness)
    _,                            // [3] reserved
}

// Cinematic finishing (#167 Tier 1): `Shared.finishing[8]` = halation (0..3) + lens
// flares (4..7). A Look — preset-captured. Both amounts 0 → today's look (they only
// act inside the FX pass anyway). Halation/flares live in fx.wgsl.
param_block! {
    pack_finishing, pack_finishing_preset, [8];
    (f32, hal_amount),    // [0] halation strength (0 = off)
    (f32, hal_threshold), // [1] bright-pass threshold
    (f32, hal_width),     // [2] halo radius
    (f32, hal_warmth),    // [3] red-bleed tint amount
    (f32, lf_amount),     // [4] lens-flare strength (0 = off)
    (f32, lf_ghosts),     // [5] ghost intensity
    (f32, lf_halo),       // [6] halo-ring intensity
    (f32, lf_streak),     // [7] anamorphic streak intensity
}

// Emissive cubes as real lights (#167 Tier 3): `Shared.manylight[4]` = [enabled,
// intensity, radius_frac, count]. A Look — preset-captured. enabled 0 → count 0 → off.
param_block! {
    pack_manylight, pack_manylight_preset, [4];
    (bool, ml_enabled),  // [0] on/off
    (f32, ml_intensity), // [1] emitted-radiance scale
    (f32, ml_radius),    // [2] falloff radius as a fraction of the scene diagonal
    (i32, ml_count),     // [3] how many of the brightest cubes to use
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::Shared;

    // --- Layout golden (#103): the Shared IPC byte layout must not change. ----
    // The plugin (writer) and visual (reader) are separate processes, so any size
    // or offset drift silently corrupts every field the visual reads. These are
    // default-independent (pure layout), so they only fail on a real layout change.
    // 2196 + synchrotron[24] (#150) = 2292; + fx[16] + volume[8] (#152 Tier 1) = 2388;
    // + temporal[8] + ssgi[4] (#152 Tier 2) = 2436; + shadow[4] + vxgi[4] (#152 Tier 3)
    // = 2468; + reflect[4] (#163 Tier 1) = 2484; + refl_probe[4] (#163 Tier 2) = 2500;
    // + vxgi_spec[4] (#163 Tier 3) = 2516; + membrane_fx[4] = 2532; + finishing[8]
    // (#167 Tier 1) = 2564; + manylight[4] (#167 Tier 3) = 2580; + vecfield[24]
    // (#173 Tier 1) = 2676; + vecbuild[64] (#173 Tier 3) = 2932; + fluidvis[12]
    // (#182 Tier 1) = 2980; + fluid2[8] (#182 Tier 2) = 3012; + liquid[16]
    // (#182 Tier 3a) = 3076; + liquid2[4] (T3a block 2) = 3092; + fluidgi[4]
    // + caustic[4] (#182 Tier 4) = 3124; + liqmat[8] (T4 follow-up) = 3156;
    // + liqmat2[8] = 3188; + rails[24] (#187 Tier 1) = 3284; + rt[8]
    // (#195 Tier 0) = 3316; + refrmat[4] (Refractive material) = 3332;
    // + rt2[8] (#195 Tier 2) = 3364; + scenery[16] (#187 pivot) = 3428;
    // + rt3[8] (#195 Tier 4) = 3460; + rt4[8] (#200 Tier 4½ p3) = 3492;
    // + neural[8] = 3524; + aniso[4] = 3540; + axon[24] = 3636; + coat[8] = 3668;
    // + terra[16] = 3732; + neural2[8] = 3764; + neural3[8] = 3796; + body[4] = 3812;
    // + micro[8] = 3844; + water[8] (#206 T3) = 3876; + water2[8] (water physics) = 3908;
    // + pathtrace_on: u32 (#200 Tier 4, re-seated after water2) = 3912;
    // + ndenoise[8] (#200 Tier 5a, re-seated after pathtrace_on) = 3944;
    // + emit[4] (#214 Tier 5 pt 1, re-seated after ndenoise on the main re-merge) = 3960;
    // + ssrefr[4] (#214 Tier 5 pt 2, re-seated after emit on the main re-merge) = 3976;
    // + upscale[8] (#200 Tier 5c, re-seated after ssrefr) = 4008;
    // + restir[4] (#200 Tier 5d, re-seated after upscale) = 4024;
    // + neural_net[16] (#226 Tier 1, re-seated after restir on the main re-merge) = 4088;
    // + neural_edge[8] (#226 Tier 1.5, axon-bundle edges) = 4120;
    // + maxenergy[8] (#247 Tier 1, Maxwell field energization, re-seated after neural_edge) = 4152;
    // + neural_net2[8] (#226 Tier 2, re-seated after maxenergy on the main re-merge) = 4184;
    // + nn_gen u32 (#226 Tier 3 connectome load counter, re-landed after neural_net2) = 4188;
    // + neural_mlp[8] (#226 Tier 4 MLP look) = 4220;
    // + neural_attn[8] (#226 Tier 5 attention look) = 4252;
    // + tube[4] (PR #276 welded Swept Tubes, appended after neural_attn on main) = 4268;
    // + neural_surface[16] (#260 Tier 1 Neural Tissue, appended after tube; LAYOUT_VERSION bumped to 0x0251) = 4332;
    // + neural_surface2[8] (#260 Tier 4 tissue context; tail append after neural_surface) = 4364;
    // + brain[16] (#275 Tier 1 brain model; tail append after neural_surface2) = 4428;
    // + thinfilm[4] (#258 Tier 1, tail append after brain) = 4444;
    // + ptglass[4] (#258 Tier 2 path-tracer dielectric BTDF; tail append after thinfilm) = 4460;
    // + tube_profile: f32 (PR #276 follow-up: welded cross-section circle↔square; LAYOUT_VERSION 0x0251→0x0252) = 4464;
    // + lens[8] (#258 Tier 3 analytic lens SDF; tail append after tube_profile) = 4496;
    // + spectral[4] (#258 Tier 4 spectral light transport; tail append after lens) = 4512;
    // + demo[8] (#288 Demo scene bench; tail append after spectral) = 4544;
    // + audiodip[8] (#248 Tier 1 audio-driven dipole; tail append after demo) = 4576;
    // + mxforce[4] (#248 field-force particle drive; tail append after audiodip) = 4592;
    // + mxforce2[4] (#248 acoustic pump + beat coupling; tail append after mxforce) = 4608;
    // + mxforce3[4] (#248 coupled dynamo mode; tail append after mxforce2) = 4624;
    // + pbeads[8] (#298 Tier 1 shaded particle beads; tail append after mxforce3) = 4656;
    // + audiodip2[4] (#248 Tier 3 waveform shells; tail append after pbeads) = 4672;
    // + matcol[8] (#305 Tier 1 generator+scenery HSV; tail append after audiodip2) = 4704;
    // + pbeads2[4] (#305 Tier 1 bead HSV; tail append after matcol) = 4720;
    // + ptcaustic[4] (#258 Tier 5 photon-mapped caustics; tail append after pbeads2) = 4736;
    // + skyrefl[4] (#305 Tier 2 live-sky cloud reflections; tail append after ptcaustic) = 4752;
    // + cam_seq[4]+cam_dolly[4]+cam_clock[4]+cam_audio[4] (#307 Tier 1 cinematic camera;
    //   tail append after skyrefl on the #314↔main merge; LAYOUT_VERSION 0x0252→0x0253) = 4816;
    // + cam_frame[8] (#307 Tier 2 roll/FOV/framing; tail append after cam_audio on the
    //   #316↔main merge; LAYOUT_VERSION 0x0253→0x0254) = 4848;
    // + cam_story[24] (#307 Tier 3 storyboard; tail append after cam_frame;
    //   LAYOUT_VERSION 0x0254→0x0255) = 4944.
    // + nrc[8] (#256 Tier 0 live neural radiance cache; tail append after cam_story;
    //   LAYOUT_VERSION 0x0255→0x0256) = 4976.
    // + nrc2[4] (#256 Tier 1 NRC-guided sampling + firefly clamp; tail append after nrc;
    //   LAYOUT_VERSION 0x0256→0x0257) = 4992.
    // + nrc3[4] (#256 Tier 2 cache GI supersedes DDGI + lit reflections; tail append
    //   after nrc2; LAYOUT_VERSION 0x0257→0x0258) = 5008.
    // + nrc4[8] (#256 Tier 3 cache volumetrics + cached caustics; tail append after
    //   nrc3; LAYOUT_VERSION 0x0258→0x0259) = 5040.
    // + acoustic[16] (#325 acoustic Duo-Field generator; re-seated to the true tail
    //   after nrc4 on the main merge; LAYOUT_VERSION 0x0259→0x025A) = 5104.
    // + acoustic2[8] (#325 Tier 4 cavity Chladni modes + intensity flux; tail append
    //   after acoustic; LAYOUT_VERSION 0x025A→0x025B) = 5136.
    // + acoustic3[8] (#325 Tier 5 cavity 3-D tween + per-axis audio breathe; tail append
    //   after acoustic2; LAYOUT_VERSION 0x025B→0x025C) = 5168.
    // + sonify[16] (#339 Tier 1 Duo-Field synthesis Sound card; re-seated after
    //   acoustic3 on the #336 merge) = 5232.
    // + voices[64] (#339 runtime-written played-note radiators; LAYOUT_VERSION 0x025C→0x025D) = 5488.
    // + audiometer[16] (#333 Tiers 1–2 calibrated LUFS/dBTP/LRA/correlation; tail append
    //   after voices on the #339 merge; LAYOUT_VERSION 0x025D→0x025E) = 5552.
    // + audiospectrum[128] (#333 Tier 2 calibrated RTA band levels; tail append after
    //   audiometer; LAYOUT_VERSION 0x025E→0x025F) = 6064.
    // + analytical[8] (#333 Tier 3 Analyzer/Calibrated instrument mode; tail append after
    //   audiospectrum; LAYOUT_VERSION 0x025F→0x0260) = 6096.
    // + fieldvol[8] (#348 Field Volume density-cloud source/exposure) + colour[8] (#349
    //   calibrated cross-cutting tint); both tail-appended after analytical; LAYOUT_VERSION
    //   0x0260→0x0261 (one bump, both blocks) = 6160.
    // + scopewave[260] (#346 Tier 1 triggered oscilloscope display frame, runtime) +
    //   chamber[16] (#346 Field Chamber panel look, captured Look); both re-seated to the
    //   true tail after colour on the main merge; LAYOUT_VERSION 0x0261→0x0262 = 7264.
    // + emissive[4] (Material Emissive, HDR self-emission in the surface's own colour;
    //   tail append after chamber; LAYOUT_VERSION 0x0262→0x0263) = 7280.
    // + splat[8] (Gaussian Splatting surface — SurfaceMode::Splat = 8; tail append after
    //   emissive; LAYOUT_VERSION 0x0263→0x0264) = 7312.
    // + plexus[4] (Plexus surface-mode controls; tail append after splat; LAYOUT_VERSION
    //   0x0264→0x0265) = 7328.
    // + plexus2[4] + plexus_node_mat[8] + plexus_edge_mat[8] (Tier 2 impostors +
    //   independent materials; tail append; LAYOUT_VERSION 0x0265→0x0266) = 7408.
    // + plexus3[4] (Tier 3 signal propagation; tail append; LAYOUT_VERSION 0x0266→0x0267) = 7424.
    // + plexus4[4] (Tier-1 shape morph; tail append; LAYOUT_VERSION 0x0267→0x0268) = 7440.
    // + splat2[4] (Splat Tier 3 solidity; tail append after plexus4; LAYOUT_VERSION 0x0268→0x0269) = 7456.
    // + mx_eb[4] (Maxwell E↔B phase dial; tail append after splat2; LAYOUT_VERSION 0x0269→0x026A) = 7472.
    // + plexus_overlay[4] (Plexus overlay outer-shell controls; tail append after mx_eb;
    //   LAYOUT_VERSION 0x026A→0x026B) = 7488.
    // + field[10] + field_gen (#381 Tier 1 Field Engine live coefficients + program-load
    //   counter; tail append after plexus_overlay; LAYOUT_VERSION 0x026B→0x026C) = 7532.
    // + mapattractor[10] (#380 Density-Map Attractor + a_drive/b_drive; tail append after
    //   field_gen; LAYOUT_VERSION 0x026C→0x026D) = 7572.
    // + origin_mode: u32 (Original cube-field Corner/Centered origin; tail append after
    //   mapattractor; LAYOUT_VERSION 0x026D→0x026E) = 7576.
    // + maporbit[8] (#380 Tier 2 beat-locked parameter orbit; tail append after
    //   origin_mode; LAYOUT_VERSION 0x026E→0x026F) = 7608.
    // + agent[8] (#317 Tier 1 AI-Performer runtime block; tail append after
    //   maporbit; LAYOUT_VERSION 0x026F→0x0270) = 7640.
    // + mind[8] (#367 Tier 1 visible-mind specimen; runtime-stamped mind_on/model_gen/
    //   topo_mode; tail append after agent; LAYOUT_VERSION 0x0270→0x0271) = 7672.
    // + mapattractor2[4] (#380 Tier 3 extra map coefficients c/d + colour-by-dynamics
    //   mode; re-seated after mind on the #389/#390 merge; LAYOUT_VERSION 0x0271→0x0272) = 7688.
    // + fieldsim[8] (#381 Tier 3 time-marched PDE sim; re-seated after mapattractor2 on
    //   the #393 merge; LAYOUT_VERSION 0x0272→0x0273) = 7720.
    // + kaleido[16] (#361 Tier 1 Scene Kaleidoscope; re-seated to the true tail after
    //   fieldsim on the #363↔main sync; LAYOUT_VERSION 0x0273→0x0274) = 7784.
    // + instrument[16] (#391 Tier 1 Quantitative Instrumentation — field probes +
    //   energy ledger + Poynting flux + CSV; tail append after kaleido;
    //   LAYOUT_VERSION 0x0274→0x0275) = 7848.
    // + instrument2[8] (#391 Tier 1 instrumentation-HUD presentation — panel
    //   opacity/bevel + size + dock; tail append after instrument;
    //   LAYOUT_VERSION 0x0275→0x0276) = 7880.
    // + atlas[8] (#423 Tier 1 the atlas — runtime-stamped design-space control:
    //   gen counter + on/roofline toggles; tail append after instrument2;
    //   LAYOUT_VERSION 0x0276→0x0277) = 7912.
    // + fieldclip_gen: u32 (#407 Tier A Field Playback clip-load counter; runtime-
    //   stamped, tail append after atlas; LAYOUT_VERSION 0x0277→0x0278) = 7916.
    // + nca_gen: u32 (#407 Tier B Neural CA learned-surrogate model-load counter;
    //   runtime-stamped, tail-appended after fieldclip_gen; LAYOUT_VERSION 0x0278→0x0279) = 7920.
    // + fdtd[8] (#412 Tier 3 Phase 0 FDTD Maxwell solver; tail append after
    //   nca_gen; LAYOUT_VERSION 0x0279→0x027A) = 7952.
    // + bevel: f32 (node bevel — cube→sphere rounded-box morph; tail append after
    //   fdtd; LAYOUT_VERSION 0x027A→0x027B) = 7956.
    // + creature[8] (#476 Tier 1 Creature Engine — SDF-raymarched sea creatures;
    //   tail append after bevel; LAYOUT_VERSION 0x027B→0x027C) = 7988.
    // + creature2[8] (#476 Tier 2a metachronal wave; tail append after creature;
    //   LAYOUT_VERSION 0x027C→0x027D) = 8020.
    // + creature_gen: u32 (#476 Tier 2b JSON body-plan load counter; runtime-stamped,
    //   tail append after creature2; LAYOUT_VERSION 0x027D→0x027E) = 8024.
    // MERGE of main's 0x0280 layout (with creature3) and the #472 Tier 2/3 material
    // stack, tail-appended after creature_gen in declared order:
    // + creature3[4] (#476 Tier 2c anatomy overlay) = 8024 + 16 = 8040.
    // + material[8] (#472 Tier 1 PBR texture set) = 8040 + 32 = 8072.
    // + material_gen: u32 (#472 Tier 1 runtime load counter) = 8072 + 4 = 8076.  (0x0280)
    // + material_layer[18] + material_grad[8] (#472 Tier 2 single-layer noise graph;
    //   after material_gen) = 8076 + 72 + 32 = 8180.  (0x0281)
    // + material_layer2[18] + material_grad2[8] + material_derive[8] (#472 Tier 3 layer
    //   stack + blend + derived normal/AO; after material_grad) = 8180 + 72 + 32 + 32
    //   = 8316.  (0x0282)
    // + material_live[8] (#472 Tier 5 animation + height→vertex displacement + reserved
    //   audio-drive; tail append after material_derive; LAYOUT_VERSION 0x0282→0x0283) = 8348.
    // + the #541 S2 Tier 1 mindview spine (pane→lens selector; a RESERVATION — nothing
    //   writes it, all-zero = today's single viewport): mindview[8] = 8348 + 32 = 8380,
    //   mindview_pane[4*8] = 8380 + 128 = 8508, mindview_gen: u32 = 8508 + 4 = 8512.
    //   Tail append after material_live; LAYOUT_VERSION 0x0283→0x0284.
    // + the organon#217 T3 PBR-text look controls: glyph[16] = 8512 + 64 = 8576,
    //   glyph_cam[8] = 8576 + 32 = 8608, capsule[4] = 8608 + 16 = 8624. Tail append after
    //   mindview_gen; LAYOUT_VERSION 0x0285→0x0286.
    const EXPECTED_SHARED_SIZE: usize = 8624;

    #[test]
    fn shared_layout_is_stable() {
        assert_eq!(
            std::mem::size_of::<Shared>(),
            EXPECTED_SHARED_SIZE,
            "Shared size changed — the IPC byte layout is not allowed to drift \
             (writer/reader are separate processes). If this is intentional, bump \
             ipc::LAYOUT_VERSION and update this golden."
        );
        // Pin the offsets of the most-recently-appended blocks (the tail is where
        // new fields land, so it's the likeliest accidental-shift site).
        assert_eq!(std::mem::offset_of!(Shared, kifs), 1508, "kifs offset drift");
        assert_eq!(std::mem::offset_of!(Shared, boids), 1628, "boids offset drift");
        assert_eq!(std::mem::offset_of!(Shared, bell), 1724, "bell offset drift");
        assert_eq!(std::mem::offset_of!(Shared, atmosphere), 1756, "atmosphere offset drift");
        assert_eq!(std::mem::offset_of!(Shared, clouds), 1788, "clouds offset drift");
        assert_eq!(std::mem::offset_of!(Shared, ocean), 1836, "ocean offset drift");
        assert_eq!(std::mem::offset_of!(Shared, tessellation), 1888, "tessellation offset drift");
        assert_eq!(std::mem::offset_of!(Shared, minimal_surface), 1952, "minimal_surface offset drift");
        assert_eq!(std::mem::offset_of!(Shared, capture), 2016, "capture offset drift");
        assert_eq!(std::mem::offset_of!(Shared, overlay), 2064, "overlay offset drift");
        assert_eq!(std::mem::offset_of!(Shared, overlay_gen), 2128, "overlay_gen offset drift");
        assert_eq!(std::mem::offset_of!(Shared, axes), 2132, "axes offset drift");
        assert_eq!(std::mem::offset_of!(Shared, synchrotron), 2196, "synchrotron offset drift");
        assert_eq!(std::mem::offset_of!(Shared, fx), 2292, "fx offset drift");
        assert_eq!(std::mem::offset_of!(Shared, volume), 2356, "volume offset drift");
        assert_eq!(std::mem::offset_of!(Shared, temporal), 2388, "temporal offset drift");
        assert_eq!(std::mem::offset_of!(Shared, ssgi), 2420, "ssgi offset drift");
        assert_eq!(std::mem::offset_of!(Shared, shadow), 2436, "shadow offset drift");
        assert_eq!(std::mem::offset_of!(Shared, vxgi), 2452, "vxgi offset drift");
        assert_eq!(std::mem::offset_of!(Shared, reflect), 2468, "reflect offset drift");
        assert_eq!(std::mem::offset_of!(Shared, refl_probe), 2484, "refl_probe offset drift");
        assert_eq!(std::mem::offset_of!(Shared, vxgi_spec), 2500, "vxgi_spec offset drift");
        assert_eq!(std::mem::offset_of!(Shared, membrane_fx), 2516, "membrane_fx offset drift");
        assert_eq!(std::mem::offset_of!(Shared, finishing), 2532, "finishing offset drift");
        assert_eq!(std::mem::offset_of!(Shared, manylight), 2564, "manylight offset drift");
        assert_eq!(std::mem::offset_of!(Shared, vecfield), 2580, "vecfield offset drift");
        assert_eq!(std::mem::offset_of!(Shared, vecbuild), 2676, "vecbuild offset drift");
        assert_eq!(std::mem::offset_of!(Shared, fluidvis), 2932, "fluidvis offset drift");
        assert_eq!(std::mem::offset_of!(Shared, fluid2), 2980, "fluid2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, liquid), 3012, "liquid offset drift");
        assert_eq!(std::mem::offset_of!(Shared, liquid2), 3076, "liquid2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, fluidgi), 3092, "fluidgi offset drift");
        assert_eq!(std::mem::offset_of!(Shared, caustic), 3108, "caustic offset drift");
        assert_eq!(std::mem::offset_of!(Shared, liqmat), 3124, "liqmat offset drift");
        assert_eq!(std::mem::offset_of!(Shared, liqmat2), 3156, "liqmat2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, rails), 3188, "rails offset drift");
        assert_eq!(std::mem::offset_of!(Shared, rt), 3284, "rt offset drift");
        assert_eq!(std::mem::offset_of!(Shared, refrmat), 3316, "refrmat offset drift");
        assert_eq!(std::mem::offset_of!(Shared, rt2), 3332, "rt2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, scenery), 3364, "scenery offset drift");
        assert_eq!(std::mem::offset_of!(Shared, rt3), 3428, "rt3 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, rt4), 3460, "rt4 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, neural), 3492, "neural offset drift");
        assert_eq!(std::mem::offset_of!(Shared, aniso), 3524, "aniso offset drift");
        assert_eq!(std::mem::offset_of!(Shared, axon), 3540, "axon offset drift");
        assert_eq!(std::mem::offset_of!(Shared, coat), 3636, "coat offset drift");
        assert_eq!(std::mem::offset_of!(Shared, terra), 3668, "terra offset drift");
        assert_eq!(std::mem::offset_of!(Shared, neural2), 3732, "neural2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, neural3), 3764, "neural3 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, body), 3796, "body offset drift");
        assert_eq!(std::mem::offset_of!(Shared, micro), 3812, "micro offset drift");
        assert_eq!(std::mem::offset_of!(Shared, water), 3844, "water offset drift");
        assert_eq!(std::mem::offset_of!(Shared, water2), 3876, "water2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, pathtrace_on), 3908, "pathtrace_on offset drift");
        assert_eq!(std::mem::offset_of!(Shared, ndenoise), 3912, "ndenoise offset drift");
        assert_eq!(std::mem::offset_of!(Shared, emit), 3944, "emit offset drift");
        assert_eq!(std::mem::offset_of!(Shared, ssrefr), 3960, "ssrefr offset drift");
        assert_eq!(std::mem::offset_of!(Shared, upscale), 3976, "upscale offset drift");
        assert_eq!(std::mem::offset_of!(Shared, restir), 4008, "restir offset drift");
        assert_eq!(std::mem::offset_of!(Shared, neural_net), 4024, "neural_net offset drift");
        assert_eq!(std::mem::offset_of!(Shared, neural_edge), 4088, "neural_edge offset drift");
        assert_eq!(std::mem::offset_of!(Shared, maxenergy), 4120, "maxenergy offset drift");
        assert_eq!(std::mem::offset_of!(Shared, neural_net2), 4152, "neural_net2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, nn_gen), 4184, "nn_gen offset drift");
        assert_eq!(std::mem::offset_of!(Shared, neural_mlp), 4188, "neural_mlp offset drift");
        assert_eq!(std::mem::offset_of!(Shared, neural_attn), 4220, "neural_attn offset drift");
        assert_eq!(std::mem::offset_of!(Shared, tube), 4252, "tube offset drift");
        assert_eq!(std::mem::offset_of!(Shared, neural_surface), 4268, "neural_surface offset drift");
        assert_eq!(std::mem::offset_of!(Shared, neural_surface2), 4332, "neural_surface2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, brain), 4364, "brain offset drift");
        assert_eq!(std::mem::offset_of!(Shared, thinfilm), 4428, "thinfilm offset drift");
        assert_eq!(std::mem::offset_of!(Shared, ptglass), 4444, "ptglass offset drift");
        assert_eq!(std::mem::offset_of!(Shared, lens), 4464, "lens offset drift");
        assert_eq!(std::mem::offset_of!(Shared, spectral), 4496, "spectral offset drift");
        assert_eq!(std::mem::offset_of!(Shared, demo), 4512, "demo offset drift");
        assert_eq!(std::mem::offset_of!(Shared, audiodip), 4544, "audiodip offset drift");
        assert_eq!(std::mem::offset_of!(Shared, mxforce), 4576, "mxforce offset drift");
        assert_eq!(std::mem::offset_of!(Shared, mxforce2), 4592, "mxforce2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, mxforce3), 4608, "mxforce3 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, pbeads), 4624, "pbeads offset drift");
        assert_eq!(std::mem::offset_of!(Shared, audiodip2), 4656, "audiodip2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, matcol), 4672, "matcol offset drift");
        assert_eq!(std::mem::offset_of!(Shared, pbeads2), 4704, "pbeads2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, ptcaustic), 4720, "ptcaustic offset drift");
        assert_eq!(std::mem::offset_of!(Shared, skyrefl), 4736, "skyrefl offset drift");
        assert_eq!(std::mem::offset_of!(Shared, nrc), 4944, "nrc offset drift");
        assert_eq!(std::mem::offset_of!(Shared, nrc2), 4976, "nrc2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, nrc3), 4992, "nrc3 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, nrc4), 5008, "nrc4 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, acoustic), 5040, "acoustic offset drift");
        assert_eq!(std::mem::offset_of!(Shared, acoustic2), 5104, "acoustic2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, acoustic3), 5136, "acoustic3 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, sonify), 5168, "sonify offset drift");
        assert_eq!(std::mem::offset_of!(Shared, voices), 5232, "voices offset drift");
        assert_eq!(std::mem::offset_of!(Shared, audiometer), 5488, "audiometer offset drift");
        assert_eq!(std::mem::offset_of!(Shared, audiospectrum), 5552, "audiospectrum offset drift");
        assert_eq!(std::mem::offset_of!(Shared, fieldvol), 6096, "fieldvol offset drift");
        assert_eq!(std::mem::offset_of!(Shared, colour), 6128, "colour offset drift");
        // #346 Field Chamber blocks re-seated to the true tail after `colour` on the merge.
        assert_eq!(std::mem::offset_of!(Shared, scopewave), 6160, "scopewave offset drift");
        assert_eq!(std::mem::offset_of!(Shared, chamber), 7200, "chamber offset drift");
        assert_eq!(std::mem::offset_of!(Shared, emissive), 7264, "emissive offset drift");
        assert_eq!(std::mem::offset_of!(Shared, splat), 7280, "splat offset drift");
        assert_eq!(std::mem::offset_of!(Shared, plexus), 7312, "plexus offset drift");
        assert_eq!(std::mem::offset_of!(Shared, plexus2), 7328, "plexus2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, plexus_node_mat), 7344, "plexus_node_mat offset drift");
        assert_eq!(std::mem::offset_of!(Shared, plexus_edge_mat), 7376, "plexus_edge_mat offset drift");
        assert_eq!(std::mem::offset_of!(Shared, plexus3), 7408, "plexus3 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, plexus4), 7424, "plexus4 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, splat2), 7440, "splat2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, mx_eb), 7456, "mx_eb offset drift");
        assert_eq!(std::mem::offset_of!(Shared, plexus_overlay), 7472, "plexus_overlay offset drift");
        assert_eq!(std::mem::offset_of!(Shared, field), 7488, "field offset drift");
        assert_eq!(std::mem::offset_of!(Shared, field_gen), 7528, "field_gen offset drift");
        assert_eq!(std::mem::offset_of!(Shared, mapattractor), 7532, "mapattractor offset drift");
        assert_eq!(std::mem::offset_of!(Shared, origin_mode), 7572, "origin_mode offset drift");
        assert_eq!(std::mem::offset_of!(Shared, maporbit), 7576, "maporbit offset drift");
        assert_eq!(std::mem::offset_of!(Shared, agent), 7608, "agent offset drift");
        assert_eq!(std::mem::offset_of!(Shared, mind), 7640, "mind offset drift");
        assert_eq!(std::mem::offset_of!(Shared, mapattractor2), 7672, "mapattractor2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, fieldsim), 7688, "fieldsim offset drift");
        assert_eq!(std::mem::offset_of!(Shared, kaleido), 7720, "kaleido offset drift");
        assert_eq!(std::mem::offset_of!(Shared, instrument), 7784, "instrument offset drift");
        assert_eq!(std::mem::offset_of!(Shared, instrument2), 7848, "instrument2 offset drift");
        assert_eq!(std::mem::offset_of!(Shared, atlas), 7880, "atlas offset drift");
        assert_eq!(std::mem::offset_of!(Shared, fieldclip_gen), 7912, "fieldclip_gen offset drift");
        assert_eq!(std::mem::offset_of!(Shared, nca_gen), 7916, "nca_gen offset drift");
        assert_eq!(std::mem::offset_of!(Shared, fdtd), 7920, "fdtd offset drift");
        // #541 S2 T1 — the mindview spine sits at the tail. `material_live` (the
        // previous tail) must NOT have moved: an inserted rather than appended field
        // is the one mistake that compiles, runs, and just shows the wrong pane.
        assert_eq!(std::mem::offset_of!(Shared, material_live), 8316, "material_live offset drift");
        assert_eq!(
            std::mem::offset_of!(Shared, mindview),
            8348,
            "the mindview append must begin exactly at the old Shared size"
        );
        assert_eq!(std::mem::offset_of!(Shared, mindview_pane), 8380, "mindview_pane offset drift");
        assert_eq!(std::mem::offset_of!(Shared, mindview_gen), 8508, "mindview_gen offset drift");
        // organon#217 T3 — the PBR-text look controls sit at the tail; the mindview
        // spine (the previous tail) must NOT have moved.
        assert_eq!(
            std::mem::offset_of!(Shared, glyph),
            8512,
            "the T3 append must begin exactly at the old Shared size"
        );
        assert_eq!(std::mem::offset_of!(Shared, glyph_cam), 8576, "glyph_cam offset drift");
        assert_eq!(std::mem::offset_of!(Shared, capsule), 8608, "capsule offset drift");
    }

    // --- Pilot: the generated bell packing is byte-identical to the old hand code.
    #[test]
    fn bell_packing_is_byte_identical() {
        // Matches the `bell` default in ipc::Shared::default() and the old inline
        // packing: [physical=0, depth=0.5, stiffness=8, damping=0.99, open=1.7,
        // rate=0.1, _, _].
        let expected = [0.0, 0.5, 8.0, 0.99, 1.7, 0.1, 0.0, 0.0];
        let p = OrganicMathParams::default();
        assert_eq!(pack_bell(&p), expected, "params→Shared bell packing drifted");

        // The preset packer must agree with the param packer (they can't drift —
        // same slot list — but prove it on the default-captured preset).
        let pv = PresetValues::capture(&p);
        assert_eq!(
            pack_bell_preset(&pv),
            pack_bell(&p),
            "preset→Shared and params→Shared bell packing disagree"
        );
    }

    // --- Refractive material: the appended refrmat block packs the absorption. -
    #[test]
    fn refrmat_packs_the_absorption() {
        let p = OrganicMathParams::default();
        // [absorption, overlay off, blend 1 (inert while overlay = 0), _]
        assert_eq!(pack_refrmat(&p), [1.0, 0.0, 1.0, 0.0], "refrmat default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(
            pack_refrmat_preset(&pv),
            pack_refrmat(&p),
            "preset→Shared and params→Shared refrmat packing disagree"
        );
        // The overlay checkbox + blend ride slots [1]/[2].
        let mut pv2 = pv.clone();
        pv2.refr_overlay = true;
        pv2.refr_blend = 0.3;
        assert_eq!(pack_refrmat_preset(&pv2), [1.0, 1.0, 0.3, 0.0]);
        // The enum wire value the visual branches on (cube.wgsl mat_type >= 2.5).
        use crate::params::MaterialType;
        assert_eq!(MaterialType::Refractive.to_u32(), 3);
        assert_eq!(MaterialType::from_u32(3), MaterialType::Refractive);
    }

    // --- Anisotropy (#214 T1): the appended aniso block packs the streak dials. --
    #[test]
    fn aniso_packs_the_streak() {
        let p = OrganicMathParams::default();
        // [amount 0 (isotropic), rotation 0, overlay off, blend 1 (inert while off)]
        assert_eq!(pack_aniso(&p), [0.0, 0.0, 0.0, 1.0], "aniso default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(
            pack_aniso_preset(&pv),
            pack_aniso(&p),
            "preset→Shared and params→Shared aniso packing disagree"
        );
        let mut pv2 = pv.clone();
        pv2.anisotropy = -0.7;
        pv2.aniso_rotation = 90.0;
        pv2.aniso_overlay = true;
        pv2.aniso_blend = 0.5;
        assert_eq!(pack_aniso_preset(&pv2), [-0.7, 90.0, 1.0, 0.5]);
        // The enum wire value the cube shader branches on (mat_type > 3.5).
        use crate::params::MaterialType;
        assert_eq!(MaterialType::Anisotropic.to_u32(), 4);
        assert_eq!(MaterialType::from_u32(4), MaterialType::Anisotropic);
    }

    // --- Surface lobes (#214 T2): the appended coat block packs clearcoat + sheen. -
    #[test]
    fn coat_packs_the_lobes() {
        let p = OrganicMathParams::default();
        // [clearcoat 1, cc_rough 0.1, cc_overlay 0, sheen_overlay 0, sheen 1,
        //  sheen_rough 0.3, sheen_tint 0, _] — full strength but inert while off.
        assert_eq!(pack_coat(&p), [1.0, 0.1, 0.0, 0.0, 1.0, 0.3, 0.0, 0.0], "coat default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(
            pack_coat_preset(&pv),
            pack_coat(&p),
            "preset→Shared and params→Shared coat packing disagree"
        );
        let mut pv2 = pv.clone();
        pv2.clearcoat = 0.6;
        pv2.clearcoat_overlay = true;
        pv2.sheen = 0.5;
        pv2.sheen_tint = 0.8;
        pv2.sheen_overlay = true;
        assert_eq!(pack_coat_preset(&pv2), [0.6, 0.1, 1.0, 1.0, 0.5, 0.3, 0.8, 0.0]);
        // The enum wire values the cube shader branches on (5 = Clearcoat, 6 = Velvet).
        use crate::params::MaterialType;
        assert_eq!(MaterialType::Clearcoat.to_u32(), 5);
        assert_eq!(MaterialType::Velvet.to_u32(), 6);
        assert_eq!(MaterialType::from_u32(5), MaterialType::Clearcoat);
        assert_eq!(MaterialType::from_u32(6), MaterialType::Velvet);
    }

    // --- Body optics (#214 T3): the appended body block packs the SSS + interior. -
    #[test]
    fn body_packs_the_optics() {
        let p = OrganicMathParams::default();
        // [sss_thickness 0, sss_radius 1, interior_scatter 0, _] — inert.
        assert_eq!(pack_body(&p), [0.0, 1.0, 0.0, 0.0], "body default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(
            pack_body_preset(&pv),
            pack_body(&p),
            "preset→Shared and params→Shared body packing disagree"
        );
        let mut pv2 = pv.clone();
        pv2.sss_thickness = 0.8;
        pv2.sss_radius = 2.5;
        pv2.interior_scatter = 0.6;
        assert_eq!(pack_body_preset(&pv2), [0.8, 2.5, 0.6, 0.0]);
        // The Subsurface material wire id (re-seated to 7 on the main re-merge —
        // the Tier-2 Clearcoat/Velvet types took 5/6).
        use crate::params::MaterialType;
        assert_eq!(MaterialType::Subsurface.to_u32(), 7);
        assert_eq!(MaterialType::from_u32(7), MaterialType::Subsurface);
    }

    // --- Microstructure (#214 T4): the appended micro block packs the dials. -------
    #[test]
    fn micro_packs_the_dials() {
        let p = OrganicMathParams::default();
        // [glitter 0, density 12, sharpness 0.6, diffraction 0, freq 8, retro 0, _, _]
        assert_eq!(pack_micro(&p), [0.0, 12.0, 0.6, 0.0, 8.0, 0.0, 0.0, 0.0], "micro default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(
            pack_micro_preset(&pv),
            pack_micro(&p),
            "preset→Shared and params→Shared micro packing disagree"
        );
        let mut pv2 = pv.clone();
        pv2.glitter = 0.5;
        pv2.glitter_density = 20.0;
        pv2.diffraction = 0.7;
        pv2.diffraction_freq = 12.0;
        pv2.retro = 0.4;
        assert_eq!(pack_micro_preset(&pv2), [0.5, 20.0, 0.6, 0.7, 12.0, 0.4, 0.0, 0.0]);
    }

    // --- Spectral emission (#214 T5 pt 1): the appended emit block packs the dials. -
    #[test]
    fn emit_packs_the_spectrum() {
        let p = OrganicMathParams::default();
        // [fluorescence 0, hue 0.33, incandescence 0, temperature 3000] — inert.
        assert_eq!(pack_emit(&p), [0.0, 0.33, 0.0, 3000.0], "emit default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(
            pack_emit_preset(&pv),
            pack_emit(&p),
            "preset→Shared and params→Shared emit packing disagree"
        );
        let mut pv2 = pv.clone();
        pv2.fluorescence = 0.6;
        pv2.fluor_hue = 0.7;
        pv2.incandescence = 0.4;
        pv2.temperature = 6500.0;
        assert_eq!(pack_emit_preset(&pv2), [0.6, 0.7, 0.4, 6500.0]);
    }

    // --- Screen-space refraction (#214 T5 pt 2): the appended ssrefr block. --------
    #[test]
    fn ssrefr_packs_the_dials() {
        let p = OrganicMathParams::default();
        // [strength 0 (off), displace 0.5, _, _]
        assert_eq!(pack_ssrefr(&p), [0.0, 0.5, 0.0, 0.0], "ssrefr default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(
            pack_ssrefr_preset(&pv),
            pack_ssrefr(&p),
            "preset→Shared and params→Shared ssrefr packing disagree"
        );
        let mut pv2 = pv.clone();
        pv2.refract_ss = 0.8;
        pv2.refract_dist = 1.5;
        assert_eq!(pack_ssrefr_preset(&pv2), [0.8, 1.5, 0.0, 0.0]);
    }

    // --- Maxwell field energization (#247 Tier 1): the appended maxenergy block. ----
    #[test]
    fn maxenergy_packs_the_dials() {
        let p = OrganicMathParams::default();
        // [energize 0 (off), gain 1, knee 4, hue 0.08, antenna_len 6, antenna 0, 0, 0]
        assert_eq!(
            pack_maxenergy(&p),
            [0.0, 1.0, 4.0, 0.08, 6.0, 0.0, 0.0, 0.0],
            "maxenergy default drifted"
        );
        let pv = PresetValues::capture(&p);
        assert_eq!(
            pack_maxenergy_preset(&pv),
            pack_maxenergy(&p),
            "preset→Shared and params→Shared maxenergy packing disagree"
        );
        let mut pv2 = pv.clone();
        pv2.mn_energize = true;
        pv2.mn_gain = 2.5;
        pv2.mn_knee = 6.0;
        pv2.mn_hue = 0.5;
        pv2.mn_antenna = true;
        pv2.mn_antenna_len = 12.0;
        pv2.mn_dye_inject = 1.5;
        assert_eq!(pack_maxenergy_preset(&pv2), [1.0, 2.5, 6.0, 0.5, 12.0, 1.0, 1.5, 0.0]);
    }

    // --- Audio-driven dipole (#248 Tiers 1+2): the appended audiodip block. ----
    #[test]
    fn audiodip_packs_the_dials() {
        let p = OrganicMathParams::default();
        // [drive 0, amount 1, floor 0.1, multipole 0, spread 0.25, band_hue 0.7,
        //  stereo 0.5, pitch 0.5]  (#248 T3 fills [6..8])
        assert_eq!(
            pack_audiodip(&p),
            [0.0, 1.0, 0.1, 0.0, 0.25, 0.7, 0.5, 0.5],
            "audiodip default drifted"
        );
        let pv = PresetValues::capture(&p);
        assert_eq!(
            pack_audiodip_preset(&pv),
            pack_audiodip(&p),
            "preset→Shared and params→Shared audiodip packing disagree"
        );
        let mut pv2 = pv.clone();
        pv2.ad_drive = true;
        pv2.ad_amount = 2.0;
        pv2.ad_floor = 0.25;
        pv2.ad_multipole = true;
        pv2.ad_spread = 0.5;
        pv2.ad_band_hue = 1.0;
        pv2.ad_stereo = 0.8;
        pv2.ad_pitch = 1.5;
        assert_eq!(pack_audiodip_preset(&pv2), [1.0, 2.0, 0.25, 1.0, 0.5, 1.0, 0.8, 1.5]);
    }

    // --- Audio Tier 3 waveform shells (#248): the appended audiodip2 block. ----
    #[test]
    fn audiodip2_packs_the_dials() {
        let p = OrganicMathParams::default();
        assert_eq!(pack_audiodip2(&p), [0.0, 0.0, 0.0, 0.0], "audiodip2 default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(pack_audiodip2_preset(&pv), pack_audiodip2(&p), "audiodip2 packing disagree");
        let mut pv2 = pv.clone();
        pv2.ad_wave = 0.7;
        assert_eq!(pack_audiodip2_preset(&pv2), [0.7, 0.0, 0.0, 0.0]);
    }

    // --- Field-force particle drive (#248): the appended mxforce block. ----
    #[test]
    fn mxforce_packs_the_dials() {
        let p = OrganicMathParams::default();
        // [force off, gain 1, contrast 1, stir rate 0.3] → inert until force is enabled.
        assert_eq!(pack_mxforce(&p), [0.0, 1.0, 1.0, 0.3], "mxforce default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(
            pack_mxforce_preset(&pv),
            pack_mxforce(&p),
            "preset→Shared and params→Shared mxforce packing disagree"
        );
        let mut pv2 = pv.clone();
        pv2.mn_force = true;
        pv2.mn_force_gain = 2.5;
        pv2.mn_energy_contrast = 1.8;
        pv2.mn_stir_rate = 0.5;
        assert_eq!(pack_mxforce_preset(&pv2), [1.0, 2.5, 1.8, 0.5]);
    }

    // --- Acoustic pump + beat coupling (#248): the appended mxforce2 block. ----
    #[test]
    fn mxforce2_packs_the_dials() {
        let p = OrganicMathParams::default();
        // [pump 0 (off), beat spin force 0, pump_scale 3, spin slowdown 1.5].
        assert_eq!(pack_mxforce2(&p), [0.0, 0.0, 3.0, 1.5], "mxforce2 default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(pack_mxforce2_preset(&pv), pack_mxforce2(&p), "mxforce2 packing disagree");
        let mut pv2 = pv.clone();
        pv2.mn_pump = 2.0;
        pv2.mn_swirl_beat = 0.75;
        pv2.mn_pump_scale = 5.0;
        pv2.mn_swirl_decay = 2.0;
        assert_eq!(pack_mxforce2_preset(&pv2), [2.0, 0.75, 5.0, 2.0]);
    }

    // --- Coupled dynamo (#248): the appended mxforce3 block. ----
    #[test]
    fn mxforce3_packs_the_dials() {
        let p = OrganicMathParams::default();
        // [beat mode −1 (turbine), ring freq 2, hue cycle 0, _].
        assert_eq!(pack_mxforce3(&p), [-1.0, 2.0, 0.0, 0.0], "mxforce3 default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(pack_mxforce3_preset(&pv), pack_mxforce3(&p), "mxforce3 packing disagree");
        let mut pv2 = pv.clone();
        pv2.mn_mode_mix = 0.5;
        pv2.mn_ring_freq = 5.0;
        pv2.mn_hue_cycle = 1.0;
        assert_eq!(pack_mxforce3_preset(&pv2), [0.5, 5.0, 1.0, 0.0]);
    }

    // --- Shaded particle beads (#298 Tier 1 + Tier 2): the appended pbeads block. ----
    #[test]
    fn pbeads_packs_the_dials() {
        let p = OrganicMathParams::default();
        // [beads off (0), metallic 0.9, roughness 0.2, material Standard (0),
        //  shape Sphere (0), ior 1.45, shape_param 0.5, beads_rt off (0)].
        assert_eq!(pack_pbeads(&p), [0.0, 0.9, 0.2, 0.0, 0.0, 1.45, 0.5, 0.0], "pbeads default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(pack_pbeads_preset(&pv), pack_pbeads(&p), "pbeads packing disagree");
        let mut pv2 = pv.clone();
        pv2.particles_beads = true;
        pv2.particles_metallic = 0.25;
        pv2.particles_roughness = 0.5;
        pv2.particles_material = 2; // Glass
        pv2.particles_shape = 3;    // Rounded Box
        pv2.particles_ior = 1.33;
        pv2.particles_shape_param = 0.8;
        pv2.particles_beads_rt = true; // #298 Tier 4
        assert_eq!(pack_pbeads_preset(&pv2), [1.0, 0.25, 0.5, 2.0, 3.0, 1.33, 0.8, 1.0]);
    }

    // --- Per-material + bead HSV (#305 Tier 1): the appended matcol/pbeads2 blocks. --
    #[test]
    fn material_hsv_packs_the_dials() {
        let p = OrganicMathParams::default();
        // Identity default: hue 0, cycle 0, sat 1, value 1 for generator + scenery.
        assert_eq!(pack_matcol(&p), [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0], "matcol default drifted");
        assert_eq!(pack_pbeads2(&p), [0.0, 0.0, 1.0, 1.0], "pbeads2 default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(pack_matcol_preset(&pv), pack_matcol(&p), "matcol packing disagree");
        assert_eq!(pack_pbeads2_preset(&pv), pack_pbeads2(&p), "pbeads2 packing disagree");
        let mut pv2 = pv.clone();
        pv2.mat_hue = 0.25;
        pv2.mat_saturation = 0.5;
        pv2.scen_value = 0.3;
        pv2.particles_bead_hue = 0.7;
        pv2.particles_bead_val = 0.6;
        assert_eq!(pack_matcol_preset(&pv2), [0.25, 0.0, 0.5, 1.0, 0.0, 0.0, 1.0, 0.3]);
        assert_eq!(pack_pbeads2_preset(&pv2), [0.7, 0.0, 1.0, 0.6]);
    }

    // --- Live-sky cloud reflections (#305 Tier 2): the appended skyrefl block. --
    #[test]
    fn skyrefl_packs_the_dials() {
        let p = OrganicMathParams::default();
        assert_eq!(pack_skyrefl(&p), [0.0, 0.55, 0.08, 0.7], "skyrefl default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(pack_skyrefl_preset(&pv), pack_skyrefl(&p), "skyrefl packing disagree");
        let mut pv2 = pv.clone();
        pv2.sky_reflect_clouds = true;
        pv2.sky_cloud_cover = 0.3;
        assert_eq!(pack_skyrefl_preset(&pv2), [1.0, 0.3, 0.08, 0.7]);
    }

    // --- Neural radiance cache — live (#256 Tier 0): the appended nrc block. -------
    #[test]
    fn nrc_packs_the_cache_dials() {
        let p = OrganicMathParams::default();
        // [enable off, confidence 0.5, lr 0.02, omega 4, terminate 2, samples 8, seed 1, _]
        assert_eq!(pack_nrc(&p), [0.0, 0.5, 0.02, 4.0, 2.0, 8.0, 1.0, 0.0], "nrc default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(pack_nrc_preset(&pv), pack_nrc(&p), "nrc packing disagree");
        let mut pv2 = pv.clone();
        pv2.nrc_enable = true;
        pv2.nrc_confidence = 0.8;
        pv2.nrc_terminate = 3;
        pv2.nrc_seed = 42;
        assert_eq!(pack_nrc_preset(&pv2), [1.0, 0.8, 0.02, 4.0, 3.0, 8.0, 42.0, 0.0]);
    }

    // --- NRC RT-stack synergies (#256 Tier 1): the appended nrc2 block. -----------
    #[test]
    fn nrc2_packs_the_synergy_dials() {
        let p = OrganicMathParams::default();
        // [guide off, 4 candidates, firefly off, clamp 8]
        assert_eq!(pack_nrc2(&p), [0.0, 4.0, 0.0, 8.0], "nrc2 default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(pack_nrc2_preset(&pv), pack_nrc2(&p), "nrc2 packing disagree");
        let mut pv2 = pv.clone();
        pv2.nrc_guide = true;
        pv2.nrc_guide_candidates = 6;
        pv2.nrc_firefly = true;
        pv2.nrc_firefly_clamp = 12.0;
        assert_eq!(pack_nrc2_preset(&pv2), [1.0, 6.0, 1.0, 12.0]);
    }

    // --- NRC light-field uses (#256 Tier 2): the appended nrc3 block. -------------
    #[test]
    fn nrc3_packs_the_lightfield_dials() {
        let p = OrganicMathParams::default();
        // [gi off, strength 1, reflect off, _]
        assert_eq!(pack_nrc3(&p), [0.0, 1.0, 0.0, 0.0], "nrc3 default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(pack_nrc3_preset(&pv), pack_nrc3(&p), "nrc3 packing disagree");
        let mut pv2 = pv.clone();
        pv2.nrc_gi = true;
        pv2.nrc_gi_strength = 2.5;
        pv2.nrc_reflect = true;
        assert_eq!(pack_nrc3_preset(&pv2), [1.0, 2.5, 1.0, 0.0]);
    }

    // --- NRC hard transport + volumetrics (#256 Tier 3): the appended nrc4 block. --
    #[test]
    fn nrc4_packs_the_transport_dials() {
        let p = OrganicMathParams::default();
        // [volume off, density 0.15, 16 steps, strength 1, caustic off, gain 1, _, _]
        assert_eq!(pack_nrc4(&p), [0.0, 0.15, 16.0, 1.0, 0.0, 1.0, 0.0, 0.0], "nrc4 default drifted");
        let pv = PresetValues::capture(&p);
        assert_eq!(pack_nrc4_preset(&pv), pack_nrc4(&p), "nrc4 packing disagree");
        let mut pv2 = pv.clone();
        pv2.nrc_volume = true;
        pv2.nrc_volume_density = 0.4;
        pv2.nrc_volume_steps = 32;
        pv2.nrc_caustic = true;
        pv2.nrc_caustic_gain = 2.0;
        assert_eq!(pack_nrc4_preset(&pv2), [1.0, 0.4, 32.0, 1.0, 1.0, 2.0, 0.0, 0.0]);
    }

    // --- Full Default→Shared snapshot golden (#103 bulk-migration safety net). -
    // Hashes the entire `Shared` produced by the param Default. Any packing slip
    // during a block migration (wrong field, swapped slot, wrong cast) changes
    // these bytes and fails here. Captured from the pre-migration code, so a
    // layout-preserving refactor must keep it identical. (If a *default* ever
    // legitimately changes, update this golden in that same PR.)
    fn fnv1a(bytes: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    // #152 Tier 2 appended temporal[8] + ssgi[4] with non-zero defaults on top of the
    // Tier 1 fx/volume tail, so the Default→Shared byte image (and this hash) changes.
    // Recomputed on the local Mac build (FNV-1a is streaming + the blocks are appended,
    // so the new hash continues the old one over the appended tail).
    // #152 Tier 3 appended shadow[4] with non-zero defaults, so the Default→Shared
    // byte image (and this hash) changes. NOT recomputed in the authoring
    // (remote/Linux) session — the `nih_plug` git dep is unfetchable there, so
    // `cargo test` can't run. On the first local build this test fails and prints the
    // actual hash (assert_eq! shows `left`); paste it here. (See the PR notes.)
    // membrane_fx[4] appended after the vxgi_spec tail (default [1,0,0,0] — on by default)
    // changes the Default→Shared byte image (and this hash). Recomputed on the local Mac build.
    // #167 Tier 1 appends finishing[8] (default [0,0.6,1,0.6, 0,0.5,0.4,0.3]). FNV-1a is
    // streaming, so the new golden is the Mac-verified membrane_fx hash folded over those
    // 32 appended bytes → 2433535240323895650. If the local build still differs, paste the
    // printed value.
    // #167 Tier 3 appends manylight[4] (default [0, 1.0, 0.5, 24.0]). FNV-1a streams, so
    // the new golden is the Mac-verified finishing[8] hash folded over those 16 appended
    // bytes → 8898377404434103621. If the local build differs, paste the printed value.
    // #173 Tier 1 appends vecfield[24] (defaults [0, 12, 12, 12, 10, 0.5, 1, 0.1, 0, 0,
    // 0.3, 0.6, 0, 0…]). Verified by both routes: the manylight hash folded over those 96
    // appended bytes AND the live `cargo test` print agree → 858127972553250512, so the
    // image before the tail is untouched.
    // #173 Tier 2 fills the reserved vecfield tail (slots 15–22 gain non-zero defaults:
    // 96 seeds / 160 steps / ds 0.15 / bidir 1 / flow_speed 1 / line thickness 0.06) —
    // same block size, new bytes. Dual-verified again (manylight hash folded over the
    // full 96-byte vecfield image == the live print) → 14397755014477007094.
    // #173 Tier 3 appends vecbuild[64] (the builder defaults = the flagship: Fx = y²,
    // Fy = −x², Fz = 0.5·sin z, direct op, mix 0.5). Dual-verified (T2 hash folded over
    // the 256 appended bytes == the live print) → 5093225966689106103.
    // #182 Tier 1 appends fluidvis[12] (Fluid Ink: off, but non-zero look defaults —
    // rate 2 / radius 1.5 / extinction 4 / scatter 1 / emissive 0.6 / g 0.45 /
    // dissipation 0.15 / steps 96 / MacCormack + half-res on). Dual-verified: the
    // FNV state over the unchanged 2932-byte prefix equals the previous golden
    // (byte-identical refactor), and folding the 48 appended default bytes over it
    // reproduces the live hash → 11246331697874608406.
    // (T1 follow-up: `fluidvis[11]` reserved → `reveal`, default 0.3 — the same
    // prefix-fold verification gives 16483941646559126924.)
    // #182 Tier 2 appends fluid2[8] (all inert: heat_decay 0.3, substeps 1, rest 0).
    // Dual-verified the same way: folding the 32 appended default bytes over the
    // T1+reveal hash reproduces the live hash → 5389912562054962923.
    // #182 Tier 3a appends liquid[16] (off, non-zero look/feel defaults — see
    // ipc::Shared; gravity defaults to 0 after on-Mac feedback — the pool
    // drained to the floor instantly). Same prefix-fold verification: folding
    // the 64 appended default bytes over the T2 hash → 578693329190468315.
    // T3a block 2 appends liquid2[4] (all zero: tank offset_y 0). Folding the
    // 16 zero bytes over that → 397423525873547675.
    // #182 T4 appends fluidgi[4] (all zero) + caustic[4] ([0,1,0,0] — sharpness
    // 1). Folding the 32 appended default bytes over that → 7615307650478680722.
    // T4 follow-up appends liqmat[8] ([0 (use scene), 0, 0.05, 1.33, 0, 0,
    // 0.5 (absorption), 0]) + liqmat2[8] (all zero). Folding those 64 default
    // bytes over the caustic-tail hash → 16999645894047608752.
    // #187 Tier 1 appends rails[24] (defaults [8, 6, 3, 0, 0.5, 0, 36, 4, 24, 0.6,
    // 0.5, 8, 0.5, 0.1, 0.3, 6, 0.05, 0…]). Dual-verified (the #182 liqmat2 hash
    // folded over the 96 appended bytes == the live `cargo test` print — the full
    // suite runs remotely) → 11861502761680046551.
    // #187 Tier 2 fills the reserved rails tail (slots 17–20: archetype 0, diverge
    // 137.50776, shells 2, parastichy 13) — same block size, new bytes.
    // Dual-verified (the #182 liqmat2 hash folded over the full 96-byte rails
    // image == the live print) → 3053086658908858106.
    // #187 Tier 3 fills slots [3] (change_every ordinal 3 = 8 bars) + [21]
    // (evolve, default 0). Dual-verified the same way → 15300995071342096378.
    // #195 Tier 0 appends rt[8] (all-zero defaults) — the rails hash folded over
    // 32 zero bytes → 14674542288175869178.
    // #195 Tier 1 fills rt[2..5] (shadows 0, soft 0.15, strength 1, fill 0) —
    // the rails-T3 hash folded over the 32-byte rt image → 14106351950072074901.
    // Refractive material appends refrmat[4] AFTER rt[8] (re-merge re-fold);
    // the refraction overlay fills slots [1..2] — default [1 (absorption),
    // 0 (overlay off), 1 (blend, inert while off), 0]. Dual-verified: the #195
    // T1 hash FNV-folded over those 16 bytes == the live `cargo test` print →
    // 14935245487198634805.
    // #195 Tier 2 appends rt2[8] AFTER refrmat (re-merge re-seat; default
    // [0, 1, 0.4, 2, 1, 0…]) — the refrmat hash folded over those 32 bytes →
    // 3446936669316655146.
    // #195 Tier 3 fills the spare rt2[5..7] (AO source 0, 2 RT AO rays) —
    // the refrmat hash folded over the new 32-byte rt2 image →
    // 296445944315547498.
    // #187 scenery pivot appends scenery[16] AFTER rt2 (mode off, material
    // defaults mirror the main look) — the T3 rt2 hash folded over the 64
    // appended bytes → 14070314859079343188.
    // Merged: #211 defaults (default-16 rays, denoise) + #213 rt4[8] temporal
    // + #215 part-4 variance defaults (rt4[4]=32 max samples, rt4[5]=3 clamp).
    // #200 Tier 4 re-seats the per-display `pathtrace_on: u32` after water2 →
    // 15831868405128436335 (size 3912). #200 Tier 5a then re-seats ndenoise[8]
    // (default [0, 0.5, 1, 4, 0, 0, 0, 0]) after pathtrace_on on the main re-merge;
    // the pathtrace golden FNV-folded over those 32 bytes → 12341594941139960671 (size 3944).
    // #214 Tier 5 pt 2 appends ssrefr[4] after emit → 6434111062467797200 (size 3976).
    // #200 Tier 5c appends upscale[8] after ssrefr → 5543673833715034752 (size 4008).
    // #200 Tier 5d then re-seats restir[4] (default [0, 0, 0, 0]) after upscale on the
    // main re-merge; the upscale golden FNV-folded over those 16 zero bytes →
    // 12963604863967365760 (size 4024).
    // #226 Tier 1 then appends neural_net[16] (default [3, 48, 6, 0.15, 4, 1, 12, 0.5,
    // 1.2, 0.14, 0.25, 16, 0.5, 0.12, 0, 0]) after restir; the restir golden FNV-folded
    // over those 64 bytes = 10906484920525887969 (size 4088). (Plain tail append — no
    // LAYOUT_VERSION bump, matching emit/ssrefr/upscale/restir.)
    // #226 Tier 1.5 then appends neural_edge[8] (default [1, 0.4, 0.6, 5, 0, 6, 0, 0])
    // after neural_net; the neural_net golden FNV-folded over those 32 bytes =
    // 395367203430032178 (size 4120).
    // #247 Tier 1 then re-seats maxenergy[8] after neural_edge on the main re-merge (size 4152).
    // #247 Tier 2 fills the reserved antenna_len (slot 4 = 6.0), so the maxenergy default
    // becomes [0, 1, 4, 0.08, 6, 0, 0, 0]; the neural_edge golden FNV-folded over those 32
    // bytes = 4448661560647265280 (no size change from Tier 1).
    // #226 Tier 2 then appends neural_net2[8] (default [0, 0.5, 8, 1, 0.6, 0.6, 2, 0])
    // after maxenergy on the main re-merge; the Tier-2 maxenergy golden FNV-folded over
    // those 32 bytes = 8031160609356058239 (size 4184).
    // #226 Tier 3 (re-landed) then appends the `nn_gen: u32` connectome-load counter (0)
    // after neural_net2 (size 4188). #226 Tier 4 then appends neural_mlp[8] (default
    // [0.8, 0.05, 1, 0, 0, 0, 0, 0]) after nn_gen (size 4220). #226 Tier 5 then appends
    // neural_attn[8] (default [0, 0, 0.05, 24, 0.5, 0, 0, 0]) after neural_mlp (size 4252).
    // PR #276 appends tube[4] (default [0, 1, 0.5, 0]) after neural_attn (size 4268).
    // #260 Tier 1 then appends neural_surface[16] (default [1, 0, 0.35, 0, …0], the Neural
    // Tissue surface dials) after tube AND the tier1↔main merge bumps ipc::LAYOUT_VERSION to 0x0251
    // (combined layout), growing Shared to 4332 bytes.
    // #260 Tier 2 FILLS reserved neural_surface[5..10] (dendrite density 0 / length 1.0 /
    // taper 0.62 / neuron_type 0 / spines 0) — no size/LAYOUT_VERSION change, but the non-zero
    // length/taper defaults move the default byte image, so the hash re-pins here.
    // #260 Tiers 3/4 fill neural_surface + tail-append neural_surface2[8] (Shared 4332 → 4364).
    // #275 brain[16] tail-append → 4428; #258 T1 thinfilm[4] tail-append (default [0, 0.3, 1.33, 0.5],
    // thickness 0 = inert) → 4444; #258 T2 ptglass[4] tail-append (default [0,0,0,0], off) → 4460.
    // PR #276 follow-up tail-appends tube_profile: f32 (default 1.0 = circle) → 4464, LAYOUT_VERSION 0x0252;
    // the new default byte image re-hashes here.
    // #258 T3 tail-appends lens[8] (default [1, 0.6, 0.25, 0, 150, 128, 0, 0]); #258 T4
    // tail-appends spectral[4] (default [0, 40, 3, 0], off). #288 tail-appends demo[8]
    // (default [0, 1, 1, 1, 1, 0.15, 4, 0]); inert unless generator = Demo, but the non-zero
    // scene dials move the byte image. Hash re-pinned here for the combined tail.
    // #248 T1 tail-appends audiodip[8] (default [0, 1, 0.1, 0, 0, 0, 0, 0], drive off →
    // inert); the non-zero amount/floor defaults move the byte image → re-pinned.
    // #248 T2 fills audiodip[3..6] (multipole 0, spread 0.25, band_hue 0.7 — all inert
    // until multipole mode engages); the non-zero defaults re-pin the hash again.
    // #248 field-force tail-appends mxforce[4] (default [0, 1, 1, 0.3], force off →
    // direction drive + flat energization; slot 3 = stir rate Hz); the non-zero
    // gain/contrast/stir-rate re-pin the hash. #248 acoustic pump then tail-appends
    // mxforce2[4] (default [0, 0, 3, 1.5], beat couplings off; pump_scale 3 + spin
    // slowdown 1.5 re-pin again). #248 coupled dynamo then tail-appends mxforce3[4]
    // (default [−1, 2, 0, 0]; beat-mode crossfade −1 = turbine, ring freq 2 Hz — re-pins).
    // #298 Tier 1 then tail-appends pbeads[8] (default [0, 0.9, 0.2, 0, 0, 0, 0, 0];
    // beads off → additive sparks byte-identical, but the non-zero metallic/roughness
    // defaults move the default byte image). Size 4624 → 4656. #298 Tier 2 then FILLS
    // reserved pbeads[3..7] (material 0, shape 0, ior 1.45, shape_param 0.5).
    // #248 Tier 3 then tail-appends audiodip2[4] (waveform shells; 4656 → 4672) and fills
    // audiodip[6..8] = [stereo 0.5, pitch 0.5] + maxenergy[7] = aura_field 0.
    // #305 Tier 1 tail-appends matcol[8] (gen+scenery HSV, [0,0,1,1] each) + pbeads2[4]
    // (bead HSV, [0,0,1,1]) AFTER audiodip2 (size → 4720). #258 T5 tail-appends
    // ptcaustic[4] (default [0, 128, 1, 2], enable OFF; size → 4736). #305 Tier 2
    // tail-appends skyrefl[4] (live-sky cloud reflections, default [0, 0.55, 0.08, 0.7],
    // off); size → 4752. #307 Tier 1 tail-appends the cinematic-camera blocks
    // cam_seq/cam_dolly/cam_clock/cam_audio after skyrefl (size → 4816, LAYOUT_VERSION 0x0253).
    // #307 Tier 2 then tail-appends cam_frame[8] after cam_audio (roll 0, FOV 45,
    // framing off); the non-zero FOV default re-pins. Size → 4848, LAYOUT_VERSION 0x0254.
    // #307 (PR #316 review) then fills the reserved cam_frame[5] with seq_mix (default
    // 1 = fully sequencer). #307 Tier 3 tail-appends cam_story[24] after cam_frame
    // (storyboard demo playlist; size → 4944, LAYOUT_VERSION 0x0255). #320 (this merge) also
    // sets the Maxwell osc-divide default Quarter → maxwell[23]=2.0 (mx_osc_sync off,
    // reuses existing slots — no new field / no size change), which re-pins the image.
    // #256 Tier 0 tail-appends nrc[8] (live radiance cache, default
    // [0, 0.5, 0.02, 4, 2, 8, 1, 0], enable OFF → tracer byte-identical); the non-zero
    // dials move the default byte image. Size → 4976, LAYOUT_VERSION 0x0256.
    // #256 Tier 1 tail-appends nrc2[4] (NRC-guided sampling + firefly clamp, default
    // [0, 4, 0, 8], both OFF → tracer byte-identical); the non-zero candidate/clamp
    // defaults move the image. Size → 4992, LAYOUT_VERSION 0x0257.
    // #256 Tier 2 tail-appends nrc3[4] (cache GI supersedes DDGI + lit reflections,
    // default [0, 1, 0, 0], all OFF → byte-identical); the non-zero strength default
    // moves the image. Size → 5008, LAYOUT_VERSION 0x0258.
    // #256 Tier 3 tail-appends nrc4[8] (cache volumetrics + cached caustics, default
    // [0, 0.15, 16, 1, 0, 1, 0, 0], all OFF → byte-identical); the non-zero volume
    // density/steps/strength + caustic gain defaults move the image. Size → 5040,
    // LAYOUT_VERSION 0x0259.
    // #325 re-seats acoustic[16] to the true tail after nrc4 (a dipole on a 5×24
    // lattice; geometry blend 0 = pressure, aura_blend 1 = velocity; beat pump off;
    // size → 5104, LAYOUT_VERSION 0x025A), whose non-zero defaults re-pin the image.
    // #325 Tier 4 tail-appends acoustic2[8] (Radiating model, a (2,2,1) cavity mode in a
    // box of half-extent 8, morph/intensity 0; size → 5136, LAYOUT_VERSION 0x025B) → re-pins again.
    // #325 Tier 5 tail-appends acoustic3[8] (cavity tween 0.6, per-axis audio breathe 0;
    // size → 5168, LAYOUT_VERSION 0x025C) → the non-zero tween default re-pins the hash again.
    // #339 re-seats sonify[16] (non-zero Sound-card defaults) + voices[64] after acoustic3
    // (size → 5488, LAYOUT_VERSION 0x025D). #333 Tiers 1–2 then tail-append audiometer[16] (silence
    // default) + audiospectrum[128] (−120 floor) after voices (size → 6064, LAYOUT_VERSION 0x025F) →
    // re-pins the hash (the −120 RTA tail + the new blocks change the FNV walk).
    // #333 Tier 3 tail-appends analytical[8] (Expressive default, non-zero calibration
    // targets: −14/−50 LUFS, −1 dBTP; size → 6096, LAYOUT_VERSION 0x0260) → re-pins the hash.
    // #348/#349 tail-append fieldvol[8] (Legacy source, smooth/gain 1) + colour[8]
    // (Aesthetic, dB window −60..0, full amount) after analytical (size → 6160, LAYOUT_VERSION
    // 0x0261 — one bump for both blocks) → the non-zero defaults re-pin the hash.
    // #348 field-lines (flow) repurposed the spare fieldvol[5..8] slots (density 160).
    // #346 Field Chamber then re-seats scopewave[260] (0 default) + chamber[16] (non-zero
    // panel look defaults) to the true tail after colour (size → 7264, LAYOUT_VERSION 0x0261→0x0262)
    // → the non-zero chamber defaults re-pin the hash.
    // + splat[8] (Gaussian Splatting surface; non-zero Look defaults — radius 0.55,
    //   opacity 0.85, falloff 1, mode Lit, cutoff 0.003, aniso 1 — re-pin the hash; LAYOUT_VERSION 0x0264).
    // + plexus[4] + plexus2[4] + plexus_node_mat[8] + plexus_edge_mat[8] + plexus3[4]
    //   (Plexus Tiers 1–3, re-seated after splat on the main merge; non-zero defaults
    //   re-pin the hash); LAYOUT_VERSION 0x0264→0x0267.
    // + plexus4[4] (Tier-1 shape morph; defaults 1/1 = sphere nodes + circular struts → re-pin); LAYOUT_VERSION 0x0267→0x0268.
    // + splat Tier 3 re-seats the reserved splat[6]/[7] slots as scatter=1/jitter=0.35 (non-zero defaults re-pin).
    // + splat2[4] (Splat Tier 3 solidity, default 0) tail-appended after plexus4; LAYOUT_VERSION 0x0268→0x0269 re-pins the hash.
    // + mx_eb[4] (Maxwell E↔B phase dial; 0° default = 16 appended zero bytes → re-pin) after splat2; LAYOUT_VERSION 0x0269→0x026A.
    // Combined tail after the main merge: plexus_overlay[4] (off, shell 1.15/0.2/12 defaults)
    // then #381 field[10]+field_gen, then #380 mapattractor[10] — all move the byte image,
    // so re-pinned; LAYOUT_VERSION 0x026D (combined FieldEngine + MapAttractor layout).
    // + origin_mode: u32 (Original cube-field origin; default 0 = Corner → geometry
    //   byte-identical, but the appended zero u32 grows the byte image; size → 7576,
    //   LAYOUT_VERSION 0x026D→0x026E) → re-pins the hash.
    // + maporbit[8] (#380 Tier 2 parameter orbit; non-zero defaults — Linear mode,
    //   loop 16, radii 1.5, fa 1/fb 2, ψ π/2, free 0.05 — grow the byte image; size →
    //   7608, LAYOUT_VERSION 0x026E→0x026F) → re-pins the hash.
    // + agent[8] (#317 Tier 1 AI-Performer runtime block; all-zero default, but the
    //   appended 32 zero bytes grow the byte image; size → 7640, LAYOUT_VERSION 0x026F→0x0270)
    //   → re-pins the hash.
    // + mind[8] (#367 Tier 1 visible-mind specimen; runtime-stamped, default all-zero →
    //   32 appended zero bytes grow the byte image; size → 7672, LAYOUT_VERSION 0x0270→0x0271) → re-pins.
    // + mapattractor2[4] (#380 Tier 3; defaults [c 1.5, d 1.5, color 0 StepSpeed, _ 0]
    //   grow the byte image; re-seated after mind on the #389/#390 merge; size → 7688,
    //   LAYOUT_VERSION 0x0271→0x0272) → re-pins the hash.
    // + fieldsim[8] (#381 Tier 3 PDE sim; default Off but non-zero D 1/time_scale 1/
    //   feed 0.037/kill 0.06/potential 1/res 64 grow the byte image; re-seated after
    //   mapattractor2 on the #393 merge; size → 7720, LAYOUT_VERSION 0x0272→0x0273) → re-pins the hash.
    // + kaleido[16] (#361 Tier 1 Scene Kaleidoscope; re-seated to the true tail after
    //   fieldsim on the #363↔main sync; non-zero Look defaults — 6-fold, spin 0.1, zoom 1,
    //   mix 1, seam 0.5 — grow the byte image; size → 7784, LAYOUT_VERSION 0x0273→0x0274) → re-pins.
    // + atlas[8] (#423 Tier 1 the atlas — runtime-stamped, all-zero default; the 32
    //   appended zero bytes fold into the FNV walk; size → 7912, LAYOUT_VERSION 0x0276→0x0277)
    //   → re-pins the hash.
    // + fieldclip_gen: u32 (#407 Tier A Field Playback clip-load counter; default 0 —
    //   appends 4 zero bytes after atlas; size → 7916, LAYOUT_VERSION 0x0277→0x0278) → re-pins.
    // + nca_gen: u32 (#407 Tier B Neural CA model-load counter; default 0 — appends 4
    //   more zero bytes after fieldclip_gen; size → 7920, LAYOUT_VERSION 0x0278→0x0279) → re-pins.
    // + fdtd[8] (#412 Tier 3 Phase 0 FDTD Maxwell solver; default off but non-zero
    //   res 64 / ω 8 / drive 1 / substeps 4 / sponge 8 / extent 12 grow the byte
    //   image; tail append after nca_gen; size → 7952, LAYOUT_VERSION 0x0279→0x027A) → re-pins.
    // + bevel: f32 (node bevel — cube→sphere; default 0 — appends 4 zero bytes after
    //   fdtd; size → 7956, LAYOUT_VERSION 0x027A→0x027B). Verified by the live `cargo
    //   test` print on the Linux build (nih_plug fetched + alsa/x11/gl/jack dev libs
    //   installed) → 16350260627663112156.
    // MERGE of main's 0x0280 layout (creature3[4] + material[8] + material_gen) and the
    // #472 Tier 2/3 material stack (material_layer[18]+material_grad[8], then
    // material_layer2[18]+material_grad2[8]+material_derive[8]); LAYOUT_VERSION 0x0282.
    // The default byte image is a fresh combination of both, so neither branch's hash
    // applies — re-pinned from the live `cargo test` print on this merge → 14551963606549652252.
    // + material_live[8] (anim off, speed 0.1, mode Drift, flow_x 1, displace 0,
    //   audio_drive RESERVED 0) after material_derive (#472 Tier 5); LAYOUT_VERSION
    //   0x0282→0x0283. Re-pinned from the live `cargo test` print on this reconciliation
    //   (Tier 5 re-based onto main's 0x0282 layout) → 11052600774956997082.
    // + the #541 T1 mindview spine — mindview[8] + mindview_pane[4*8] + mindview_gen,
    //   tail-appended after material_live; LAYOUT_VERSION 0x0283→0x0284, Shared 8348 →
    //   8512 bytes. The whole-struct hash necessarily moves when the struct grows, so
    //   this is re-pinned → 896555630126019813. What makes that safe to re-pin rather
    //   than merely convenient is the companion test below: the PREVIOUS golden is by
    //   definition the hash of the previous 8348 bytes, so asserting the new struct's
    //   first 8348 bytes still hash to it proves the append moved nothing ahead of it.
    // + #618 Tier 0a — the `Shared` seqlock. LAYOUT_VERSION 0x0284→0x0285, size
    //   UNCHANGED at 8512. No field moved and no field was added: the only byte that
    //   differs is `layout_version` itself, which the packers stamp, so the
    //   whole-struct hash moves → 8436249494989495788. This is the one re-pin shape
    //   the companion prefix test cannot vouch for by growth (nothing grew), and it
    //   does not have to: that test rewinds `layout_version` to the pinned value
    //   before hashing, so it stays green across ANY version bump and keeps proving
    //   the other 8508 bytes did not move. If this hash moves and the prefix test
    //   also fails, something real shifted — do not re-pin, go find it.
    // + organon#217 T3 — glyph[16] (cell_w 1, depth 0.18, gap 0.06, gain 3, faceplate
    //   0.03, backplane 0.06/0.06/0.065, margin 1.5, back_depth 0.25, default_fg 0.75,
    //   bevel 0, crown 0, reserved 0) + glyph_cam[8] (hold 0, tilt 0, zoom 1, reserved 0)
    //   + capsule[4] (all 0), tail-appended after mindview_gen; LAYOUT_VERSION
    //   0x0285→0x0286, Shared 8512 → 8624 bytes. Non-zero defaults grow the byte image,
    //   so the whole-struct hash necessarily moves → re-pinned from the live `cargo test`
    //   print → 11090782705610843067. The companion `the_glyph_append_leaves_every_pre_
    //   append_byte_where_it_was` proves the first 8512 bytes still hash to the previous
    //   golden, which is what makes this re-pin a growth and not a shift.
    const GOLDEN_DEFAULT_SHARED_HASH: u64 = 11090782705610843067;

    /// The byte image of every field that existed before the #541 T1 append, and the
    /// hash it produced. Saved Ableton sets and stored presets are decoded against
    /// these offsets, so a shift here is the failure mode that silently corrupts a
    /// user's session — not a test-maintenance chore.
    const PRE_MINDVIEW_SHARED_SIZE: usize = 8348;
    const PRE_MINDVIEW_GOLDEN_HASH: u64 = 11052600774956997082;
    /// The `LAYOUT_VERSION` that `PRE_MINDVIEW_GOLDEN_HASH` was pinned at. The field is
    /// stored inside the prefix, so the append changes it by design.
    const PRE_MINDVIEW_LAYOUT_VERSION: u32 = 0x0283;

    /// organon#217 T3 — the same guard one append later: the byte image of every field
    /// that existed before `glyph` / `glyph_cam` / `capsule`, and the hash it produced
    /// (the 0x0285 `GOLDEN_DEFAULT_SHARED_HASH`, which was the hash of the whole 8512-
    /// byte struct at the time).
    const PRE_GLYPH_SHARED_SIZE: usize = 8512;
    const PRE_GLYPH_GOLDEN_HASH: u64 = 8436249494989495788;
    const PRE_GLYPH_LAYOUT_VERSION: u32 = 0x0285;


    // #187 Tier 3: the factory Rails presets must stay recallable — well-formed,
    // uniquely named, targeting the Rails generator, and serde-round-trippable
    // (a broken factory preset would poison the seeded user store).
    #[test]
    fn builtin_rails_presets_are_wellformed() {
        let b = crate::preset::builtin_rails_presets();
        assert_eq!(b.len(), 5);
        // #187 pivot: the factory rides run scenery-only — generator off,
        // Zone corridor on.
        let rails = crate::params::GeneratorMode::None.to_u32();
        let mut names: Vec<&str> = b.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), b.len(), "duplicate factory preset names");
        for p in &b {
            assert!(p.name.starts_with("Rails — "), "{} not namespaced", p.name);
            assert_eq!(p.values.generator, rails, "{} must switch the generator off", p.name);
            assert_eq!(p.values.sc_mode, 1, "{} must enable the Zone scenery", p.name);
            let json = serde_json::to_string(&p.values).unwrap();
            let back: PresetValues = serde_json::from_str(&json).unwrap();
            assert_eq!(back, p.values, "{} does not round-trip", p.name);
        }
    }

    #[test]
    fn default_shared_snapshot_is_stable() {
        let s = OrganicMathParams::default().to_shared();
        let h = fnv1a(bytemuck::bytes_of(&s));
        assert_eq!(
            h, GOLDEN_DEFAULT_SHARED_HASH,
            "Default→Shared bytes changed — a param packing migration is not \
             byte-identical (or a default legitimately changed; if so update this \
             golden in the same PR)."
        );
    }

    /// The #541 T1 append must be *purely* an append: every byte that existed before
    /// `mindview` has to sit at the same offset afterwards.
    ///
    /// This is checkable exactly, not by inspection. `PRE_MINDVIEW_GOLDEN_HASH` is the
    /// previous `GOLDEN_DEFAULT_SHARED_HASH`, and that value was the hash of the whole
    /// struct back when the whole struct was `PRE_MINDVIEW_SHARED_SIZE` bytes. So
    /// hashing the first that-many bytes of today's struct and getting the same number
    /// back proves the prefix is byte-identical — nothing shifted, nothing repacked.
    ///
    /// With one deliberate exception, which this test rewinds rather than tolerates:
    /// `layout_version` lives *inside* that prefix, and the whole point of the append
    /// is that it bumps 0x0283 → 0x0284. Setting the field back to the version the
    /// golden was pinned at isolates that one intended byte change, so the assertion
    /// still fails on any *other* movement in the prefix. Widening the exception (say,
    /// by skipping a byte range) would blunt exactly the check that matters.
    ///
    /// Without this, bumping the whole-struct golden would be indistinguishable from
    /// papering over a real layout regression, since both look like "the hash moved".
    #[test]
    fn the_mindview_append_leaves_every_pre_append_byte_where_it_was() {
        let mut s = OrganicMathParams::default().to_shared();
        s.layout_version = PRE_MINDVIEW_LAYOUT_VERSION;
        let bytes = bytemuck::bytes_of(&s);
        assert!(
            bytes.len() > PRE_MINDVIEW_SHARED_SIZE,
            "Shared should have grown past the pre-append size, got {}",
            bytes.len()
        );
        assert_eq!(
            fnv1a(&bytes[..PRE_MINDVIEW_SHARED_SIZE]),
            PRE_MINDVIEW_GOLDEN_HASH,
            "the bytes ahead of the mindview append moved — an existing field changed \
             offset or packing. Saved sets and stored presets decode against those \
             offsets, so this corrupts sessions rather than merely failing a test. Do \
             NOT re-pin this value to make it pass."
        );
    }

    /// organon#217 T3 — the same proof for the look-control append: the first 8512
    /// bytes of today's struct, with `layout_version` rewound to 0x0285, still hash to
    /// the 0x0285 golden. So the three new blocks are purely an append; nothing ahead
    /// of them moved or repacked, and a preset saved yesterday decodes at the same
    /// offsets today.
    #[test]
    fn the_glyph_append_leaves_every_pre_append_byte_where_it_was() {
        let mut s = OrganicMathParams::default().to_shared();
        s.layout_version = PRE_GLYPH_LAYOUT_VERSION;
        let bytes = bytemuck::bytes_of(&s);
        assert!(
            bytes.len() > PRE_GLYPH_SHARED_SIZE,
            "Shared should have grown past the pre-append size, got {}",
            bytes.len()
        );
        assert_eq!(
            fnv1a(&bytes[..PRE_GLYPH_SHARED_SIZE]),
            PRE_GLYPH_GOLDEN_HASH,
            "the bytes ahead of the T3 append moved — an existing field changed offset \
             or packing. Do NOT re-pin this value to make it pass."
        );
    }

    /// organon#217 T3 — the factory `faceplate` preset must stay recallable: uniquely
    /// named, serde-round-trippable, and asking for exactly the things the ladder rung
    /// promises (the held camera, a dark room, the bevel + crown, halation), each of
    /// which is a link a recall has to reach. Its Look-side glyph values pack to the
    /// `Shared.glyph` slots `world::glyph_look_from` reads.
    #[test]
    fn builtin_text_presets_are_wellformed() {
        let b = crate::preset::builtin_text_presets();
        assert_eq!(b.len(), 1);
        let p = &b[0];
        assert_eq!(p.name, "faceplate");
        let json = serde_json::to_string(&p.values).unwrap();
        let back: PresetValues = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p.values, "faceplate does not round-trip");
        assert!(p.values.glyph_cam_hold, "faceplate must hold the camera (T5 cannot converge otherwise)");
        assert_eq!(p.values.cam_path, crate::params::CamPath::Off.to_u32(), "and the orbit path is off");
        assert!(!p.values.atmos_enabled, "a dark room: no atmosphere");
        assert!(!p.values.bg_visible, "a dark room: background hidden");
        assert!(p.values.env_intensity < 0.5, "a dark room: the IBL dimmed to a sheen");
        assert!(p.values.glyph_bevel > 0.0 && p.values.glyph_crown > 0.0, "the two normal-varying controls");
        // organon#217 T9 — the spec-sheet tile: a soft core, and every cell tiled.
        assert!(p.values.glyph_profile > 0.0, "faceplate must ask for an emission profile");
        assert!(p.values.glyph_dark_tiles, "faceplate must tile every cell (the spec-sheet plate)");
        assert!(p.values.fx_enabled, "halation lives in the FX pass, which must be on");
        assert!(p.values.hal_amount > 0.0);
        // The preset's glyph values reach the Shared slots, in the contract's order.
        let s = p.values.to_shared();
        assert_eq!(s.glyph[11], p.values.glyph_bevel);
        assert_eq!(s.glyph[12], p.values.glyph_crown);
        assert_eq!(s.glyph[13], p.values.glyph_profile, "profile rides slot 13 (`Uniforms.shape.z`)");
        assert_eq!(s.glyph[14], 1.0, "dark tiles ride slot 14 as a 0/1 flag");
        assert_eq!(s.glyph[15], 0.0, "slot 15 is untouched by this rung (T12's lane, inert at 0)");
        assert_eq!(s.glyph_cam[0], 1.0);
        assert_eq!(s.glyph_cam[1], p.values.glyph_cam_tilt);
        assert_eq!(s.capsule[0], 0.0, "faceplate is not the bottled rung");
        // Everything the rung does not name is T1's look — the T1 grid, dressed.
        let d = OrganicMathParams::default().to_shared();
        assert_eq!(s.glyph[..11], d.glyph[..11]);
    }

    // --- Capture/apply drift guard (#103, PR 4) -------------------------------
    // A preset-captured param must survive `params → PresetValues → Shared` the
    // same as `params → Shared` directly. If a param is added to `params`/the
    // packing table but forgotten in `PresetValues`/`capture`, the two snapshots
    // diverge here and this fails — so a silently-dropped preset param becomes a
    // test failure rather than a "preset doesn't restore that knob" surprise.
    //
    // (Array-block params are *already* compile-protected — the `pack_*_preset`
    // packers reference each `PresetValues` field, so a missing one won't build.
    // This additionally covers the hand-written scalar fields.)
    //
    // Fields presets intentionally don't capture (per-display / quality / runtime-
    // written) are blanked in both before comparing.
    fn blank_uncaptured(s: &mut Shared) {
        s.seq = 0;
        s.layout_version = 0;
        s.hdr_gen = 0;
        s.material_gen = 0; // #472 material folder-load counter (runtime, sidecar)
        s.transport = [0.0; 4]; // host transport (runtime)
        s.audio = [0.0; 8]; // live band analysis (runtime)
        s.voices = [0.0; 64]; // #339 runtime-written voice bank (played notes)
        // #354: terrain/stars/atmosphere/clouds/ocean + capture are now
        // Environment/Settings-captured, so they are NO LONGER blanked — the
        // mirror test verifies their pack_*_preset packers byte-match.
        s.hdr_output = 0; // per-display HDR / quality — captured but ordinal↔count
        s.hdr_knee = 0.0; //   conversion differs from the live packer, so not
        s.hdr_wide = 0; //     mirror-verified here (compile-checked via capture).
        s.hdr_vivid = 0.0;
        s.msaa = 0;
        s.pathtrace_on = 0; // per-display path-tracer toggle (#200 Tier 4)
        s.render_scale = 0.0; // captured; not mirror-verified (see above)
        s.render_auto = 0;
        s.overlay = [0.0; 16]; // per-display capture overlay style (#135 Phase 2)
        s.overlay_gen = 0; // runtime-written (sidecar counter)
        s.creature_gen = 0; // #476 T2b runtime-written (creature-JSON load counter)
        s.axes = [0.0; 16]; // per-display capture decoration (#135 P5)
        s.scopewave = [0.0; 260]; // #346 runtime-written oscilloscope display frame
        s.temporal = [0.0; 8]; // per-display TAA / motion blur / stochastic (#152 T2)
        s.rt[1] = 0.0; // per-display RT debug view (#195 Tier 0)
    }

    #[test]
    fn captured_params_survive_the_preset_mirror() {
        let p = OrganicMathParams::default();
        let mut from_params = p.to_shared();
        let mut from_preset = PresetValues::capture(&p).to_shared();
        blank_uncaptured(&mut from_params);
        blank_uncaptured(&mut from_preset);
        assert_eq!(
            fnv1a(bytemuck::bytes_of(&from_params)),
            fnv1a(bytemuck::bytes_of(&from_preset)),
            "params→Shared and params→PresetValues→Shared disagree on a captured \
             field — a param was likely added to `params` but not mirrored in \
             `PresetValues`/`capture` (or a preset default drifted from the param \
             default)."
        );
    }

    // --- Preset JSON schema round-trips (saved presets must keep loading). -----
    #[test]
    fn preset_json_round_trips() {
        let pv = PresetValues::capture(&OrganicMathParams::default());
        let json = serde_json::to_string(&pv).expect("serialize");
        let back: PresetValues = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(pv, back, "PresetValues did not survive a JSON round-trip");
    }

    // Pre-E↔B-blend presets stored a generator bool (`mx_trace_b`) + an aura enum
    // (`mn_aura_field`: 0=follow, 1=Electric, 2=Magnetic). `migrate_legacy_fields`
    // must fold them onto the 0=E…1=B blends WITHOUT the inversion Bugbot flagged
    // (old Electric=1 must NOT become blend 1.0 = B), and must leave new presets
    // (no legacy keys) untouched.
    #[test]
    fn legacy_aura_generator_presets_migrate_to_blends() {
        let base = || PresetValues::capture(&OrganicMathParams::default());

        // Generator B + Electric aura: gen→B, aura→E (the anti-inversion case).
        let mut pv = base();
        pv.mx_trace_b = Some(true);
        pv.mn_aura_field = Some(1);
        pv.migrate_legacy_fields();
        assert_eq!(pv.mx_gen_blend, 1.0);
        assert_eq!(pv.mx_aura_blend, 0.0, "Electric preset must stay E, not flip to B");
        assert_eq!(pv.mx_trace_b, None);
        assert_eq!(pv.mn_aura_field, None);

        // Generator E + Magnetic aura → aura B.
        let mut pv = base();
        pv.mx_trace_b = Some(false);
        pv.mn_aura_field = Some(2);
        pv.migrate_legacy_fields();
        assert_eq!(pv.mx_gen_blend, 0.0);
        assert_eq!(pv.mx_aura_blend, 1.0);

        // Follow Generator (0) → aura tracks the migrated generator blend (B here).
        let mut pv = base();
        pv.mx_trace_b = Some(true);
        pv.mn_aura_field = Some(0);
        pv.migrate_legacy_fields();
        assert_eq!(pv.mx_aura_blend, 1.0);

        // Pre-#304 preset (generator key present, no aura key) → aura follows gen.
        let mut pv = base();
        pv.mx_trace_b = Some(true);
        pv.mn_aura_field = None;
        pv.migrate_legacy_fields();
        assert_eq!(pv.mx_aura_blend, 1.0);

        // New preset (no legacy keys) → the stored blends are left untouched.
        let mut pv = base();
        pv.mx_gen_blend = 0.3;
        pv.mx_aura_blend = 0.7;
        pv.migrate_legacy_fields();
        assert_eq!(pv.mx_gen_blend, 0.3);
        assert_eq!(pv.mx_aura_blend, 0.7);
    }

    // --- Per-tab preset partition drift guard (#145) --------------------------
    // The `for_each_tab_field!` SSoT in `preset.rs` assigns every captured field
    // to exactly one editor tab; it drives both `apply_tab` (partial recall) and
    // this list. Here we prove the partition is total + disjoint over the *actual*
    // captured `PresetValues` fields — so a future param added to `params`/
    // `capture` but forgotten in the partition can't silently fall out of every
    // tab preset (it becomes a test failure instead).
    #[test]
    fn tab_partition_is_exactly_the_captured_fields() {
        use std::collections::HashSet;

        let list = PresetValues::tab_field_list();

        // Disjoint: no field assigned to two tabs.
        let mut partition: HashSet<&'static str> = HashSet::new();
        for (name, _) in &list {
            assert!(
                partition.insert(*name),
                "field `{name}` is assigned to more than one tab"
            );
        }

        // Total: the partition names == the captured serde fields, exactly, save
        // for the **deliberately out-of-band** fields listed below. Each is a file
        // path — a loaded `.hdr` and a loaded `.gguf` specimen — restored by
        // re-driving a sidecar in `apply_recall` (and kept in its owning bucket by
        // `subset_entry`), not applied through the tab partition like a normal
        // param: neither is a nih-plug param, so neither can be a
        // `for_each_tab_field!` entry.
        //
        // ⚠️ **This list is the exception, so adding to it is the decision, not the
        // bookkeeping.** A field dropped here is a field this guard stops watching;
        // it earns that only by having its own restore path and its own test. Both
        // current entries do (`preset::recall_redrives`, and the
        // `Environment keeps hdr_path` / `Generator keeps model_path` filter
        // tests).
        //
        // `capture()` reads the live sidecars, so on a machine with an `.hdr` or a
        // `.gguf` loaded the corresponding field is non-empty and serializes (both
        // are `skip_serializing_if` empty, so they vanish on clean CI); dropping
        // them here keeps this drift guard hermetic instead of failing only when a
        // developer happens to have one loaded.
        const OUT_OF_BAND: [&str; 2] = ["hdr_path", "model_path"];
        let json =
            serde_json::to_value(PresetValues::capture(&OrganicMathParams::default())).unwrap();
        let mut captured: HashSet<String> = json.as_object().unwrap().keys().cloned().collect();
        for name in OUT_OF_BAND {
            captured.remove(name);
        }
        let partition: HashSet<String> = partition.iter().map(|s| s.to_string()).collect();

        let orphans: Vec<_> = captured.difference(&partition).collect();
        let strays: Vec<_> = partition.difference(&captured).collect();
        assert!(
            orphans.is_empty(),
            "captured PresetValues fields missing from the tab partition \
             (they'd never recall in any tab preset): {orphans:?}"
        );
        assert!(
            strays.is_empty(),
            "tab partition references names that aren't captured PresetValues fields: {strays:?}"
        );
        assert_eq!(list.len(), captured.len());
    }
}
