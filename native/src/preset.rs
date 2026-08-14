//! Preset system: a preset is the full state of every parameter. Saved to a
//! single JSON file in the user data dir; recall sets every parameter through
//! the host's `ParamSetter` (so it's automation-recordable and undoable).

use crate::params::{
    AcousticModel, AcousticSource, AnalyticalMode, AoSource,
    AttractorField, AxonMode, BarPeriod, CalColourSource, CalLut, CamOrder, CamPath, CamTransition, ColourMode, DemoScene, DnaForm, DollyWave, FieldKind, FieldPreset, FdtdSource, FieldVolSource, FluxAxis, GeneratorMode, HudDock, KifsPalette, KifsPattern, KifsSpace,
    AnimMode, BakeRes, BlendMode, KaleidoMode, KifsView, LiqMaterial, LiqRender, LiqShape, LSystem, MatChannel, MaterialType, MatNoise, MatProjection, MembraneArmBuild, MembraneWeave, MinimalFamily, ModTarget, NeuralFireMode, NeuralTopology, NeuronType, OriginMode, OrganicMathParams, OscDivision, Palette,
    PanelStyle, ParticleMaterial, ParticleShape, ParticleTier, PhylSurface, PtComposite, PulseSource, RailArchetype, RailCellLen, RailChangeEvery, ReflectionSource, RenderStyle, RippleGeom, SceneryMode, ScenerySurface, SurfaceMode, TerraForm,
    SynthMode, SynthPlayMode, SynthQuantize, TuningLayout,
    SplatMode, MapKindParam, MapOrbitModeParam, MapColorParam, PdePreset,
    AspectPreset, Msaa, MeterAveraging, MeterWeighting, SpectrumMode, TerrainNoise, TerrainPalette, TerrainRes,
    SyncView, TempoSource, TessHeightMode, TessView, TilingConstruct, TilingFamily, ToneMap, VecFieldPreset,
    VecFieldOp, VecFieldView, VecLineColor, VecMagMap, VecSeedMode, VecTermFunc, VecTint,
};
use nih_plug::prelude::*;
use crate::params::{HostBoidsForm, HostFuncName, HostGeneratorMode, HostOscDivision};

/// #626 Tier 3 — the tab taxonomy now lives in `organon-core`; re-exported here so
/// every existing `crate::preset::{UiTab, EditorTab}` path keeps resolving.
///
/// Named, not glob: `pub use organon_core::tabs::*` would let core silently widen
/// this module's surface later. `ARCHITECTURE.md` §19 records why the facade exists
/// at all rather than rewriting ~60 call sites — chiefly that Tier 4 moves `gguf`
/// again, and call-site churn done twice is churn done wrong.
pub use organon_core::tabs::{EditorTab, UiTab};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The complete, serializable parameter state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PresetValues {
    #[serde(default)] // 0 = Original (old presets predate the generator selector)
    pub generator: u32,
    #[serde(default)] pub loop_count_x: i32,
    #[serde(default)] pub loop_count_y: i32,
    #[serde(default)] pub loop_count_z: i32,
    #[serde(default)] pub loop_count_q: i32,
    #[serde(default)] pub rot_func: u32,
    #[serde(default)] pub rot_amp_x: f32,
    #[serde(default)] pub rot_amp_y: f32,
    #[serde(default)] pub rot_amp_z: f32,
    #[serde(default)] pub rot_mod_x: f32,
    #[serde(default)] pub rot_mod_y: f32,
    #[serde(default)] pub rot_mod_z: f32,
    #[serde(default)]
    pub continuous: bool,
    #[serde(default)] // 0 = constant spin
    pub cont_shape: f32,
    #[serde(default)] pub trans_func: u32,
    #[serde(default)] pub trans_amp_x: f32,
    #[serde(default)] pub trans_amp_y: f32,
    #[serde(default)] pub trans_amp_z: f32,
    #[serde(default)] pub trans_mod_x: f32,
    #[serde(default)] pub trans_mod_y: f32,
    #[serde(default)] pub trans_mod_z: f32,
    #[serde(default)] pub scale_func: u32,
    #[serde(default)] pub scale_amp: f32,
    // --- Frenet–Serret generator (defaults = the clean helix bundle) ---
    #[serde(default = "def_frenet_strands")]
    pub frenet_strands: i32,
    #[serde(default = "def_frenet_nodes")]
    pub frenet_nodes: i32,
    #[serde(default = "def_frenet_step")]
    pub frenet_step: f32,
    #[serde(default)] // 0 = Sin
    pub frenet_func: u32,
    #[serde(default = "def_frenet_kappa")]
    pub frenet_kappa: f32,
    #[serde(default)]
    pub frenet_kappa_amp: f32,
    #[serde(default = "def_one")]
    pub frenet_kappa_freq: f32,
    #[serde(default = "def_frenet_tau")]
    pub frenet_tau: f32,
    #[serde(default)]
    pub frenet_tau_amp: f32,
    #[serde(default = "def_one")]
    pub frenet_tau_freq: f32,
    #[serde(default = "def_frenet_spread")]
    pub frenet_spread: f32,
    #[serde(default = "def_frenet_thickness")]
    pub frenet_thickness: f32,
    // --- DNA double-helix generator (defaults = relaxed B-DNA) ---
    #[serde(default = "def_dna_form")] // 1 = B-DNA
    pub dna_form: u32,
    #[serde(default = "def_dna_bp")]
    pub dna_bp: i32,
    #[serde(default = "def_dna_bp_per_turn")]
    pub dna_bp_per_turn: f32,
    #[serde(default = "def_dna_rise")]
    pub dna_rise: f32,
    #[serde(default = "def_dna_radius")]
    pub dna_radius: f32,
    #[serde(default = "def_dna_groove")]
    pub dna_groove: f32,
    #[serde(default)]
    pub dna_left: bool,
    #[serde(default)]
    pub dna_sigma: f32,
    #[serde(default = "def_dna_super_radius")]
    pub dna_super_radius: f32,
    #[serde(default = "def_dna_seed")]
    pub dna_seed: i32,
    #[serde(default = "def_dna_thickness")]
    pub dna_thickness: f32,
    #[serde(default)]
    pub dna_twist_breathe: f32,
    // --- Strange-attractor generator (defaults = Lorenz, flowing trail) ---
    #[serde(default)] // 0 = Lorenz
    pub attr_field: u32,
    #[serde(default = "def_attr_seeds")]
    pub attr_seeds: i32,
    #[serde(default = "def_attr_seed")]
    pub attr_seed: i32,
    #[serde(default = "def_attr_spread")]
    pub attr_spread: f32,
    #[serde(default = "def_one")]
    pub attr_dt: f32,
    #[serde(default = "def_attr_trail")]
    pub attr_trail: i32,
    #[serde(default = "def_one")]
    pub attr_speed: f32,
    #[serde(default = "def_one")]
    pub attr_scale: f32,
    #[serde(default = "def_attr_thickness")]
    pub attr_thickness: f32,
    // --- Boids / flocking generator (#52; defaults = 120-agent gather) ---
    #[serde(default = "def_boids_count")]
    pub boids_count: i32,
    #[serde(default = "def_boids_perception")]
    pub boids_perception: f32,
    #[serde(default = "def_boids_separation")]
    pub boids_separation: f32,
    #[serde(default = "def_boids_sep")]
    pub boids_sep: f32,
    #[serde(default = "def_one")]
    pub boids_align: f32,
    #[serde(default = "def_one")]
    pub boids_cohere: f32,
    #[serde(default = "def_boids_max_speed")]
    pub boids_max_speed: f32,
    #[serde(default = "def_boids_max_force")]
    pub boids_max_force: f32,
    #[serde(default = "def_boids_trail")]
    pub boids_trail: i32,
    #[serde(default = "def_boids_bounds")]
    pub boids_bounds: f32,
    #[serde(default = "def_boids_goal")]
    pub boids_goal: f32,
    #[serde(default = "def_boids_thickness")]
    pub boids_thickness: f32,
    #[serde(default = "def_boids_seed")]
    pub boids_seed: i32,
    #[serde(default = "def_one")]
    pub boids_speed: f32,
    #[serde(default = "def_boids_scale")]
    pub boids_scale: f32,
    #[serde(default = "def_boids_form")]
    pub boids_form: u32,
    #[serde(default = "def_boids_size")]
    pub boids_size: f32,
    #[serde(default = "def_boids_bank")]
    pub boids_bank: f32,
    // --- Spherical-harmonic generator (defaults = Y₂₀ + Y₃₀ bell) ---
    #[serde(default = "def_harm_mode0")]
    pub harm_mode0: i32,
    #[serde(default = "def_harm_amp0")]
    pub harm_amp0: f32,
    #[serde(default = "def_one")]
    pub harm_freq0: f32,
    #[serde(default = "def_harm_mode1")]
    pub harm_mode1: i32,
    #[serde(default = "def_harm_amp1")]
    pub harm_amp1: f32,
    #[serde(default = "def_harm_freq1")]
    pub harm_freq1: f32,
    #[serde(default = "def_harm_mode2")]
    pub harm_mode2: i32,
    #[serde(default)]
    pub harm_amp2: f32,
    #[serde(default = "def_harm_freq2")]
    pub harm_freq2: f32,
    #[serde(default = "def_harm_radius")]
    pub harm_radius: f32,
    #[serde(default = "def_harm_theta")]
    pub harm_theta: i32,
    #[serde(default = "def_harm_phi")]
    pub harm_phi: i32,
    #[serde(default = "def_harm_thickness")]
    pub harm_thickness: f32,
    // --- Soft-body bell (#99): physical mode on the harmonic generator ---
    #[serde(default)] // false = closed-form
    pub bell_physical: bool,
    #[serde(default = "def_bell_stroke_depth")]
    pub bell_stroke_depth: f32,
    #[serde(default = "def_bell_stiffness")]
    pub bell_stiffness: i32,
    #[serde(default = "def_bell_damping")]
    pub bell_damping: f32,
    #[serde(default = "def_bell_open")]
    pub bell_open: f32,
    #[serde(default = "def_bell_rate")]
    pub bell_speed: f32,
    // --- L-system generator (defaults = depth-4 fern) ---
    #[serde(default)] // 0 = Fern
    pub ls_system: u32,
    #[serde(default = "def_ls_depth")]
    pub ls_depth: i32,
    #[serde(default = "def_ls_angle")]
    pub ls_angle: f32,
    #[serde(default = "def_ls_step")]
    pub ls_step: f32,
    #[serde(default)]
    pub ls_sway_amp: f32,
    #[serde(default = "def_one")]
    pub ls_sway_freq: f32,
    #[serde(default = "def_one")]
    pub ls_grow: f32,
    #[serde(default = "def_ls_thickness")]
    pub ls_thickness: f32,
    // --- Curl-noise flow-field generator (defaults = free flow) ---
    #[serde(default = "def_cn_seeds")]
    pub cn_seeds: i32,
    #[serde(default = "def_cn_seed")]
    pub cn_seed: i32,
    #[serde(default = "def_cn_spread")]
    pub cn_spread: f32,
    #[serde(default = "def_cn_scale")]
    pub cn_scale: f32,
    #[serde(default = "def_cn_steps")]
    pub cn_steps: i32,
    #[serde(default = "def_cn_dt")]
    pub cn_dt: f32,
    #[serde(default = "def_one")]
    pub cn_flow: f32,
    #[serde(default)]
    pub cn_bound: f32,
    #[serde(default = "def_cn_thickness")]
    pub cn_thickness: f32,
    // --- Circular-polarization generator (defaults = a warm radiating bloom) ---
    #[serde(default = "def_pol_rings")]
    pub pol_rings: i32,
    #[serde(default = "def_pol_spokes")]
    pub pol_spokes: i32,
    #[serde(default = "def_pol_samples")]
    pub pol_samples: i32,
    #[serde(default = "def_pol_len")]
    pub pol_len: f32,
    #[serde(default = "def_pol_k")]
    pub pol_k: f32,
    #[serde(default = "def_pol_amp")]
    pub pol_amp: f32,
    #[serde(default)]
    pub pol_falloff: f32,
    #[serde(default)]
    pub pol_handed: bool,
    #[serde(default = "def_pol_spread")]
    pub pol_spread: f32,
    #[serde(default)]
    pub pol_swirl: f32,
    #[serde(default)]
    pub pol_show_b: bool,
    #[serde(default = "def_pol_thickness")]
    pub pol_thickness: f32,
    // --- Maxwell radiation-field generator (defaults = one oscillating dipole) ---
    #[serde(default)]
    pub mx_lines: bool,
    #[serde(default)]
    pub mx_gen_blend: f32,
    #[serde(default = "def_true")]
    pub mx_dipoles: bool,
    #[serde(default = "def_mx_sources")]
    pub mx_sources: i32,
    #[serde(default = "def_mx_separation")]
    pub mx_separation: f32,
    #[serde(default = "def_mx_phase")]
    pub mx_phase: f32,
    #[serde(default)]
    pub mx_swirl: f32,
    #[serde(default)]
    pub mx_near: f32,
    #[serde(default = "def_mx_k")]
    pub mx_k: f32,
    #[serde(default = "def_mx_amp")]
    pub mx_amp: f32,
    #[serde(default = "def_mx_rmin")]
    pub mx_rmin: f32,
    #[serde(default = "def_mx_thickness")]
    pub mx_thickness: f32,
    #[serde(default = "def_mx_rings")]
    pub mx_rings: i32,
    #[serde(default = "def_mx_spokes")]
    pub mx_spokes: i32,
    #[serde(default = "def_mx_samples")]
    pub mx_samples: i32,
    #[serde(default = "def_mx_raylen")]
    pub mx_raylen: f32,
    #[serde(default = "def_mx_spread")]
    pub mx_spread: f32,
    #[serde(default = "def_mx_seeds")]
    pub mx_seeds: i32,
    #[serde(default = "def_mx_steps")]
    pub mx_steps: i32,
    #[serde(default = "def_mx_ds")]
    pub mx_ds: f32,
    #[serde(default = "def_mx_bound")]
    pub mx_bound: f32,
    #[serde(default)] // false = raw-magnitude "wave" strands (the original lattice look)
    pub mx_norm_field: bool,
    #[serde(default)] // false = free-running Speed clock (historical oscillation)
    pub mx_osc_sync: bool,
    #[serde(default = "def_mx_osc_div")]
    pub mx_osc_div: u32,
    #[serde(default)] // 0° = far-field / in-phase lock
    pub mx_eb_phase: f32,
    // --- Acoustic-field generator (#325, Duo-Field N1) ---
    #[serde(default = "def_ac_source")]
    pub ac_source: u32,
    #[serde(default = "def_ac_k")]
    pub ac_k: f32,
    #[serde(default = "def_ac_near")]
    pub ac_near: f32,
    #[serde(default = "def_ac_amp")]
    pub ac_amp: f32,
    #[serde(default = "def_ac_separation")]
    pub ac_separation: f32,
    #[serde(default = "def_ac_rmin")]
    pub ac_rmin: f32,
    #[serde(default)] // 0 = pressure geometry
    pub ac_blend: f32,
    #[serde(default)] // false = raw-magnitude "wave" strands
    pub ac_norm_field: bool,
    #[serde(default = "def_ac_rings")]
    pub ac_rings: i32,
    #[serde(default = "def_ac_spokes")]
    pub ac_spokes: i32,
    #[serde(default = "def_ac_samples")]
    pub ac_samples: i32,
    #[serde(default = "def_ac_raylen")]
    pub ac_raylen: f32,
    #[serde(default = "def_ac_spread")]
    pub ac_spread: f32,
    #[serde(default = "def_ac_thickness")]
    pub ac_thickness: f32,
    #[serde(default = "def_ac_aura_blend")]
    pub ac_aura_blend: f32,
    #[serde(default)] // beat pump off
    pub ac_beat_pump: f32,
    // --- #381 Tier 1 Field Engine (arbitrary closed-form field equations) ---
    #[serde(default)] // 0 = Auto (infer kind)
    pub field_kind: u32,
    #[serde(default)] // 0 = Coulomb
    pub field_preset: u32,
    #[serde(default = "def_field_scale")]
    pub field_scale: f32,
    #[serde(default = "def_field_extent")]
    pub field_extent: f32,
    #[serde(default = "def_field_ab")]
    pub field_a: f32,
    #[serde(default = "def_field_ab")]
    pub field_b: f32,
    #[serde(default = "def_field_density")]
    pub field_density: i32,
    #[serde(default = "def_field_ab")]
    pub field_gain: f32,
    #[serde(default = "def_field_thickness")]
    pub field_thickness: f32,
    // --- #381 Tier 3 Field Engine (time-marched PDE dynamics) ---
    #[serde(default)] // 0 = Off (static field → byte-identical)
    pub pde_preset: u32,
    #[serde(default = "def_field_ab")] // D = 1
    pub sim_diffusion: f32,
    #[serde(default = "def_field_ab")] // time_scale = 1
    pub sim_time_scale: f32,
    #[serde(default = "def_sim_feed")]
    pub sim_feed: f32,
    #[serde(default = "def_sim_kill")]
    pub sim_kill: f32,
    #[serde(default = "def_field_ab")] // potential = 1
    pub sim_potential: f32,
    #[serde(default)] // forcing = 0
    pub sim_forcing: f32,
    #[serde(default = "def_sim_res")]
    pub sim_res: i32,
    // --- #346 Field Chamber (analyzer panels on the box back walls) ---
    #[serde(default)] // panels off → byte-identical
    pub panels_on: bool,
    #[serde(default)] // Flat
    pub panel_style: u32,
    #[serde(default = "def_ch_rear")]
    pub panel_rear: bool,
    #[serde(default = "def_ch_right")]
    pub panel_right: bool,
    #[serde(default = "def_ch_opacity")]
    pub panel_opacity: f32,
    #[serde(default = "def_ch_fill")]
    pub panel_fill: f32,
    #[serde(default = "def_ch_scope_amp")]
    pub panel_scope_amp: f32,
    #[serde(default = "def_ch_db_floor")]
    pub panel_db_floor: f32,
    #[serde(default = "def_ch_db_top")]
    pub panel_db_top: f32,
    #[serde(default)] // Standard
    pub panel_material: u32,
    #[serde(default = "def_ch_metallic")]
    pub panel_metallic: f32,
    #[serde(default = "def_ch_roughness")]
    pub panel_roughness: f32,
    #[serde(default)] // no emissive
    pub panel_emissive: f32,
    #[serde(default = "def_ch_thickness")]
    pub panel_thickness: f32,
    #[serde(default)] // fixed world axes
    pub panel_wall_rel: bool,
    #[serde(default = "def_ch_scope_time_ms")]
    pub panel_scope_time_ms: f32,
    #[serde(default = "def_ch_scope_trigger")]
    pub panel_scope_trigger: i32,
    #[serde(default = "def_ch_scope_channel")]
    pub panel_scope_channel: i32,

    // --- #339 Duo-Field synthesis (Sound card) ---
    #[serde(default)] // synth off (byte-identical passthrough)
    pub sn_on: bool,
    #[serde(default)] // Generative
    pub sn_play_mode: u32,
    #[serde(default = "def_sn_gain")]
    pub sn_gain: f32,
    #[serde(default = "def_sn_mix")]
    pub sn_mix: f32,
    #[serde(default = "def_sn_gen_freq")]
    pub sn_tuning: f32,
    #[serde(default = "def_sn_gen_amp")]
    pub sn_gen_amp: f32,
    // --- #339 Tier 2 oscillator lattice ---
    #[serde(default)] // Probes
    pub sn_mode: u32,
    #[serde(default = "def_sn_bank")]
    pub sn_bank: i32,
    #[serde(default)] // Octaves
    pub sn_tuning_layout: u32,
    #[serde(default = "def_sn_tune_spread")]
    pub sn_tune_spread: f32,
    #[serde(default)] // harmonic (no stretch)
    pub sn_tune_stretch: f32,
    #[serde(default = "def_sn_shell_r")]
    pub sn_shell_r: f32,
    #[serde(default = "def_sn_shell_rate")]
    pub sn_shell_rate: f32,
    #[serde(default = "def_sn_t60")]
    pub sn_t60: f32,
    #[serde(default = "def_sn_bright")]
    pub sn_bright: f32,
    #[serde(default = "def_sn_grain_size")]
    pub sn_grain_size: f32,
    #[serde(default = "def_sn_grain_density")]
    pub sn_grain_density: f32,
    #[serde(default = "def_sn_attack")]
    pub sn_attack: f32,
    #[serde(default = "def_sn_decay")]
    pub sn_decay: f32,
    #[serde(default = "def_sn_sustain")]
    pub sn_sustain: f32,
    #[serde(default = "def_sn_release")]
    pub sn_release: f32,
    #[serde(default)] // no glide
    pub sn_glide: f32,
    #[serde(default = "def_sn_bend_range")]
    pub sn_bend_range: f32,
    #[serde(default)] // stacked
    pub sn_place_spread: f32,
    #[serde(default = "def_sn_a4")]
    pub sn_a4: f32,
    #[serde(default = "def_sn_probe_lx")]
    pub sn_probe_lx: f32,
    #[serde(default)]
    pub sn_probe_ly: f32,
    #[serde(default = "def_sn_probe_lz")]
    pub sn_probe_lz: f32,
    #[serde(default = "def_sn_probe_rx")]
    pub sn_probe_rx: f32,
    #[serde(default)]
    pub sn_probe_ry: f32,
    #[serde(default = "def_sn_probe_lz")]
    pub sn_probe_rz: f32,
    #[serde(default)] // fixed probes
    pub sn_probe_cam: bool,
    #[serde(default = "def_sn_gen_freq")]
    pub sn_vis_pivot: f32,
    #[serde(default = "def_sn_vis_anchor")]
    pub sn_vis_anchor: f32,
    #[serde(default = "def_sn_vis_slope")]
    pub sn_vis_slope: f32,
    #[serde(default = "def_sn_vis_k_anchor")]
    pub sn_vis_k_anchor: f32,
    #[serde(default = "def_sn_vis_slope")]
    pub sn_vis_k_slope: f32,
    #[serde(default)] // Free
    pub sn_vis_quantize: u32,
    // --- Acoustic Tier 4 (#325): cavity modes + intensity flux ---
    #[serde(default)] // 0 = Radiating (Tiers 1–3)
    pub ac2_model: u32,
    #[serde(default = "def_ac2_nx")]
    pub ac2_nx: i32,
    #[serde(default = "def_ac2_ny")]
    pub ac2_ny: i32,
    #[serde(default = "def_ac2_nz")]
    pub ac2_nz: i32,
    #[serde(default)] // static modes
    pub ac2_morph: f32,
    #[serde(default = "def_ac2_cav_scale")]
    pub ac2_cav_scale: f32,
    #[serde(default)] // intensity flux off
    pub ac2_intensity: f32,
    // --- Acoustic Tier 5 (#325): cavity 3-D tween + per-axis audio breathe ---
    #[serde(default = "def_ac2_tween")]
    pub ac2_tween: f32,
    #[serde(default)] // audio → X breathe off
    pub ac2_audio_x: f32,
    #[serde(default)] // audio → Y breathe off
    pub ac2_audio_y: f32,
    #[serde(default)] // audio → Z breathe off
    pub ac2_audio_z: f32,
    // --- Analyzer / Calibrated instrument mode (#333 Tier 3) ---
    #[serde(default)] // 0 = Expressive
    pub analytical_mode: u32,
    #[serde(default = "def_an_target_lufs")]
    pub an_target_lufs: f32,
    #[serde(default = "def_an_floor_lufs")]
    pub an_floor_lufs: f32,
    #[serde(default = "def_an_tp_ceiling")]
    pub an_tp_ceiling: f32,
    #[serde(default)] // 0 correlation alarm threshold
    pub an_corr_alarm: f32,
    #[serde(default)] // instrument HUD off
    pub an_reference_hud: bool,
    // --- Field Volume (#348) ---
    #[serde(default)] // 0 = Legacy (today's Volume, byte-identical)
    pub fv_source: u32,
    #[serde(default = "def_one")] // neutral smoothing
    pub fv_smooth: f32,
    #[serde(default)] // 0 dB exposure
    pub fv_exposure_db: f32,
    #[serde(default)] // calibrated brightness off
    pub fv_calibrate: bool,
    #[serde(default = "def_one")] // neutral gain
    pub fv_gain: f32,
    #[serde(default)] // field-lines off
    pub fv_lines: bool,
    #[serde(default = "def_fv_line_density")]
    pub fv_line_density: f32,
    #[serde(default = "def_fv_line_thickness")]
    pub fv_line_thickness: f32,
    // --- Calibrated colour (#349) ---
    #[serde(default)] // 0 = Aesthetic (today's tint, byte-identical)
    pub col_mode: u32,
    #[serde(default = "def_col_lo_db")]
    pub col_lo_db: f32,
    #[serde(default)] // 0 dBFS high end
    pub col_hi_db: f32,
    #[serde(default)] // 0 = Turbo
    pub col_lut: u32,
    #[serde(default)] // 0 = Auto source
    pub col_source: u32,
    #[serde(default = "def_one")] // full calibrated blend
    pub col_amount: f32,
    // --- Axon Waveguide generator (#218 Tier 1) ---
    #[serde(default = "def_ax_count")]
    pub ax_count: i32,
    #[serde(default = "def_ax_length")]
    pub ax_length: f32,
    #[serde(default = "def_ax_bundle")]
    pub ax_bundle: f32,
    #[serde(default = "def_ax_samples")]
    pub ax_samples: i32,
    #[serde(default = "def_ax_thickness")]
    pub ax_thickness: f32,
    #[serde(default = "def_ax_node_spacing")]
    pub ax_node_spacing: f32,
    #[serde(default = "def_ax_node_dip")]
    pub ax_node_dip: f32,
    #[serde(default = "def_ax_pulse_speed")]
    pub ax_pulse_speed: f32,
    #[serde(default = "def_ax_pulse_width")]
    pub ax_pulse_width: f32,
    #[serde(default = "def_ax_stagger")]
    pub ax_stagger: f32,
    #[serde(default)]
    pub ax_splay: f32,
    #[serde(default = "def_ax_seed")]
    pub ax_seed: i32,
    #[serde(default)]
    pub ax_mode: u32,
    #[serde(default)]
    pub ax_mode_amount: f32,
    #[serde(default = "def_ax_bend")]
    pub ax_bend: f32,
    #[serde(default = "def_ax_curve")]
    pub ax_curve: f32,
    #[serde(default = "def_ax_tortuosity")]
    pub ax_tortuosity: f32,
    #[serde(default)]
    pub ax_dti: f32,
    #[serde(default)]
    pub ax_dispersion: f32,
    #[serde(default)]
    pub ax_polarization: f32,
    // --- Neural Network generator (#226 Tier 1) ---
    #[serde(default = "def_nw_topology")]
    pub nw_topology: u32,
    #[serde(default = "def_nw_nodes")]
    pub nw_nodes: i32,
    #[serde(default = "def_nw_connectivity")]
    pub nw_connectivity: i32,
    #[serde(default = "def_nw_rewire")]
    pub nw_rewire: f32,
    #[serde(default = "def_nw_layers")]
    pub nw_layers: i32,
    #[serde(default = "def_nw_seed")]
    pub nw_seed: i32,
    #[serde(default = "def_nw_extent")]
    pub nw_extent: f32,
    #[serde(default = "def_nw_node_size")]
    pub nw_node_size: f32,
    #[serde(default = "def_nw_node_glow")]
    pub nw_node_glow: f32,
    #[serde(default = "def_nw_edge_thickness")]
    pub nw_edge_thickness: f32,
    #[serde(default = "def_nw_edge_bow")]
    pub nw_edge_bow: f32,
    #[serde(default = "def_nw_edge_samples")]
    pub nw_edge_samples: i32,
    #[serde(default = "def_nw_pulse_speed")]
    pub nw_pulse_speed: f32,
    #[serde(default = "def_nw_pulse_width")]
    pub nw_pulse_width: f32,
    // #226 Tier 1.5 — axon-bundle edges + dendritic somas.
    #[serde(default = "def_nw_edge_fibres")]
    pub nw_edge_fibres: i32,
    #[serde(default = "def_nw_bundle_radius")]
    pub nw_bundle_radius: f32,
    #[serde(default = "def_nw_edge_node_dip")]
    pub nw_edge_node_dip: f32,
    #[serde(default = "def_nw_ranvier")]
    pub nw_ranvier: i32,
    #[serde(default)] // dendrite length (0 = plain soma)
    pub nw_dendrite: f32,
    #[serde(default = "def_nw_dendrite_count")]
    pub nw_dendrite_count: i32,
    // --- Neural Network signal propagation (#226 Tier 2) ---
    #[serde(default)]
    pub nw_fire_mode: u32,
    #[serde(default = "def_nw_threshold")]
    pub nw_threshold: f32,
    #[serde(default = "def_nw_conduction")]
    pub nw_conduction: f32,
    #[serde(default = "def_nw_refractory")]
    pub nw_refractory: f32,
    #[serde(default = "def_nw_decay")]
    pub nw_decay: f32,
    #[serde(default = "def_nw_deposit")]
    pub nw_deposit: f32,
    #[serde(default = "def_nw_stim_rate")]
    pub nw_stim_rate: f32,
    #[serde(default)]
    pub nw_motes: f32,
    // --- Neural Network MLP (#226 Tier 4) ---
    #[serde(default = "def_nw_sign_colour")]
    pub nw_sign_colour: f32,
    #[serde(default = "def_nw_sparsify")]
    pub nw_sparsify: f32,
    #[serde(default = "def_nw_layer_gap")]
    pub nw_layer_gap: f32,
    #[serde(default)]
    pub nw_mlp_drive: f32,
    // --- Neural Network Attention (#226 Tier 5) ---
    #[serde(default)]
    pub nw_attn_layer: f32,
    #[serde(default)]
    pub nw_attn_head: f32,
    #[serde(default = "def_nw_attn_threshold")]
    pub nw_attn_threshold: f32,
    #[serde(default = "def_nw_attn_tokens")]
    pub nw_attn_tokens: f32,
    #[serde(default = "def_nw_attn_reveal")]
    pub nw_attn_reveal: f32,
    #[serde(default)]
    pub nw_attn_sweep: f32,
    #[serde(default)]
    pub nw_attn_ring: f32,
    // --- Neural Tissue surface (#260 Tier 1) ---
    #[serde(default = "def_nt_soma_size")]
    pub nt_soma_size: f32,
    #[serde(default)]
    pub nt_soma_shape: f32,
    #[serde(default = "def_nt_bouton_size")]
    pub nt_bouton_size: f32,
    #[serde(default)]
    pub nt_membrane_sss: f32,
    #[serde(default)]
    pub nt_membrane_irid: f32,
    // --- Neural Tissue morphology (#260 Tier 2) ---
    #[serde(default)]
    pub nt_dendrite_density: f32,
    #[serde(default = "def_nt_dendrite_length")]
    pub nt_dendrite_length: f32,
    #[serde(default = "def_nt_dendrite_taper")]
    pub nt_dendrite_taper: f32,
    #[serde(default)]
    pub nt_neuron_type: u32,
    #[serde(default)]
    pub nt_spines: f32,
    // --- Neural Tissue myelin (#260 Tier 3) ---
    #[serde(default)]
    pub nt_myelin_amount: f32,
    #[serde(default = "def_nt_ranvier_spacing")]
    pub nt_ranvier_spacing: f32,
    #[serde(default = "def_nt_sheath_scale")]
    pub nt_sheath_scale: f32,
    // --- Neural Tissue synapse + tissue context (#260 Tier 4, final) ---
    #[serde(default)]
    pub nt_synapse_cleft: f32,
    #[serde(default)]
    pub nt_synapse_glow: f32,
    #[serde(default)]
    pub nt_synapse_vesicles: f32,
    #[serde(default)]
    pub nt_glia: f32,
    #[serde(default)]
    pub nt_capillary: f32,
    // --- Brain model (#275 Tier 1) ---
    #[serde(default = "def_br_fold_depth")]
    pub br_fold_depth: f32,
    #[serde(default = "def_br_fold_freq")]
    pub br_fold_freq: f32,
    #[serde(default = "def_br_hemi_gap")]
    pub br_hemi_gap: f32,
    #[serde(default = "def_br_local_k")]
    pub br_local_k: i32,
    #[serde(default = "def_br_cerebellum")]
    pub br_cerebellum: f32,
    #[serde(default = "def_br_assoc")]
    pub br_assoc: f32,
    #[serde(default = "def_br_callosum")]
    pub br_callosum: f32,
    #[serde(default = "def_br_subcortical")]
    pub br_subcortical: f32,
    #[serde(default)]
    pub br_region_hi: f32,
    #[serde(default)]
    pub br_target: i32,
    #[serde(default)]
    pub br_stim_amount: f32,
    #[serde(default = "def_br_stim_rate")]
    pub br_stim_rate: f32,
    #[serde(default)]
    pub br_signal_swell: f32,
    // --- Physical thin-film interference (#258 Tier 1) ---
    #[serde(default)]
    pub film_thickness: f32,
    #[serde(default = "def_film_thickness_var")]
    pub film_thickness_var: f32,
    #[serde(default = "def_film_ior")]
    pub film_ior: f32,
    #[serde(default = "def_film_drainage")]
    pub film_drainage: f32,
    // --- Demo scene bench (#288) ---
    #[serde(default)]
    pub demo_scene: u32,
    #[serde(default = "def_demo_size")]
    pub demo_size: f32,
    #[serde(default = "def_true")]
    pub demo_objects: bool,
    #[serde(default = "def_true")]
    pub demo_static_cam: bool,
    #[serde(default = "def_demo_light")]
    pub demo_light: f32,
    #[serde(default = "def_demo_roughness")]
    pub demo_roughness: f32,
    #[serde(default = "def_demo_count")]
    pub demo_count: i32,
    #[serde(default)]
    pub demo_spin: f32,
    // --- Synchrotron radiation generator (#150, Phase 1) ---
    #[serde(default = "def_sy_radius")]
    pub sy_radius: f32,
    #[serde(default = "def_sy_beta")]
    pub sy_beta: f32,
    #[serde(default = "def_sy_charges")]
    pub sy_charges: i32,
    #[serde(default = "def_sy_grid")]
    pub sy_grid: i32,
    #[serde(default = "def_sy_extent")]
    pub sy_extent: f32,
    #[serde(default = "def_sy_near")]
    pub sy_near: f32,
    #[serde(default = "def_sy_amp")]
    pub sy_amp: f32,
    #[serde(default = "def_sy_thickness")]
    pub sy_thickness: f32,
    #[serde(default = "def_sy_rmin")]
    pub sy_rmin: f32,
    #[serde(default)] // false = orbit plane (XY)
    pub sy_perp: bool,
    #[serde(default)] // 0 = Field arrows (Phase 3)
    pub sy_view: u32,
    #[serde(default = "def_sy_line_seeds")]
    pub sy_line_seeds: i32,
    #[serde(default = "def_sy_line_steps")]
    pub sy_line_steps: i32,
    #[serde(default = "def_sy_line_ds")]
    pub sy_line_ds: f32,
    #[serde(default = "def_sy_line_bound")]
    pub sy_line_bound: f32,
    #[serde(default = "def_sy_vol_layers")]
    pub sy_vol_layers: i32,
    #[serde(default)] // 0 = keep everything
    pub sy_reveal: f32,
    #[serde(default)] // false = no inversion
    pub sy_invert: bool,
    #[serde(default = "def_sy_invert_radius")]
    pub sy_invert_radius: f32,
    #[serde(default)] // 0 = planar orbit
    pub sy_tilt: f32,
    #[serde(default)] // 0 = no precession
    pub sy_precess: f32,
    // --- Vector-field plotter generator (#173, Tier 1) ---
    #[serde(default)] // 0 = Parabolic swirl (y², −x²)
    pub vf_preset: u32,
    #[serde(default = "def_vf_grid")]
    pub vf_grid_x: i32,
    #[serde(default = "def_vf_grid")]
    pub vf_grid_y: i32,
    #[serde(default = "def_vf_grid")]
    pub vf_grid_z: i32,
    #[serde(default = "def_vf_extent")]
    pub vf_extent: f32,
    #[serde(default = "def_vf_field_scale")]
    pub vf_field_scale: f32,
    #[serde(default = "def_vf_amp")]
    pub vf_amp: f32,
    #[serde(default = "def_vf_thickness")]
    pub vf_thickness: f32,
    #[serde(default)] // 0 = Soft saturate
    pub vf_mag_map: u32,
    #[serde(default)] // 0 = Magnitude
    pub vf_tint_mode: u32,
    #[serde(default = "def_vf_evolve")]
    pub vf_evolve: f32,
    #[serde(default = "def_vf_z_lift")]
    pub vf_z_lift: f32,
    #[serde(default)] // 0 = keep the full lattice
    pub vf_reveal: f32,
    // Tier 2 (#173): field lines.
    #[serde(default)] // 0 = Field arrows (the Tier-1 view)
    pub vf_view: u32,
    #[serde(default)] // 0 = Lattice
    pub vf_seed_mode: u32,
    #[serde(default = "def_vf_line_seeds")]
    pub vf_line_seeds: i32,
    #[serde(default = "def_vf_line_steps")]
    pub vf_line_steps: i32,
    #[serde(default = "def_vf_line_ds")]
    pub vf_line_ds: f32,
    #[serde(default = "def_true")]
    pub vf_bidir: bool,
    #[serde(default)] // 0 = Magnitude
    pub vf_line_color: u32,
    #[serde(default)] // 0 = flow pulse off
    pub vf_flow: f32,
    #[serde(default = "def_vf_flow_speed")]
    pub vf_flow_speed: f32,
    #[serde(default = "def_vf_line_thickness")]
    pub vf_line_thickness: f32,
    // --- Vector-field function builder (#173, Tier 3) ---
    #[serde(default = "def_vb_func_square")]
    pub vb_x1_func: u32,
    #[serde(default = "def_one")]
    pub vb_x1_gain: f32,
    #[serde(default)]
    pub vb_x1_a: f32,
    #[serde(default = "def_one")]
    pub vb_x1_b: f32,
    #[serde(default)]
    pub vb_x1_c: f32,
    #[serde(default)]
    pub vb_x1_phase: f32,
    #[serde(default)] // 0 = Off
    pub vb_x2_func: u32,
    #[serde(default = "def_one")]
    pub vb_x2_gain: f32,
    #[serde(default = "def_one")]
    pub vb_x2_a: f32,
    #[serde(default)]
    pub vb_x2_b: f32,
    #[serde(default)]
    pub vb_x2_c: f32,
    #[serde(default)]
    pub vb_x2_phase: f32,
    #[serde(default)] // 0 = Off
    pub vb_x3_func: u32,
    #[serde(default = "def_one")]
    pub vb_x3_gain: f32,
    #[serde(default = "def_one")]
    pub vb_x3_a: f32,
    #[serde(default)]
    pub vb_x3_b: f32,
    #[serde(default)]
    pub vb_x3_c: f32,
    #[serde(default)]
    pub vb_x3_phase: f32,
    #[serde(default = "def_vb_func_square")]
    pub vb_y1_func: u32,
    #[serde(default = "def_neg_one")]
    pub vb_y1_gain: f32,
    #[serde(default = "def_one")]
    pub vb_y1_a: f32,
    #[serde(default)]
    pub vb_y1_b: f32,
    #[serde(default)]
    pub vb_y1_c: f32,
    #[serde(default)]
    pub vb_y1_phase: f32,
    #[serde(default)] // 0 = Off
    pub vb_y2_func: u32,
    #[serde(default = "def_one")]
    pub vb_y2_gain: f32,
    #[serde(default)]
    pub vb_y2_a: f32,
    #[serde(default = "def_one")]
    pub vb_y2_b: f32,
    #[serde(default)]
    pub vb_y2_c: f32,
    #[serde(default)]
    pub vb_y2_phase: f32,
    #[serde(default)] // 0 = Off
    pub vb_y3_func: u32,
    #[serde(default = "def_one")]
    pub vb_y3_gain: f32,
    #[serde(default)]
    pub vb_y3_a: f32,
    #[serde(default = "def_one")]
    pub vb_y3_b: f32,
    #[serde(default)]
    pub vb_y3_c: f32,
    #[serde(default)]
    pub vb_y3_phase: f32,
    #[serde(default = "def_vb_func_sin")]
    pub vb_z1_func: u32,
    #[serde(default = "def_half")]
    pub vb_z1_gain: f32,
    #[serde(default)]
    pub vb_z1_a: f32,
    #[serde(default)]
    pub vb_z1_b: f32,
    #[serde(default = "def_one")]
    pub vb_z1_c: f32,
    #[serde(default)]
    pub vb_z1_phase: f32,
    #[serde(default)] // 0 = Off
    pub vb_z2_func: u32,
    #[serde(default = "def_one")]
    pub vb_z2_gain: f32,
    #[serde(default)]
    pub vb_z2_a: f32,
    #[serde(default)]
    pub vb_z2_b: f32,
    #[serde(default = "def_one")]
    pub vb_z2_c: f32,
    #[serde(default)]
    pub vb_z2_phase: f32,
    #[serde(default)] // 0 = Off
    pub vb_z3_func: u32,
    #[serde(default = "def_one")]
    pub vb_z3_gain: f32,
    #[serde(default)]
    pub vb_z3_a: f32,
    #[serde(default)]
    pub vb_z3_b: f32,
    #[serde(default = "def_one")]
    pub vb_z3_c: f32,
    #[serde(default)]
    pub vb_z3_phase: f32,    #[serde(default)] // 0 = Direct
    pub vb_op: u32,
    #[serde(default = "def_half")]
    pub vb_mix: f32,
    // --- Z0NE rails generator (#187, Tier 1) ---
    #[serde(default = "def_rl_speed")]
    pub rl_speed: f32,
    #[serde(default = "def_rl_bore")]
    pub rl_bore: f32,
    #[serde(default = "def_rl_cell_len")] // ordinal 3 = 2 bars (8 beats)
    pub rl_cell_len: u32,
    #[serde(default = "def_half")]
    pub rl_variance: f32,
    #[serde(default)]
    pub rl_seed: i32,
    #[serde(default = "def_rl_ring_n")]
    pub rl_ring_n: i32,
    #[serde(default = "def_rl_rows_beat")]
    pub rl_rows_beat: i32,
    #[serde(default = "def_rl_horizon")]
    pub rl_horizon: f32,
    #[serde(default = "def_rl_rib_gain")]
    pub rl_rib_gain: f32,
    #[serde(default = "def_half")]
    pub rl_thickness: f32,
    #[serde(default = "def_rl_lobes")]
    pub rl_lobes: i32,
    #[serde(default = "def_half")]
    pub rl_spike: f32,
    #[serde(default = "def_rl_twist")]
    pub rl_twist: f32,
    #[serde(default = "def_rl_swell")]
    pub rl_swell: f32,
    #[serde(default = "def_rl_fade")]
    pub rl_fade: f32,
    #[serde(default = "def_rl_color_flow")]
    pub rl_color_flow: f32,
    #[serde(default)] // 0 = Throat (the Tier 1 look)
    pub rl_archetype: u32,
    #[serde(default = "def_rl_diverge")]
    pub rl_diverge: f32,
    #[serde(default = "def_rl_shells")]
    pub rl_shells: i32,
    #[serde(default = "def_rl_parastichy")]
    pub rl_parastichy: i32,
    #[serde(default = "def_rl_change_every")] // ordinal 3 = 8 bars (32 beats)
    pub rl_change_every: u32,
    #[serde(default)] // 0 = evolve off
    pub rl_evolve: f32,
    // --- Scenery layer (#187 pivot): concurrent scenery + its own material ---
    #[serde(default)] // 0 = None (scenery off)
    pub sc_mode: u32,
    #[serde(default)] // 0 = Cubes
    pub sc_surface: u32,
    #[serde(default)] // 0 = Standard
    pub sc_mat: u32,
    #[serde(default)]
    pub sc_metallic: f32,
    #[serde(default = "def_sc_roughness")]
    pub sc_roughness: f32,
    #[serde(default = "def_sc_glow")]
    pub sc_glow: f32,
    #[serde(default)] // 0 = no HDR emissive on scenery
    pub sc_emissive: f32,
    #[serde(default = "def_one")]
    pub sc_opacity: f32,
    #[serde(default = "def_sc_ior")]
    pub sc_ior: f32,
    #[serde(default)] // 0 = Native
    pub sc_palette: u32,
    #[serde(default)]
    pub sc_sss: f32,
    #[serde(default = "def_sc_sss_dist")]
    pub sc_sss_dist: f32,
    #[serde(default = "def_sc_sss_pow")]
    pub sc_sss_pow: f32,
    #[serde(default)]
    pub sc_irid: f32,
    #[serde(default = "def_sc_irid_scale")]
    pub sc_irid_scale: f32,
    #[serde(default)]
    pub sc_irid_shift: f32,
    // --- Terra scenery landform (#206 Tier 2) ---
    #[serde(default)] // 0 = Fjord
    pub terra_form: u32,
    #[serde(default = "def_terra_ridge")]
    pub terra_ridge: f32,
    #[serde(default = "def_one")]
    pub terra_channel: f32,
    #[serde(default = "def_terra_width")]
    pub terra_width: f32,
    #[serde(default = "def_terra_steep")]
    pub terra_steep: f32,
    #[serde(default)]
    pub terra_terrace: f32,
    #[serde(default = "def_terra_rough")]
    pub terra_rough: f32,
    #[serde(default = "def_terra_meander")]
    pub terra_meander: f32,
    #[serde(default = "def_terra_water_level")]
    pub terra_water_level: f32,
    #[serde(default = "def_true")]
    pub terra_water_on: bool,
    #[serde(default = "def_terra_clearance")]
    pub terra_clearance: f32,
    #[serde(default = "def_terra_noise_freq")]
    pub terra_noise_freq: f32,
    // --- Scenery water floor (#206 Tier 3; a Look — instant, not latched) ---
    #[serde(default = "def_wt_mat")] // 2 = Glass
    pub wt_mat: u32,
    #[serde(default = "def_wt_roughness")]
    pub wt_roughness: f32,
    #[serde(default = "def_wt_ior")]
    pub wt_ior: f32,
    #[serde(default = "def_wt_opacity")]
    pub wt_opacity: f32,
    #[serde(default)]
    pub wt_glow: f32,
    #[serde(default = "def_wt_ripple")]
    pub wt_ripple: f32,
    #[serde(default = "def_wt_ripple_freq")]
    pub wt_ripple_freq: f32,
    #[serde(default = "def_wt_absorb")]
    pub wt_absorb: f32,
    #[serde(default = "def_wt_glitter")]
    pub wt_glitter: f32,
    #[serde(default = "def_wt_reflect")]
    pub wt_reflect: f32,
    // --- Phyllotaxis / golden-angle generator (defaults = sunflower disk) ---
    #[serde(default)] // 0 = Disk
    pub phyl_surface: u32,
    #[serde(default = "def_phyl_count")]
    pub phyl_count: i32,
    #[serde(default = "def_phyl_divergence")]
    pub phyl_divergence: f32,
    #[serde(default = "def_one")]
    pub phyl_radius: f32,
    #[serde(default = "def_phyl_parastichy")]
    pub phyl_parastichy: i32,
    #[serde(default = "def_phyl_height")]
    pub phyl_height: f32,
    #[serde(default = "def_phyl_growth")]
    pub phyl_growth: f32,
    #[serde(default)]
    pub phyl_breathe_amp: f32,
    #[serde(default = "def_phyl_breathe_freq")]
    pub phyl_breathe_freq: f32,
    #[serde(default = "def_phyl_rot")]
    pub phyl_rot: f32,
    #[serde(default = "def_phyl_thickness")]
    pub phyl_thickness: f32,
    // --- Tessellation generator (#121; defaults = Penrose P3, depth 4) ---
    #[serde(default)] // 0 = Penrose P3
    pub tess_family: u32,
    #[serde(default = "def_tess_depth")]
    pub tess_depth: i32,
    #[serde(default = "def_tess_scale")]
    pub tess_scale: f32,
    #[serde(default = "def_tess_thickness")]
    pub tess_thickness: f32,
    #[serde(default = "def_tess_view")] // 2 = Extruded
    pub tess_view: u32,
    #[serde(default = "def_tess_height")]
    pub tess_height: f32,
    #[serde(default)] // 0 = Uniform
    pub tess_height_mode: u32,
    #[serde(default)]
    pub tess_beat_infl: f32,
    #[serde(default)]
    pub tess_ripple_amt: f32,
    #[serde(default = "def_tess_ripple_freq")]
    pub tess_ripple_freq: f32,
    #[serde(default)] // 0 = Inflation
    pub tess_construct: u32,
    #[serde(default)]
    pub tess_phason: f32,
    #[serde(default = "def_tess_grid_n")]
    pub tess_grid_n: i32,
    #[serde(default)]
    pub tess_ammann: f32,
    #[serde(default = "def_tess_hyp_p")]
    pub tess_hyp_p: i32,
    #[serde(default = "def_tess_hyp_q")]
    pub tess_hyp_q: i32,
    // --- Mandelbulb generator (defaults = the classic power-8 set) ---
    #[serde(default = "def_mb_power")]
    pub mb_power: f32,
    #[serde(default = "def_mb_iter")]
    pub mb_iter: i32,
    #[serde(default = "def_mb_scale")]
    pub mb_scale: f32,
    #[serde(default = "def_mb_detail")]
    pub mb_detail: i32,
    #[serde(default = "def_mb_spin")]
    pub mb_spin: f32,
    #[serde(default)]
    pub mb_morph: f32,
    #[serde(default = "def_mb_color")]
    pub mb_color: f32,
    #[serde(default = "def_mb_bailout")]
    pub mb_bailout: f32,
    // --- Creature Engine (#476 Tier 1; defaults = the bell jelly) ---
    #[serde(default)]
    pub cr_form: i32,
    #[serde(default = "def_cr_scale")]
    pub cr_scale: f32,
    #[serde(default = "def_cr_detail")]
    pub cr_detail: i32,
    #[serde(default = "def_cr_swim")]
    pub cr_swim: f32,
    #[serde(default = "def_cr_warp_amp")]
    pub cr_warp_amp: f32,
    #[serde(default = "def_cr_warp_freq")]
    pub cr_warp_freq: f32,
    #[serde(default = "def_cr_rim")]
    pub cr_rim: f32,
    #[serde(default = "def_cr_glow")]
    pub cr_glow: f32,
    // --- Creature Engine Tier 2a (#476): the metachronal wave ---
    #[serde(default = "def_cr_wave_speed")]
    pub cr_wave_speed: f32,
    #[serde(default = "def_cr_wave_freq")]
    pub cr_wave_freq: f32,
    #[serde(default = "def_cr_wave_sharp")]
    pub cr_wave_sharp: f32,
    #[serde(default)]
    pub cr_wave_amt: f32,
    // --- Creature Engine Tier 2c (#476): the anatomy overlay ---
    #[serde(default)]
    pub cr_overlay: bool,
    #[serde(default = "def_cr_overlay_opacity")]
    pub cr_overlay_opacity: f32,
    #[serde(default = "def_cr_overlay_bright")]
    pub cr_overlay_bright: f32,
    // --- Minimal surfaces / TPMS generator (#127) ---
    #[serde(default)]
    pub ms_family: u32,
    #[serde(default = "def_ms_scale")]
    pub ms_scale: f32,
    #[serde(default = "def_ms_cells")]
    pub ms_cells: f32,
    #[serde(default)]
    pub ms_iso: f32,
    #[serde(default = "def_ms_thickness")]
    pub ms_thickness: f32,
    #[serde(default)]
    pub ms_twist: f32,
    #[serde(default = "def_ms_detail")]
    pub ms_detail: i32,
    #[serde(default = "def_ms_color")]
    pub ms_color: f32,
    #[serde(default)]
    pub ms_beat_iso: f32,
    #[serde(default)]
    pub ms_bend: f32,
    #[serde(default = "def_ms_uv_res")]
    pub ms_uv_res: i32,
    #[serde(default = "def_ms_extent")]
    pub ms_extent: f32,
    #[serde(default)]
    pub ms_bend_phase: f32,
    #[serde(default = "def_ms_turns")]
    pub ms_turns: f32,
    #[serde(default = "def_ms_form_res")]
    pub ms_form_res: f32,
    // --- Lens generator (#258 Tier 3) ---
    #[serde(default = "def_lens_focal")]
    pub lens_focal: f32,
    #[serde(default = "def_lens_aperture")]
    pub lens_aperture: f32,
    #[serde(default = "def_lens_thickness")]
    pub lens_thickness: f32,
    #[serde(default)]
    pub lens_plano: bool,
    #[serde(default = "def_lens_scale")]
    pub lens_scale: f32,
    #[serde(default = "def_lens_detail")]
    pub lens_detail: i32,
    // --- Kaleidoscopic Fractal (KIFS) generator ---
    #[serde(default = "def_kf_sectors")]
    pub kf_sectors: f32,
    #[serde(default = "def_kf_fold")]
    pub kf_fold: f32,
    #[serde(default = "def_kf_iter")]
    pub kf_iter: i32,
    #[serde(default)]
    pub kf_iter_rot: f32,
    #[serde(default = "def_kf_spin")]
    pub kf_spin: f32,
    #[serde(default = "def_kf_breathe")]
    pub kf_breathe: f32,
    #[serde(default = "def_kf_zoom")]
    pub kf_zoom: f32,
    #[serde(default)]
    pub kf_tunnel: bool,
    #[serde(default = "def_kf_rays")]
    pub kf_rays: i32,
    #[serde(default = "def_kf_ring")]
    pub kf_ring: f32,
    #[serde(default = "def_kf_glow")]
    pub kf_glow: f32,
    #[serde(default)]
    pub kf_hue: f32,
    #[serde(default)]
    pub kf_pattern: u32,
    #[serde(default)]
    pub kf_palette: u32,
    #[serde(default = "def_kf_color_speed")]
    pub kf_color_speed: f32,
    #[serde(default = "def_one")]
    pub kf_churn: f32,
    #[serde(default)]
    pub kf_e8_flow: f32,
    #[serde(default)]
    pub kf_warp: f32,
    #[serde(default = "def_kf_flow")]
    pub kf_flow: f32,
    #[serde(default = "def_kf_petals")]
    pub kf_petals: i32,
    #[serde(default = "def_one")]
    pub kf_contrast: f32,
    #[serde(default = "def_kf_sharp")]
    pub kf_sharp: f32,
    #[serde(default)]
    pub kf_invert: f32,
    #[serde(default)]
    pub kf_dispersion: f32,
    #[serde(default)]
    pub kf_space: u32,
    #[serde(default)] // 0 = Field (2-D); old presets → Field, unchanged
    pub kf_view: u32,
    #[serde(default = "def_kf_relief")] // relief height (Relief mode)
    pub kf_relief: f32,
    #[serde(default = "def_kf_relief_elev")]
    pub kf_relief_elev: f32,
    #[serde(default = "def_kf_relief_steps")]
    pub kf_relief_steps: i32,
    #[serde(default = "def_kf_relief_shine")]
    pub kf_relief_shine: f32,
    // Scene Kaleidoscope (#361 Tier 1). All serde-default → off (old presets keep
    // their look; kal_on = false so the fold is inert regardless of the rest).
    #[serde(default)]
    pub kal_on: bool,
    #[serde(default = "def_kal_sectors")]
    pub kal_sectors: f32,
    #[serde(default)] // 0 = FullFrame
    pub kal_mode: u32,
    #[serde(default = "def_kal_spin")]
    pub kal_spin: f32,
    #[serde(default)]
    pub kal_roll: f32,
    #[serde(default = "def_kal_unit")]
    pub kal_zoom: f32,
    #[serde(default)]
    pub kal_center_x: f32,
    #[serde(default)]
    pub kal_center_y: f32,
    #[serde(default = "def_kal_unit")]
    pub kal_mix: f32,
    #[serde(default)]
    pub kal_twist: f32,
    #[serde(default)]
    pub kal_tint_hue: f32,
    #[serde(default)]
    pub kal_tint_amt: f32,
    #[serde(default = "def_kal_seam")]
    pub kal_seam: f32,
    // Quantitative instrumentation (#391 Tier 1). All serde-default → the HUD is off
    // (old presets keep their look; instr_hud = false so nothing is drawn).
    #[serde(default)]
    pub instr_hud: bool,
    #[serde(default = "def_true")]
    pub instr_probe_on: bool,
    #[serde(default = "def_instr_probe_x")]
    pub instr_probe_x: f32,
    #[serde(default)]
    pub instr_probe_y: f32,
    #[serde(default)]
    pub instr_probe_z: f32,
    #[serde(default = "def_true")]
    pub instr_ledger_on: bool,
    #[serde(default = "def_instr_ledger_half")]
    pub instr_ledger_half: f32,
    #[serde(default = "def_instr_ledger_res")]
    pub instr_ledger_res: f32,
    #[serde(default = "def_true")]
    pub instr_flux_on: bool,
    #[serde(default = "def_instr_probe_x")]
    pub instr_flux_x: f32,
    #[serde(default)]
    pub instr_flux_y: f32,
    #[serde(default)]
    pub instr_flux_z: f32,
    #[serde(default = "def_instr_flux_size")]
    pub instr_flux_size: f32,
    #[serde(default)] // 0 = X
    pub instr_flux_axis: u32,
    #[serde(default = "def_instr_flux_res")]
    pub instr_flux_res: f32,
    #[serde(default)]
    pub instr_csv_log: bool,
    #[serde(default = "def_instr_panel_opacity")]
    pub instr_panel_opacity: f32,
    #[serde(default = "def_instr_panel_bevel")]
    pub instr_panel_bevel: f32,
    #[serde(default = "def_instr_hud_scale")]
    pub instr_hud_scale: f32,
    #[serde(default)] // 0 = TopLeft
    pub instr_hud_dock: u32,
    // FDTD Maxwell solver (#412 Tier 3 Phase 0). All serde-default → off (old presets
    // keep the analytic Maxwell path; fdtd_on = false).
    #[serde(default)]
    pub fdtd_on: bool,
    #[serde(default = "def_fdtd_res")]
    pub fdtd_res: f32,
    #[serde(default)] // 0 = Pulse
    pub fdtd_source: u32,
    #[serde(default = "def_fdtd_freq")]
    pub fdtd_freq: f32,
    #[serde(default = "def_fdtd_drive")]
    pub fdtd_drive: f32,
    #[serde(default = "def_fdtd_substeps")]
    pub fdtd_substeps: f32,
    #[serde(default = "def_fdtd_boundary")]
    pub fdtd_boundary: f32,
    #[serde(default = "def_fdtd_extent")]
    pub fdtd_extent: f32,
    #[serde(default)] // 0 = Original
    pub surface_mode: u32,
    #[serde(default)] // 0 = Corner (grid corner at the origin — the historical look)
    pub origin_mode: u32,
    #[serde(default)] // 0 = sharp cube (old presets predate the bevel)
    pub bevel: f32,
    // #472 Tier 1 procedural/texture materials (old presets predate them → off +
    // neutral defaults, so they load byte-identical to today).
    #[serde(default)] // false = scalar-uniform PBR (today's look)
    pub mat_enable: bool,
    #[serde(default)] // 0 = Triplanar
    pub mat_projection: u32,
    #[serde(default = "def_one")]
    pub mat_scale: f32,
    // #472 Tier 2 procedural single-layer noise graph (old presets predate it → off
    // + FBM/albedo defaults, so they load byte-identical).
    #[serde(default)] // false = not procedural (Tier-1/scalar path)
    pub mp_enable: bool,
    #[serde(default = "def_three")] // 3 = FBM
    pub mp_noise: u32,
    #[serde(default)] // 0 = Albedo
    pub mp_channel: u32,
    #[serde(default = "def_four")]
    pub mp_scale: f32,
    #[serde(default)]
    pub mp_rotation: f32,
    #[serde(default)]
    pub mp_offset_x: f32,
    #[serde(default)]
    pub mp_offset_y: f32,
    #[serde(default = "def_five_i")]
    pub mp_octaves: i32,
    #[serde(default = "def_two")]
    pub mp_lacunarity: f32,
    #[serde(default = "def_half")]
    pub mp_gain: f32,
    #[serde(default)]
    pub mp_warp: f32,
    #[serde(default = "def_one")]
    pub mp_contrast: f32,
    #[serde(default = "def_one")]
    pub mp_gamma: f32,
    #[serde(default)]
    pub mp_remap_lo: f32,
    #[serde(default = "def_one")]
    pub mp_remap_hi: f32,
    #[serde(default)]
    pub mp_invert: bool,
    #[serde(default)]
    pub mp_seed: i32,
    #[serde(default = "def_one_u32")] // 1 = 512²
    pub mp_res: u32,
    #[serde(default = "def_grad_lo_r")]
    pub mp_lo_r: f32,
    #[serde(default = "def_grad_lo_r")]
    pub mp_lo_g: f32,
    #[serde(default = "def_grad_lo_b")]
    pub mp_lo_b: f32,
    #[serde(default = "def_grad_hi_r")]
    pub mp_hi_r: f32,
    #[serde(default = "def_grad_hi_g")]
    pub mp_hi_g: f32,
    #[serde(default = "def_grad_hi_b")]
    pub mp_hi_b: f32,
    // #472 Tier 3 overlay layer 2 (old presets predate it → disabled + neutral).
    #[serde(default)] pub mp2_enable: bool,            // false
    #[serde(default)] pub mp2_blend: u32,              // 0 (Normal)
    #[serde(default = "def_three")] pub mp2_noise: u32,        // 3 (FBM)
    #[serde(default = "def_one_u32")] pub mp2_channel: u32,    // 1 (Roughness)
    #[serde(default = "def_six")] pub mp2_scale: f32,          // 6.0
    #[serde(default)] pub mp2_rotation: f32,           // 0
    #[serde(default)] pub mp2_offset_x: f32,           // 0
    #[serde(default)] pub mp2_offset_y: f32,           // 0
    #[serde(default = "def_four_i32")] pub mp2_octaves: i32,   // 4
    #[serde(default = "def_two")] pub mp2_lacunarity: f32,     // 2.0
    #[serde(default = "def_half")] pub mp2_gain: f32,          // 0.5
    #[serde(default)] pub mp2_warp: f32,               // 0
    #[serde(default = "def_one")] pub mp2_contrast: f32,       // 1.0
    #[serde(default = "def_one")] pub mp2_gamma: f32,          // 1.0
    #[serde(default)] pub mp2_remap_lo: f32,           // 0
    #[serde(default = "def_one")] pub mp2_remap_hi: f32,       // 1.0
    #[serde(default)] pub mp2_invert: bool,            // false
    #[serde(default = "def_one_i32")] pub mp2_seed: i32,       // 1
    #[serde(default = "def_grad_lo_r")] pub mp2_lo_r: f32,     // 0.04
    #[serde(default = "def_grad_lo_r")] pub mp2_lo_g: f32,     // 0.04
    #[serde(default = "def_grad_lo_b")] pub mp2_lo_b: f32,     // 0.05
    #[serde(default = "def_grad_hi_r")] pub mp2_hi_r: f32,     // 0.80
    #[serde(default = "def_grad_hi_g")] pub mp2_hi_g: f32,     // 0.76
    #[serde(default = "def_grad_hi_b")] pub mp2_hi_b: f32,     // 0.70
    // #472 Tier 3 derived maps (old presets predate it → off).
    #[serde(default)] pub mat_derive_normal: bool,     // false
    #[serde(default)] pub mat_derive_ao: bool,         // false
    #[serde(default)] pub mat_normal_source_albedo: bool, // false
    #[serde(default = "def_one")] pub mat_derive_normal_strength: f32, // 1.0
    #[serde(default = "def_one")] pub mat_derive_ao_strength: f32,     // 1.0
    #[serde(default = "def_two")] pub mat_derive_ao_radius: f32,       // 2.0
    // #472 Tier 5 live: animation + displacement (old presets predate it → off).
    #[serde(default)] pub mat_anim_enable: bool,
    #[serde(default = "def_anim_speed")] pub mat_anim_speed: f32,
    #[serde(default)] pub mat_anim_mode: u32,
    #[serde(default = "def_one")] pub mat_flow_x: f32,
    #[serde(default)] pub mat_flow_y: f32,
    #[serde(default)] pub mat_displace: f32,
    // Plexus surface-mode controls (old presets predate it → neutral defaults).
    #[serde(default = "def_plexus_radius")]
    pub plexus_radius: f32,
    #[serde(default = "def_plexus_links")]
    pub plexus_links: f32,
    #[serde(default = "def_plexus_strut")]
    pub plexus_strut: f32,
    #[serde(default = "def_plexus_marker")]
    pub plexus_marker: f32,
    // Plexus Tier 2 impostors + independent materials (old presets → neutral defaults).
    #[serde(default)] // false = Tier 1 cubes
    pub plexus_impostor: bool,
    #[serde(default = "def_true")]
    pub plexus_edges: bool,
    #[serde(default = "def_plexus_node_radius")]
    pub plexus_node_radius: f32,
    #[serde(default = "def_plexus_edge_radius")]
    pub plexus_edge_radius: f32,
    #[serde(default)] // 0 = Standard
    pub plexus_node_type: u32,
    #[serde(default = "def_plexus_node_metallic")]
    pub plexus_node_metallic: f32,
    #[serde(default = "def_plexus_node_rough")]
    pub plexus_node_rough: f32,
    #[serde(default = "def_ior")]
    pub plexus_node_ior: f32,
    #[serde(default)]
    pub plexus_node_hue: f32,
    #[serde(default)]
    pub plexus_node_sat: f32,
    #[serde(default = "def_one")]
    pub plexus_node_val: f32,
    #[serde(default = "def_plexus_node_emissive")]
    pub plexus_node_emissive: f32,
    #[serde(default)] // 0 = Standard
    pub plexus_edge_type: u32,
    #[serde(default)]
    pub plexus_edge_metallic: f32,
    #[serde(default = "def_plexus_edge_rough")]
    pub plexus_edge_rough: f32,
    #[serde(default = "def_ior")]
    pub plexus_edge_ior: f32,
    #[serde(default = "def_plexus_edge_hue")]
    pub plexus_edge_hue: f32,
    #[serde(default = "def_plexus_edge_sat")]
    pub plexus_edge_sat: f32,
    #[serde(default = "def_one")]
    pub plexus_edge_val: f32,
    #[serde(default = "def_plexus_edge_emissive")]
    pub plexus_edge_emissive: f32,
    // Plexus Tier 3 signal propagation.
    #[serde(default)] // false = off
    pub plexus_signal: bool,
    #[serde(default = "def_one")]
    pub plexus_signal_speed: f32,
    #[serde(default = "def_plexus_signal_gain")]
    pub plexus_signal_gain: f32,
    #[serde(default = "def_plexus_signal_width")]
    pub plexus_signal_width: f32,
    // Plexus Tier-1 shape morph (old presets → 0 = current sharp look).
    #[serde(default)]
    pub plexus_node_shape: f32,
    #[serde(default)]
    pub plexus_edge_shape: f32,
    // Plexus overlay (old presets predate it → off + neutral shell defaults).
    #[serde(default)]
    pub plexus_overlay_on: bool,
    #[serde(default = "def_plexus_shell_scale")]
    pub plexus_shell_scale: f32,
    #[serde(default = "def_plexus_shell_depth")]
    pub plexus_shell_depth: f32,
    #[serde(default = "def_plexus_shell_bins")]
    pub plexus_shell_bins: f32,
    #[serde(default)] pub animate: bool,
    #[serde(default)] pub inc_scale: f32,
    // Old presets predate the split speed dial: they stored an already-small
    // inc_scale, so defaulting the exponent to 0 (×1) reproduces their speed.
    #[serde(default)]
    pub speed_exp: i32,
    #[serde(default)] pub ambient: f32,
    #[serde(default)] pub key_intensity: f32,
    #[serde(default)] pub fill_intensity: f32,
    #[serde(default)] pub elevation: f32,
    #[serde(default)] pub azimuth: f32,
    #[serde(default)] pub glow: f32,
    #[serde(default)] // 0 = no HDR emissive (glow-only, as before)
    pub mat_emissive: f32,
    #[serde(default)] pub opacity: f32,
    #[serde(default)] pub pulse: bool,
    #[serde(default = "def_true")]
    pub tempo_sync: bool,
    #[serde(default)] pub tempo: f32,
    // --- Pulse source + audio-reactive (default to the inert "off" state) ---
    #[serde(default)] // 0 = Beat clock
    pub pulse_source: u32,
    #[serde(default)]
    pub audio_react: bool,
    #[serde(default = "def_one")]
    pub audio_gain: f32,
    #[serde(default = "def_audio_attack")]
    pub audio_attack: f32,
    #[serde(default = "def_audio_release")]
    pub audio_release: f32,
    #[serde(default)] // 0 = Standard
    pub mat_type: u32,
    #[serde(default = "def_ior")]
    pub ior: f32,
    #[serde(default = "def_one")] // Refractive-material Beer–Lambert strength
    pub mat_absorb: f32,
    #[serde(default)] // refraction overlay on Standard/Chrome/Glass (off)
    pub refr_overlay: bool,
    #[serde(default = "def_one")] // overlay strength (inert while overlay off)
    pub refr_blend: f32,
    #[serde(default)] // screen-space refraction strength (0 = off)
    pub refract_ss: f32,
    #[serde(default = "def_refract_dist")]
    pub refract_dist: f32,
    #[serde(default)] // anisotropy amount (0 = isotropic)
    pub anisotropy: f32,
    #[serde(default)] // brush rotation (degrees)
    pub aniso_rotation: f32,
    #[serde(default)] // anisotropy overlay on Standard/Chrome (off)
    pub aniso_overlay: bool,
    #[serde(default = "def_one")] // overlay strength (inert while overlay off)
    pub aniso_blend: f32,
    #[serde(default = "def_one")] // clearcoat strength (inert while type/overlay off)
    pub clearcoat: f32,
    #[serde(default = "def_clearcoat_rough")]
    pub clearcoat_rough: f32,
    #[serde(default)] // clearcoat overlay on Standard/Chrome (off)
    pub clearcoat_overlay: bool,
    #[serde(default = "def_one")] // sheen amount (inert while type/overlay off)
    pub sheen: f32,
    #[serde(default = "def_sheen_rough")]
    pub sheen_rough: f32,
    #[serde(default)] // sheen tint (0 = white fuzz)
    pub sheen_tint: f32,
    #[serde(default)] // sheen overlay on Standard/Chrome (off)
    pub sheen_overlay: bool,
    #[serde(default)] // SSS thickness drive (0 = today's translucency)
    pub sss_thickness: f32,
    #[serde(default = "def_one")] // SSS penetration radius
    pub sss_radius: f32,
    #[serde(default)] // interior in-scatter glow (0 = off)
    pub interior_scatter: f32,
    #[serde(default)] // glitter amount (0 = off)
    pub glitter: f32,
    #[serde(default = "def_glitter_density")]
    pub glitter_density: f32,
    #[serde(default = "def_glitter_sharpness")]
    pub glitter_sharpness: f32,
    #[serde(default)] // diffraction amount (0 = off)
    pub diffraction: f32,
    #[serde(default = "def_diffraction_freq")]
    pub diffraction_freq: f32,
    #[serde(default)] // retroreflection amount (0 = off)
    pub retro: f32,
    #[serde(default)] // fluorescence amount (0 = off)
    pub fluorescence: f32,
    #[serde(default = "def_fluor_hue")]
    pub fluor_hue: f32,
    #[serde(default)] // incandescence amount (0 = off)
    pub incandescence: f32,
    #[serde(default = "def_temperature")]
    pub temperature: f32,
    #[serde(default = "def_metallic")]
    pub metallic: f32,
    #[serde(default = "def_roughness")]
    pub roughness: f32,
    #[serde(default)] // EV stops; 0 = neutral (1x)
    pub exposure: f32,
    #[serde(default = "def_one")]
    pub env_intensity: f32,
    #[serde(default)]
    pub env_rotation: f32,
    #[serde(default = "def_bloom")]
    pub bloom_intensity: f32,
    #[serde(default = "def_one")]
    pub bloom_threshold: f32,
    #[serde(default)] // 0 = Off
    pub cam_path: u32,
    #[serde(default = "def_cam_speed")]
    pub cam_speed: f32,
    #[serde(default = "def_cam_kick")]
    pub cam_kick: f32,
    #[serde(default = "def_cam_damping")]
    pub cam_damping: f32,
    #[serde(default = "def_one")]
    pub cam_amount: f32,
    // --- Cinematic camera (#307 Tier 1) ---
    #[serde(default = "def_true")]
    pub cam_beat_momentum: bool,
    #[serde(default)] // false
    pub cam_seq_enabled: bool,
    #[serde(default = "def_bar8")] // 3 = B8
    pub cam_bars_per_shot: u32,
    #[serde(default)] // 0 = Series
    pub cam_seq_order: u32,
    #[serde(default)] // 0 = Glide
    pub cam_transition: u32,
    #[serde(default = "def_one")]
    pub cam_transition_bars: f32,
    #[serde(default = "def_dolly_period")]
    pub cam_dolly_period: f32,
    #[serde(default)] // 0 = inert
    pub cam_dolly_depth: f32,
    #[serde(default)] // 0 = Sine
    pub cam_dolly_wave: u32,
    #[serde(default)] // 0 = Host
    pub tempo_source: u32,
    #[serde(default = "def_beats_per_bar")]
    pub beats_per_bar: i32,
    // --- Cinematic camera (#307 Tier 2) ---
    #[serde(default)] // 0 = no roll
    pub cam_roll: f32,
    #[serde(default = "def_fov")]
    pub cam_fov: f32,
    #[serde(default)] // 0 = no dolly-zoom
    pub cam_fov_dolly: f32,
    #[serde(default)] // 0 = never hold
    pub cam_hold_prob: f32,
    #[serde(default)] // false
    pub cam_phrase_lock: bool,
    #[serde(default = "def_one")] // 1 = fully sequencer
    pub cam_seq_mix: f32,
    // --- Cinematic camera (#307 Tier 3): storyboard ---
    #[serde(default)] // false
    pub cam_story_enabled: bool,
    #[serde(default = "def_story_count")]
    pub cam_story_count: i32,
    #[serde(default)] // 0 = Series
    pub cam_story_mode: u32,
    #[serde(default = "def_story_seed")]
    pub cam_story_seed: i32,
    #[serde(default = "def_shot0_path")] // HCircle
    pub cam_shot0_path: u32,
    #[serde(default = "def_bar8")]
    pub cam_shot0_bars: u32,
    #[serde(default = "def_one")]
    pub cam_shot0_radius: f32,
    #[serde(default = "def_shot1_path")] // Spiral
    pub cam_shot1_path: u32,
    #[serde(default = "def_bar8")]
    pub cam_shot1_bars: u32,
    #[serde(default = "def_shot1_radius")]
    pub cam_shot1_radius: f32,
    #[serde(default = "def_shot2_path")] // Figure-8
    pub cam_shot2_path: u32,
    #[serde(default = "def_bar4")]
    pub cam_shot2_bars: u32,
    #[serde(default = "def_shot2_radius")]
    pub cam_shot2_radius: f32,
    #[serde(default = "def_shot3_path")] // Boom
    pub cam_shot3_path: u32,
    #[serde(default = "def_bar8")]
    pub cam_shot3_bars: u32,
    #[serde(default = "def_shot3_radius")]
    pub cam_shot3_radius: f32,
    #[serde(default)] // 0 = None
    pub mod_a_target: u32,
    #[serde(default)]
    pub mod_a_depth: f32,
    #[serde(default)] // 0 = None
    pub mod_b_target: u32,
    #[serde(default)]
    pub mod_b_depth: f32,
    // --- Speed Pulse (default to the inert state: amount 0) ---
    #[serde(default)]
    pub speed_pulse_amount: f32,
    #[serde(default = "def_speed_pulse_attack")]
    pub speed_pulse_attack: f32,
    #[serde(default = "def_speed_pulse_decay")]
    pub speed_pulse_decay: f32,
    // --- Breath (universal scene-scale pulse) — default to inert (amount 0). ---
    #[serde(default)]
    pub breath_amount: f32,
    #[serde(default = "def_breath_attack")]
    pub breath_attack: f32,
    #[serde(default = "def_breath_decay")]
    pub breath_decay: f32,
    // --- Surface FX (additive modifiers) — all default to the inert state. ---
    #[serde(default)]
    pub subsurface: f32,
    #[serde(default = "def_sss_distortion")]
    pub sss_distortion: f32,
    #[serde(default = "def_sss_power")]
    pub sss_power: f32,
    #[serde(default)]
    pub iridescence: f32,
    #[serde(default = "def_irid_scale")]
    pub irid_scale: f32,
    #[serde(default)]
    pub irid_shift: f32,
    #[serde(default)] // 0 = Native
    pub palette: u32,
    // --- Ambient occlusion (off by default) ---
    #[serde(default)]
    pub ssao: bool,
    #[serde(default = "def_ssao_radius")]
    pub ssao_radius: f32,
    #[serde(default = "def_one")]
    pub ssao_intensity: f32,
    #[serde(default = "def_ssao_bias")]
    pub ssao_bias: f32,
    // --- Renderer look (MSAA is a quality setting, not captured) ---
    #[serde(default)] // 0 = ACES
    pub tonemap: u32,
    #[serde(default = "def_bg_tonemap")] // 1 = AgX (gentler on HDR backdrops)
    pub bg_tonemap: u32,
    #[serde(default = "def_true")]
    pub bg_visible: bool,
    #[serde(default = "def_one")]
    pub bg_intensity: f32,
    #[serde(default = "def_tint_hue")]
    pub env_tint_hue: f32,
    #[serde(default)] // 0 = no tint
    pub env_tint_amt: f32,
    // --- Per-material HSV (#305 Tier 1); identity defaults so old presets load
    // unchanged (hue/cycle 0, sat/value 1). ---
    #[serde(default)]
    pub mat_hue: f32,
    #[serde(default)]
    pub mat_hue_cycle: f32,
    #[serde(default = "def_one")]
    pub mat_saturation: f32,
    #[serde(default = "def_one")]
    pub mat_value: f32,
    #[serde(default)]
    pub scen_hue: f32,
    #[serde(default)]
    pub scen_hue_cycle: f32,
    #[serde(default = "def_one")]
    pub scen_saturation: f32,
    #[serde(default = "def_one")]
    pub scen_value: f32,
    // --- Live-sky cloud reflections (#305 Tier 2); off by default. ---
    #[serde(default)]
    pub sky_reflect_clouds: bool,
    #[serde(default = "def_cloud_cover")]
    pub sky_cloud_cover: f32,
    #[serde(default = "def_cloud_speed")]
    pub sky_cloud_speed: f32,
    #[serde(default = "def_cloud_strength")]
    pub sky_cloud_strength: f32,
    // --- Neural radiance cache — live (#256 Tier 0); off by default. ---
    #[serde(default)]
    pub nrc_enable: bool,
    #[serde(default = "def_nrc_confidence")]
    pub nrc_confidence: f32,
    #[serde(default = "def_nrc_learn_rate")]
    pub nrc_learn_rate: f32,
    #[serde(default = "def_nrc_omega")]
    pub nrc_omega: f32,
    #[serde(default = "def_nrc_terminate")]
    pub nrc_terminate: i32,
    #[serde(default = "def_nrc_samples")]
    pub nrc_train_samples: i32,
    #[serde(default = "def_nrc_seed")]
    pub nrc_seed: i32,
    // --- Neural radiance cache — RT-stack synergies (#256 Tier 1); off by default. ---
    #[serde(default)]
    pub nrc_guide: bool,
    #[serde(default = "def_nrc_guide_candidates")]
    pub nrc_guide_candidates: i32,
    #[serde(default)]
    pub nrc_firefly: bool,
    #[serde(default = "def_nrc_firefly_clamp")]
    pub nrc_firefly_clamp: f32,
    // --- Neural radiance cache — light-field uses (#256 Tier 2); off by default. ---
    #[serde(default)]
    pub nrc_gi: bool,
    #[serde(default = "def_nrc_gi_strength")]
    pub nrc_gi_strength: f32,
    #[serde(default)]
    pub nrc_reflect: bool,
    // --- Neural radiance cache — hard transport + volumetrics (#256 Tier 3); off by default. ---
    #[serde(default)]
    pub nrc_volume: bool,
    #[serde(default = "def_nrc_volume_density")]
    pub nrc_volume_density: f32,
    #[serde(default = "def_nrc_volume_steps")]
    pub nrc_volume_steps: i32,
    #[serde(default = "def_nrc_volume_strength")]
    pub nrc_volume_strength: f32,
    #[serde(default)]
    pub nrc_caustic: bool,
    #[serde(default = "def_nrc_caustic_gain")]
    pub nrc_caustic_gain: f32,
    #[serde(default)]
    pub particles_bead_hue: f32,
    #[serde(default)]
    pub particles_bead_hue_cycle: f32,
    #[serde(default = "def_one")]
    pub particles_bead_sat: f32,
    #[serde(default = "def_one")]
    pub particles_bead_val: f32,
    #[serde(default)] // 0 = no HDR emissive on beads
    pub particles_bead_emissive: f32,
    // --- Metaball mode look ---
    #[serde(default = "def_metaball_radius")]
    pub metaball_radius: f32,
    #[serde(default = "def_metaball_threshold")]
    pub metaball_threshold: f32,
    #[serde(default = "def_one")]
    pub metaball_smooth: f32,
    // --- Gaussian Splatting surface (SurfaceMode::Splat) ---
    #[serde(default = "def_splat_radius")]
    pub splat_radius: f32,
    #[serde(default = "def_splat_opacity")]
    pub splat_opacity: f32,
    #[serde(default = "def_one")]
    pub splat_falloff: f32,
    #[serde(default = "def_one_u32")] // SplatMode::Lit
    pub splat_mode: u32,
    #[serde(default = "def_splat_cutoff")]
    pub splat_cutoff: f32,
    #[serde(default = "def_one")]
    pub splat_aniso: f32,
    #[serde(default = "def_one_i32")] // Tier 3: 1 sub-splat/node
    pub splat_scatter: i32,
    #[serde(default = "def_splat_jitter")]
    pub splat_jitter: f32,
    #[serde(default)] // 0 = soft Gaussian (unchanged)
    pub splat_solid: f32,
    // --- Density-Map Attractor (#380 Tier 1) ---
    #[serde(default)] // MapKind Complexus
    pub ma_kind: u32,
    #[serde(default = "def_ma_ab")]
    pub ma_a: f32,
    #[serde(default = "def_ma_ab")]
    pub ma_b: f32,
    // --- Density-Map Attractor (#380 Tier 3): extra coefficients + colour mode ---
    #[serde(default = "def_ma_ab")] // c = 1.5 (matches a/b)
    pub ma_c: f32,
    #[serde(default = "def_ma_ab")] // d = 1.5
    pub ma_d: f32,
    #[serde(default)] // MapColor StepSpeed (0 → byte-identical)
    pub ma_color: u32,
    #[serde(default = "def_ma_points_k")]
    pub ma_points_k: i32,
    #[serde(default = "def_ma_warmup")]
    pub ma_warmup: i32,
    #[serde(default = "def_ma_scale")]
    pub ma_scale: f32,
    #[serde(default = "def_ma_size")]
    pub ma_size: f32,
    #[serde(default = "def_one")]
    pub ma_intensity: f32,
    #[serde(default)] // 0 = static a/b (Tier-1 default)
    pub ma_a_drive: f32,
    #[serde(default)]
    pub ma_b_drive: f32,
    // --- Density-Map Attractor parameter orbit (#380 Tier 2) ---
    #[serde(default = "def_ma_orbit_mode")] // Linear (Tier-1-compatible default)
    pub ma_orbit: u32,
    #[serde(default = "def_ma_loop_beats")]
    pub ma_loop_beats: f32,
    #[serde(default = "def_ma_ab")] // radius 1.5
    pub ma_orbit_ra: f32,
    #[serde(default = "def_ma_ab")]
    pub ma_orbit_rb: f32,
    #[serde(default = "def_one_i32")]
    pub ma_orbit_fa: i32,
    #[serde(default = "def_two_i32")]
    pub ma_orbit_fb: i32,
    #[serde(default = "def_frac_pi_2")]
    pub ma_orbit_psi: f32,
    #[serde(default = "def_ma_orbit_free")]
    pub ma_orbit_free: f32,
    // --- Contiguous (welded) Swept Tubes ---
    #[serde(default)] // welding off
    pub tube_weld: bool,
    #[serde(default = "def_true")]
    pub tube_end_cap: bool,
    #[serde(default = "def_half")]
    pub tube_cap_round: f32,
    #[serde(default)] // rounded (no bevel)
    pub tube_cap_bevel: f32,
    #[serde(default = "def_one")] // circle
    pub tube_profile: f32,
    // --- Voxel mode look ---
    #[serde(default = "def_voxel_res")]
    pub voxel_res: f32,
    #[serde(default = "def_voxel_threshold")]
    pub voxel_threshold: f32,
    #[serde(default = "def_one")]
    pub voxel_radius: f32,
    #[serde(default)] // 0 = no glow
    pub voxel_emission: f32,
    #[serde(default = "def_one")]
    pub voxel_ao: f32,
    #[serde(default = "def_voxel_shadow")]
    pub voxel_shadow: f32,
    #[serde(default)] // 0 = no posterize
    pub voxel_quantize: f32,
    #[serde(default)] // 0 = no beat pump
    pub voxel_beat: f32,
    // --- Voxel GI (#89) ---
    #[serde(default)] // off
    pub voxel_gi: bool,
    #[serde(default = "def_one")]
    pub voxel_gi_strength: f32,
    #[serde(default = "def_voxel_gi_distance")]
    pub voxel_gi_distance: f32,
    #[serde(default = "def_voxel_gi_sky")]
    pub voxel_gi_sky: f32,
    // --- Bioluminescence ---
    #[serde(default)] // 0 = no cycling
    pub color_cycle: f32,
    #[serde(default)] // 0 = ripple off
    pub ripple_intensity: f32,
    #[serde(default = "def_ripple_speed")]
    pub ripple_speed: f32,
    #[serde(default = "def_one")]
    pub ripple_freq: f32,
    #[serde(default = "def_ripple_sharp")]
    pub ripple_sharp: f32,
    #[serde(default)] // 0 = Radial
    pub ripple_geom: u32,
    // --- Membrane mode ---
    #[serde(default)] // 0 = Auto
    pub membrane_weave: u32,
    #[serde(default)] // false = membrane only
    pub membrane_show_strands: bool,
    #[serde(default)] // false = continuous shell
    pub membrane_arms: bool,
    #[serde(default)] // 0 = Impostor (capsules)
    pub membrane_arm_build: u32,
    #[serde(default)] // false = leave the seam open
    pub membrane_close: bool,
    #[serde(default)] // 0 = auto capsule radius (per-node thickness)
    pub membrane_arm_radius: f32,
    // --- Reaction–diffusion skin ---
    #[serde(default)] // 0 = off
    pub rd_intensity: f32,
    #[serde(default = "def_rd_feed")]
    pub rd_feed: f32,
    #[serde(default = "def_rd_kill")]
    pub rd_kill: f32,
    #[serde(default = "def_rd_scale")]
    pub rd_scale: f32,
    #[serde(default)]
    pub rd_albedo_mix: f32,
    // --- Particle Aura (#81) — captured as a look. All #[serde(default)] so old
    // presets (no particle fields) load with the aura Off + sane idle defaults. ---
    #[serde(default)] // 0 = Off
    pub particles_tier: u32,
    #[serde(default = "def_particles_count_k")]
    pub particles_count_k: i32,
    #[serde(default = "def_particles_grid_res")]
    pub particles_grid_res: i32,
    #[serde(default = "def_particles_speed")]
    pub particles_speed: f32,
    #[serde(default = "def_particles_lifetime")]
    pub particles_lifetime: f32,
    #[serde(default = "def_particles_spawn_radius")]
    pub particles_spawn_radius: f32,
    #[serde(default = "def_particles_size")]
    pub particles_size: f32,
    #[serde(default = "def_particles_emissive")]
    pub particles_emissive: f32,
    #[serde(default)]
    pub particles_ribbon: bool,
    #[serde(default = "def_particles_ribbon_stretch")]
    pub particles_ribbon_stretch: f32,
    #[serde(default)]
    pub particles_hue_shift: f32,
    #[serde(default = "def_particles_beat_burst")]
    pub particles_beat_burst: f32,
    #[serde(default = "def_particles_drag")]
    pub particles_drag: f32,
    #[serde(default = "def_particles_turbulence")]
    pub particles_turbulence: f32,
    #[serde(default = "def_particles_alpha")]
    pub particles_alpha: f32,
    #[serde(default)]
    pub particles_hide_generator: bool,
    // --- Shaded particle beads (#298 Tier 1) — a look; #[serde(default)] so old
    // presets load with sparks (beads off) + sane bead material defaults. ---
    #[serde(default)]
    pub particles_beads: bool,
    #[serde(default = "def_particles_metallic")]
    pub particles_metallic: f32,
    #[serde(default = "def_particles_roughness")]
    pub particles_roughness: f32,
    #[serde(default)] // 0 = Standard
    pub particles_material: u32,
    #[serde(default)] // 0 = Sphere
    pub particles_shape: u32,
    #[serde(default = "def_particles_ior")]
    pub particles_ior: f32,
    #[serde(default = "def_particles_shape_param")]
    pub particles_shape_param: f32,
    #[serde(default)]
    pub particles_beads_rt: bool,
    // --- Maxwell field energization (#247 Tier 1) — a particle lighting mode, captured
    // as a look; all #[serde(default)] so old presets load inert. ---
    #[serde(default)] // energization off
    pub mn_energize: bool,
    #[serde(default = "def_one")] // energy brightness gain
    pub mn_gain: f32,
    #[serde(default = "def_mn_knee")]
    pub mn_knee: f32,
    #[serde(default = "def_mn_hue")]
    pub mn_hue: f32,
    #[serde(default)] // finite antenna off (#247 Tier 2)
    pub mn_antenna: bool,
    #[serde(default = "def_mn_antenna_len")]
    pub mn_antenna_len: f32,
    #[serde(default)] // fluid dye injection off (#247 Tier 3)
    pub mn_dye_inject: f32,
    #[serde(default)] // aura E↔B blend: 0 = pure E (#feat/maxwell-eb-blend)
    pub mx_aura_blend: f32,
    // --- Legacy migration (deserialize-only): presets saved before the E↔B blend
    // (#feat/maxwell-eb-blend) stored a generator bool (`mx_trace_b`) and, from #304,
    // an aura enum (`mn_aura_field`: 0 = follow generator, 1 = Electric, 2 = Magnetic).
    // Read here and folded into the blends above by `migrate_legacy_fields()` on load;
    // `skip_serializing` so they never re-persist (and stay out of the round-trip /
    // tab-partition field sets). ---
    #[serde(default, skip_serializing)]
    pub mx_trace_b: Option<bool>,
    #[serde(default, skip_serializing)]
    pub mn_aura_field: Option<u32>,
    // --- Field-force particle drive (#248) — all defaulted so old presets load inert. ---
    #[serde(default)] // force drive off
    pub mn_force: bool,
    #[serde(default = "def_one")] // force strength
    pub mn_force_gain: f32,
    #[serde(default = "def_one")] // energization core contrast (1 = current look)
    pub mn_energy_contrast: f32,
    #[serde(default = "def_stir_rate")] // fluid swirl reversal rate (Hz)
    pub mn_stir_rate: f32,
    #[serde(default)] // acoustic pump off
    pub mn_pump: f32,
    #[serde(default)] // beat spin force off (manual reversal)
    pub mn_swirl_beat: f32,
    #[serde(default = "def_pump_scale")] // pump core size
    pub mn_pump_scale: f32,
    #[serde(default = "def_swirl_decay")] // turbine spin slowdown (1/s)
    pub mn_swirl_decay: f32,
    #[serde(default = "def_mode_mix")] // beat mode crossfade (−1 turbine .. +1 dynamo)
    pub mn_mode_mix: f32,
    #[serde(default = "def_ring_freq")] // E↔B ring frequency (Hz)
    pub mn_ring_freq: f32,
    #[serde(default)] // hue cycle off
    pub mn_hue_cycle: f32,
    // --- Audio-driven dipole radiation (#248 Tier 1) — RMS envelope → Maxwell drive
    // amplitude; all #[serde(default)] so old presets load inert. ---
    #[serde(default)] // audio drive off
    pub ad_drive: bool,
    #[serde(default = "def_one")] // unity RMS → drive gain
    pub ad_amount: f32,
    #[serde(default = "def_ad_floor")]
    pub ad_floor: f32,
    #[serde(default)] // spectrum → multipole off (#248 Tier 2)
    pub ad_multipole: bool,
    #[serde(default = "def_ad_spread")]
    pub ad_spread: f32,
    #[serde(default = "def_ad_band_hue")]
    pub ad_band_hue: f32,
    #[serde(default = "def_half")] // #248 T3 stereo lean
    pub ad_stereo: f32,
    #[serde(default = "def_half")] // #248 T3 pitch → rate
    pub ad_pitch: f32,
    #[serde(default)] // #248 T3 waveform shells off
    pub ad_wave: f32,
    #[serde(default = "def_fluid_force")]
    pub fluid_force: f32,
    #[serde(default = "def_fluid_vorticity")]
    pub fluid_vorticity: f32,
    #[serde(default = "def_fluid_dissipation")]
    pub fluid_dissipation: f32,
    #[serde(default = "def_fluid_iters")]
    pub fluid_iters: i32,
    #[serde(default = "def_fluid_inflow_decay")]
    pub fluid_inflow_decay: f32,
    // --- Fluid Ink (#182 Tier 1): dye in the stirred medium ---
    #[serde(default)]
    pub ink_enabled: bool,
    #[serde(default = "def_ink_rate")]
    pub ink_rate: f32,
    #[serde(default = "def_ink_radius")]
    pub ink_radius: f32,
    #[serde(default = "def_ink_extinction")]
    pub ink_extinction: f32,
    #[serde(default = "def_one")]
    pub ink_scatter: f32,
    #[serde(default = "def_ink_emissive")]
    pub ink_emissive: f32,
    #[serde(default = "def_ink_anisotropy")]
    pub ink_anisotropy: f32,
    #[serde(default = "def_ink_dissipation")]
    pub ink_dissipation: f32,
    #[serde(default = "def_ink_steps")]
    pub ink_steps: f32,
    #[serde(default = "def_true")]
    pub ink_maccormack: bool,
    #[serde(default = "def_true")]
    pub ink_half_res: bool,
    #[serde(default = "def_ink_reveal")]
    pub ink_reveal: f32,
    // --- Fluid medium Tier 2 (#182): boundaries / buoyancy / detail / beat ---
    #[serde(default)]
    pub fl2_boundaries: bool,
    #[serde(default)]
    pub fl2_buoyancy: f32,
    #[serde(default = "def_fl2_heat_decay")]
    pub fl2_heat_decay: f32,
    #[serde(default)]
    pub fl2_detail: f32,
    #[serde(default)]
    pub fl2_splash: f32,
    #[serde(default)]
    pub fl2_dye_gate: f32,
    #[serde(default)]
    pub fl2_res: i32,
    #[serde(default = "def_fl2_substeps")]
    pub fl2_substeps: i32,
    // --- MLS-MPM liquid (#182 Tier 3a) ---
    #[serde(default)]
    pub liq_enabled: bool,
    #[serde(default = "def_liq_count")]
    pub liq_count: i32,
    #[serde(default = "def_liq_res")]
    pub liq_res: i32,
    #[serde(default)]
    pub liq_gravity: f32,
    #[serde(default = "def_liq_stiffness")]
    pub liq_stiffness: f32,
    #[serde(default = "def_liq_viscosity")]
    pub liq_viscosity: f32,
    #[serde(default = "def_liq_container")]
    pub liq_container: f32,
    #[serde(default)]
    pub liq_open_top: bool,
    #[serde(default = "def_true")]
    pub liq_collide: bool,
    #[serde(default = "def_one")]
    pub liq_stir: f32,
    #[serde(default = "def_one")]
    pub liq_density: f32,
    #[serde(default = "def_liq_threshold")]
    pub liq_threshold: f32,
    #[serde(default = "def_liq_hue")]
    pub liq_hue: f32,
    #[serde(default = "def_liq_sat")]
    pub liq_sat: f32,
    #[serde(default = "def_liq_substeps")]
    pub liq_substeps: i32,
    #[serde(default)]
    pub liq_offset_y: f32,
    #[serde(default)]
    pub liq_shape: u32,
    #[serde(default)]
    pub liq_reveal: f32,
    // --- Fluid light coupling (#182 Tier 4) ---
    #[serde(default)]
    pub fgi_gi: f32,
    #[serde(default)]
    pub fgi_shadow: f32,
    #[serde(default)]
    pub fgi_receive: bool,
    #[serde(default)]
    pub fgi_sway: f32,
    #[serde(default)]
    pub ca_amount: f32,
    #[serde(default = "def_one")]
    pub ca_sharpness: f32,
    #[serde(default)]
    pub liq_material: u32,
    #[serde(default)]
    pub liq_metallic: f32,
    #[serde(default = "def_liq_roughness")]
    pub liq_roughness: f32,
    #[serde(default = "def_liq_ior")]
    pub liq_ior: f32,
    #[serde(default)]
    pub ghost_light: bool,
    #[serde(default)]
    pub liq_render: u32,
    #[serde(default = "def_liq_absorb")]
    pub liq_absorb: f32,
    #[serde(default)]
    pub liq_glow: f32,
    #[serde(default)]
    pub liq_chrome_purity: f32,
    #[serde(default)]
    pub liq_glass_clarity: f32,
    #[serde(default)]
    pub liq_f0: f32,
    #[serde(default)]
    pub liq_dispersion: f32,
    #[serde(default)]
    pub liq_gcaustic: f32,
    #[serde(default)]
    pub liq_thin_film: f32,
    /// Hardware RT master (#195 Tier 0). The debug view is per-display and is
    /// deliberately NOT captured (like HDR/MSAA).
    #[serde(default)]
    pub rt_enable: bool,
    // RT shadows (#195 Tier 1) — Looks. Strength defaults to 1 so presets
    // saved before this field recall a working shadow when enabled.
    #[serde(default)]
    pub rt_shadows: bool,
    #[serde(default)]
    pub rt_shadow_soft: f32,
    #[serde(default = "def_one")]
    pub rt_shadow_strength: f32,
    #[serde(default)]
    pub rt_shadow_fill: bool,
    // RT reflections (#195 Tier 2) — Looks. The dials default to working
    // values so presets saved before these fields recall a live reflection
    // when enabled.
    #[serde(default)]
    pub rt_reflect: bool,
    #[serde(default = "def_one")]
    pub rt_reflect_intensity: f32,
    #[serde(default = "def_rt_reflect_rough")]
    pub rt_reflect_rough: f32,
    #[serde(default = "def_rt_reflect_reach")]
    pub rt_reflect_reach: f32,
    #[serde(default = "def_true")]
    pub rt_reflect_shadows: bool,
    #[serde(default = "def_rt_reflect_rays")]
    pub rt_reflect_rays: i32,
    // RT AO (#195 Tier 3) — Looks. Source 0 = GTAO (old presets unchanged).
    #[serde(default)]
    pub ao_source: u32,
    #[serde(default = "def_rt_ao_rays")]
    pub rt_ao_rays: i32,
    // RT diffuse GI (#195 Tier 4) — Looks. Dials default to working values so
    // presets saved before these fields recall a live bounce when enabled.
    #[serde(default)]
    pub rt_gi: bool,
    #[serde(default = "def_one")]
    pub rt_gi_intensity: f32,
    #[serde(default = "def_rt_ao_rays")]
    pub rt_gi_rays: i32,
    #[serde(default = "def_rt_gi_reach")]
    pub rt_gi_reach: f32,
    #[serde(default = "def_true")]
    pub rt_gi_shadows: bool,
    // Beat-aware temporal accumulator (#200 Tier 4½ part 3) — Looks. Dials
    // default to working values so old presets recall a live accumulation when
    // enabled.
    #[serde(default)]
    pub rt_temporal: bool,
    #[serde(default = "def_rt_temporal_feedback")]
    pub rt_temporal_feedback: f32,
    #[serde(default = "def_rt_temporal_beat")]
    pub rt_temporal_beat: f32,
    // Part 4 (variance-guided SVGF). Dials default to working values so old
    // presets recall a sensible clamp when variance is enabled.
    #[serde(default)]
    pub rt_temporal_variance: bool,
    #[serde(default = "def_rt_temporal_accum")]
    pub rt_temporal_accum: f32,
    #[serde(default = "def_rt_temporal_clamp")]
    pub rt_temporal_clamp: f32,
    // RT denoise (#200 Tier 4½ part 2) — Looks. Amount defaults to 1 so a preset
    // saved before this recalls a working denoise when the toggle is enabled.
    #[serde(default)]
    pub rt_denoise: bool,
    #[serde(default = "def_one")]
    pub rt_denoise_amount: f32,
    // Neural denoiser (#200 Tier 5a) — Looks. Off; net/seed/omega default to
    // working values so an old preset recalls a sensible filter when enabled.
    #[serde(default)]
    pub nd_enable: bool,
    #[serde(default = "def_half")]
    pub nd_strength: f32,
    #[serde(default = "def_neural_seed_a")]
    pub nd_seed: i32,
    #[serde(default = "def_neural_omega")]
    pub nd_omega: f32,
    // Learned upscaler (#200 Tier 5c) — Looks. Off; sharpen/seed default to
    // working values so an old preset recalls a sensible upscaler when enabled.
    #[serde(default)]
    pub up_enable: bool,
    #[serde(default = "def_half")]
    pub up_sharpen: f32,
    #[serde(default = "def_neural_seed_a")]
    pub up_seed: i32,
    // Neural field foundation (#200 Tier 0) — Looks. Ships dark; dials default to
    // working values so a preset recalls a sensible network when enabled later.
    #[serde(default)]
    pub neural_enable: bool,
    #[serde(default = "def_neural_seed_a")]
    pub neural_seed_a: i32,
    #[serde(default = "def_neural_seed_b")]
    pub neural_seed_b: i32,
    #[serde(default)]
    pub neural_walk: f32,
    #[serde(default = "def_neural_omega")]
    pub neural_omega: f32,
    // Neural field generator (#200 Tier 1) — Looks. Dials default to the working
    // isosurface so old presets recall a sensible field when the mode is chosen.
    #[serde(default = "def_neural_scale")]
    pub neural_scale: f32,
    #[serde(default = "def_neural_coord")]
    pub neural_coord: f32,
    #[serde(default)]
    pub neural_iso: f32,
    #[serde(default = "def_neural_steps")]
    pub neural_steps: i32,
    #[serde(default = "def_neural_march")]
    pub neural_march: f32,
    #[serde(default = "def_neural_color")]
    pub neural_color: f32,
    #[serde(default)]
    pub neural_walk_rate: f32,
    // Neural field strand form (#200 Tier 1b) — Looks.
    #[serde(default)]
    pub neural_strands_mode: bool,
    #[serde(default = "def_neural_strands_dim")]
    pub neural_strands_cols: i32,
    #[serde(default = "def_neural_strands_dim")]
    pub neural_strands_rows: i32,
    #[serde(default = "def_neural_strands_extent")]
    pub neural_strands_extent: f32,
    #[serde(default = "def_one")]
    pub neural_strands_displace: f32,
    // --- Jewel Box (#80): SSR + bounced GI + spectral glass (all off by default) ---
    #[serde(default)]
    pub ssr: bool,
    #[serde(default = "def_one")]
    pub ssr_intensity: f32,
    #[serde(default = "def_ssr_max_rough")]
    pub ssr_max_roughness: f32,
    #[serde(default = "def_ssr_thickness")]
    pub ssr_thickness: f32,
    #[serde(default = "def_ssr_steps")]
    pub ssr_steps: i32,
    #[serde(default)]
    pub gi: bool,
    #[serde(default = "def_one")]
    pub gi_intensity: f32,
    #[serde(default = "def_one")]
    pub gi_falloff: f32,
    #[serde(default)]
    pub glass_dispersion: f32,
    #[serde(default)]
    pub glass_caustic: f32,
    #[serde(default)]
    pub glass_thin_film: f32,
    // --- Post-composite creative FX (#152, Tier 1) — Look ---
    #[serde(default)]
    pub fx_enabled: bool,
    #[serde(default)] // RenderStyle::None
    pub fx_style: u32,
    #[serde(default = "def_fx_style_amt")]
    pub fx_style_amt: f32,
    #[serde(default)]
    pub fx_dof: f32,
    #[serde(default = "def_half")]
    pub fx_dof_focus: f32,
    #[serde(default = "def_fx_dof_range")]
    pub fx_dof_range: f32,
    #[serde(default)]
    pub fx_chroma: f32,
    #[serde(default)]
    pub fx_vignette: f32,
    #[serde(default)]
    pub fx_grain: f32,
    #[serde(default = "def_one")]
    pub fx_grade_sat: f32,
    #[serde(default = "def_one")]
    pub fx_grade_contrast: f32,
    #[serde(default)]
    pub fx_grade_temp: f32,
    #[serde(default = "def_one")]
    pub fx_grade_gain: f32,
    #[serde(default)]
    pub fx_feedback: f32,
    #[serde(default = "def_fx_outline")]
    pub fx_outline: f32,
    // --- Emissive volume surface mode (#152, Tier 1) — Surface (Generator) ---
    #[serde(default = "def_volume_radius")]
    pub volume_radius: f32,
    #[serde(default = "def_one")]
    pub volume_density: f32,
    #[serde(default = "def_volume_emission")]
    pub volume_emission: f32,
    #[serde(default = "def_volume_absorption")]
    pub volume_absorption: f32,
    #[serde(default = "def_volume_steps")]
    pub volume_steps: i32,
    // --- Screen-space GI (#152 Tier 2) — Look (temporal/TAA is per-display, not captured) ---
    #[serde(default)]
    pub ssgi: bool,
    #[serde(default = "def_one")]
    pub ssgi_intensity: f32,
    #[serde(default = "def_ssgi_radius")]
    pub ssgi_radius: f32,
    #[serde(default = "def_ssgi_rays")]
    pub ssgi_rays: i32,
    // --- Cast shadows (#152 Tier 3) — Look ---
    #[serde(default)]
    pub shadow_enabled: bool,
    #[serde(default = "def_shadow_bias")]
    pub shadow_bias: f32,
    #[serde(default = "def_one")]
    pub shadow_strength: f32,
    // --- Voxel GI (#152 Tier 3, #10) — Look ---
    #[serde(default)]
    pub vxgi_enabled: bool,
    #[serde(default = "def_one")]
    pub vxgi_intensity: f32,
    #[serde(default = "def_vxgi_rays")]
    pub vxgi_rays: i32,
    #[serde(default = "def_vxgi_steps")]
    pub vxgi_steps: i32,
    // --- Reflection look (#163 Tier 1) — Look. All default 0 (today's look). ---
    #[serde(default)]
    pub reflect_tint: f32,
    #[serde(default)]
    pub chrome_purity: f32,
    #[serde(default)]
    pub glass_clarity: f32,
    #[serde(default)]
    pub f0_override: f32,
    // --- Reflection probe / parallax (#163 Tier 2) — Look ---
    // Defaults (source 0 = EnvOnly, scale/height/blend 1) = today's look, so old
    // presets that predate these fields deserialize to the current behaviour.
    #[serde(default)]
    pub refl_source: u32,
    #[serde(default = "def_one")]
    pub refl_box_scale: f32,
    #[serde(default = "def_one")]
    pub refl_box_height: f32,
    #[serde(default = "def_one")]
    pub refl_blend: f32,
    // --- VXGI specular reflections (#163 Tier 3) — Look. strength 0 = today. ---
    #[serde(default)]
    pub vxgi_spec_strength: f32,
    #[serde(default = "def_vxgi_spec_aperture")]
    pub vxgi_spec_aperture: f32,
    #[serde(default = "def_one")]
    pub vxgi_spec_reach: f32,
    #[serde(default = "def_vxgi_spec_steps")]
    pub vxgi_spec_steps: i32,
    // --- Membrane screen-space FX (perf opt-in) — Look. Off = today's membrane. ---
    #[serde(default)]
    pub membrane_fx: bool,
    // --- Cinematic finishing (#167 Tier 1): halation + lens flares — Look. Off = today. ---
    #[serde(default)]
    pub hal_amount: f32,
    #[serde(default = "def_hal_threshold")]
    pub hal_threshold: f32,
    #[serde(default = "def_one")]
    pub hal_width: f32,
    #[serde(default = "def_hal_warmth")]
    pub hal_warmth: f32,
    #[serde(default)]
    pub lf_amount: f32,
    #[serde(default = "def_lf_ghosts")]
    pub lf_ghosts: f32,
    #[serde(default = "def_lf_halo")]
    pub lf_halo: f32,
    #[serde(default = "def_lf_streak")]
    pub lf_streak: f32,
    // --- Emissive cubes as real lights (#167 Tier 3) — Look. Off = today. ---
    #[serde(default)]
    pub ml_enabled: bool,
    #[serde(default = "def_one")]
    pub ml_intensity: f32,
    #[serde(default = "def_ml_radius")]
    pub ml_radius: f32,
    #[serde(default = "def_ml_count")]
    pub ml_count: i32,
    // ReSTIR many-lights (#200 Tier 5d) — a Look. Off = brightest-N.
    #[serde(default)]
    pub ml_restir: bool,
    // Path-tracer dielectric BTDF (#258 Tier 2) — Looks. Off = diffuse-only tracer.
    #[serde(default)]
    pub pt_dielectric: bool,
    #[serde(default)]
    pub pt_absorb: f32,
    // Path-tracer composite mode + augment amount — Looks. Default 0 = Replace / untouched.
    #[serde(default)]
    pub pt_composite: u32,
    #[serde(default)]
    pub pt_augment: f32,
    // Spectral dispersion (#258 T4) — Looks. Sample budget is per-quality (not saved).
    #[serde(default)]
    pub spectral_enable: bool,
    #[serde(default = "def_spectral_abbe")]
    pub spectral_abbe: f32,
    // Photon-mapped caustics (#258 T5) — Looks. Photon budget is per-quality (not saved).
    #[serde(default)]
    pub pt_caustics: bool,
    #[serde(default = "def_pt_caustic_intensity")]
    pub pt_caustic_intensity: f32,
    #[serde(default = "def_pt_caustic_radius")]
    pub pt_caustic_radius: f32,

    // ===================================================================
    // #354 — Environment (world layer + sky/IBL) captured by presets.
    // Enums are stored as u32. Serde defaults are 0/false/0.0: they only
    // matter for OLD presets that predate the field (which recall the world
    // layer to "off"); NEW presets capture live values in `capture()`.
    // ===================================================================
    // Terrain + sun / day-cycle.
    #[serde(default)] pub terrain_enabled: bool,
    #[serde(default)] pub terrain_height: f32,
    #[serde(default)] pub terrain_snow: f32,
    #[serde(default)] pub terrain_fog: f32,
    #[serde(default)] pub terrain_sun_elev: f32,
    #[serde(default)] pub terrain_sun_azim: f32,
    #[serde(default)] pub terrain_sun_int: f32,
    #[serde(default)] pub terrain_scroll: f32,
    #[serde(default)] pub terrain_ride: f32,
    #[serde(default)] pub terrain_noise: u32,
    #[serde(default)] pub terrain_palette: u32,
    #[serde(default)] pub terrain_emissive: f32,
    #[serde(default)] pub terrain_day_speed: f32,
    #[serde(default)] pub terrain_sun_scene: bool,
    #[serde(default)] pub terrain_scatter: f32,
    #[serde(default)] pub terrain_godray: f32,
    #[serde(default)] pub terrain_water: bool,
    #[serde(default)] pub terrain_water_level: f32,
    #[serde(default)] pub terrain_water_hue: f32,
    #[serde(default)] pub terrain_water_ripple: f32,
    #[serde(default)] pub terrain_seed: i32,
    #[serde(default)] pub terrain_ridged: bool,
    #[serde(default)] pub terrain_brightness: f32,
    #[serde(default)] pub terrain_haze: f32,
    #[serde(default)] pub terrain_steps: i32,
    #[serde(default)] pub terrain_octaves: i32,
    #[serde(default)] pub terrain_res: u32,
    // Starfield + sun disc.
    #[serde(default)] pub stars_enabled: bool,
    #[serde(default)] pub stars_brightness: f32,
    #[serde(default)] pub stars_twinkle: f32,
    #[serde(default)] pub stars_twinkle_speed: f32,
    #[serde(default)] pub stars_size: f32,
    #[serde(default)] pub stars_latitude: f32,
    #[serde(default)] pub stars_sky_speed: f32,
    #[serde(default)] pub stars_mag_limit: f32,
    #[serde(default)] pub stars_saturation: f32,
    #[serde(default)] pub stars_sun: bool,
    #[serde(default)] pub stars_sun_bright: f32,
    #[serde(default)] pub stars_sun_size: f32,
    #[serde(default)] pub stars_sun_warmth: f32,
    // Atmosphere (physical sky, #100).
    #[serde(default)] pub atmos_enabled: bool,
    #[serde(default)] pub atmos_turbidity: f32,
    #[serde(default)] pub atmos_mie_g: f32,
    #[serde(default)] pub atmos_sun_int: f32,
    #[serde(default)] pub atmos_ground_albedo: f32,
    #[serde(default)] pub atmos_exposure: f32,
    #[serde(default)] pub atmos_aerial: f32,
    #[serde(default)] pub atmos_rayleigh: f32,
    // Volumetric clouds (#102 A).
    #[serde(default)] pub clouds_enabled: bool,
    #[serde(default)] pub clouds_coverage: f32,
    #[serde(default)] pub clouds_density: f32,
    #[serde(default)] pub clouds_base: f32,
    #[serde(default)] pub clouds_thickness: f32,
    #[serde(default)] pub clouds_steps: i32,
    #[serde(default)] pub clouds_detail: f32,
    #[serde(default)] pub clouds_drift: f32,
    #[serde(default)] pub clouds_hg: f32,
    #[serde(default)] pub clouds_absorption: f32,
    #[serde(default)] pub clouds_shadow: f32,
    #[serde(default)] pub clouds_ambient: f32,
    // FFT ocean (#102 B).
    #[serde(default)] pub ocean_enabled: bool,
    #[serde(default)] pub ocean_level: f32,
    #[serde(default)] pub ocean_wind_speed: f32,
    #[serde(default)] pub ocean_wind_dir: f32,
    #[serde(default)] pub ocean_amplitude: f32,
    #[serde(default)] pub ocean_choppiness: f32,
    #[serde(default)] pub ocean_tile_size: f32,
    #[serde(default)] pub ocean_foam: f32,
    #[serde(default)] pub ocean_glitter: f32,
    #[serde(default)] pub ocean_hue: f32,
    #[serde(default)] pub ocean_depth: f32,
    /// The loaded `.hdr` environment reference (#354): the path last opened via
    /// "Open HDR Environment…". Restored on recall by re-driving the sidecar +
    /// bumping `hdr_gen`. Empty = don't touch the current environment. Applied
    /// out-of-band (not through the tab partition), and skipped from JSON when
    /// empty so it stays out of the partition drift-guard.
    #[serde(default, skip_serializing_if = "String::is_empty")] pub hdr_path: String,

    // ===================================================================
    // #354 — Settings (renderer / output / capture) captured by presets.
    // ===================================================================
    #[serde(default)] pub hdr_output: bool,
    #[serde(default)] pub hdr_knee: f32,
    #[serde(default)] pub hdr_wide: bool,
    #[serde(default)] pub hdr_vivid: f32,
    #[serde(default)] pub msaa: u32,
    #[serde(default)] pub render_scale: f32,
    #[serde(default)] pub render_auto: bool,
    // Output framing / letterbox (the pack_capture[12] block).
    #[serde(default)] pub aspect_preset: u32,
    #[serde(default)] pub out_long_edge: i32,
    #[serde(default)] pub out_custom_w: i32,
    #[serde(default)] pub out_custom_h: i32,
    #[serde(default)] pub letterbox_r: f32,
    #[serde(default)] pub letterbox_g: f32,
    #[serde(default)] pub letterbox_b: f32,
    #[serde(default)] pub frame_guide: bool,
    #[serde(default)] pub lock_window: bool,

    // ===================================================================
    // #354 — Audio metering config captured by the Audio preset.
    // ===================================================================
    #[serde(default)] pub meter_res: u32,
    #[serde(default)] pub meter_weight: u32,
    #[serde(default)] pub meter_averaging: u32,
    #[serde(default)] pub meter_hud: bool,
}

fn def_pt_caustic_intensity() -> f32 { 1.0 }
fn def_pt_caustic_radius() -> f32 { 2.0 }

fn def_ml_radius() -> f32 { 0.5 }
fn def_ml_count() -> i32 { 24 }

fn def_hal_threshold() -> f32 { 0.6 }
fn def_hal_warmth() -> f32 { 0.6 }
fn def_lf_ghosts() -> f32 { 0.5 }
fn def_lf_halo() -> f32 { 0.4 }
fn def_lf_streak() -> f32 { 0.3 }
fn def_vxgi_rays() -> i32 { 4 }
fn def_vxgi_steps() -> i32 { 12 }
fn def_vxgi_spec_aperture() -> f32 { 0.2 }
fn def_vxgi_spec_steps() -> i32 { 24 }

fn def_shadow_bias() -> f32 { 0.0015 }
fn def_rt_reflect_rough() -> f32 { 0.4 }
fn def_rt_reflect_reach() -> f32 { 2.0 }
fn def_rt_ao_rays() -> i32 { 16 }
fn def_rt_reflect_rays() -> i32 { 16 }
fn def_rt_gi_reach() -> f32 { 2.0 }
fn def_rt_temporal_feedback() -> f32 { 0.9 }
fn def_rt_temporal_beat() -> f32 { 0.7 }
fn def_rt_temporal_accum() -> f32 { 32.0 }
fn def_rt_temporal_clamp() -> f32 { 3.0 }
fn def_neural_seed_a() -> i32 { 1 }
fn def_neural_seed_b() -> i32 { 2 }
fn def_neural_omega() -> f32 { 4.0 }
fn def_neural_scale() -> f32 { 120.0 }
fn def_neural_coord() -> f32 { 1.5 }
fn def_neural_steps() -> i32 { 96 }
fn def_neural_march() -> f32 { 0.6 }
fn def_neural_color() -> f32 { 0.8 }
fn def_neural_strands_dim() -> i32 { 48 }
fn def_neural_strands_extent() -> f32 { 2.5 }
fn def_frenet_strands() -> i32 { 24 }
fn def_frenet_nodes() -> i32 { 200 }
fn def_frenet_step() -> f32 { 0.12 }
fn def_frenet_kappa() -> f32 { 0.35 }
fn def_frenet_tau() -> f32 { 0.12 }
fn def_frenet_spread() -> f32 { 0.15 }
fn def_frenet_thickness() -> f32 { 0.25 }

fn def_dna_form() -> u32 { 1 } // B-DNA
fn def_dna_bp() -> i32 { 48 }
fn def_dna_bp_per_turn() -> f32 { 10.5 }
fn def_dna_rise() -> f32 { 3.32 }
fn def_dna_radius() -> f32 { 10.0 }
fn def_dna_groove() -> f32 { 144.0 }
fn def_dna_super_radius() -> f32 { 24.0 }
fn def_dna_seed() -> i32 { 1 }
fn def_dna_thickness() -> f32 { 0.16 }

fn def_attr_seeds() -> i32 { 6 }
fn def_attr_seed() -> i32 { 1 }
fn def_attr_spread() -> f32 { 0.6 }
fn def_attr_trail() -> i32 { 300 }
fn def_attr_thickness() -> f32 { 0.12 }
fn def_boids_count() -> i32 { 120 }
fn def_boids_perception() -> f32 { 3.0 }
fn def_boids_separation() -> f32 { 1.2 }
fn def_boids_sep() -> f32 { 1.5 }
fn def_boids_max_speed() -> f32 { 3.0 }
fn def_boids_max_force() -> f32 { 4.0 }
fn def_boids_trail() -> i32 { 64 }
fn def_boids_bounds() -> f32 { 6.0 }
fn def_boids_goal() -> f32 { 0.3 }
fn def_boids_thickness() -> f32 { 1.5 }
fn def_boids_seed() -> i32 { 1 }
fn def_boids_scale() -> f32 { 30.0 }
fn def_boids_form() -> u32 { 1 } // Fish
fn def_boids_size() -> f32 { 14.0 }
fn def_boids_bank() -> f32 { 0.6 }

fn def_harm_mode0() -> i32 { 4 }
fn def_harm_amp0() -> f32 { 0.5 }
fn def_harm_mode1() -> i32 { 8 }
fn def_harm_amp1() -> f32 { 0.25 }
fn def_harm_freq1() -> f32 { 0.5 }
fn def_harm_mode2() -> i32 { 6 }
fn def_harm_freq2() -> f32 { 0.75 }
fn def_harm_radius() -> f32 { 6.0 }
fn def_harm_theta() -> i32 { 48 }
fn def_harm_phi() -> i32 { 64 }
fn def_harm_thickness() -> f32 { 0.12 }
fn def_bell_stroke_depth() -> f32 { 0.5 }
fn def_bell_stiffness() -> i32 { 8 }
fn def_bell_damping() -> f32 { 0.99 }
fn def_bell_open() -> f32 { 1.7 }
fn def_bell_rate() -> f32 { 0.1 }

fn def_ls_depth() -> i32 { 4 }
fn def_ls_angle() -> f32 { 25.0 }
fn def_ls_step() -> f32 { 0.5 }
fn def_ls_thickness() -> f32 { 0.08 }

fn def_cn_seeds() -> i32 { 12 }
fn def_cn_seed() -> i32 { 1 }
fn def_cn_spread() -> f32 { 4.0 }
fn def_cn_scale() -> f32 { 0.3 }
fn def_cn_steps() -> i32 { 200 }
fn def_cn_dt() -> f32 { 0.08 }
fn def_cn_thickness() -> f32 { 0.1 }
fn def_pol_rings() -> i32 { 4 }
fn def_pol_spokes() -> i32 { 20 }
fn def_pol_samples() -> i32 { 64 }
fn def_pol_len() -> f32 { 14.0 }
fn def_pol_k() -> f32 { 1.6 }
fn def_pol_amp() -> f32 { 2.2 }
fn def_pol_spread() -> f32 { 150.0 }
fn def_pol_thickness() -> f32 { 0.12 }
fn def_mx_sources() -> i32 { 1 }
fn def_mx_separation() -> f32 { 6.0 }
fn def_mx_phase() -> f32 { 1.57 }
fn def_mx_k() -> f32 { 1.2 }
fn def_mx_amp() -> f32 { 3.0 }
fn def_mx_rmin() -> f32 { 0.6 }
fn def_mx_thickness() -> f32 { 0.12 }
fn def_mx_rings() -> i32 { 5 }
fn def_mx_spokes() -> i32 { 24 }
fn def_mx_samples() -> i32 { 48 }
fn def_mx_raylen() -> f32 { 12.0 }
fn def_mx_spread() -> f32 { 180.0 }
fn def_mx_seeds() -> i32 { 8 }
fn def_mx_steps() -> i32 { 200 }
fn def_mx_ds() -> f32 { 0.15 }
fn def_mx_bound() -> f32 { 40.0 }
fn def_mx_osc_div() -> u32 { OscDivision::Quarter.to_u32() }
fn def_ax_count() -> i32 { 24 }
fn def_ax_length() -> f32 { 24.0 }
fn def_ax_bundle() -> f32 { 4.0 }
fn def_ax_samples() -> i32 { 64 }
fn def_ax_thickness() -> f32 { 0.16 }
fn def_ax_node_spacing() -> f32 { 3.0 }
fn def_ax_node_dip() -> f32 { 0.55 }
fn def_ax_pulse_speed() -> f32 { 0.6 }
fn def_ax_pulse_width() -> f32 { 0.10 }
fn def_ax_stagger() -> f32 { 1.0 }
fn def_ax_seed() -> i32 { 1 }
fn def_ax_bend() -> f32 { 0.35 }
fn def_ax_curve() -> f32 { 0.6 }
fn def_ax_tortuosity() -> f32 { 0.3 }
fn def_nw_topology() -> u32 { 3 }
fn def_nw_nodes() -> i32 { 48 }
fn def_nw_connectivity() -> i32 { 6 }
fn def_nw_rewire() -> f32 { 0.15 }
fn def_nw_layers() -> i32 { 4 }
fn def_nw_seed() -> i32 { 1 }
fn def_nw_extent() -> f32 { 12.0 }
fn def_nw_node_size() -> f32 { 0.5 }
fn def_nw_node_glow() -> f32 { 1.2 }
fn def_nw_edge_thickness() -> f32 { 0.14 }
fn def_nw_edge_bow() -> f32 { 0.25 }
fn def_nw_edge_samples() -> i32 { 16 }
fn def_nw_pulse_speed() -> f32 { 0.5 }
fn def_nw_pulse_width() -> f32 { 0.12 }
fn def_nw_edge_fibres() -> i32 { 1 }
fn def_nw_bundle_radius() -> f32 { 0.4 }
fn def_nw_edge_node_dip() -> f32 { 0.6 }
fn def_nw_ranvier() -> i32 { 5 }
fn def_nw_dendrite_count() -> i32 { 6 }
fn def_nw_threshold() -> f32 { 0.5 }
fn def_nw_conduction() -> f32 { 8.0 }
fn def_nw_refractory() -> f32 { 1.0 }
fn def_nw_decay() -> f32 { 0.6 }
fn def_nw_deposit() -> f32 { 0.6 }
fn def_nw_stim_rate() -> f32 { 2.0 }
fn def_nw_sign_colour() -> f32 { 0.8 }
fn def_nw_sparsify() -> f32 { 0.05 }
fn def_nw_layer_gap() -> f32 { 1.0 }
fn def_nw_attn_threshold() -> f32 { 0.05 }
fn def_nw_attn_tokens() -> f32 { 24.0 }
fn def_nw_attn_reveal() -> f32 { 0.5 }
fn def_nt_soma_size() -> f32 { 1.0 }
fn def_nt_bouton_size() -> f32 { 0.35 }
fn def_nt_dendrite_length() -> f32 { 1.0 }
fn def_nt_dendrite_taper() -> f32 { 0.62 }
fn def_nt_ranvier_spacing() -> f32 { 0.6 }
fn def_nt_sheath_scale() -> f32 { 0.5 }
fn def_br_fold_depth() -> f32 { 0.12 }
fn def_br_fold_freq() -> f32 { 5.0 }
fn def_br_hemi_gap() -> f32 { 0.1 }
fn def_br_local_k() -> i32 { 8 }
fn def_br_cerebellum() -> f32 { 0.14 }
fn def_br_assoc() -> f32 { 0.25 }
fn def_br_callosum() -> f32 { 0.4 }
fn def_br_subcortical() -> f32 { 0.2 }
fn def_br_stim_rate() -> f32 { 2.0 }
fn def_film_thickness_var() -> f32 { 0.3 }
fn def_film_ior() -> f32 { 1.33 }
fn def_spectral_abbe() -> f32 { 40.0 }
fn def_film_drainage() -> f32 { 0.5 }
fn def_demo_size() -> f32 { 1.0 }
fn def_demo_light() -> f32 { 1.0 }
fn def_demo_roughness() -> f32 { 0.15 }
fn def_demo_count() -> i32 { 4 }
fn def_sy_radius() -> f32 { 4.0 }
fn def_sy_beta() -> f32 { 0.5 }
fn def_sy_charges() -> i32 { 1 }
fn def_sy_grid() -> i32 { 28 }
fn def_sy_extent() -> f32 { 14.0 }
fn def_sy_near() -> f32 { 1.0 }
fn def_sy_amp() -> f32 { 1.0 }
fn def_sy_thickness() -> f32 { 0.10 }
fn def_sy_rmin() -> f32 { 0.5 }
fn def_sy_line_seeds() -> i32 { 64 }
fn def_sy_line_steps() -> i32 { 220 }
fn def_sy_line_ds() -> f32 { 0.12 }
fn def_sy_line_bound() -> f32 { 40.0 }
fn def_sy_vol_layers() -> i32 { 7 }
fn def_sy_invert_radius() -> f32 { 8.0 }
fn def_vf_grid() -> i32 { 12 }
fn def_vf_extent() -> f32 { 10.0 }
fn def_vf_field_scale() -> f32 { 0.5 }
fn def_vf_amp() -> f32 { 1.0 }
fn def_vf_thickness() -> f32 { 0.10 }
fn def_vf_evolve() -> f32 { 0.3 }
fn def_vf_z_lift() -> f32 { 0.6 }
fn def_vf_line_seeds() -> i32 { 96 }
fn def_vf_line_steps() -> i32 { 160 }
fn def_vf_line_ds() -> f32 { 0.15 }
fn def_vf_flow_speed() -> f32 { 1.0 }
fn def_vf_line_thickness() -> f32 { 0.06 }
// Acoustic-field generator (#325) — mirror the params defaults.
fn def_ac_source() -> u32 { 1 } // Dipole
fn def_ac_k() -> f32 { 1.5 }
fn def_ac_near() -> f32 { 0.5 }
fn def_ac_amp() -> f32 { 1.5 }
fn def_ac_separation() -> f32 { 1.5 }
fn def_ac_rmin() -> f32 { 0.3 }
fn def_ac_rings() -> i32 { 5 }
fn def_ac_spokes() -> i32 { 24 }
fn def_ac_samples() -> i32 { 48 }
fn def_ac_raylen() -> f32 { 8.0 }
fn def_ac_spread() -> f32 { 180.0 }
fn def_ac_thickness() -> f32 { 0.1 }
fn def_ac_aura_blend() -> f32 { 1.0 } // 1 = velocity particle channel
// #381 Tier 1 Field Engine — mirror the params defaults.
fn def_field_scale() -> f32 { 1.0 }
fn def_field_extent() -> f32 { 6.0 }
fn def_field_ab() -> f32 { 1.0 } // a / b / gain all default 1
fn def_field_density() -> i32 { 12 }
fn def_field_thickness() -> f32 { 0.12 }
fn def_sim_feed() -> f32 { 0.037 }
fn def_sim_kill() -> f32 { 0.06 }
fn def_sim_res() -> i32 { 64 }
// #339 Duo-Field synthesis — mirror the params defaults.
fn def_sn_gain() -> f32 { 0.6 }
fn def_sn_mix() -> f32 { 1.0 }
// #346 Field Chamber preset defaults (mirror the param defaults so old presets that
// predate the chamber load with sensible values — panels_on defaults false, so an old
// preset never draws them regardless).
fn def_ch_rear() -> bool { true }
fn def_ch_right() -> bool { true }
fn def_ch_opacity() -> f32 { 0.85 }
fn def_ch_fill() -> f32 { 0.9 }
fn def_ch_scope_amp() -> f32 { 1.0 }
fn def_ch_db_floor() -> f32 { -60.0 }
fn def_ch_db_top() -> f32 { 0.0 }
fn def_ch_metallic() -> f32 { 0.8 }
fn def_ch_roughness() -> f32 { 0.25 }
fn def_ch_thickness() -> f32 { 0.03 }
fn def_ch_scope_time_ms() -> f32 { 20.0 }
fn def_ch_scope_trigger() -> i32 { 1 }
fn def_ch_scope_channel() -> i32 { 2 }
fn def_sn_gen_freq() -> f32 { 110.0 } // tuning + lens pivot default (A2 ≈ 110 Hz)
fn def_sn_gen_amp() -> f32 { 1.0 }
fn def_sn_bank() -> i32 { 32 }
fn def_sn_tune_spread() -> f32 { 0.25 }
fn def_sn_shell_r() -> f32 { 2.5 }
fn def_sn_shell_rate() -> f32 { 1.0 }
fn def_sn_t60() -> f32 { 1.5 }
fn def_sn_bright() -> f32 { 0.5 }
fn def_sn_grain_size() -> f32 { 0.06 }
fn def_sn_grain_density() -> f32 { 0.5 }
fn def_sn_attack() -> f32 { 0.01 }
fn def_sn_decay() -> f32 { 0.2 }
fn def_sn_sustain() -> f32 { 0.7 }
fn def_sn_release() -> f32 { 0.4 }
fn def_sn_bend_range() -> f32 { 2.0 }
fn def_sn_a4() -> f32 { 440.0 }
fn def_sn_probe_lx() -> f32 { -0.09 }
fn def_sn_probe_lz() -> f32 { 1.2 }
fn def_sn_probe_rx() -> f32 { 0.09 }
fn def_sn_vis_anchor() -> f32 { 0.5 }
fn def_sn_vis_slope() -> f32 { 0.34 }
fn def_sn_vis_k_anchor() -> f32 { 1.5 }
fn def_ac2_nx() -> i32 { 2 }
fn def_ac2_ny() -> i32 { 2 }
fn def_ac2_nz() -> i32 { 1 }
fn def_ac2_cav_scale() -> f32 { 8.0 }
fn def_ac2_tween() -> f32 { 0.6 }
fn def_an_target_lufs() -> f32 { -14.0 }
fn def_an_floor_lufs() -> f32 { -50.0 }
fn def_an_tp_ceiling() -> f32 { -1.0 }
fn def_col_lo_db() -> f32 { -60.0 }
fn def_fv_line_density() -> f32 { 160.0 }
fn def_fv_line_thickness() -> f32 { 0.09 }
fn def_vb_func_square() -> u32 { 3 }
fn def_vb_func_sin() -> u32 { 6 }
fn def_neg_one() -> f32 { -1.0 }
fn def_phyl_count() -> i32 { 1500 }
fn def_phyl_divergence() -> f32 { 137.50776 }
fn def_phyl_parastichy() -> i32 { 21 }
fn def_phyl_height() -> f32 { 8.0 }
fn def_phyl_growth() -> f32 { 2.0 }
fn def_phyl_breathe_freq() -> f32 { 0.5 }
fn def_phyl_rot() -> f32 { 0.3 }
fn def_phyl_thickness() -> f32 { 0.1 }
fn def_tess_depth() -> i32 { 4 }
fn def_tess_scale() -> f32 { 8.0 }
fn def_tess_thickness() -> f32 { 0.06 }
fn def_tess_view() -> u32 { 2 } // Extruded
fn def_tess_height() -> f32 { 0.25 }
fn def_tess_ripple_freq() -> f32 { 2.0 }
fn def_tess_grid_n() -> i32 { 6 }
fn def_tess_hyp_p() -> i32 { 7 }
fn def_tess_hyp_q() -> i32 { 3 }

fn def_mb_power() -> f32 { 8.0 }
fn def_mb_iter() -> i32 { 8 }
fn def_mb_scale() -> f32 { 150.0 }
fn def_mb_detail() -> i32 { 96 }
fn def_mb_spin() -> f32 { 1.0 }
fn def_mb_color() -> f32 { 1.0 }
fn def_mb_bailout() -> f32 { 2.0 }
fn def_cr_scale() -> f32 { 120.0 }
fn def_cr_detail() -> i32 { 128 }
fn def_cr_swim() -> f32 { 1.0 }
fn def_cr_warp_amp() -> f32 { 0.06 }
fn def_cr_warp_freq() -> f32 { 4.0 }
fn def_cr_rim() -> f32 { 0.6 }
fn def_cr_glow() -> f32 { 1.0 }
fn def_cr_wave_speed() -> f32 { 1.0 }
fn def_cr_wave_freq() -> f32 { 3.0 }
fn def_cr_wave_sharp() -> f32 { 2.5 }
fn def_cr_overlay_opacity() -> f32 { 1.0 }
fn def_cr_overlay_bright() -> f32 { 1.0 }
fn def_ms_scale() -> f32 { 150.0 }
fn def_ms_cells() -> f32 { 6.0 }
fn def_ms_thickness() -> f32 { 0.06 }
fn def_ms_detail() -> i32 { 160 }
fn def_ms_color() -> f32 { 1.0 }
fn def_ms_uv_res() -> i32 { 96 }
fn def_ms_extent() -> f32 { 2.0 }
fn def_ms_turns() -> f32 { 1.0 }
fn def_ms_form_res() -> f32 { 0.5 }
fn def_lens_focal() -> f32 { 1.0 }
fn def_lens_aperture() -> f32 { 0.6 }
fn def_lens_thickness() -> f32 { 0.25 }
fn def_lens_scale() -> f32 { 150.0 }
fn def_lens_detail() -> i32 { 128 }

fn def_kf_sectors() -> f32 { 12.0 }
fn def_kf_fold() -> f32 { 0.65 }
fn def_kf_iter() -> i32 { 6 }
fn def_kf_spin() -> f32 { 1.0 }
fn def_kf_breathe() -> f32 { 0.25 }
fn def_kf_zoom() -> f32 { 1.2 }
fn def_kf_rays() -> i32 { 18 }
fn def_kf_ring() -> f32 { 1.0 }
fn def_kf_glow() -> f32 { 1.0 }
fn def_kf_color_speed() -> f32 { 0.08 }
fn def_kf_flow() -> f32 { 0.004 }
fn def_kf_petals() -> i32 { 6 }
fn def_kf_sharp() -> f32 { 0.5 }
fn def_kf_relief() -> f32 { 0.5 }
fn def_kf_relief_elev() -> f32 { 1.25 }
fn def_kf_relief_steps() -> i32 { 96 }
fn def_kf_relief_shine() -> f32 { 0.5 }
fn def_kal_sectors() -> f32 { 6.0 }
fn def_kal_spin() -> f32 { 0.1 }
fn def_kal_unit() -> f32 { 1.0 }
fn def_kal_seam() -> f32 { 0.5 }
fn def_instr_probe_x() -> f32 { 2.0 }
fn def_instr_ledger_half() -> f32 { 4.0 }
fn def_instr_ledger_res() -> f32 { 12.0 }
fn def_instr_flux_size() -> f32 { 2.0 }
fn def_instr_flux_res() -> f32 { 16.0 }
fn def_instr_panel_opacity() -> f32 { 0.55 }
fn def_instr_panel_bevel() -> f32 { 0.35 }
fn def_instr_hud_scale() -> f32 { 1.0 }
fn def_fdtd_res() -> f32 { 64.0 }
fn def_fdtd_freq() -> f32 { 8.0 }
fn def_fdtd_drive() -> f32 { 1.0 }
fn def_fdtd_substeps() -> f32 { 4.0 }
fn def_fdtd_boundary() -> f32 { 8.0 }
fn def_fdtd_extent() -> f32 { 12.0 }

fn def_half() -> f32 { 0.5 }
fn def_fx_style_amt() -> f32 { 4.0 }
fn def_fx_dof_range() -> f32 { 0.25 }
fn def_fx_outline() -> f32 { 0.15 }
fn def_volume_radius() -> f32 { 1.5 }
fn def_volume_emission() -> f32 { 1.5 }
fn def_volume_absorption() -> f32 { 0.6 }
fn def_volume_steps() -> i32 { 96 }
fn def_metaball_radius() -> f32 { 1.3 }
fn def_metaball_threshold() -> f32 { 0.6 }
fn def_splat_radius() -> f32 { 0.55 }
fn def_splat_opacity() -> f32 { 0.85 }
fn def_splat_cutoff() -> f32 { 0.003 }
fn def_one_i32() -> i32 { 1 }
fn def_splat_jitter() -> f32 { 0.35 }
fn def_ma_ab() -> f32 { 1.5 }
fn def_ma_points_k() -> i32 { 60 }
fn def_ma_warmup() -> i32 { 50 }
fn def_ma_scale() -> f32 { 12.0 }
fn def_ma_size() -> f32 { 0.08 }
// #380 Tier 2 parameter-orbit defaults (mirror params.rs).
fn def_ma_orbit_mode() -> u32 { 1 } // Linear (Tier-1-compatible)
fn def_ma_loop_beats() -> f32 { 16.0 }
fn def_ma_orbit_free() -> f32 { 0.05 }
fn def_two_i32() -> i32 { 2 }
fn def_frac_pi_2() -> f32 { std::f32::consts::FRAC_PI_2 }
fn def_one_u32() -> u32 { 1 }
fn def_voxel_res() -> f32 { 96.0 }
fn def_voxel_threshold() -> f32 { 0.5 }
fn def_voxel_shadow() -> f32 { 0.6 }
fn def_voxel_gi_distance() -> f32 { 0.5 }
fn def_voxel_gi_sky() -> f32 { 0.2 }
fn def_ripple_speed() -> f32 { 0.3 }
fn def_ripple_sharp() -> f32 { 2.0 }
fn def_rd_feed() -> f32 { 0.037 }
fn def_rd_kill() -> f32 { 0.06 }
fn def_rd_scale() -> f32 { 0.02 }

fn def_particles_count_k() -> i32 { 200 }
fn def_particles_grid_res() -> i32 { 32 }
fn def_particles_speed() -> f32 { 1.0 }
fn def_particles_lifetime() -> f32 { 3.0 }
fn def_particles_spawn_radius() -> f32 { 0.6 }
fn def_particles_size() -> f32 { 0.06 }
fn def_particles_emissive() -> f32 { 1.0 }
fn def_particles_ribbon_stretch() -> f32 { 0.5 }
fn def_particles_metallic() -> f32 { 0.9 }
fn def_particles_roughness() -> f32 { 0.2 }
fn def_particles_ior() -> f32 { 1.45 }
fn def_particles_shape_param() -> f32 { 0.5 }
fn def_particles_beat_burst() -> f32 { 0.3 }
fn def_particles_drag() -> f32 { 0.1 }
fn def_particles_turbulence() -> f32 { 0.2 }
fn def_particles_alpha() -> f32 { 1.0 }
fn def_mn_knee() -> f32 { 4.0 }
fn def_mn_hue() -> f32 { 0.08 }
fn def_mn_antenna_len() -> f32 { 6.0 }
fn def_ad_floor() -> f32 { 0.1 }
fn def_stir_rate() -> f32 { 0.3 }
fn def_pump_scale() -> f32 { 3.0 }
fn def_swirl_decay() -> f32 { 1.5 }
fn def_ring_freq() -> f32 { 2.0 }
fn def_mode_mix() -> f32 { -1.0 }
fn def_ad_spread() -> f32 { 0.25 }
fn def_ad_band_hue() -> f32 { 0.7 }
fn def_fluid_force() -> f32 { 12.0 }
fn def_fluid_vorticity() -> f32 { 6.0 }
fn def_fluid_dissipation() -> f32 { 0.4 }
fn def_fluid_iters() -> i32 { 24 }
fn def_fluid_inflow_decay() -> f32 { 1.5 }
// Fluid Ink (#182 Tier 1) — match params.rs / ipc::Shared defaults.
fn def_ink_rate() -> f32 { 2.0 }
fn def_ink_radius() -> f32 { 1.5 }
fn def_ink_extinction() -> f32 { 4.0 }
fn def_ink_emissive() -> f32 { 0.6 }
fn def_ink_anisotropy() -> f32 { 0.45 }
fn def_ink_dissipation() -> f32 { 0.15 }
fn def_ink_steps() -> f32 { 96.0 }
fn def_ink_reveal() -> f32 { 0.3 }
// Fluid medium Tier 2 (#182) — match params.rs / ipc::Shared defaults.
fn def_fl2_heat_decay() -> f32 { 0.3 }
fn def_fl2_substeps() -> i32 { 1 }
// MLS-MPM liquid (#182 Tier 3a) — match params.rs / ipc::Shared defaults.
fn def_liq_count() -> i32 { 100 }
fn def_liq_res() -> i32 { 64 }
fn def_liq_stiffness() -> f32 { 3.0 }
fn def_liq_roughness() -> f32 { 0.05 }
fn def_liq_ior() -> f32 { 1.33 }
fn def_liq_absorb() -> f32 { 0.5 }
fn def_liq_viscosity() -> f32 { 0.05 }
fn def_liq_container() -> f32 { 10.0 }
fn def_liq_threshold() -> f32 { 0.35 }
fn def_liq_hue() -> f32 { 0.55 }
fn def_liq_sat() -> f32 { 0.35 }
fn def_liq_substeps() -> i32 { 2 }

// Z0NE rails (#187, Tier 1) — mirror the rl_* param defaults.
fn def_rl_speed() -> f32 { 8.0 }
fn def_rl_bore() -> f32 { 6.0 }
fn def_rl_cell_len() -> u32 { 3 } // RailCellLen::TwoBars (8 beats)
fn def_rl_ring_n() -> i32 { 36 }
fn def_rl_rows_beat() -> i32 { 4 }
fn def_rl_horizon() -> f32 { 24.0 }
fn def_rl_rib_gain() -> f32 { 0.6 }
fn def_rl_lobes() -> i32 { 8 }
fn def_rl_twist() -> f32 { 0.1 }
fn def_rl_swell() -> f32 { 0.3 }
fn def_rl_fade() -> f32 { 6.0 }
fn def_rl_color_flow() -> f32 { 0.05 }
fn def_rl_diverge() -> f32 { 137.50776 }
fn def_rl_shells() -> i32 { 2 }
fn def_rl_parastichy() -> i32 { 13 }
fn def_rl_change_every() -> u32 { 3 } // RailChangeEvery::EightBars (32 beats)
// Scenery (#187 pivot) — mirror the sc_* param defaults.
fn def_sc_roughness() -> f32 { 0.35 }
fn def_sc_glow() -> f32 { 0.2 }
fn def_sc_ior() -> f32 { 1.45 }
fn def_sc_sss_dist() -> f32 { 0.3 }
fn def_sc_sss_pow() -> f32 { 4.0 }
fn def_sc_irid_scale() -> f32 { 2.0 }
// Terra (#206 Tier 2) — mirror the terra_* param defaults.
fn def_terra_ridge() -> f32 { 2.0 }
fn def_terra_width() -> f32 { 3.0 }
fn def_terra_steep() -> f32 { 0.7 }
fn def_terra_rough() -> f32 { 0.4 }
fn def_terra_meander() -> f32 { 0.6 }
fn def_terra_water_level() -> f32 { -1.0 }
fn def_terra_clearance() -> f32 { 1.5 }
fn def_terra_noise_freq() -> f32 { 0.15 }
fn def_wt_mat() -> u32 { 2 } // Glass
fn def_wt_roughness() -> f32 { 0.06 }
fn def_wt_ior() -> f32 { 1.33 }
fn def_wt_opacity() -> f32 { 0.7 }
fn def_wt_ripple() -> f32 { 0.15 }
fn def_wt_ripple_freq() -> f32 { 0.6 }
fn def_wt_absorb() -> f32 { 1.4 }
fn def_wt_glitter() -> f32 { 0.6 }
fn def_wt_reflect() -> f32 { 0.15 }

fn def_true() -> bool { true }
fn def_bg_tonemap() -> u32 { 1 } // AgX
fn def_audio_attack() -> f32 { 10.0 }
fn def_audio_release() -> f32 { 200.0 }
fn def_speed_pulse_attack() -> f32 { 5.0 }
fn def_speed_pulse_decay() -> f32 { 350.0 }
fn def_breath_attack() -> f32 { 8.0 }
fn def_breath_decay() -> f32 { 400.0 }
fn def_sss_distortion() -> f32 { 0.3 }
fn def_sss_power() -> f32 { 4.0 }
fn def_irid_scale() -> f32 { 2.0 }
fn def_ssao_radius() -> f32 { 1.5 }
fn def_ssao_bias() -> f32 { 0.025 }
fn def_ssr_max_rough() -> f32 { 0.4 }
fn def_ssr_thickness() -> f32 { 0.5 }
fn def_ssr_steps() -> i32 { 48 }
fn def_ssgi_radius() -> f32 { 2.0 }
fn def_ssgi_rays() -> i32 { 4 }
fn def_cam_speed() -> f32 { 0.05 }
fn def_cam_kick() -> f32 { 0.08 }
fn def_cam_damping() -> f32 { 0.4 }
fn def_bar8() -> u32 { 3 }
fn def_dolly_period() -> f32 { 4.0 }
fn def_beats_per_bar() -> i32 { 4 }
fn def_fov() -> f32 { 45.0 }
fn def_bar4() -> u32 { 2 }
fn def_story_count() -> i32 { 4 }
fn def_story_seed() -> i32 { 1 }
fn def_shot0_path() -> u32 { 1 } // HCircle
fn def_shot1_path() -> u32 { 4 } // Spiral
fn def_shot2_path() -> u32 { 3 } // Figure-8
fn def_shot3_path() -> u32 { 5 } // Boom
fn def_shot1_radius() -> f32 { 1.3 }
fn def_shot2_radius() -> f32 { 0.8 }
fn def_shot3_radius() -> f32 { 1.5 }
fn def_metallic() -> f32 { 0.0 }
fn def_ior() -> f32 { 1.45 }
fn def_roughness() -> f32 { 0.35 }
fn def_plexus_radius() -> f32 { 1.6 }
fn def_plexus_links() -> f32 { 8.0 }
fn def_plexus_strut() -> f32 { 0.07 }
fn def_plexus_marker() -> f32 { 0.24 }
fn def_plexus_node_radius() -> f32 { 0.35 }
fn def_plexus_edge_radius() -> f32 { 0.09 }
fn def_plexus_node_metallic() -> f32 { 0.1 }
fn def_plexus_node_rough() -> f32 { 0.4 }
fn def_plexus_node_emissive() -> f32 { 0.6 }
fn def_plexus_edge_rough() -> f32 { 0.6 }
fn def_plexus_edge_hue() -> f32 { 0.58 }
fn def_plexus_edge_sat() -> f32 { 0.4 }
fn def_plexus_edge_emissive() -> f32 { 0.3 }
fn def_plexus_signal_gain() -> f32 { 1.5 }
fn def_plexus_signal_width() -> f32 { 0.18 }
fn def_plexus_shell_scale() -> f32 { 1.15 }
fn def_plexus_shell_depth() -> f32 { 0.2 }
fn def_plexus_shell_bins() -> f32 { 12.0 }
fn def_one() -> f32 { 1.0 }
// #472 Tier 2 procedural-layer serde defaults (match ipc::Shared::default()).
fn def_two() -> f32 { 2.0 }
fn def_three() -> u32 { 3 }
fn def_six() -> f32 { 6.0 }
fn def_four_i32() -> i32 { 4 }
fn def_anim_speed() -> f32 { 0.1 }
fn def_four() -> f32 { 4.0 }
fn def_five_i() -> i32 { 5 }
fn def_grad_lo_r() -> f32 { 0.04 }
fn def_grad_lo_b() -> f32 { 0.05 }
fn def_grad_hi_r() -> f32 { 0.80 }
fn def_grad_hi_g() -> f32 { 0.76 }
fn def_grad_hi_b() -> f32 { 0.70 }
fn def_cloud_cover() -> f32 { 0.55 }
fn def_cloud_speed() -> f32 { 0.08 }
fn def_cloud_strength() -> f32 { 0.7 }
// Neural radiance cache — live (#256 Tier 0) preset defaults.
fn def_nrc_confidence() -> f32 { 0.5 }
fn def_nrc_learn_rate() -> f32 { 0.02 }
fn def_nrc_omega() -> f32 { 4.0 }
fn def_nrc_terminate() -> i32 { 2 }
fn def_nrc_samples() -> i32 { 8 }
fn def_nrc_seed() -> i32 { 1 }
fn def_nrc_guide_candidates() -> i32 { 4 }
fn def_nrc_firefly_clamp() -> f32 { 8.0 }
fn def_nrc_gi_strength() -> f32 { 1.0 }
fn def_nrc_volume_density() -> f32 { 0.15 }
fn def_nrc_volume_steps() -> i32 { 16 }
fn def_nrc_volume_strength() -> f32 { 1.0 }
fn def_nrc_caustic_gain() -> f32 { 1.0 }
fn def_clearcoat_rough() -> f32 { 0.1 }
fn def_sheen_rough() -> f32 { 0.3 }
fn def_refract_dist() -> f32 { 0.5 }
fn def_glitter_density() -> f32 { 12.0 }
fn def_glitter_sharpness() -> f32 { 0.6 }
fn def_diffraction_freq() -> f32 { 8.0 }
fn def_fluor_hue() -> f32 { 0.33 }
fn def_temperature() -> f32 { 3000.0 }
fn def_bloom() -> f32 { 0.08 }
fn def_tint_hue() -> f32 { 40.0 }

/// Single source of truth for the per-tab preset partition (#145): every
/// captured `PresetValues` field tagged with the editor tab it belongs to.
/// Drives both `apply_tab` (partial recall) and the `tab_field_list` drift
/// guard, so the recall partition and the coverage test cannot drift apart.
/// `$op!` is invoked once per field as `(tab, field, scalar)` or
/// `(tab, field, enum, EnumType)`; per-display/quality fields (HDR/MSAA/
/// resolution/terrain/…) are absent here exactly as they are absent from
/// `apply` — so tab presets honour the same exclusions automatically.
macro_rules! for_each_tab_field {
    ($op:ident) => {
        // ---- Generator (321) ----
        $op!(Generator, generator, enum, HostGeneratorMode);
        $op!(Generator, loop_count_x, scalar);
        $op!(Generator, loop_count_y, scalar);
        $op!(Generator, loop_count_z, scalar);
        $op!(Generator, loop_count_q, scalar);
        $op!(Generator, rot_func, enum, HostFuncName);
        $op!(Generator, rot_amp_x, scalar);
        $op!(Generator, rot_amp_y, scalar);
        $op!(Generator, rot_amp_z, scalar);
        $op!(Generator, rot_mod_x, scalar);
        $op!(Generator, rot_mod_y, scalar);
        $op!(Generator, rot_mod_z, scalar);
        $op!(Generator, continuous, scalar);
        $op!(Generator, cont_shape, scalar);
        $op!(Generator, trans_func, enum, HostFuncName);
        $op!(Generator, trans_amp_x, scalar);
        $op!(Generator, trans_amp_y, scalar);
        $op!(Generator, trans_amp_z, scalar);
        $op!(Generator, trans_mod_x, scalar);
        $op!(Generator, trans_mod_y, scalar);
        $op!(Generator, trans_mod_z, scalar);
        $op!(Generator, scale_func, enum, HostFuncName);
        $op!(Generator, scale_amp, scalar);
        $op!(Generator, frenet_strands, scalar);
        $op!(Generator, frenet_nodes, scalar);
        $op!(Generator, frenet_step, scalar);
        $op!(Generator, frenet_func, enum, HostFuncName);
        $op!(Generator, frenet_kappa, scalar);
        $op!(Generator, frenet_kappa_amp, scalar);
        $op!(Generator, frenet_kappa_freq, scalar);
        $op!(Generator, frenet_tau, scalar);
        $op!(Generator, frenet_tau_amp, scalar);
        $op!(Generator, frenet_tau_freq, scalar);
        $op!(Generator, frenet_spread, scalar);
        $op!(Generator, frenet_thickness, scalar);
        $op!(Generator, dna_form, enum, DnaForm);
        $op!(Generator, dna_bp, scalar);
        $op!(Generator, dna_bp_per_turn, scalar);
        $op!(Generator, dna_rise, scalar);
        $op!(Generator, dna_radius, scalar);
        $op!(Generator, dna_groove, scalar);
        $op!(Generator, dna_left, scalar);
        $op!(Generator, dna_sigma, scalar);
        $op!(Generator, dna_super_radius, scalar);
        $op!(Generator, dna_seed, scalar);
        $op!(Generator, dna_thickness, scalar);
        $op!(Generator, dna_twist_breathe, scalar);
        $op!(Generator, attr_field, enum, AttractorField);
        $op!(Generator, attr_seeds, scalar);
        $op!(Generator, attr_seed, scalar);
        $op!(Generator, attr_spread, scalar);
        $op!(Generator, attr_dt, scalar);
        $op!(Generator, attr_trail, scalar);
        $op!(Generator, attr_speed, scalar);
        $op!(Generator, attr_scale, scalar);
        $op!(Generator, attr_thickness, scalar);
        $op!(Generator, boids_count, scalar);
        $op!(Generator, boids_perception, scalar);
        $op!(Generator, boids_separation, scalar);
        $op!(Generator, boids_sep, scalar);
        $op!(Generator, boids_align, scalar);
        $op!(Generator, boids_cohere, scalar);
        $op!(Generator, boids_max_speed, scalar);
        $op!(Generator, boids_max_force, scalar);
        $op!(Generator, boids_trail, scalar);
        $op!(Generator, boids_bounds, scalar);
        $op!(Generator, boids_goal, scalar);
        $op!(Generator, boids_thickness, scalar);
        $op!(Generator, boids_seed, scalar);
        $op!(Generator, boids_speed, scalar);
        $op!(Generator, boids_scale, scalar);
        $op!(Generator, boids_form, enum, HostBoidsForm);
        $op!(Generator, boids_size, scalar);
        $op!(Generator, boids_bank, scalar);
        $op!(Generator, harm_mode0, scalar);
        $op!(Generator, harm_amp0, scalar);
        $op!(Generator, harm_freq0, scalar);
        $op!(Generator, harm_mode1, scalar);
        $op!(Generator, harm_amp1, scalar);
        $op!(Generator, harm_freq1, scalar);
        $op!(Generator, harm_mode2, scalar);
        $op!(Generator, harm_amp2, scalar);
        $op!(Generator, harm_freq2, scalar);
        $op!(Generator, harm_radius, scalar);
        $op!(Generator, harm_theta, scalar);
        $op!(Generator, harm_phi, scalar);
        $op!(Generator, harm_thickness, scalar);
        $op!(Generator, bell_physical, scalar);
        $op!(Generator, bell_stroke_depth, scalar);
        $op!(Generator, bell_stiffness, scalar);
        $op!(Generator, bell_damping, scalar);
        $op!(Generator, bell_open, scalar);
        $op!(Generator, bell_speed, scalar);
        $op!(Generator, ls_system, enum, LSystem);
        $op!(Generator, ls_depth, scalar);
        $op!(Generator, ls_angle, scalar);
        $op!(Generator, ls_step, scalar);
        $op!(Generator, ls_sway_amp, scalar);
        $op!(Generator, ls_sway_freq, scalar);
        $op!(Generator, ls_grow, scalar);
        $op!(Generator, ls_thickness, scalar);
        $op!(Generator, cn_seeds, scalar);
        $op!(Generator, cn_seed, scalar);
        $op!(Generator, cn_spread, scalar);
        $op!(Generator, cn_scale, scalar);
        $op!(Generator, cn_steps, scalar);
        $op!(Generator, cn_dt, scalar);
        $op!(Generator, cn_flow, scalar);
        $op!(Generator, cn_bound, scalar);
        $op!(Generator, cn_thickness, scalar);
        $op!(Generator, pol_rings, scalar);
        $op!(Generator, pol_spokes, scalar);
        $op!(Generator, pol_samples, scalar);
        $op!(Generator, pol_len, scalar);
        $op!(Generator, pol_k, scalar);
        $op!(Generator, pol_amp, scalar);
        $op!(Generator, pol_falloff, scalar);
        $op!(Generator, pol_handed, scalar);
        $op!(Generator, pol_spread, scalar);
        $op!(Generator, pol_swirl, scalar);
        $op!(Generator, pol_show_b, scalar);
        $op!(Generator, pol_thickness, scalar);
        $op!(Generator, mx_lines, scalar);
        $op!(Generator, mx_gen_blend, scalar);
        $op!(Generator, mx_dipoles, scalar);
        $op!(Generator, mx_sources, scalar);
        $op!(Generator, mx_separation, scalar);
        $op!(Generator, mx_phase, scalar);
        $op!(Generator, mx_swirl, scalar);
        $op!(Generator, mx_near, scalar);
        $op!(Generator, mx_k, scalar);
        $op!(Generator, mx_amp, scalar);
        $op!(Generator, mx_rmin, scalar);
        $op!(Generator, mx_thickness, scalar);
        $op!(Generator, mx_rings, scalar);
        $op!(Generator, mx_spokes, scalar);
        $op!(Generator, mx_samples, scalar);
        $op!(Generator, mx_raylen, scalar);
        $op!(Generator, mx_spread, scalar);
        $op!(Generator, mx_seeds, scalar);
        $op!(Generator, mx_steps, scalar);
        $op!(Generator, mx_ds, scalar);
        $op!(Generator, mx_bound, scalar);
        $op!(Generator, mx_norm_field, scalar);
        $op!(Generator, mx_osc_sync, scalar);
        $op!(Generator, mx_osc_div, enum, HostOscDivision);
        $op!(Generator, mx_eb_phase, scalar);
        // #412 Tier 3 Phase 0 FDTD solver (captured Generator). Bool uses `scalar`.
        $op!(Generator, fdtd_on, scalar);
        $op!(Generator, fdtd_res, scalar);
        $op!(Generator, fdtd_source, enum, FdtdSource);
        $op!(Generator, fdtd_freq, scalar);
        $op!(Generator, fdtd_drive, scalar);
        $op!(Generator, fdtd_substeps, scalar);
        $op!(Generator, fdtd_boundary, scalar);
        $op!(Generator, fdtd_extent, scalar);
        $op!(Generator, ac_source, enum, AcousticSource);
        $op!(Generator, ac_k, scalar);
        $op!(Generator, ac_near, scalar);
        $op!(Generator, ac_amp, scalar);
        $op!(Generator, ac_separation, scalar);
        $op!(Generator, ac_rmin, scalar);
        $op!(Generator, ac_blend, scalar);
        $op!(Generator, ac_norm_field, scalar);
        $op!(Generator, ac_rings, scalar);
        $op!(Generator, ac_spokes, scalar);
        $op!(Generator, ac_samples, scalar);
        $op!(Generator, ac_raylen, scalar);
        $op!(Generator, ac_spread, scalar);
        $op!(Generator, ac_thickness, scalar);
        $op!(Generator, ac_aura_blend, scalar);
        $op!(Generator, ac_beat_pump, scalar);
        // #381 Tier 1 Field Engine (the program text rides the sidecar, like the
        // connectome; only the live coefficients are preset-captured).
        $op!(Generator, field_kind, enum, FieldKind);
        $op!(Generator, field_preset, enum, FieldPreset);
        $op!(Generator, field_scale, scalar);
        $op!(Generator, field_extent, scalar);
        $op!(Generator, field_a, scalar);
        $op!(Generator, field_b, scalar);
        $op!(Generator, field_density, scalar);
        $op!(Generator, field_gain, scalar);
        $op!(Generator, field_thickness, scalar);
        // #381 Tier 3 Field Engine PDE dynamics (Generator block).
        $op!(Generator, pde_preset, enum, PdePreset);
        $op!(Generator, sim_diffusion, scalar);
        $op!(Generator, sim_time_scale, scalar);
        $op!(Generator, sim_feed, scalar);
        $op!(Generator, sim_kill, scalar);
        $op!(Generator, sim_potential, scalar);
        $op!(Generator, sim_forcing, scalar);
        $op!(Generator, sim_res, scalar);
        // #346 Field Chamber — the analyzer panels are a Look preset.
        $op!(Look, panels_on, scalar);
        $op!(Look, panel_style, enum, PanelStyle);
        $op!(Look, panel_rear, scalar);
        $op!(Look, panel_right, scalar);
        $op!(Look, panel_opacity, scalar);
        $op!(Look, panel_fill, scalar);
        $op!(Look, panel_scope_amp, scalar);
        $op!(Look, panel_db_floor, scalar);
        $op!(Look, panel_db_top, scalar);
        $op!(Look, panel_material, enum, MaterialType);
        $op!(Look, panel_metallic, scalar);
        $op!(Look, panel_roughness, scalar);
        $op!(Look, panel_emissive, scalar);
        $op!(Look, panel_thickness, scalar);
        $op!(Look, panel_wall_rel, scalar);
        $op!(Look, panel_scope_time_ms, scalar);
        $op!(Look, panel_scope_trigger, scalar);
        $op!(Look, panel_scope_channel, scalar);
        // #339 Duo-Field synthesis — a playable patch is a Look (Sound) preset.
        $op!(Synth, sn_on, scalar);
        $op!(Synth, sn_play_mode, enum, SynthPlayMode);
        $op!(Synth, sn_gain, scalar);
        $op!(Synth, sn_mix, scalar);
        $op!(Synth, sn_tuning, scalar);
        $op!(Synth, sn_gen_amp, scalar);
        $op!(Synth, sn_mode, enum, SynthMode);
        $op!(Synth, sn_bank, scalar);
        $op!(Synth, sn_tuning_layout, enum, TuningLayout);
        $op!(Synth, sn_tune_spread, scalar);
        $op!(Synth, sn_tune_stretch, scalar);
        $op!(Synth, sn_shell_r, scalar);
        $op!(Synth, sn_shell_rate, scalar);
        $op!(Synth, sn_t60, scalar);
        $op!(Synth, sn_bright, scalar);
        $op!(Synth, sn_grain_size, scalar);
        $op!(Synth, sn_grain_density, scalar);
        $op!(Synth, sn_attack, scalar);
        $op!(Synth, sn_decay, scalar);
        $op!(Synth, sn_sustain, scalar);
        $op!(Synth, sn_release, scalar);
        $op!(Synth, sn_glide, scalar);
        $op!(Synth, sn_bend_range, scalar);
        $op!(Synth, sn_place_spread, scalar);
        $op!(Synth, sn_a4, scalar);
        $op!(Synth, sn_probe_lx, scalar);
        $op!(Synth, sn_probe_ly, scalar);
        $op!(Synth, sn_probe_lz, scalar);
        $op!(Synth, sn_probe_rx, scalar);
        $op!(Synth, sn_probe_ry, scalar);
        $op!(Synth, sn_probe_rz, scalar);
        $op!(Synth, sn_probe_cam, scalar);
        $op!(Synth, sn_vis_pivot, scalar);
        $op!(Synth, sn_vis_anchor, scalar);
        $op!(Synth, sn_vis_slope, scalar);
        $op!(Synth, sn_vis_k_anchor, scalar);
        $op!(Synth, sn_vis_k_slope, scalar);
        $op!(Synth, sn_vis_quantize, enum, SynthQuantize);
        $op!(Generator, ac2_model, enum, AcousticModel);
        $op!(Generator, ac2_nx, scalar);
        $op!(Generator, ac2_ny, scalar);
        $op!(Generator, ac2_nz, scalar);
        $op!(Generator, ac2_morph, scalar);
        $op!(Generator, ac2_cav_scale, scalar);
        $op!(Generator, ac2_intensity, scalar);
        $op!(Generator, ac2_tween, scalar);
        $op!(Generator, ac2_audio_x, scalar);
        $op!(Generator, ac2_audio_y, scalar);
        $op!(Generator, ac2_audio_z, scalar);
        $op!(Audio, analytical_mode, enum, AnalyticalMode);
        $op!(Audio, an_target_lufs, scalar);
        $op!(Audio, an_floor_lufs, scalar);
        $op!(Audio, an_tp_ceiling, scalar);
        $op!(Audio, an_corr_alarm, scalar);
        $op!(Audio, an_reference_hud, scalar);
        // #348 Field Volume + #349 calibrated colour (both cross-cutting Looks).
        $op!(Look, fv_source, enum, FieldVolSource);
        $op!(Look, fv_smooth, scalar);
        $op!(Look, fv_exposure_db, scalar);
        $op!(Look, fv_calibrate, scalar);
        $op!(Look, fv_gain, scalar);
        $op!(Look, fv_lines, scalar);
        $op!(Look, fv_line_density, scalar);
        $op!(Look, fv_line_thickness, scalar);
        $op!(Look, col_mode, enum, ColourMode);
        $op!(Look, col_lo_db, scalar);
        $op!(Look, col_hi_db, scalar);
        $op!(Look, col_lut, enum, CalLut);
        $op!(Look, col_source, enum, CalColourSource);
        $op!(Look, col_amount, scalar);
        $op!(Generator, ax_count, scalar);
        $op!(Generator, ax_length, scalar);
        $op!(Generator, ax_bundle, scalar);
        $op!(Generator, ax_samples, scalar);
        $op!(Generator, ax_thickness, scalar);
        $op!(Generator, ax_node_spacing, scalar);
        $op!(Generator, ax_node_dip, scalar);
        $op!(Generator, ax_pulse_speed, scalar);
        $op!(Generator, ax_pulse_width, scalar);
        $op!(Generator, ax_stagger, scalar);
        $op!(Generator, ax_splay, scalar);
        $op!(Generator, ax_seed, scalar);
        $op!(Generator, ax_mode, enum, AxonMode);
        $op!(Generator, ax_mode_amount, scalar);
        $op!(Generator, ax_bend, scalar);
        $op!(Generator, ax_curve, scalar);
        $op!(Generator, ax_tortuosity, scalar);
        $op!(Generator, ax_dti, scalar);
        $op!(Generator, ax_dispersion, scalar);
        $op!(Generator, ax_polarization, scalar);
        $op!(Generator, nw_topology, enum, NeuralTopology);
        $op!(Generator, nw_nodes, scalar);
        $op!(Generator, nw_connectivity, scalar);
        $op!(Generator, nw_rewire, scalar);
        $op!(Generator, nw_layers, scalar);
        $op!(Generator, nw_seed, scalar);
        $op!(Generator, nw_extent, scalar);
        $op!(Generator, nw_node_size, scalar);
        $op!(Generator, nw_node_glow, scalar);
        $op!(Generator, nw_edge_thickness, scalar);
        $op!(Generator, nw_edge_bow, scalar);
        $op!(Generator, nw_edge_samples, scalar);
        $op!(Generator, nw_pulse_speed, scalar);
        $op!(Generator, nw_pulse_width, scalar);
        $op!(Generator, nw_edge_fibres, scalar);
        $op!(Generator, nw_bundle_radius, scalar);
        $op!(Generator, nw_edge_node_dip, scalar);
        $op!(Generator, nw_ranvier, scalar);
        $op!(Generator, nw_dendrite, scalar);
        $op!(Generator, nw_dendrite_count, scalar);
        $op!(Generator, nw_fire_mode, enum, NeuralFireMode);
        $op!(Generator, nw_threshold, scalar);
        $op!(Generator, nw_conduction, scalar);
        $op!(Generator, nw_refractory, scalar);
        $op!(Generator, nw_decay, scalar);
        $op!(Generator, nw_deposit, scalar);
        $op!(Generator, nw_stim_rate, scalar);
        $op!(Generator, nw_motes, scalar);
        $op!(Generator, nw_sign_colour, scalar);
        $op!(Generator, nw_sparsify, scalar);
        $op!(Generator, nw_layer_gap, scalar);
        $op!(Generator, nw_mlp_drive, scalar);
        $op!(Generator, nw_attn_layer, scalar);
        $op!(Generator, nw_attn_head, scalar);
        $op!(Generator, nw_attn_threshold, scalar);
        $op!(Generator, nw_attn_tokens, scalar);
        $op!(Generator, nw_attn_reveal, scalar);
        $op!(Generator, nw_attn_sweep, scalar);
        $op!(Generator, nw_attn_ring, scalar);
        $op!(Generator, nt_soma_size, scalar);
        $op!(Generator, nt_soma_shape, scalar);
        $op!(Generator, nt_bouton_size, scalar);
        $op!(Generator, nt_membrane_sss, scalar);
        $op!(Generator, nt_membrane_irid, scalar);
        $op!(Generator, nt_dendrite_density, scalar);
        $op!(Generator, nt_dendrite_length, scalar);
        $op!(Generator, nt_dendrite_taper, scalar);
        $op!(Generator, nt_neuron_type, enum, NeuronType);
        $op!(Generator, nt_spines, scalar);
        $op!(Generator, nt_myelin_amount, scalar);
        $op!(Generator, nt_ranvier_spacing, scalar);
        $op!(Generator, nt_sheath_scale, scalar);
        $op!(Generator, nt_synapse_cleft, scalar);
        $op!(Generator, nt_synapse_glow, scalar);
        $op!(Generator, nt_synapse_vesicles, scalar);
        $op!(Generator, nt_glia, scalar);
        $op!(Generator, nt_capillary, scalar);
        $op!(Generator, br_fold_depth, scalar);
        $op!(Generator, br_fold_freq, scalar);
        $op!(Generator, br_hemi_gap, scalar);
        $op!(Generator, br_local_k, scalar);
        $op!(Generator, br_cerebellum, scalar);
        $op!(Generator, br_assoc, scalar);
        $op!(Generator, br_callosum, scalar);
        $op!(Generator, br_subcortical, scalar);
        $op!(Generator, br_region_hi, scalar);
        $op!(Generator, br_target, scalar);
        $op!(Generator, br_stim_amount, scalar);
        $op!(Generator, br_stim_rate, scalar);
        $op!(Generator, br_signal_swell, scalar);
        // Demo scene bench (#288) — Generator tab.
        $op!(Generator, demo_scene, enum, DemoScene);
        $op!(Generator, demo_size, scalar);
        $op!(Generator, demo_objects, scalar);
        $op!(Generator, demo_static_cam, scalar);
        $op!(Generator, demo_light, scalar);
        $op!(Generator, demo_roughness, scalar);
        $op!(Generator, demo_count, scalar);
        $op!(Generator, demo_spin, scalar);
        $op!(Generator, sy_radius, scalar);
        $op!(Generator, sy_beta, scalar);
        $op!(Generator, sy_charges, scalar);
        $op!(Generator, sy_grid, scalar);
        $op!(Generator, sy_extent, scalar);
        $op!(Generator, sy_near, scalar);
        $op!(Generator, sy_amp, scalar);
        $op!(Generator, sy_thickness, scalar);
        $op!(Generator, sy_rmin, scalar);
        $op!(Generator, sy_perp, scalar);
        $op!(Generator, sy_view, enum, SyncView);
        $op!(Generator, sy_line_seeds, scalar);
        $op!(Generator, sy_line_steps, scalar);
        $op!(Generator, sy_line_ds, scalar);
        $op!(Generator, sy_line_bound, scalar);
        $op!(Generator, sy_vol_layers, scalar);
        $op!(Generator, sy_reveal, scalar);
        $op!(Generator, sy_invert, scalar);
        $op!(Generator, sy_invert_radius, scalar);
        $op!(Generator, sy_tilt, scalar);
        $op!(Generator, sy_precess, scalar);
        $op!(Generator, vf_preset, enum, VecFieldPreset);
        $op!(Generator, vf_grid_x, scalar);
        $op!(Generator, vf_grid_y, scalar);
        $op!(Generator, vf_grid_z, scalar);
        $op!(Generator, vf_extent, scalar);
        $op!(Generator, vf_field_scale, scalar);
        $op!(Generator, vf_amp, scalar);
        $op!(Generator, vf_thickness, scalar);
        $op!(Generator, vf_mag_map, enum, VecMagMap);
        $op!(Generator, vf_tint_mode, enum, VecTint);
        $op!(Generator, vf_evolve, scalar);
        $op!(Generator, vf_z_lift, scalar);
        $op!(Generator, vf_reveal, scalar);
        $op!(Generator, vf_view, enum, VecFieldView);
        $op!(Generator, vf_seed_mode, enum, VecSeedMode);
        $op!(Generator, vf_line_seeds, scalar);
        $op!(Generator, vf_line_steps, scalar);
        $op!(Generator, vf_line_ds, scalar);
        $op!(Generator, vf_bidir, scalar);
        $op!(Generator, vf_line_color, enum, VecLineColor);
        $op!(Generator, vf_flow, scalar);
        $op!(Generator, vf_flow_speed, scalar);
        $op!(Generator, vf_line_thickness, scalar);
        $op!(Generator, vb_x1_func, enum, VecTermFunc);
        $op!(Generator, vb_x1_gain, scalar);
        $op!(Generator, vb_x1_a, scalar);
        $op!(Generator, vb_x1_b, scalar);
        $op!(Generator, vb_x1_c, scalar);
        $op!(Generator, vb_x1_phase, scalar);
        $op!(Generator, vb_x2_func, enum, VecTermFunc);
        $op!(Generator, vb_x2_gain, scalar);
        $op!(Generator, vb_x2_a, scalar);
        $op!(Generator, vb_x2_b, scalar);
        $op!(Generator, vb_x2_c, scalar);
        $op!(Generator, vb_x2_phase, scalar);
        $op!(Generator, vb_x3_func, enum, VecTermFunc);
        $op!(Generator, vb_x3_gain, scalar);
        $op!(Generator, vb_x3_a, scalar);
        $op!(Generator, vb_x3_b, scalar);
        $op!(Generator, vb_x3_c, scalar);
        $op!(Generator, vb_x3_phase, scalar);
        $op!(Generator, vb_y1_func, enum, VecTermFunc);
        $op!(Generator, vb_y1_gain, scalar);
        $op!(Generator, vb_y1_a, scalar);
        $op!(Generator, vb_y1_b, scalar);
        $op!(Generator, vb_y1_c, scalar);
        $op!(Generator, vb_y1_phase, scalar);
        $op!(Generator, vb_y2_func, enum, VecTermFunc);
        $op!(Generator, vb_y2_gain, scalar);
        $op!(Generator, vb_y2_a, scalar);
        $op!(Generator, vb_y2_b, scalar);
        $op!(Generator, vb_y2_c, scalar);
        $op!(Generator, vb_y2_phase, scalar);
        $op!(Generator, vb_y3_func, enum, VecTermFunc);
        $op!(Generator, vb_y3_gain, scalar);
        $op!(Generator, vb_y3_a, scalar);
        $op!(Generator, vb_y3_b, scalar);
        $op!(Generator, vb_y3_c, scalar);
        $op!(Generator, vb_y3_phase, scalar);
        $op!(Generator, vb_z1_func, enum, VecTermFunc);
        $op!(Generator, vb_z1_gain, scalar);
        $op!(Generator, vb_z1_a, scalar);
        $op!(Generator, vb_z1_b, scalar);
        $op!(Generator, vb_z1_c, scalar);
        $op!(Generator, vb_z1_phase, scalar);
        $op!(Generator, vb_z2_func, enum, VecTermFunc);
        $op!(Generator, vb_z2_gain, scalar);
        $op!(Generator, vb_z2_a, scalar);
        $op!(Generator, vb_z2_b, scalar);
        $op!(Generator, vb_z2_c, scalar);
        $op!(Generator, vb_z2_phase, scalar);
        $op!(Generator, vb_z3_func, enum, VecTermFunc);
        $op!(Generator, vb_z3_gain, scalar);
        $op!(Generator, vb_z3_a, scalar);
        $op!(Generator, vb_z3_b, scalar);
        $op!(Generator, vb_z3_c, scalar);
        $op!(Generator, vb_z3_phase, scalar);
        $op!(Generator, vb_op, enum, VecFieldOp);
        $op!(Generator, vb_mix, scalar);
        $op!(Generator, rl_speed, scalar);
        $op!(Generator, rl_bore, scalar);
        $op!(Generator, rl_cell_len, enum, RailCellLen);
        $op!(Generator, rl_variance, scalar);
        $op!(Generator, rl_seed, scalar);
        $op!(Generator, rl_ring_n, scalar);
        $op!(Generator, rl_rows_beat, scalar);
        $op!(Generator, rl_horizon, scalar);
        $op!(Generator, rl_rib_gain, scalar);
        $op!(Generator, rl_thickness, scalar);
        $op!(Generator, rl_lobes, scalar);
        $op!(Generator, rl_spike, scalar);
        $op!(Generator, rl_twist, scalar);
        $op!(Generator, rl_swell, scalar);
        $op!(Generator, rl_fade, scalar);
        $op!(Generator, rl_color_flow, scalar);
        $op!(Generator, rl_archetype, enum, RailArchetype);
        $op!(Generator, rl_diverge, scalar);
        $op!(Generator, rl_shells, scalar);
        $op!(Generator, rl_parastichy, scalar);
        $op!(Generator, rl_change_every, enum, RailChangeEvery);
        $op!(Generator, rl_evolve, scalar);
        $op!(Generator, sc_mode, enum, SceneryMode);
        $op!(Generator, sc_surface, enum, ScenerySurface);
        $op!(Generator, sc_mat, enum, MaterialType);
        $op!(Generator, sc_metallic, scalar);
        $op!(Generator, sc_roughness, scalar);
        $op!(Generator, sc_glow, scalar);
        $op!(Generator, sc_emissive, scalar);
        $op!(Generator, sc_opacity, scalar);
        $op!(Generator, sc_ior, scalar);
        $op!(Generator, sc_palette, enum, Palette);
        $op!(Generator, sc_sss, scalar);
        $op!(Generator, sc_sss_dist, scalar);
        $op!(Generator, sc_sss_pow, scalar);
        $op!(Generator, sc_irid, scalar);
        $op!(Generator, sc_irid_scale, scalar);
        $op!(Generator, sc_irid_shift, scalar);
        $op!(Generator, terra_form, enum, TerraForm);
        $op!(Generator, terra_ridge, scalar);
        $op!(Generator, terra_channel, scalar);
        $op!(Generator, terra_width, scalar);
        $op!(Generator, terra_steep, scalar);
        $op!(Generator, terra_terrace, scalar);
        $op!(Generator, terra_rough, scalar);
        $op!(Generator, terra_meander, scalar);
        $op!(Generator, terra_water_level, scalar);
        $op!(Generator, terra_water_on, scalar);
        $op!(Generator, terra_clearance, scalar);
        $op!(Generator, terra_noise_freq, scalar);
        $op!(Generator, wt_mat, enum, MaterialType);
        $op!(Generator, wt_roughness, scalar);
        $op!(Generator, wt_ior, scalar);
        $op!(Generator, wt_opacity, scalar);
        $op!(Generator, wt_glow, scalar);
        $op!(Generator, wt_ripple, scalar);
        $op!(Generator, wt_ripple_freq, scalar);
        $op!(Generator, wt_absorb, scalar);
        $op!(Generator, wt_glitter, scalar);
        $op!(Generator, wt_reflect, scalar);
        $op!(Generator, phyl_surface, enum, PhylSurface);
        $op!(Generator, phyl_count, scalar);
        $op!(Generator, phyl_divergence, scalar);
        $op!(Generator, phyl_radius, scalar);
        $op!(Generator, phyl_parastichy, scalar);
        $op!(Generator, phyl_height, scalar);
        $op!(Generator, phyl_growth, scalar);
        $op!(Generator, phyl_breathe_amp, scalar);
        $op!(Generator, phyl_breathe_freq, scalar);
        $op!(Generator, phyl_rot, scalar);
        $op!(Generator, phyl_thickness, scalar);
        $op!(Generator, tess_family, enum, TilingFamily);
        $op!(Generator, tess_depth, scalar);
        $op!(Generator, tess_scale, scalar);
        $op!(Generator, tess_thickness, scalar);
        $op!(Generator, tess_view, enum, TessView);
        $op!(Generator, tess_height, scalar);
        $op!(Generator, tess_height_mode, enum, TessHeightMode);
        $op!(Generator, tess_beat_infl, scalar);
        $op!(Generator, tess_ripple_amt, scalar);
        $op!(Generator, tess_ripple_freq, scalar);
        $op!(Generator, tess_construct, enum, TilingConstruct);
        $op!(Generator, tess_phason, scalar);
        $op!(Generator, tess_grid_n, scalar);
        $op!(Generator, tess_ammann, scalar);
        $op!(Generator, tess_hyp_p, scalar);
        $op!(Generator, tess_hyp_q, scalar);
        $op!(Generator, mb_power, scalar);
        $op!(Generator, mb_iter, scalar);
        $op!(Generator, mb_scale, scalar);
        $op!(Generator, mb_detail, scalar);
        $op!(Generator, mb_spin, scalar);
        $op!(Generator, mb_morph, scalar);
        $op!(Generator, mb_color, scalar);
        $op!(Generator, mb_bailout, scalar);
        $op!(Generator, cr_form, scalar);
        $op!(Generator, cr_scale, scalar);
        $op!(Generator, cr_detail, scalar);
        $op!(Generator, cr_swim, scalar);
        $op!(Generator, cr_warp_amp, scalar);
        $op!(Generator, cr_warp_freq, scalar);
        $op!(Generator, cr_rim, scalar);
        $op!(Generator, cr_glow, scalar);
        $op!(Generator, cr_wave_speed, scalar);
        $op!(Generator, cr_wave_freq, scalar);
        $op!(Generator, cr_wave_sharp, scalar);
        $op!(Generator, cr_wave_amt, scalar);
        $op!(Generator, cr_overlay, scalar);
        $op!(Generator, cr_overlay_opacity, scalar);
        $op!(Generator, cr_overlay_bright, scalar);
        $op!(Generator, ms_family, enum, MinimalFamily);
        $op!(Generator, ms_scale, scalar);
        $op!(Generator, ms_cells, scalar);
        $op!(Generator, ms_iso, scalar);
        $op!(Generator, ms_thickness, scalar);
        $op!(Generator, ms_twist, scalar);
        $op!(Generator, ms_detail, scalar);
        $op!(Generator, ms_color, scalar);
        $op!(Generator, ms_beat_iso, scalar);
        $op!(Generator, ms_bend, scalar);
        $op!(Generator, ms_uv_res, scalar);
        $op!(Generator, ms_extent, scalar);
        $op!(Generator, ms_bend_phase, scalar);
        $op!(Generator, ms_turns, scalar);
        $op!(Generator, ms_form_res, scalar);
        $op!(Generator, lens_focal, scalar);
        $op!(Generator, lens_aperture, scalar);
        $op!(Generator, lens_thickness, scalar);
        $op!(Generator, lens_plano, scalar);
        $op!(Generator, lens_scale, scalar);
        $op!(Generator, lens_detail, scalar);
        $op!(Generator, kf_sectors, scalar);
        $op!(Generator, kf_fold, scalar);
        $op!(Generator, kf_iter, scalar);
        $op!(Generator, kf_iter_rot, scalar);
        $op!(Generator, kf_spin, scalar);
        $op!(Generator, kf_breathe, scalar);
        $op!(Generator, kf_zoom, scalar);
        $op!(Generator, kf_tunnel, scalar);
        $op!(Generator, kf_rays, scalar);
        $op!(Generator, kf_ring, scalar);
        $op!(Generator, kf_glow, scalar);
        $op!(Generator, kf_hue, scalar);
        $op!(Generator, kf_pattern, enum, KifsPattern);
        $op!(Generator, kf_palette, enum, KifsPalette);
        $op!(Generator, kf_color_speed, scalar);
        $op!(Generator, kf_churn, scalar);
        $op!(Generator, kf_e8_flow, scalar);
        $op!(Generator, kf_warp, scalar);
        $op!(Generator, kf_flow, scalar);
        $op!(Generator, kf_petals, scalar);
        $op!(Generator, kf_contrast, scalar);
        $op!(Generator, kf_sharp, scalar);
        $op!(Generator, kf_invert, scalar);
        $op!(Generator, kf_dispersion, scalar);
        $op!(Generator, kf_space, enum, KifsSpace);
        $op!(Generator, kf_view, enum, KifsView);
        $op!(Generator, kf_relief, scalar);
        $op!(Generator, kf_relief_elev, scalar);
        $op!(Generator, kf_relief_steps, scalar);
        $op!(Generator, kf_relief_shine, scalar);
        $op!(Generator, surface_mode, enum, SurfaceMode);
        $op!(Generator, plexus_radius, scalar);
        $op!(Generator, plexus_links, scalar);
        $op!(Generator, plexus_strut, scalar);
        $op!(Generator, plexus_marker, scalar);
        $op!(Generator, plexus_impostor, scalar);
        $op!(Generator, plexus_edges, scalar);
        $op!(Generator, plexus_node_radius, scalar);
        $op!(Generator, plexus_edge_radius, scalar);
        $op!(Generator, plexus_node_type, enum, MaterialType);
        $op!(Generator, plexus_node_metallic, scalar);
        $op!(Generator, plexus_node_rough, scalar);
        $op!(Generator, plexus_node_ior, scalar);
        $op!(Generator, plexus_node_hue, scalar);
        $op!(Generator, plexus_node_sat, scalar);
        $op!(Generator, plexus_node_val, scalar);
        $op!(Generator, plexus_node_emissive, scalar);
        $op!(Generator, plexus_edge_type, enum, MaterialType);
        $op!(Generator, plexus_edge_metallic, scalar);
        $op!(Generator, plexus_edge_rough, scalar);
        $op!(Generator, plexus_edge_ior, scalar);
        $op!(Generator, plexus_edge_hue, scalar);
        $op!(Generator, plexus_edge_sat, scalar);
        $op!(Generator, plexus_edge_val, scalar);
        $op!(Generator, plexus_edge_emissive, scalar);
        $op!(Generator, plexus_signal, scalar);
        $op!(Generator, plexus_signal_speed, scalar);
        $op!(Generator, plexus_signal_gain, scalar);
        $op!(Generator, plexus_signal_width, scalar);
        $op!(Generator, plexus_node_shape, scalar);
        $op!(Generator, plexus_edge_shape, scalar);
        $op!(Generator, plexus_overlay_on, scalar);
        $op!(Generator, plexus_shell_scale, scalar);
        $op!(Generator, plexus_shell_depth, scalar);
        $op!(Generator, plexus_shell_bins, scalar);
        $op!(Generator, origin_mode, enum, OriginMode);
        $op!(Generator, bevel, scalar);
        // #472 Tier 1 procedural/texture materials (Look — the material is a look).
        $op!(Look, mat_enable, scalar);
        $op!(Look, mat_projection, enum, MatProjection);
        $op!(Look, mat_scale, scalar);
        // #472 Tier 2 procedural noise layer (Look).
        $op!(Look, mp_enable, scalar);
        $op!(Look, mp_noise, enum, MatNoise);
        $op!(Look, mp_channel, enum, MatChannel);
        $op!(Look, mp_scale, scalar);
        $op!(Look, mp_rotation, scalar);
        $op!(Look, mp_offset_x, scalar);
        $op!(Look, mp_offset_y, scalar);
        $op!(Look, mp_octaves, scalar);
        $op!(Look, mp_lacunarity, scalar);
        $op!(Look, mp_gain, scalar);
        $op!(Look, mp_warp, scalar);
        $op!(Look, mp_contrast, scalar);
        $op!(Look, mp_gamma, scalar);
        $op!(Look, mp_remap_lo, scalar);
        $op!(Look, mp_remap_hi, scalar);
        $op!(Look, mp_invert, scalar);
        $op!(Look, mp_seed, scalar);
        $op!(Look, mp_res, enum, BakeRes);
        $op!(Look, mp_lo_r, scalar);
        $op!(Look, mp_lo_g, scalar);
        $op!(Look, mp_lo_b, scalar);
        $op!(Look, mp_hi_r, scalar);
        $op!(Look, mp_hi_g, scalar);
        $op!(Look, mp_hi_b, scalar);
        // #472 Tier 3 overlay layer 2 + derived maps (Look).
        $op!(Look, mp2_enable, scalar);
        $op!(Look, mp2_blend, enum, BlendMode);
        $op!(Look, mp2_noise, enum, MatNoise);
        $op!(Look, mp2_channel, enum, MatChannel);
        $op!(Look, mp2_scale, scalar);
        $op!(Look, mp2_rotation, scalar);
        $op!(Look, mp2_offset_x, scalar);
        $op!(Look, mp2_offset_y, scalar);
        $op!(Look, mp2_octaves, scalar);
        $op!(Look, mp2_lacunarity, scalar);
        $op!(Look, mp2_gain, scalar);
        $op!(Look, mp2_warp, scalar);
        $op!(Look, mp2_contrast, scalar);
        $op!(Look, mp2_gamma, scalar);
        $op!(Look, mp2_remap_lo, scalar);
        $op!(Look, mp2_remap_hi, scalar);
        $op!(Look, mp2_invert, scalar);
        $op!(Look, mp2_seed, scalar);
        $op!(Look, mp2_lo_r, scalar);
        $op!(Look, mp2_lo_g, scalar);
        $op!(Look, mp2_lo_b, scalar);
        $op!(Look, mp2_hi_r, scalar);
        $op!(Look, mp2_hi_g, scalar);
        $op!(Look, mp2_hi_b, scalar);
        $op!(Look, mat_derive_normal, scalar);
        $op!(Look, mat_derive_ao, scalar);
        $op!(Look, mat_normal_source_albedo, scalar);
        $op!(Look, mat_derive_normal_strength, scalar);
        $op!(Look, mat_derive_ao_strength, scalar);
        $op!(Look, mat_derive_ao_radius, scalar);
        // #472 Tier 5 live animation + displacement (Look).
        $op!(Look, mat_anim_enable, scalar);
        $op!(Look, mat_anim_speed, scalar);
        $op!(Look, mat_anim_mode, enum, AnimMode);
        $op!(Look, mat_flow_x, scalar);
        $op!(Look, mat_flow_y, scalar);
        $op!(Look, mat_displace, scalar);
        $op!(Generator, palette, enum, Palette);
        $op!(Generator, metaball_radius, scalar);
        $op!(Generator, metaball_threshold, scalar);
        $op!(Generator, metaball_smooth, scalar);
        $op!(Generator, splat_radius, scalar);
        $op!(Generator, splat_opacity, scalar);
        $op!(Generator, splat_falloff, scalar);
        $op!(Generator, splat_mode, enum, SplatMode);
        $op!(Generator, splat_cutoff, scalar);
        $op!(Generator, splat_aniso, scalar);
        $op!(Generator, splat_scatter, scalar);
        $op!(Generator, splat_jitter, scalar);
        $op!(Generator, splat_solid, scalar);
        $op!(Generator, ma_kind, enum, MapKindParam);
        $op!(Generator, ma_a, scalar);
        $op!(Generator, ma_b, scalar);
        $op!(Generator, ma_c, scalar);
        $op!(Generator, ma_d, scalar);
        $op!(Generator, ma_color, enum, MapColorParam);
        $op!(Generator, ma_points_k, scalar);
        $op!(Generator, ma_warmup, scalar);
        $op!(Generator, ma_scale, scalar);
        $op!(Generator, ma_size, scalar);
        $op!(Generator, ma_intensity, scalar);
        $op!(Generator, ma_a_drive, scalar);
        $op!(Generator, ma_b_drive, scalar);
        $op!(Generator, ma_orbit, enum, MapOrbitModeParam);
        $op!(Generator, ma_loop_beats, scalar);
        $op!(Generator, ma_orbit_ra, scalar);
        $op!(Generator, ma_orbit_rb, scalar);
        $op!(Generator, ma_orbit_fa, scalar);
        $op!(Generator, ma_orbit_fb, scalar);
        $op!(Generator, ma_orbit_psi, scalar);
        $op!(Generator, ma_orbit_free, scalar);
        $op!(Generator, tube_weld, scalar);
        $op!(Generator, tube_end_cap, scalar);
        $op!(Generator, tube_cap_round, scalar);
        $op!(Generator, tube_cap_bevel, scalar);
        $op!(Generator, tube_profile, scalar);
        $op!(Generator, voxel_res, scalar);
        $op!(Generator, voxel_threshold, scalar);
        $op!(Generator, voxel_radius, scalar);
        $op!(Generator, voxel_emission, scalar);
        $op!(Generator, voxel_ao, scalar);
        $op!(Generator, voxel_shadow, scalar);
        $op!(Generator, voxel_quantize, scalar);
        $op!(Generator, voxel_beat, scalar);
        $op!(Generator, voxel_gi, scalar);
        $op!(Generator, voxel_gi_strength, scalar);
        $op!(Generator, voxel_gi_distance, scalar);
        $op!(Generator, voxel_gi_sky, scalar);
        $op!(Generator, membrane_weave, enum, MembraneWeave);
        $op!(Generator, membrane_show_strands, scalar);
        $op!(Generator, membrane_arms, scalar);
        $op!(Generator, membrane_arm_build, enum, MembraneArmBuild);
        $op!(Generator, membrane_close, scalar);
        $op!(Generator, membrane_arm_radius, scalar);
        // Emissive volume surface mode (#152) — Surface lives on the Generator tab.
        $op!(Generator, volume_radius, scalar);
        $op!(Generator, volume_density, scalar);
        $op!(Generator, volume_emission, scalar);
        $op!(Generator, volume_absorption, scalar);
        $op!(Generator, volume_steps, scalar);
        // ---- Motion (59) ----
        $op!(Motion, animate, scalar);
        $op!(Motion, inc_scale, scalar);
        $op!(Motion, speed_exp, scalar);
        $op!(Settings, pulse, scalar);
        $op!(Settings, tempo_sync, scalar);
        $op!(Settings, tempo, scalar);
        $op!(Settings, pulse_source, enum, PulseSource);
        $op!(Settings, audio_react, scalar);
        $op!(Audio, audio_gain, scalar);
        $op!(Audio, audio_attack, scalar);
        $op!(Audio, audio_release, scalar);
        $op!(Generator, ad_drive, scalar);
        $op!(Generator, ad_amount, scalar);
        $op!(Generator, ad_floor, scalar);
        $op!(Generator, ad_multipole, scalar);
        $op!(Generator, ad_spread, scalar);
        $op!(Generator, ad_band_hue, scalar);
        $op!(Generator, ad_stereo, scalar);
        $op!(Generator, ad_pitch, scalar);
        $op!(Generator, ad_wave, scalar);
        $op!(Motion, cam_path, enum, CamPath);
        $op!(Motion, cam_speed, scalar);
        $op!(Motion, cam_kick, scalar);
        $op!(Motion, cam_damping, scalar);
        $op!(Motion, cam_amount, scalar);
        $op!(Motion, cam_beat_momentum, scalar);
        $op!(Motion, cam_seq_enabled, scalar);
        $op!(Motion, cam_bars_per_shot, enum, BarPeriod);
        $op!(Motion, cam_seq_order, enum, CamOrder);
        $op!(Motion, cam_transition, enum, CamTransition);
        $op!(Motion, cam_transition_bars, scalar);
        $op!(Motion, cam_dolly_period, scalar);
        $op!(Motion, cam_dolly_depth, scalar);
        $op!(Motion, cam_dolly_wave, enum, DollyWave);
        $op!(Settings, tempo_source, enum, TempoSource);
        $op!(Settings, beats_per_bar, scalar);
        $op!(Motion, cam_roll, scalar);
        $op!(Motion, cam_fov, scalar);
        $op!(Motion, cam_fov_dolly, scalar);
        $op!(Motion, cam_hold_prob, scalar);
        $op!(Motion, cam_phrase_lock, scalar);
        $op!(Motion, cam_seq_mix, scalar);
        $op!(Motion, cam_story_enabled, scalar);
        $op!(Motion, cam_story_count, scalar);
        $op!(Motion, cam_story_mode, enum, CamOrder);
        $op!(Motion, cam_story_seed, scalar);
        $op!(Motion, cam_shot0_path, enum, CamPath);
        $op!(Motion, cam_shot0_bars, enum, BarPeriod);
        $op!(Motion, cam_shot0_radius, scalar);
        $op!(Motion, cam_shot1_path, enum, CamPath);
        $op!(Motion, cam_shot1_bars, enum, BarPeriod);
        $op!(Motion, cam_shot1_radius, scalar);
        $op!(Motion, cam_shot2_path, enum, CamPath);
        $op!(Motion, cam_shot2_bars, enum, BarPeriod);
        $op!(Motion, cam_shot2_radius, scalar);
        $op!(Motion, cam_shot3_path, enum, CamPath);
        $op!(Motion, cam_shot3_bars, enum, BarPeriod);
        $op!(Motion, cam_shot3_radius, scalar);
        $op!(Audio, mod_a_target, enum, ModTarget);
        $op!(Audio, mod_a_depth, scalar);
        $op!(Audio, mod_b_target, enum, ModTarget);
        $op!(Audio, mod_b_depth, scalar);
        $op!(Motion, speed_pulse_amount, scalar);
        $op!(Motion, speed_pulse_attack, scalar);
        $op!(Motion, speed_pulse_decay, scalar);
        $op!(Motion, breath_amount, scalar);
        $op!(Motion, breath_attack, scalar);
        $op!(Motion, breath_decay, scalar);
        // ---- Look (76) ----
        $op!(Look, ambient, scalar);
        $op!(Look, key_intensity, scalar);
        $op!(Look, fill_intensity, scalar);
        $op!(Look, elevation, scalar);
        $op!(Look, azimuth, scalar);
        $op!(Look, glow, scalar);
        $op!(Look, mat_emissive, scalar);
        $op!(Look, opacity, scalar);
        // Scene Kaleidoscope (#361 Tier 1) — a captured Look.
        $op!(Look, kal_on, scalar);
        $op!(Look, kal_sectors, scalar);
        $op!(Look, kal_mode, enum, KaleidoMode);
        $op!(Look, kal_spin, scalar);
        $op!(Look, kal_roll, scalar);
        $op!(Look, kal_zoom, scalar);
        $op!(Look, kal_center_x, scalar);
        $op!(Look, kal_center_y, scalar);
        $op!(Look, kal_mix, scalar);
        $op!(Look, kal_twist, scalar);
        $op!(Look, kal_tint_hue, scalar);
        $op!(Look, kal_tint_amt, scalar);
        $op!(Look, kal_seam, scalar);
        // #391 Tier 1 quantitative instrumentation (captured Look). Bools use the
        // `scalar` kind like `kal_on` (the macro has no separate bool arm).
        $op!(Look, instr_hud, scalar);
        $op!(Look, instr_probe_on, scalar);
        $op!(Look, instr_probe_x, scalar);
        $op!(Look, instr_probe_y, scalar);
        $op!(Look, instr_probe_z, scalar);
        $op!(Look, instr_ledger_on, scalar);
        $op!(Look, instr_ledger_half, scalar);
        $op!(Look, instr_ledger_res, scalar);
        $op!(Look, instr_flux_on, scalar);
        $op!(Look, instr_flux_x, scalar);
        $op!(Look, instr_flux_y, scalar);
        $op!(Look, instr_flux_z, scalar);
        $op!(Look, instr_flux_size, scalar);
        $op!(Look, instr_flux_axis, enum, FluxAxis);
        $op!(Look, instr_flux_res, scalar);
        $op!(Look, instr_csv_log, scalar);
        $op!(Look, instr_panel_opacity, scalar);
        $op!(Look, instr_panel_bevel, scalar);
        $op!(Look, instr_hud_scale, scalar);
        $op!(Look, instr_hud_dock, enum, HudDock);
        $op!(Look, mat_type, enum, MaterialType);
        $op!(Look, ior, scalar);
        $op!(Look, mat_absorb, scalar);
        $op!(Look, refr_overlay, scalar);
        $op!(Look, refr_blend, scalar);
        $op!(Look, refract_ss, scalar);
        $op!(Look, refract_dist, scalar);
        $op!(Look, anisotropy, scalar);
        $op!(Look, aniso_rotation, scalar);
        $op!(Look, aniso_overlay, scalar);
        $op!(Look, aniso_blend, scalar);
        $op!(Look, clearcoat, scalar);
        $op!(Look, clearcoat_rough, scalar);
        $op!(Look, clearcoat_overlay, scalar);
        $op!(Look, sheen, scalar);
        $op!(Look, sheen_rough, scalar);
        $op!(Look, sheen_tint, scalar);
        $op!(Look, sheen_overlay, scalar);
        $op!(Look, sss_thickness, scalar);
        $op!(Look, sss_radius, scalar);
        $op!(Look, interior_scatter, scalar);
        $op!(Look, glitter, scalar);
        $op!(Look, glitter_density, scalar);
        $op!(Look, glitter_sharpness, scalar);
        $op!(Look, diffraction, scalar);
        $op!(Look, diffraction_freq, scalar);
        $op!(Look, retro, scalar);
        $op!(Look, fluorescence, scalar);
        $op!(Look, fluor_hue, scalar);
        $op!(Look, incandescence, scalar);
        $op!(Look, temperature, scalar);
        $op!(Look, metallic, scalar);
        $op!(Look, roughness, scalar);
        $op!(Environment, exposure, scalar);
        $op!(Environment, env_intensity, scalar);
        $op!(Environment, env_rotation, scalar);
        $op!(Look, bloom_intensity, scalar);
        $op!(Look, bloom_threshold, scalar);
        $op!(Look, subsurface, scalar);
        $op!(Look, sss_distortion, scalar);
        $op!(Look, sss_power, scalar);
        $op!(Look, iridescence, scalar);
        $op!(Look, irid_scale, scalar);
        $op!(Look, irid_shift, scalar);
        $op!(Look, ssao, scalar);
        $op!(Look, ssao_radius, scalar);
        $op!(Look, ssao_intensity, scalar);
        $op!(Look, ssao_bias, scalar);
        $op!(Look, ssr, scalar);
        $op!(Look, ssr_intensity, scalar);
        $op!(Look, ssr_max_roughness, scalar);
        $op!(Look, ssr_thickness, scalar);
        $op!(Look, ssr_steps, scalar);
        $op!(Look, gi, scalar);
        $op!(Look, gi_intensity, scalar);
        $op!(Look, gi_falloff, scalar);
        $op!(Look, glass_dispersion, scalar);
        $op!(Look, glass_caustic, scalar);
        $op!(Look, glass_thin_film, scalar);
        // Physical thin-film interference (#258 Tier 1) — a Look. Off (thickness 0).
        $op!(Look, film_thickness, scalar);
        $op!(Look, film_thickness_var, scalar);
        $op!(Look, film_ior, scalar);
        $op!(Look, film_drainage, scalar);
        // Screen-space GI (#152 Tier 2) — a Look. (TAA/motion-blur/stochastic are
        // per-display, in Shared.temporal, NOT captured — like MSAA.)
        $op!(Look, ssgi, scalar);
        $op!(Look, ssgi_intensity, scalar);
        $op!(Look, ssgi_radius, scalar);
        $op!(Look, ssgi_rays, scalar);
        // Cast shadows (#152 Tier 3) — a Look.
        $op!(Look, shadow_enabled, scalar);
        $op!(Look, shadow_bias, scalar);
        $op!(Look, shadow_strength, scalar);
        // Voxel GI (#152 Tier 3, #10) — a Look.
        $op!(Look, vxgi_enabled, scalar);
        $op!(Look, vxgi_intensity, scalar);
        $op!(Look, vxgi_rays, scalar);
        $op!(Look, vxgi_steps, scalar);
        // Reflection look (#163 Tier 1) — a Look.
        $op!(Look, reflect_tint, scalar);
        $op!(Look, chrome_purity, scalar);
        $op!(Look, glass_clarity, scalar);
        $op!(Look, f0_override, scalar);
        // Reflection probe / parallax (#163 Tier 2) — a Look.
        $op!(Look, refl_source, enum, ReflectionSource);
        $op!(Look, refl_box_scale, scalar);
        $op!(Look, refl_box_height, scalar);
        $op!(Look, refl_blend, scalar);
        // VXGI specular reflections (#163 Tier 3) — a Look.
        $op!(Look, vxgi_spec_strength, scalar);
        $op!(Look, vxgi_spec_aperture, scalar);
        $op!(Look, vxgi_spec_reach, scalar);
        $op!(Look, vxgi_spec_steps, scalar);
        // Membrane screen-space FX opt-in — a Look.
        $op!(Look, membrane_fx, scalar);
        // Cinematic finishing (#167 Tier 1) — halation + lens flares, a Look.
        $op!(Look, hal_amount, scalar);
        $op!(Look, hal_threshold, scalar);
        $op!(Look, hal_width, scalar);
        $op!(Look, hal_warmth, scalar);
        $op!(Look, lf_amount, scalar);
        $op!(Look, lf_ghosts, scalar);
        $op!(Look, lf_halo, scalar);
        $op!(Look, lf_streak, scalar);
        // Emissive cubes as real lights (#167 Tier 3) — a Look.
        $op!(Look, ml_enabled, scalar);
        $op!(Look, ml_intensity, scalar);
        $op!(Look, ml_radius, scalar);
        $op!(Look, ml_count, scalar);
        $op!(Look, ml_restir, scalar);
        $op!(Look, pt_dielectric, scalar);
        $op!(Look, pt_absorb, scalar);
        $op!(Look, pt_composite, enum, PtComposite);
        $op!(Look, pt_augment, scalar);
        $op!(Look, spectral_enable, scalar);
        $op!(Look, spectral_abbe, scalar);
        $op!(Look, pt_caustics, scalar);
        $op!(Look, pt_caustic_intensity, scalar);
        $op!(Look, pt_caustic_radius, scalar);
        $op!(Settings, tonemap, enum, ToneMap);
        $op!(Environment, bg_tonemap, enum, ToneMap);
        $op!(Environment, bg_visible, scalar);
        $op!(Environment, bg_intensity, scalar);
        $op!(Environment, env_tint_hue, scalar);
        $op!(Environment, env_tint_amt, scalar);
        $op!(Look, mat_hue, scalar);
        $op!(Look, mat_hue_cycle, scalar);
        $op!(Look, mat_saturation, scalar);
        $op!(Look, mat_value, scalar);
        $op!(Look, scen_hue, scalar);
        $op!(Look, scen_hue_cycle, scalar);
        $op!(Look, scen_saturation, scalar);
        $op!(Look, scen_value, scalar);
        $op!(Look, sky_reflect_clouds, scalar);
        $op!(Look, sky_cloud_cover, scalar);
        $op!(Look, sky_cloud_speed, scalar);
        $op!(Look, sky_cloud_strength, scalar);
        // Neural radiance cache — live (#256 Tier 0).
        $op!(Look, nrc_enable, scalar);
        $op!(Look, nrc_confidence, scalar);
        $op!(Look, nrc_learn_rate, scalar);
        $op!(Look, nrc_omega, scalar);
        $op!(Look, nrc_terminate, scalar);
        $op!(Look, nrc_train_samples, scalar);
        $op!(Look, nrc_seed, scalar);
        // Neural radiance cache — RT-stack synergies (#256 Tier 1).
        $op!(Look, nrc_guide, scalar);
        $op!(Look, nrc_guide_candidates, scalar);
        $op!(Look, nrc_firefly, scalar);
        $op!(Look, nrc_firefly_clamp, scalar);
        // Neural radiance cache — light-field uses (#256 Tier 2).
        $op!(Look, nrc_gi, scalar);
        $op!(Look, nrc_gi_strength, scalar);
        $op!(Look, nrc_reflect, scalar);
        // Neural radiance cache — hard transport + volumetrics (#256 Tier 3).
        $op!(Look, nrc_volume, scalar);
        $op!(Look, nrc_volume_density, scalar);
        $op!(Look, nrc_volume_steps, scalar);
        $op!(Look, nrc_volume_strength, scalar);
        $op!(Look, nrc_caustic, scalar);
        $op!(Look, nrc_caustic_gain, scalar);
        $op!(Look, particles_bead_hue, scalar);
        $op!(Look, particles_bead_hue_cycle, scalar);
        $op!(Look, particles_bead_sat, scalar);
        $op!(Look, particles_bead_val, scalar);
        $op!(Look, particles_bead_emissive, scalar);
        $op!(Look, color_cycle, scalar);
        $op!(Look, ripple_intensity, scalar);
        $op!(Look, ripple_speed, scalar);
        $op!(Look, ripple_freq, scalar);
        $op!(Look, ripple_sharp, scalar);
        $op!(Look, ripple_geom, enum, RippleGeom);
        $op!(Look, rd_intensity, scalar);
        $op!(Look, rd_feed, scalar);
        $op!(Look, rd_kill, scalar);
        $op!(Look, rd_scale, scalar);
        $op!(Look, rd_albedo_mix, scalar);
        $op!(Look, particles_tier, enum, ParticleTier);
        $op!(Look, particles_count_k, scalar);
        $op!(Look, particles_grid_res, scalar);
        $op!(Look, particles_speed, scalar);
        $op!(Look, particles_lifetime, scalar);
        $op!(Look, particles_spawn_radius, scalar);
        $op!(Look, particles_size, scalar);
        $op!(Look, particles_emissive, scalar);
        $op!(Look, particles_ribbon, scalar);
        $op!(Look, particles_ribbon_stretch, scalar);
        $op!(Look, particles_hue_shift, scalar);
        $op!(Look, particles_beat_burst, scalar);
        $op!(Look, particles_drag, scalar);
        $op!(Look, particles_turbulence, scalar);
        $op!(Look, particles_alpha, scalar);
        $op!(Look, particles_hide_generator, scalar);
        $op!(Look, particles_beads, scalar);
        $op!(Look, particles_metallic, scalar);
        $op!(Look, particles_roughness, scalar);
        $op!(Look, particles_material, enum, ParticleMaterial);
        $op!(Look, particles_shape, enum, ParticleShape);
        $op!(Look, particles_ior, scalar);
        $op!(Look, particles_shape_param, scalar);
        $op!(Look, particles_beads_rt, scalar);
        $op!(Look, mn_energize, scalar);
        $op!(Look, mn_gain, scalar);
        $op!(Look, mn_knee, scalar);
        $op!(Look, mn_hue, scalar);
        $op!(Look, mn_antenna, scalar);
        $op!(Look, mn_antenna_len, scalar);
        $op!(Look, mn_dye_inject, scalar);
        $op!(Look, mx_aura_blend, scalar);
        $op!(Look, mn_force, scalar);
        $op!(Look, mn_force_gain, scalar);
        $op!(Look, mn_energy_contrast, scalar);
        $op!(Look, mn_stir_rate, scalar);
        $op!(Look, mn_pump, scalar);
        $op!(Look, mn_swirl_beat, scalar);
        $op!(Look, mn_pump_scale, scalar);
        $op!(Look, mn_swirl_decay, scalar);
        $op!(Look, mn_mode_mix, scalar);
        $op!(Look, mn_ring_freq, scalar);
        $op!(Look, mn_hue_cycle, scalar);
        $op!(Look, fluid_force, scalar);
        $op!(Look, fluid_vorticity, scalar);
        $op!(Look, fluid_dissipation, scalar);
        $op!(Look, fluid_iters, scalar);
        $op!(Look, fluid_inflow_decay, scalar);
        // Fluid Ink (#182 Tier 1) — a Look.
        $op!(Look, ink_enabled, scalar);
        $op!(Look, ink_rate, scalar);
        $op!(Look, ink_radius, scalar);
        $op!(Look, ink_extinction, scalar);
        $op!(Look, ink_scatter, scalar);
        $op!(Look, ink_emissive, scalar);
        $op!(Look, ink_anisotropy, scalar);
        $op!(Look, ink_dissipation, scalar);
        $op!(Look, ink_steps, scalar);
        $op!(Look, ink_maccormack, scalar);
        $op!(Look, ink_half_res, scalar);
        $op!(Look, ink_reveal, scalar);
        // Fluid medium Tier 2 (#182) — a Look.
        $op!(Look, fl2_boundaries, scalar);
        $op!(Look, fl2_buoyancy, scalar);
        $op!(Look, fl2_heat_decay, scalar);
        $op!(Look, fl2_detail, scalar);
        $op!(Look, fl2_splash, scalar);
        $op!(Look, fl2_dye_gate, scalar);
        $op!(Look, fl2_res, scalar);
        $op!(Look, fl2_substeps, scalar);
        // MLS-MPM liquid (#182 Tier 3a) — a Look.
        $op!(Look, liq_enabled, scalar);
        $op!(Look, liq_count, scalar);
        $op!(Look, liq_res, scalar);
        $op!(Look, liq_gravity, scalar);
        $op!(Look, liq_stiffness, scalar);
        $op!(Look, liq_viscosity, scalar);
        $op!(Look, liq_container, scalar);
        $op!(Look, liq_open_top, scalar);
        $op!(Look, liq_collide, scalar);
        $op!(Look, liq_stir, scalar);
        $op!(Look, liq_density, scalar);
        $op!(Look, liq_threshold, scalar);
        $op!(Look, liq_hue, scalar);
        $op!(Look, liq_sat, scalar);
        $op!(Look, liq_substeps, scalar);
        $op!(Look, liq_offset_y, scalar);
        $op!(Look, liq_shape, enum, LiqShape);
        $op!(Look, liq_reveal, scalar);
        $op!(Look, fgi_gi, scalar);
        $op!(Look, fgi_shadow, scalar);
        $op!(Look, fgi_receive, scalar);
        $op!(Look, fgi_sway, scalar);
        $op!(Look, ca_amount, scalar);
        $op!(Look, ca_sharpness, scalar);
        $op!(Look, liq_material, enum, LiqMaterial);
        $op!(Look, liq_metallic, scalar);
        $op!(Look, liq_roughness, scalar);
        $op!(Look, liq_ior, scalar);
        $op!(Look, ghost_light, scalar);
        $op!(Look, liq_render, enum, LiqRender);
        $op!(Look, liq_absorb, scalar);
        $op!(Look, liq_glow, scalar);
        $op!(Look, liq_chrome_purity, scalar);
        $op!(Look, liq_glass_clarity, scalar);
        $op!(Look, liq_f0, scalar);
        $op!(Look, liq_dispersion, scalar);
        $op!(Look, liq_gcaustic, scalar);
        $op!(Look, liq_thin_film, scalar);
        // Hardware RT (#195) — Looks.
        $op!(Look, rt_enable, scalar);
        $op!(Look, rt_shadows, scalar);
        $op!(Look, rt_shadow_soft, scalar);
        $op!(Look, rt_shadow_strength, scalar);
        $op!(Look, rt_shadow_fill, scalar);
        $op!(Look, rt_reflect, scalar);
        $op!(Look, rt_reflect_intensity, scalar);
        $op!(Look, rt_reflect_rough, scalar);
        $op!(Look, rt_reflect_reach, scalar);
        $op!(Look, rt_reflect_shadows, scalar);
        $op!(Look, rt_reflect_rays, scalar);
        $op!(Look, ao_source, enum, AoSource);
        $op!(Look, rt_ao_rays, scalar);
        $op!(Look, rt_gi, scalar);
        $op!(Look, rt_gi_intensity, scalar);
        $op!(Look, rt_gi_rays, scalar);
        $op!(Look, rt_gi_reach, scalar);
        $op!(Look, rt_gi_shadows, scalar);
        $op!(Look, rt_temporal, scalar);
        $op!(Look, rt_temporal_feedback, scalar);
        $op!(Look, rt_temporal_beat, scalar);
        $op!(Look, rt_temporal_variance, scalar);
        $op!(Look, rt_temporal_accum, scalar);
        $op!(Look, rt_temporal_clamp, scalar);
        $op!(Look, rt_denoise, scalar);
        $op!(Look, rt_denoise_amount, scalar);
        $op!(Look, nd_enable, scalar);
        $op!(Look, nd_strength, scalar);
        $op!(Look, nd_seed, scalar);
        $op!(Look, nd_omega, scalar);
        $op!(Look, up_enable, scalar);
        $op!(Look, up_sharpen, scalar);
        $op!(Look, up_seed, scalar);
        $op!(Look, neural_enable, scalar);
        $op!(Look, neural_seed_a, scalar);
        $op!(Look, neural_seed_b, scalar);
        $op!(Look, neural_walk, scalar);
        $op!(Look, neural_omega, scalar);
        // Neural field generator (#200 Tier 1) — Generator-category geometry.
        $op!(Generator, neural_scale, scalar);
        $op!(Generator, neural_coord, scalar);
        $op!(Generator, neural_iso, scalar);
        $op!(Generator, neural_steps, scalar);
        $op!(Generator, neural_march, scalar);
        $op!(Generator, neural_color, scalar);
        $op!(Generator, neural_walk_rate, scalar);
        $op!(Generator, neural_strands_mode, scalar);
        $op!(Generator, neural_strands_cols, scalar);
        $op!(Generator, neural_strands_rows, scalar);
        $op!(Generator, neural_strands_extent, scalar);
        $op!(Generator, neural_strands_displace, scalar);
        // Post-composite creative FX (#152) — a Look.
        $op!(Look, fx_enabled, scalar);
        $op!(Look, fx_style, enum, RenderStyle);
        $op!(Look, fx_style_amt, scalar);
        $op!(Look, fx_dof, scalar);
        $op!(Look, fx_dof_focus, scalar);
        $op!(Look, fx_dof_range, scalar);
        $op!(Look, fx_chroma, scalar);
        $op!(Look, fx_vignette, scalar);
        $op!(Look, fx_grain, scalar);
        $op!(Look, fx_grade_sat, scalar);
        $op!(Look, fx_grade_contrast, scalar);
        $op!(Look, fx_grade_temp, scalar);
        $op!(Look, fx_grade_gain, scalar);
        $op!(Look, fx_feedback, scalar);
        $op!(Look, fx_outline, scalar);

        // ---- Environment (#354 — world layer + sky/IBL) ----
        // (exposure / env_intensity / env_rotation / bg_* / env_tint_* were
        //  re-homed to Environment above; the loaded .hdr rides `hdr_path`,
        //  which is applied out-of-band, not through this partition.)
        $op!(Environment, terrain_enabled, scalar);
        $op!(Environment, terrain_height, scalar);
        $op!(Environment, terrain_snow, scalar);
        $op!(Environment, terrain_fog, scalar);
        $op!(Environment, terrain_sun_elev, scalar);
        $op!(Environment, terrain_sun_azim, scalar);
        $op!(Environment, terrain_sun_int, scalar);
        $op!(Environment, terrain_scroll, scalar);
        $op!(Environment, terrain_ride, scalar);
        $op!(Environment, terrain_noise, enum, TerrainNoise);
        $op!(Environment, terrain_palette, enum, TerrainPalette);
        $op!(Environment, terrain_emissive, scalar);
        $op!(Environment, terrain_day_speed, scalar);
        $op!(Environment, terrain_sun_scene, scalar);
        $op!(Environment, terrain_scatter, scalar);
        $op!(Environment, terrain_godray, scalar);
        $op!(Environment, terrain_water, scalar);
        $op!(Environment, terrain_water_level, scalar);
        $op!(Environment, terrain_water_hue, scalar);
        $op!(Environment, terrain_water_ripple, scalar);
        $op!(Environment, terrain_seed, scalar);
        $op!(Environment, terrain_ridged, scalar);
        $op!(Environment, terrain_brightness, scalar);
        $op!(Environment, terrain_haze, scalar);
        $op!(Environment, terrain_steps, scalar);
        $op!(Environment, terrain_octaves, scalar);
        $op!(Environment, terrain_res, enum, TerrainRes);
        $op!(Environment, stars_enabled, scalar);
        $op!(Environment, stars_brightness, scalar);
        $op!(Environment, stars_twinkle, scalar);
        $op!(Environment, stars_twinkle_speed, scalar);
        $op!(Environment, stars_size, scalar);
        $op!(Environment, stars_latitude, scalar);
        $op!(Environment, stars_sky_speed, scalar);
        $op!(Environment, stars_mag_limit, scalar);
        $op!(Environment, stars_saturation, scalar);
        $op!(Environment, stars_sun, scalar);
        $op!(Environment, stars_sun_bright, scalar);
        $op!(Environment, stars_sun_size, scalar);
        $op!(Environment, stars_sun_warmth, scalar);
        $op!(Environment, atmos_enabled, scalar);
        $op!(Environment, atmos_turbidity, scalar);
        $op!(Environment, atmos_mie_g, scalar);
        $op!(Environment, atmos_sun_int, scalar);
        $op!(Environment, atmos_ground_albedo, scalar);
        $op!(Environment, atmos_exposure, scalar);
        $op!(Environment, atmos_aerial, scalar);
        $op!(Environment, atmos_rayleigh, scalar);
        $op!(Environment, clouds_enabled, scalar);
        $op!(Environment, clouds_coverage, scalar);
        $op!(Environment, clouds_density, scalar);
        $op!(Environment, clouds_base, scalar);
        $op!(Environment, clouds_thickness, scalar);
        $op!(Environment, clouds_steps, scalar);
        $op!(Environment, clouds_detail, scalar);
        $op!(Environment, clouds_drift, scalar);
        $op!(Environment, clouds_hg, scalar);
        $op!(Environment, clouds_absorption, scalar);
        $op!(Environment, clouds_shadow, scalar);
        $op!(Environment, clouds_ambient, scalar);
        $op!(Environment, ocean_enabled, scalar);
        $op!(Environment, ocean_level, scalar);
        $op!(Environment, ocean_wind_speed, scalar);
        $op!(Environment, ocean_wind_dir, scalar);
        $op!(Environment, ocean_amplitude, scalar);
        $op!(Environment, ocean_choppiness, scalar);
        $op!(Environment, ocean_tile_size, scalar);
        $op!(Environment, ocean_foam, scalar);
        $op!(Environment, ocean_glitter, scalar);
        $op!(Environment, ocean_hue, scalar);
        $op!(Environment, ocean_depth, scalar);

        // ---- Settings (#354 — renderer / output / capture; tonemap + sync/tempo re-homed above) ----
        $op!(Settings, hdr_output, scalar);
        $op!(Settings, hdr_knee, scalar);
        $op!(Settings, hdr_wide, scalar);
        $op!(Settings, hdr_vivid, scalar);
        $op!(Settings, msaa, enum, Msaa);
        $op!(Settings, render_scale, scalar);
        $op!(Settings, render_auto, scalar);
        $op!(Settings, aspect_preset, enum, AspectPreset);
        $op!(Settings, out_long_edge, scalar);
        $op!(Settings, out_custom_w, scalar);
        $op!(Settings, out_custom_h, scalar);
        $op!(Settings, letterbox_r, scalar);
        $op!(Settings, letterbox_g, scalar);
        $op!(Settings, letterbox_b, scalar);
        $op!(Settings, frame_guide, scalar);
        $op!(Settings, lock_window, scalar);

        // ---- Audio (#354 — metering config; an_*/mod_*/audio_* re-homed above) ----
        $op!(Audio, meter_res, enum, SpectrumMode);
        $op!(Audio, meter_weight, enum, MeterWeighting);
        $op!(Audio, meter_averaging, enum, MeterAveraging);
        $op!(Audio, meter_hud, scalar);
    };
}

impl PresetValues {
    /// Snapshot the live parameters, including the loaded `.hdr` reference (read
    /// from the sidecar). **GUI-thread only** — the sidecar read is file I/O.
    pub fn capture(p: &OrganicMathParams) -> Self {
        let mut v = Self::capture_params_only(p);
        v.hdr_path = std::fs::read_to_string(crate::ipc::hdr_sidecar_path()).unwrap_or_default();
        v
    }

    /// Snapshot the live parameters **without** the `.hdr` sidecar read, so it's
    /// safe on the audio thread (#354). `hdr_path` is left empty — the Key Map
    /// path repacks to `Shared`, which doesn't carry `hdr_path` anyway.
    pub fn capture_params_only(p: &OrganicMathParams) -> Self {
        PresetValues {
            generator: p.generator.value().to_u32(),
            loop_count_x: p.loop_count_x.value(),
            loop_count_y: p.loop_count_y.value(),
            loop_count_z: p.loop_count_z.value(),
            loop_count_q: p.loop_count_q.value(),
            rot_func: p.rot_func.value().to_u32(),
            rot_amp_x: p.rot_amp_x.value(),
            rot_amp_y: p.rot_amp_y.value(),
            rot_amp_z: p.rot_amp_z.value(),
            rot_mod_x: p.rot_mod_x.value(),
            rot_mod_y: p.rot_mod_y.value(),
            rot_mod_z: p.rot_mod_z.value(),
            continuous: p.continuous.value(),
            cont_shape: p.cont_shape.value(),
            trans_func: p.trans_func.value().to_u32(),
            trans_amp_x: p.trans_amp_x.value(),
            trans_amp_y: p.trans_amp_y.value(),
            trans_amp_z: p.trans_amp_z.value(),
            trans_mod_x: p.trans_mod_x.value(),
            trans_mod_y: p.trans_mod_y.value(),
            trans_mod_z: p.trans_mod_z.value(),
            scale_func: p.scale_func.value().to_u32(),
            scale_amp: p.scale_amp.value(),
            frenet_strands: p.frenet_strands.value(),
            frenet_nodes: p.frenet_nodes.value(),
            frenet_step: p.frenet_step.value(),
            frenet_func: p.frenet_func.value().to_u32(),
            frenet_kappa: p.frenet_kappa.value(),
            frenet_kappa_amp: p.frenet_kappa_amp.value(),
            frenet_kappa_freq: p.frenet_kappa_freq.value(),
            frenet_tau: p.frenet_tau.value(),
            frenet_tau_amp: p.frenet_tau_amp.value(),
            frenet_tau_freq: p.frenet_tau_freq.value(),
            frenet_spread: p.frenet_spread.value(),
            frenet_thickness: p.frenet_thickness.value(),
            dna_form: p.dna_form.value().to_u32(),
            dna_bp: p.dna_bp.value(),
            dna_bp_per_turn: p.dna_bp_per_turn.value(),
            dna_rise: p.dna_rise.value(),
            dna_radius: p.dna_radius.value(),
            dna_groove: p.dna_groove.value(),
            dna_left: p.dna_left.value(),
            dna_sigma: p.dna_sigma.value(),
            dna_super_radius: p.dna_super_radius.value(),
            dna_seed: p.dna_seed.value(),
            dna_thickness: p.dna_thickness.value(),
            dna_twist_breathe: p.dna_twist_breathe.value(),
            attr_field: p.attr_field.value().to_u32(),
            attr_seeds: p.attr_seeds.value(),
            attr_seed: p.attr_seed.value(),
            attr_spread: p.attr_spread.value(),
            attr_dt: p.attr_dt.value(),
            attr_trail: p.attr_trail.value(),
            attr_speed: p.attr_speed.value(),
            attr_scale: p.attr_scale.value(),
            attr_thickness: p.attr_thickness.value(),
            boids_count: p.boids_count.value(),
            boids_perception: p.boids_perception.value(),
            boids_separation: p.boids_separation.value(),
            boids_sep: p.boids_sep.value(),
            boids_align: p.boids_align.value(),
            boids_cohere: p.boids_cohere.value(),
            boids_max_speed: p.boids_max_speed.value(),
            boids_max_force: p.boids_max_force.value(),
            boids_trail: p.boids_trail.value(),
            boids_bounds: p.boids_bounds.value(),
            boids_goal: p.boids_goal.value(),
            boids_thickness: p.boids_thickness.value(),
            boids_seed: p.boids_seed.value(),
            boids_speed: p.boids_speed.value(),
            boids_scale: p.boids_scale.value(),
            boids_form: p.boids_form.value().to_u32(),
            boids_size: p.boids_size.value(),
            boids_bank: p.boids_bank.value(),
            harm_mode0: p.harm_mode0.value(),
            harm_amp0: p.harm_amp0.value(),
            harm_freq0: p.harm_freq0.value(),
            harm_mode1: p.harm_mode1.value(),
            harm_amp1: p.harm_amp1.value(),
            harm_freq1: p.harm_freq1.value(),
            harm_mode2: p.harm_mode2.value(),
            harm_amp2: p.harm_amp2.value(),
            harm_freq2: p.harm_freq2.value(),
            harm_radius: p.harm_radius.value(),
            harm_theta: p.harm_theta.value(),
            harm_phi: p.harm_phi.value(),
            harm_thickness: p.harm_thickness.value(),
            bell_physical: p.bell_physical.value(),
            bell_stroke_depth: p.bell_stroke_depth.value(),
            bell_stiffness: p.bell_stiffness.value(),
            bell_damping: p.bell_damping.value(),
            bell_open: p.bell_open.value(),
            bell_speed: p.bell_speed.value(),
            ls_system: p.ls_system.value().to_u32(),
            ls_depth: p.ls_depth.value(),
            ls_angle: p.ls_angle.value(),
            ls_step: p.ls_step.value(),
            ls_sway_amp: p.ls_sway_amp.value(),
            ls_sway_freq: p.ls_sway_freq.value(),
            ls_grow: p.ls_grow.value(),
            ls_thickness: p.ls_thickness.value(),
            cn_seeds: p.cn_seeds.value(),
            cn_seed: p.cn_seed.value(),
            cn_spread: p.cn_spread.value(),
            cn_scale: p.cn_scale.value(),
            cn_steps: p.cn_steps.value(),
            cn_dt: p.cn_dt.value(),
            cn_flow: p.cn_flow.value(),
            cn_bound: p.cn_bound.value(),
            cn_thickness: p.cn_thickness.value(),
            pol_rings: p.pol_rings.value(),
            pol_spokes: p.pol_spokes.value(),
            pol_samples: p.pol_samples.value(),
            pol_len: p.pol_len.value(),
            pol_k: p.pol_k.value(),
            pol_amp: p.pol_amp.value(),
            pol_falloff: p.pol_falloff.value(),
            pol_handed: p.pol_handed.value(),
            pol_spread: p.pol_spread.value(),
            pol_swirl: p.pol_swirl.value(),
            pol_show_b: p.pol_show_b.value(),
            pol_thickness: p.pol_thickness.value(),
            mx_lines: p.mx_lines.value(),
            mx_gen_blend: p.mx_gen_blend.value(),
            mx_dipoles: p.mx_dipoles.value(),
            mx_sources: p.mx_sources.value(),
            mx_separation: p.mx_separation.value(),
            mx_phase: p.mx_phase.value(),
            mx_swirl: p.mx_swirl.value(),
            mx_near: p.mx_near.value(),
            mx_k: p.mx_k.value(),
            mx_amp: p.mx_amp.value(),
            mx_rmin: p.mx_rmin.value(),
            mx_thickness: p.mx_thickness.value(),
            mx_rings: p.mx_rings.value(),
            mx_spokes: p.mx_spokes.value(),
            mx_samples: p.mx_samples.value(),
            mx_raylen: p.mx_raylen.value(),
            mx_spread: p.mx_spread.value(),
            mx_seeds: p.mx_seeds.value(),
            mx_steps: p.mx_steps.value(),
            mx_ds: p.mx_ds.value(),
            mx_bound: p.mx_bound.value(),
            mx_norm_field: p.mx_norm_field.value(),
            mx_osc_sync: p.mx_osc_sync.value(),
            mx_osc_div: p.mx_osc_div.value().to_u32(),
            mx_eb_phase: p.mx_eb_phase.value(),
            ac_source: p.ac_source.value().to_u32(),
            ac_k: p.ac_k.value(),
            ac_near: p.ac_near.value(),
            ac_amp: p.ac_amp.value(),
            ac_separation: p.ac_separation.value(),
            ac_rmin: p.ac_rmin.value(),
            ac_blend: p.ac_blend.value(),
            ac_norm_field: p.ac_norm_field.value(),
            ac_rings: p.ac_rings.value(),
            ac_spokes: p.ac_spokes.value(),
            ac_samples: p.ac_samples.value(),
            ac_raylen: p.ac_raylen.value(),
            ac_spread: p.ac_spread.value(),
            ac_thickness: p.ac_thickness.value(),
            ac_aura_blend: p.ac_aura_blend.value(),
            ac_beat_pump: p.ac_beat_pump.value(),
            field_kind: p.field_kind.value().to_u32(),
            field_preset: p.field_preset.value().to_u32(),
            field_scale: p.field_scale.value(),
            field_extent: p.field_extent.value(),
            field_a: p.field_a.value(),
            field_b: p.field_b.value(),
            field_density: p.field_density.value(),
            field_gain: p.field_gain.value(),
            field_thickness: p.field_thickness.value(),
            // #381 Tier 3 Field Engine PDE dynamics
            pde_preset: p.pde_preset.value().to_u32(),
            sim_diffusion: p.sim_diffusion.value(),
            sim_time_scale: p.sim_time_scale.value(),
            sim_feed: p.sim_feed.value(),
            sim_kill: p.sim_kill.value(),
            sim_potential: p.sim_potential.value(),
            sim_forcing: p.sim_forcing.value(),
            sim_res: p.sim_res.value(),
            // #339 Duo-Field synthesis
            // #346 Field Chamber
            panels_on: p.panels_on.value(),
            panel_style: p.panel_style.value().to_u32(),
            panel_rear: p.panel_rear.value(),
            panel_right: p.panel_right.value(),
            panel_opacity: p.panel_opacity.value(),
            panel_fill: p.panel_fill.value(),
            panel_scope_amp: p.panel_scope_amp.value(),
            panel_db_floor: p.panel_db_floor.value(),
            panel_db_top: p.panel_db_top.value(),
            panel_material: p.panel_material.value().to_u32(),
            panel_metallic: p.panel_metallic.value(),
            panel_roughness: p.panel_roughness.value(),
            panel_emissive: p.panel_emissive.value(),
            panel_thickness: p.panel_thickness.value(),
            panel_wall_rel: p.panel_wall_rel.value(),
            panel_scope_time_ms: p.panel_scope_time_ms.value(),
            panel_scope_trigger: p.panel_scope_trigger.value(),
            panel_scope_channel: p.panel_scope_channel.value(),
            sn_on: p.sn_on.value(),
            sn_play_mode: p.sn_play_mode.value().to_u32(),
            sn_gain: p.sn_gain.value(),
            sn_mix: p.sn_mix.value(),
            sn_tuning: p.sn_tuning.value(),
            sn_gen_amp: p.sn_gen_amp.value(),
            sn_mode: p.sn_mode.value().to_u32(),
            sn_bank: p.sn_bank.value(),
            sn_tuning_layout: p.sn_tuning_layout.value().to_u32(),
            sn_tune_spread: p.sn_tune_spread.value(),
            sn_tune_stretch: p.sn_tune_stretch.value(),
            sn_shell_r: p.sn_shell_r.value(),
            sn_shell_rate: p.sn_shell_rate.value(),
            sn_t60: p.sn_t60.value(),
            sn_bright: p.sn_bright.value(),
            sn_grain_size: p.sn_grain_size.value(),
            sn_grain_density: p.sn_grain_density.value(),
            sn_attack: p.sn_attack.value(),
            sn_decay: p.sn_decay.value(),
            sn_sustain: p.sn_sustain.value(),
            sn_release: p.sn_release.value(),
            sn_glide: p.sn_glide.value(),
            sn_bend_range: p.sn_bend_range.value(),
            sn_place_spread: p.sn_place_spread.value(),
            sn_a4: p.sn_a4.value(),
            sn_probe_lx: p.sn_probe_lx.value(),
            sn_probe_ly: p.sn_probe_ly.value(),
            sn_probe_lz: p.sn_probe_lz.value(),
            sn_probe_rx: p.sn_probe_rx.value(),
            sn_probe_ry: p.sn_probe_ry.value(),
            sn_probe_rz: p.sn_probe_rz.value(),
            sn_probe_cam: p.sn_probe_cam.value(),
            sn_vis_pivot: p.sn_vis_pivot.value(),
            sn_vis_anchor: p.sn_vis_anchor.value(),
            sn_vis_slope: p.sn_vis_slope.value(),
            sn_vis_k_anchor: p.sn_vis_k_anchor.value(),
            sn_vis_k_slope: p.sn_vis_k_slope.value(),
            sn_vis_quantize: p.sn_vis_quantize.value().to_u32(),
            ac2_model: p.ac2_model.value().to_u32(),
            ac2_nx: p.ac2_nx.value(),
            ac2_ny: p.ac2_ny.value(),
            ac2_nz: p.ac2_nz.value(),
            ac2_morph: p.ac2_morph.value(),
            ac2_cav_scale: p.ac2_cav_scale.value(),
            ac2_intensity: p.ac2_intensity.value(),
            ac2_tween: p.ac2_tween.value(),
            ac2_audio_x: p.ac2_audio_x.value(),
            ac2_audio_y: p.ac2_audio_y.value(),
            ac2_audio_z: p.ac2_audio_z.value(),
            analytical_mode: p.analytical_mode.value().to_u32(),
            an_target_lufs: p.an_target_lufs.value(),
            an_floor_lufs: p.an_floor_lufs.value(),
            an_tp_ceiling: p.an_tp_ceiling.value(),
            an_corr_alarm: p.an_corr_alarm.value(),
            an_reference_hud: p.an_reference_hud.value(),
            fv_source: p.fv_source.value().to_u32(),
            fv_smooth: p.fv_smooth.value(),
            fv_exposure_db: p.fv_exposure_db.value(),
            fv_calibrate: p.fv_calibrate.value(),
            fv_gain: p.fv_gain.value(),
            fv_lines: p.fv_lines.value(),
            fv_line_density: p.fv_line_density.value(),
            fv_line_thickness: p.fv_line_thickness.value(),
            col_mode: p.col_mode.value().to_u32(),
            col_lo_db: p.col_lo_db.value(),
            col_hi_db: p.col_hi_db.value(),
            col_lut: p.col_lut.value().to_u32(),
            col_source: p.col_source.value().to_u32(),
            col_amount: p.col_amount.value(),
            ax_count: p.ax_count.value(),
            ax_length: p.ax_length.value(),
            ax_bundle: p.ax_bundle.value(),
            ax_samples: p.ax_samples.value(),
            ax_thickness: p.ax_thickness.value(),
            ax_node_spacing: p.ax_node_spacing.value(),
            ax_node_dip: p.ax_node_dip.value(),
            ax_pulse_speed: p.ax_pulse_speed.value(),
            ax_pulse_width: p.ax_pulse_width.value(),
            ax_stagger: p.ax_stagger.value(),
            ax_splay: p.ax_splay.value(),
            ax_seed: p.ax_seed.value(),
            ax_mode: p.ax_mode.value().to_u32(),
            ax_mode_amount: p.ax_mode_amount.value(),
            ax_bend: p.ax_bend.value(),
            ax_curve: p.ax_curve.value(),
            ax_tortuosity: p.ax_tortuosity.value(),
            ax_dti: p.ax_dti.value(),
            ax_dispersion: p.ax_dispersion.value(),
            ax_polarization: p.ax_polarization.value(),
            nw_topology: p.nw_topology.value().to_u32(),
            nw_nodes: p.nw_nodes.value(),
            nw_connectivity: p.nw_connectivity.value(),
            nw_rewire: p.nw_rewire.value(),
            nw_layers: p.nw_layers.value(),
            nw_seed: p.nw_seed.value(),
            nw_extent: p.nw_extent.value(),
            nw_node_size: p.nw_node_size.value(),
            nw_node_glow: p.nw_node_glow.value(),
            nw_edge_thickness: p.nw_edge_thickness.value(),
            nw_edge_bow: p.nw_edge_bow.value(),
            nw_edge_samples: p.nw_edge_samples.value(),
            nw_pulse_speed: p.nw_pulse_speed.value(),
            nw_pulse_width: p.nw_pulse_width.value(),
            nw_edge_fibres: p.nw_edge_fibres.value(),
            nw_bundle_radius: p.nw_bundle_radius.value(),
            nw_edge_node_dip: p.nw_edge_node_dip.value(),
            nw_ranvier: p.nw_ranvier.value(),
            nw_dendrite: p.nw_dendrite.value(),
            nw_dendrite_count: p.nw_dendrite_count.value(),
            nw_fire_mode: p.nw_fire_mode.value().to_u32(),
            nw_threshold: p.nw_threshold.value(),
            nw_conduction: p.nw_conduction.value(),
            nw_refractory: p.nw_refractory.value(),
            nw_decay: p.nw_decay.value(),
            nw_deposit: p.nw_deposit.value(),
            nw_stim_rate: p.nw_stim_rate.value(),
            nw_motes: p.nw_motes.value(),
            nw_sign_colour: p.nw_sign_colour.value(),
            nw_sparsify: p.nw_sparsify.value(),
            nw_layer_gap: p.nw_layer_gap.value(),
            nw_mlp_drive: p.nw_mlp_drive.value(),
            nw_attn_layer: p.nw_attn_layer.value(),
            nw_attn_head: p.nw_attn_head.value(),
            nw_attn_threshold: p.nw_attn_threshold.value(),
            nw_attn_tokens: p.nw_attn_tokens.value(),
            nw_attn_reveal: p.nw_attn_reveal.value(),
            nw_attn_sweep: p.nw_attn_sweep.value(),
            nw_attn_ring: p.nw_attn_ring.value(),
            nt_soma_size: p.nt_soma_size.value(),
            nt_soma_shape: p.nt_soma_shape.value(),
            nt_bouton_size: p.nt_bouton_size.value(),
            nt_membrane_sss: p.nt_membrane_sss.value(),
            nt_membrane_irid: p.nt_membrane_irid.value(),
            nt_dendrite_density: p.nt_dendrite_density.value(),
            nt_dendrite_length: p.nt_dendrite_length.value(),
            nt_dendrite_taper: p.nt_dendrite_taper.value(),
            nt_neuron_type: p.nt_neuron_type.value().to_u32(),
            nt_spines: p.nt_spines.value(),
            nt_myelin_amount: p.nt_myelin_amount.value(),
            nt_ranvier_spacing: p.nt_ranvier_spacing.value(),
            nt_sheath_scale: p.nt_sheath_scale.value(),
            nt_synapse_cleft: p.nt_synapse_cleft.value(),
            nt_synapse_glow: p.nt_synapse_glow.value(),
            nt_synapse_vesicles: p.nt_synapse_vesicles.value(),
            nt_glia: p.nt_glia.value(),
            nt_capillary: p.nt_capillary.value(),
            br_fold_depth: p.br_fold_depth.value(),
            br_fold_freq: p.br_fold_freq.value(),
            br_hemi_gap: p.br_hemi_gap.value(),
            br_local_k: p.br_local_k.value(),
            br_cerebellum: p.br_cerebellum.value(),
            br_assoc: p.br_assoc.value(),
            br_callosum: p.br_callosum.value(),
            br_subcortical: p.br_subcortical.value(),
            br_region_hi: p.br_region_hi.value(),
            br_target: p.br_target.value(),
            br_stim_amount: p.br_stim_amount.value(),
            br_stim_rate: p.br_stim_rate.value(),
            br_signal_swell: p.br_signal_swell.value(),
            film_thickness: p.film_thickness.value(),
            film_thickness_var: p.film_thickness_var.value(),
            film_ior: p.film_ior.value(),
            film_drainage: p.film_drainage.value(),
            demo_scene: p.demo_scene.value().to_u32(),
            demo_size: p.demo_size.value(),
            demo_objects: p.demo_objects.value(),
            demo_static_cam: p.demo_static_cam.value(),
            demo_light: p.demo_light.value(),
            demo_roughness: p.demo_roughness.value(),
            demo_count: p.demo_count.value(),
            demo_spin: p.demo_spin.value(),
            sy_radius: p.sy_radius.value(),
            sy_beta: p.sy_beta.value(),
            sy_charges: p.sy_charges.value(),
            sy_grid: p.sy_grid.value(),
            sy_extent: p.sy_extent.value(),
            sy_near: p.sy_near.value(),
            sy_amp: p.sy_amp.value(),
            sy_thickness: p.sy_thickness.value(),
            sy_rmin: p.sy_rmin.value(),
            sy_perp: p.sy_perp.value(),
            sy_view: p.sy_view.value().to_u32(),
            sy_line_seeds: p.sy_line_seeds.value(),
            sy_line_steps: p.sy_line_steps.value(),
            sy_line_ds: p.sy_line_ds.value(),
            sy_line_bound: p.sy_line_bound.value(),
            sy_vol_layers: p.sy_vol_layers.value(),
            sy_reveal: p.sy_reveal.value(),
            sy_invert: p.sy_invert.value(),
            sy_invert_radius: p.sy_invert_radius.value(),
            sy_tilt: p.sy_tilt.value(),
            sy_precess: p.sy_precess.value(),
            vf_preset: p.vf_preset.value().to_u32(),
            vf_grid_x: p.vf_grid_x.value(),
            vf_grid_y: p.vf_grid_y.value(),
            vf_grid_z: p.vf_grid_z.value(),
            vf_extent: p.vf_extent.value(),
            vf_field_scale: p.vf_field_scale.value(),
            vf_amp: p.vf_amp.value(),
            vf_thickness: p.vf_thickness.value(),
            vf_mag_map: p.vf_mag_map.value().to_u32(),
            vf_tint_mode: p.vf_tint_mode.value().to_u32(),
            vf_evolve: p.vf_evolve.value(),
            vf_z_lift: p.vf_z_lift.value(),
            vf_reveal: p.vf_reveal.value(),
            vf_view: p.vf_view.value().to_u32(),
            vf_seed_mode: p.vf_seed_mode.value().to_u32(),
            vf_line_seeds: p.vf_line_seeds.value(),
            vf_line_steps: p.vf_line_steps.value(),
            vf_line_ds: p.vf_line_ds.value(),
            vf_bidir: p.vf_bidir.value(),
            vf_line_color: p.vf_line_color.value().to_u32(),
            vf_flow: p.vf_flow.value(),
            vf_flow_speed: p.vf_flow_speed.value(),
            vf_line_thickness: p.vf_line_thickness.value(),
            vb_x1_func: p.vb_x1_func.value().to_u32(),
            vb_x1_gain: p.vb_x1_gain.value(),
            vb_x1_a: p.vb_x1_a.value(),
            vb_x1_b: p.vb_x1_b.value(),
            vb_x1_c: p.vb_x1_c.value(),
            vb_x1_phase: p.vb_x1_phase.value(),
            vb_x2_func: p.vb_x2_func.value().to_u32(),
            vb_x2_gain: p.vb_x2_gain.value(),
            vb_x2_a: p.vb_x2_a.value(),
            vb_x2_b: p.vb_x2_b.value(),
            vb_x2_c: p.vb_x2_c.value(),
            vb_x2_phase: p.vb_x2_phase.value(),
            vb_x3_func: p.vb_x3_func.value().to_u32(),
            vb_x3_gain: p.vb_x3_gain.value(),
            vb_x3_a: p.vb_x3_a.value(),
            vb_x3_b: p.vb_x3_b.value(),
            vb_x3_c: p.vb_x3_c.value(),
            vb_x3_phase: p.vb_x3_phase.value(),
            vb_y1_func: p.vb_y1_func.value().to_u32(),
            vb_y1_gain: p.vb_y1_gain.value(),
            vb_y1_a: p.vb_y1_a.value(),
            vb_y1_b: p.vb_y1_b.value(),
            vb_y1_c: p.vb_y1_c.value(),
            vb_y1_phase: p.vb_y1_phase.value(),
            vb_y2_func: p.vb_y2_func.value().to_u32(),
            vb_y2_gain: p.vb_y2_gain.value(),
            vb_y2_a: p.vb_y2_a.value(),
            vb_y2_b: p.vb_y2_b.value(),
            vb_y2_c: p.vb_y2_c.value(),
            vb_y2_phase: p.vb_y2_phase.value(),
            vb_y3_func: p.vb_y3_func.value().to_u32(),
            vb_y3_gain: p.vb_y3_gain.value(),
            vb_y3_a: p.vb_y3_a.value(),
            vb_y3_b: p.vb_y3_b.value(),
            vb_y3_c: p.vb_y3_c.value(),
            vb_y3_phase: p.vb_y3_phase.value(),
            vb_z1_func: p.vb_z1_func.value().to_u32(),
            vb_z1_gain: p.vb_z1_gain.value(),
            vb_z1_a: p.vb_z1_a.value(),
            vb_z1_b: p.vb_z1_b.value(),
            vb_z1_c: p.vb_z1_c.value(),
            vb_z1_phase: p.vb_z1_phase.value(),
            vb_z2_func: p.vb_z2_func.value().to_u32(),
            vb_z2_gain: p.vb_z2_gain.value(),
            vb_z2_a: p.vb_z2_a.value(),
            vb_z2_b: p.vb_z2_b.value(),
            vb_z2_c: p.vb_z2_c.value(),
            vb_z2_phase: p.vb_z2_phase.value(),
            vb_z3_func: p.vb_z3_func.value().to_u32(),
            vb_z3_gain: p.vb_z3_gain.value(),
            vb_z3_a: p.vb_z3_a.value(),
            vb_z3_b: p.vb_z3_b.value(),
            vb_z3_c: p.vb_z3_c.value(),
            vb_z3_phase: p.vb_z3_phase.value(),
            vb_op: p.vb_op.value().to_u32(),
            vb_mix: p.vb_mix.value(),
            rl_speed: p.rl_speed.value(),
            rl_bore: p.rl_bore.value(),
            rl_cell_len: p.rl_cell_len.value().to_u32(),
            rl_variance: p.rl_variance.value(),
            rl_seed: p.rl_seed.value(),
            rl_ring_n: p.rl_ring_n.value(),
            rl_rows_beat: p.rl_rows_beat.value(),
            rl_horizon: p.rl_horizon.value(),
            rl_rib_gain: p.rl_rib_gain.value(),
            rl_thickness: p.rl_thickness.value(),
            rl_lobes: p.rl_lobes.value(),
            rl_spike: p.rl_spike.value(),
            rl_twist: p.rl_twist.value(),
            rl_swell: p.rl_swell.value(),
            rl_fade: p.rl_fade.value(),
            rl_color_flow: p.rl_color_flow.value(),
            rl_archetype: p.rl_archetype.value().to_u32(),
            rl_diverge: p.rl_diverge.value(),
            rl_shells: p.rl_shells.value(),
            rl_parastichy: p.rl_parastichy.value(),
            rl_change_every: p.rl_change_every.value().to_u32(),
            rl_evolve: p.rl_evolve.value(),
            sc_mode: p.sc_mode.value().to_u32(),
            sc_surface: p.sc_surface.value().to_u32(),
            sc_mat: p.sc_mat.value().to_u32(),
            sc_metallic: p.sc_metallic.value(),
            sc_roughness: p.sc_roughness.value(),
            sc_glow: p.sc_glow.value(),
            sc_emissive: p.sc_emissive.value(),
            sc_opacity: p.sc_opacity.value(),
            sc_ior: p.sc_ior.value(),
            sc_palette: p.sc_palette.value().to_u32(),
            sc_sss: p.sc_sss.value(),
            sc_sss_dist: p.sc_sss_dist.value(),
            sc_sss_pow: p.sc_sss_pow.value(),
            sc_irid: p.sc_irid.value(),
            sc_irid_scale: p.sc_irid_scale.value(),
            sc_irid_shift: p.sc_irid_shift.value(),
            terra_form: p.terra_form.value().to_u32(),
            terra_ridge: p.terra_ridge.value(),
            terra_channel: p.terra_channel.value(),
            terra_width: p.terra_width.value(),
            terra_steep: p.terra_steep.value(),
            terra_terrace: p.terra_terrace.value(),
            terra_rough: p.terra_rough.value(),
            terra_meander: p.terra_meander.value(),
            terra_water_level: p.terra_water_level.value(),
            terra_water_on: p.terra_water_on.value(),
            terra_clearance: p.terra_clearance.value(),
            terra_noise_freq: p.terra_noise_freq.value(),
            wt_mat: p.wt_mat.value().to_u32(),
            wt_roughness: p.wt_roughness.value(),
            wt_ior: p.wt_ior.value(),
            wt_opacity: p.wt_opacity.value(),
            wt_glow: p.wt_glow.value(),
            wt_ripple: p.wt_ripple.value(),
            wt_ripple_freq: p.wt_ripple_freq.value(),
            wt_absorb: p.wt_absorb.value(),
            wt_glitter: p.wt_glitter.value(),
            wt_reflect: p.wt_reflect.value(),
            phyl_surface: p.phyl_surface.value().to_u32(),
            phyl_count: p.phyl_count.value(),
            phyl_divergence: p.phyl_divergence.value(),
            phyl_radius: p.phyl_radius.value(),
            phyl_parastichy: p.phyl_parastichy.value(),
            phyl_height: p.phyl_height.value(),
            phyl_growth: p.phyl_growth.value(),
            phyl_breathe_amp: p.phyl_breathe_amp.value(),
            phyl_breathe_freq: p.phyl_breathe_freq.value(),
            phyl_rot: p.phyl_rot.value(),
            phyl_thickness: p.phyl_thickness.value(),
            tess_family: p.tess_family.value().to_u32(),
            tess_depth: p.tess_depth.value(),
            tess_scale: p.tess_scale.value(),
            tess_thickness: p.tess_thickness.value(),
            tess_view: p.tess_view.value().to_u32(),
            tess_height: p.tess_height.value(),
            tess_height_mode: p.tess_height_mode.value().to_u32(),
            tess_beat_infl: p.tess_beat_infl.value(),
            tess_ripple_amt: p.tess_ripple_amt.value(),
            tess_ripple_freq: p.tess_ripple_freq.value(),
            tess_construct: p.tess_construct.value().to_u32(),
            tess_phason: p.tess_phason.value(),
            tess_grid_n: p.tess_grid_n.value(),
            tess_ammann: p.tess_ammann.value(),
            tess_hyp_p: p.tess_hyp_p.value(),
            tess_hyp_q: p.tess_hyp_q.value(),
            mb_power: p.mb_power.value(),
            mb_iter: p.mb_iter.value(),
            mb_scale: p.mb_scale.value(),
            mb_detail: p.mb_detail.value(),
            mb_spin: p.mb_spin.value(),
            mb_morph: p.mb_morph.value(),
            mb_color: p.mb_color.value(),
            mb_bailout: p.mb_bailout.value(),
            cr_form: p.cr_form.value(),
            cr_scale: p.cr_scale.value(),
            cr_detail: p.cr_detail.value(),
            cr_swim: p.cr_swim.value(),
            cr_warp_amp: p.cr_warp_amp.value(),
            cr_warp_freq: p.cr_warp_freq.value(),
            cr_rim: p.cr_rim.value(),
            cr_glow: p.cr_glow.value(),
            cr_wave_speed: p.cr_wave_speed.value(),
            cr_wave_freq: p.cr_wave_freq.value(),
            cr_wave_sharp: p.cr_wave_sharp.value(),
            cr_wave_amt: p.cr_wave_amt.value(),
            cr_overlay: p.cr_overlay.value(),
            cr_overlay_opacity: p.cr_overlay_opacity.value(),
            cr_overlay_bright: p.cr_overlay_bright.value(),
            ms_family: p.ms_family.value().to_u32(),
            ms_scale: p.ms_scale.value(),
            ms_cells: p.ms_cells.value(),
            ms_iso: p.ms_iso.value(),
            ms_thickness: p.ms_thickness.value(),
            ms_twist: p.ms_twist.value(),
            ms_detail: p.ms_detail.value(),
            ms_color: p.ms_color.value(),
            ms_beat_iso: p.ms_beat_iso.value(),
            ms_bend: p.ms_bend.value(),
            ms_uv_res: p.ms_uv_res.value(),
            ms_extent: p.ms_extent.value(),
            ms_bend_phase: p.ms_bend_phase.value(),
            ms_turns: p.ms_turns.value(),
            ms_form_res: p.ms_form_res.value(),
            lens_focal: p.lens_focal.value(),
            lens_aperture: p.lens_aperture.value(),
            lens_thickness: p.lens_thickness.value(),
            lens_plano: p.lens_plano.value(),
            lens_scale: p.lens_scale.value(),
            lens_detail: p.lens_detail.value(),
            kf_sectors: p.kf_sectors.value(),
            kf_fold: p.kf_fold.value(),
            kf_iter: p.kf_iter.value(),
            kf_iter_rot: p.kf_iter_rot.value(),
            kf_spin: p.kf_spin.value(),
            kf_breathe: p.kf_breathe.value(),
            kf_zoom: p.kf_zoom.value(),
            kf_tunnel: p.kf_tunnel.value(),
            kf_rays: p.kf_rays.value(),
            kf_ring: p.kf_ring.value(),
            kf_glow: p.kf_glow.value(),
            kf_hue: p.kf_hue.value(),
            kf_pattern: p.kf_pattern.value().to_u32(),
            kf_palette: p.kf_palette.value().to_u32(),
            kf_color_speed: p.kf_color_speed.value(),
            kf_churn: p.kf_churn.value(),
            kf_e8_flow: p.kf_e8_flow.value(),
            kf_warp: p.kf_warp.value(),
            kf_flow: p.kf_flow.value(),
            kf_petals: p.kf_petals.value(),
            kf_contrast: p.kf_contrast.value(),
            kf_sharp: p.kf_sharp.value(),
            kf_invert: p.kf_invert.value(),
            kf_dispersion: p.kf_dispersion.value(),
            kf_space: p.kf_space.value().to_u32(),
            kf_view: p.kf_view.value().to_u32(),
            kf_relief: p.kf_relief.value(),
            kf_relief_elev: p.kf_relief_elev.value(),
            kf_relief_steps: p.kf_relief_steps.value(),
            kf_relief_shine: p.kf_relief_shine.value(),
            kal_on: p.kal_on.value(),
            kal_sectors: p.kal_sectors.value(),
            kal_mode: p.kal_mode.value().to_u32(),
            kal_spin: p.kal_spin.value(),
            kal_roll: p.kal_roll.value(),
            kal_zoom: p.kal_zoom.value(),
            kal_center_x: p.kal_center_x.value(),
            kal_center_y: p.kal_center_y.value(),
            kal_mix: p.kal_mix.value(),
            kal_twist: p.kal_twist.value(),
            kal_tint_hue: p.kal_tint_hue.value(),
            kal_tint_amt: p.kal_tint_amt.value(),
            kal_seam: p.kal_seam.value(),
            instr_hud: p.instr_hud.value(),
            instr_probe_on: p.instr_probe_on.value(),
            instr_probe_x: p.instr_probe_x.value(),
            instr_probe_y: p.instr_probe_y.value(),
            instr_probe_z: p.instr_probe_z.value(),
            instr_ledger_on: p.instr_ledger_on.value(),
            instr_ledger_half: p.instr_ledger_half.value(),
            instr_ledger_res: p.instr_ledger_res.value(),
            instr_flux_on: p.instr_flux_on.value(),
            instr_flux_x: p.instr_flux_x.value(),
            instr_flux_y: p.instr_flux_y.value(),
            instr_flux_z: p.instr_flux_z.value(),
            instr_flux_size: p.instr_flux_size.value(),
            instr_flux_axis: p.instr_flux_axis.value().to_u32(),
            instr_flux_res: p.instr_flux_res.value(),
            instr_csv_log: p.instr_csv_log.value(),
            instr_panel_opacity: p.instr_panel_opacity.value(),
            instr_panel_bevel: p.instr_panel_bevel.value(),
            instr_hud_scale: p.instr_hud_scale.value(),
            instr_hud_dock: p.instr_hud_dock.value().to_u32(),
            fdtd_on: p.fdtd_on.value(),
            fdtd_res: p.fdtd_res.value(),
            fdtd_source: p.fdtd_source.value().to_u32(),
            fdtd_freq: p.fdtd_freq.value(),
            fdtd_drive: p.fdtd_drive.value(),
            fdtd_substeps: p.fdtd_substeps.value(),
            fdtd_boundary: p.fdtd_boundary.value(),
            fdtd_extent: p.fdtd_extent.value(),
            surface_mode: p.surface_mode.value().to_u32(),
            origin_mode: p.origin_mode.value().to_u32(),
            bevel: p.bevel.value(),
            mat_enable: p.mat_enable.value(),
            mat_projection: p.mat_projection.value().to_u32(),
            mat_scale: p.mat_scale.value(),
            mp_enable: p.mp_enable.value(),
            mp_noise: p.mp_noise.value().to_u32(),
            mp_channel: p.mp_channel.value().to_u32(),
            mp_scale: p.mp_scale.value(),
            mp_rotation: p.mp_rotation.value(),
            mp_offset_x: p.mp_offset_x.value(),
            mp_offset_y: p.mp_offset_y.value(),
            mp_octaves: p.mp_octaves.value(),
            mp_lacunarity: p.mp_lacunarity.value(),
            mp_gain: p.mp_gain.value(),
            mp_warp: p.mp_warp.value(),
            mp_contrast: p.mp_contrast.value(),
            mp_gamma: p.mp_gamma.value(),
            mp_remap_lo: p.mp_remap_lo.value(),
            mp_remap_hi: p.mp_remap_hi.value(),
            mp_invert: p.mp_invert.value(),
            mp_seed: p.mp_seed.value(),
            mp_res: p.mp_res.value().to_u32(),
            mp_lo_r: p.mp_lo_r.value(),
            mp_lo_g: p.mp_lo_g.value(),
            mp_lo_b: p.mp_lo_b.value(),
            mp_hi_r: p.mp_hi_r.value(),
            mp_hi_g: p.mp_hi_g.value(),
            mp_hi_b: p.mp_hi_b.value(),
            mp2_enable: p.mp2_enable.value(),
            mp2_blend: p.mp2_blend.value().to_u32(),
            mp2_noise: p.mp2_noise.value().to_u32(),
            mp2_channel: p.mp2_channel.value().to_u32(),
            mp2_scale: p.mp2_scale.value(),
            mp2_rotation: p.mp2_rotation.value(),
            mp2_offset_x: p.mp2_offset_x.value(),
            mp2_offset_y: p.mp2_offset_y.value(),
            mp2_octaves: p.mp2_octaves.value(),
            mp2_lacunarity: p.mp2_lacunarity.value(),
            mp2_gain: p.mp2_gain.value(),
            mp2_warp: p.mp2_warp.value(),
            mp2_contrast: p.mp2_contrast.value(),
            mp2_gamma: p.mp2_gamma.value(),
            mp2_remap_lo: p.mp2_remap_lo.value(),
            mp2_remap_hi: p.mp2_remap_hi.value(),
            mp2_invert: p.mp2_invert.value(),
            mp2_seed: p.mp2_seed.value(),
            mp2_lo_r: p.mp2_lo_r.value(),
            mp2_lo_g: p.mp2_lo_g.value(),
            mp2_lo_b: p.mp2_lo_b.value(),
            mp2_hi_r: p.mp2_hi_r.value(),
            mp2_hi_g: p.mp2_hi_g.value(),
            mp2_hi_b: p.mp2_hi_b.value(),
            mat_derive_normal: p.mat_derive_normal.value(),
            mat_derive_ao: p.mat_derive_ao.value(),
            mat_normal_source_albedo: p.mat_normal_source_albedo.value(),
            mat_derive_normal_strength: p.mat_derive_normal_strength.value(),
            mat_derive_ao_strength: p.mat_derive_ao_strength.value(),
            mat_derive_ao_radius: p.mat_derive_ao_radius.value(),
            mat_anim_enable: p.mat_anim_enable.value(),
            mat_anim_speed: p.mat_anim_speed.value(),
            mat_anim_mode: p.mat_anim_mode.value().to_u32(),
            mat_flow_x: p.mat_flow_x.value(),
            mat_flow_y: p.mat_flow_y.value(),
            mat_displace: p.mat_displace.value(),
            plexus_radius: p.plexus_radius.value(),
            plexus_links: p.plexus_links.value(),
            plexus_strut: p.plexus_strut.value(),
            plexus_marker: p.plexus_marker.value(),
            plexus_impostor: p.plexus_impostor.value(),
            plexus_edges: p.plexus_edges.value(),
            plexus_node_radius: p.plexus_node_radius.value(),
            plexus_edge_radius: p.plexus_edge_radius.value(),
            plexus_node_type: p.plexus_node_type.value().to_u32(),
            plexus_node_metallic: p.plexus_node_metallic.value(),
            plexus_node_rough: p.plexus_node_rough.value(),
            plexus_node_ior: p.plexus_node_ior.value(),
            plexus_node_hue: p.plexus_node_hue.value(),
            plexus_node_sat: p.plexus_node_sat.value(),
            plexus_node_val: p.plexus_node_val.value(),
            plexus_node_emissive: p.plexus_node_emissive.value(),
            plexus_edge_type: p.plexus_edge_type.value().to_u32(),
            plexus_edge_metallic: p.plexus_edge_metallic.value(),
            plexus_edge_rough: p.plexus_edge_rough.value(),
            plexus_edge_ior: p.plexus_edge_ior.value(),
            plexus_edge_hue: p.plexus_edge_hue.value(),
            plexus_edge_sat: p.plexus_edge_sat.value(),
            plexus_edge_val: p.plexus_edge_val.value(),
            plexus_edge_emissive: p.plexus_edge_emissive.value(),
            plexus_signal: p.plexus_signal.value(),
            plexus_signal_speed: p.plexus_signal_speed.value(),
            plexus_signal_gain: p.plexus_signal_gain.value(),
            plexus_signal_width: p.plexus_signal_width.value(),
            plexus_node_shape: p.plexus_node_shape.value(),
            plexus_edge_shape: p.plexus_edge_shape.value(),
            plexus_overlay_on: p.plexus_overlay_on.value(),
            plexus_shell_scale: p.plexus_shell_scale.value(),
            plexus_shell_depth: p.plexus_shell_depth.value(),
            plexus_shell_bins: p.plexus_shell_bins.value(),
            animate: p.animate.value(),
            inc_scale: p.inc_scale.value(),
            speed_exp: p.speed_exp.value(),
            ambient: p.ambient.value(),
            key_intensity: p.key_intensity.value(),
            fill_intensity: p.fill_intensity.value(),
            elevation: p.elevation.value(),
            azimuth: p.azimuth.value(),
            glow: p.glow.value(),
            mat_emissive: p.mat_emissive.value(),
            opacity: p.opacity.value(),
            pulse: p.pulse.value(),
            tempo_sync: p.tempo_sync.value(),
            tempo: p.tempo.value(),
            pulse_source: p.pulse_source.value().to_u32(),
            audio_react: p.audio_react.value(),
            audio_gain: p.audio_gain.value(),
            audio_attack: p.audio_attack.value(),
            audio_release: p.audio_release.value(),
            mat_type: p.mat_type.value().to_u32(),
            ior: p.ior.value(),
            mat_absorb: p.mat_absorb.value(),
            refr_overlay: p.refr_overlay.value(),
            refr_blend: p.refr_blend.value(),
            refract_ss: p.refract_ss.value(),
            refract_dist: p.refract_dist.value(),
            anisotropy: p.anisotropy.value(),
            aniso_rotation: p.aniso_rotation.value(),
            aniso_overlay: p.aniso_overlay.value(),
            aniso_blend: p.aniso_blend.value(),
            clearcoat: p.clearcoat.value(),
            clearcoat_rough: p.clearcoat_rough.value(),
            clearcoat_overlay: p.clearcoat_overlay.value(),
            sheen: p.sheen.value(),
            sheen_rough: p.sheen_rough.value(),
            sheen_tint: p.sheen_tint.value(),
            sheen_overlay: p.sheen_overlay.value(),
            sss_thickness: p.sss_thickness.value(),
            sss_radius: p.sss_radius.value(),
            interior_scatter: p.interior_scatter.value(),
            glitter: p.glitter.value(),
            glitter_density: p.glitter_density.value(),
            glitter_sharpness: p.glitter_sharpness.value(),
            diffraction: p.diffraction.value(),
            diffraction_freq: p.diffraction_freq.value(),
            retro: p.retro.value(),
            fluorescence: p.fluorescence.value(),
            fluor_hue: p.fluor_hue.value(),
            incandescence: p.incandescence.value(),
            temperature: p.temperature.value(),
            metallic: p.metallic.value(),
            roughness: p.roughness.value(),
            exposure: p.exposure.value(),
            env_intensity: p.env_intensity.value(),
            env_rotation: p.env_rotation.value(),
            bloom_intensity: p.bloom_intensity.value(),
            bloom_threshold: p.bloom_threshold.value(),
            cam_path: p.cam_path.value().to_u32(),
            cam_speed: p.cam_speed.value(),
            cam_kick: p.cam_kick.value(),
            cam_damping: p.cam_damping.value(),
            cam_amount: p.cam_amount.value(),
            cam_beat_momentum: p.cam_beat_momentum.value(),
            cam_seq_enabled: p.cam_seq_enabled.value(),
            cam_bars_per_shot: p.cam_bars_per_shot.value().to_u32(),
            cam_seq_order: p.cam_seq_order.value().to_u32(),
            cam_transition: p.cam_transition.value().to_u32(),
            cam_transition_bars: p.cam_transition_bars.value(),
            cam_dolly_period: p.cam_dolly_period.value(),
            cam_dolly_depth: p.cam_dolly_depth.value(),
            cam_dolly_wave: p.cam_dolly_wave.value().to_u32(),
            tempo_source: p.tempo_source.value().to_u32(),
            beats_per_bar: p.beats_per_bar.value(),
            cam_roll: p.cam_roll.value(),
            cam_fov: p.cam_fov.value(),
            cam_fov_dolly: p.cam_fov_dolly.value(),
            cam_hold_prob: p.cam_hold_prob.value(),
            cam_phrase_lock: p.cam_phrase_lock.value(),
            cam_seq_mix: p.cam_seq_mix.value(),
            cam_story_enabled: p.cam_story_enabled.value(),
            cam_story_count: p.cam_story_count.value(),
            cam_story_mode: p.cam_story_mode.value().to_u32(),
            cam_story_seed: p.cam_story_seed.value(),
            cam_shot0_path: p.cam_shot0_path.value().to_u32(),
            cam_shot0_bars: p.cam_shot0_bars.value().to_u32(),
            cam_shot0_radius: p.cam_shot0_radius.value(),
            cam_shot1_path: p.cam_shot1_path.value().to_u32(),
            cam_shot1_bars: p.cam_shot1_bars.value().to_u32(),
            cam_shot1_radius: p.cam_shot1_radius.value(),
            cam_shot2_path: p.cam_shot2_path.value().to_u32(),
            cam_shot2_bars: p.cam_shot2_bars.value().to_u32(),
            cam_shot2_radius: p.cam_shot2_radius.value(),
            cam_shot3_path: p.cam_shot3_path.value().to_u32(),
            cam_shot3_bars: p.cam_shot3_bars.value().to_u32(),
            cam_shot3_radius: p.cam_shot3_radius.value(),
            mod_a_target: p.mod_a_target.value().to_u32(),
            mod_a_depth: p.mod_a_depth.value(),
            mod_b_target: p.mod_b_target.value().to_u32(),
            mod_b_depth: p.mod_b_depth.value(),
            speed_pulse_amount: p.speed_pulse_amount.value(),
            speed_pulse_attack: p.speed_pulse_attack.value(),
            speed_pulse_decay: p.speed_pulse_decay.value(),
            breath_amount: p.breath_amount.value(),
            breath_attack: p.breath_attack.value(),
            breath_decay: p.breath_decay.value(),
            subsurface: p.subsurface.value(),
            sss_distortion: p.sss_distortion.value(),
            sss_power: p.sss_power.value(),
            iridescence: p.iridescence.value(),
            irid_scale: p.irid_scale.value(),
            irid_shift: p.irid_shift.value(),
            palette: p.palette.value().to_u32(),
            ssao: p.ssao.value(),
            ssao_radius: p.ssao_radius.value(),
            ssao_intensity: p.ssao_intensity.value(),
            ssao_bias: p.ssao_bias.value(),
            tonemap: p.tonemap.value().to_u32(),
            bg_tonemap: p.bg_tonemap.value().to_u32(),
            bg_visible: p.bg_visible.value(),
            bg_intensity: p.bg_intensity.value(),
            env_tint_hue: p.env_tint_hue.value(),
            env_tint_amt: p.env_tint_amt.value(),
            mat_hue: p.mat_hue.value(),
            mat_hue_cycle: p.mat_hue_cycle.value(),
            mat_saturation: p.mat_saturation.value(),
            mat_value: p.mat_value.value(),
            scen_hue: p.scen_hue.value(),
            scen_hue_cycle: p.scen_hue_cycle.value(),
            scen_saturation: p.scen_saturation.value(),
            scen_value: p.scen_value.value(),
            sky_reflect_clouds: p.sky_reflect_clouds.value(),
            sky_cloud_cover: p.sky_cloud_cover.value(),
            sky_cloud_speed: p.sky_cloud_speed.value(),
            sky_cloud_strength: p.sky_cloud_strength.value(),
            // Neural radiance cache — live (#256 Tier 0).
            nrc_enable: p.nrc_enable.value(),
            nrc_confidence: p.nrc_confidence.value(),
            nrc_learn_rate: p.nrc_learn_rate.value(),
            nrc_omega: p.nrc_omega.value(),
            nrc_terminate: p.nrc_terminate.value(),
            nrc_train_samples: p.nrc_train_samples.value(),
            nrc_seed: p.nrc_seed.value(),
            // Neural radiance cache — RT-stack synergies (#256 Tier 1).
            nrc_guide: p.nrc_guide.value(),
            nrc_guide_candidates: p.nrc_guide_candidates.value(),
            nrc_firefly: p.nrc_firefly.value(),
            nrc_firefly_clamp: p.nrc_firefly_clamp.value(),
            // Neural radiance cache — light-field uses (#256 Tier 2).
            nrc_gi: p.nrc_gi.value(),
            nrc_gi_strength: p.nrc_gi_strength.value(),
            nrc_reflect: p.nrc_reflect.value(),
            // Neural radiance cache — hard transport + volumetrics (#256 Tier 3).
            nrc_volume: p.nrc_volume.value(),
            nrc_volume_density: p.nrc_volume_density.value(),
            nrc_volume_steps: p.nrc_volume_steps.value(),
            nrc_volume_strength: p.nrc_volume_strength.value(),
            nrc_caustic: p.nrc_caustic.value(),
            nrc_caustic_gain: p.nrc_caustic_gain.value(),
            particles_bead_hue: p.particles_bead_hue.value(),
            particles_bead_hue_cycle: p.particles_bead_hue_cycle.value(),
            particles_bead_sat: p.particles_bead_sat.value(),
            particles_bead_val: p.particles_bead_val.value(),
            particles_bead_emissive: p.particles_bead_emissive.value(),
            metaball_radius: p.metaball_radius.value(),
            metaball_threshold: p.metaball_threshold.value(),
            metaball_smooth: p.metaball_smooth.value(),
            splat_radius: p.splat_radius.value(),
            splat_opacity: p.splat_opacity.value(),
            splat_falloff: p.splat_falloff.value(),
            splat_mode: p.splat_mode.value().to_u32(),
            splat_cutoff: p.splat_cutoff.value(),
            splat_aniso: p.splat_aniso.value(),
            splat_scatter: p.splat_scatter.value(),
            splat_jitter: p.splat_jitter.value(),
            splat_solid: p.splat_solid.value(),
            ma_kind: p.ma_kind.value().to_u32(),
            ma_a: p.ma_a.value(),
            ma_b: p.ma_b.value(),
            ma_c: p.ma_c.value(),
            ma_d: p.ma_d.value(),
            ma_color: p.ma_color.value().to_u32(),
            ma_points_k: p.ma_points_k.value(),
            ma_warmup: p.ma_warmup.value(),
            ma_scale: p.ma_scale.value(),
            ma_size: p.ma_size.value(),
            ma_intensity: p.ma_intensity.value(),
            ma_a_drive: p.ma_a_drive.value(),
            ma_b_drive: p.ma_b_drive.value(),
            ma_orbit: p.ma_orbit.value().to_u32(),
            ma_loop_beats: p.ma_loop_beats.value(),
            ma_orbit_ra: p.ma_orbit_ra.value(),
            ma_orbit_rb: p.ma_orbit_rb.value(),
            ma_orbit_fa: p.ma_orbit_fa.value(),
            ma_orbit_fb: p.ma_orbit_fb.value(),
            ma_orbit_psi: p.ma_orbit_psi.value(),
            ma_orbit_free: p.ma_orbit_free.value(),
            tube_weld: p.tube_weld.value(),
            tube_end_cap: p.tube_end_cap.value(),
            tube_cap_round: p.tube_cap_round.value(),
            tube_cap_bevel: p.tube_cap_bevel.value(),
            tube_profile: p.tube_profile.value(),
            voxel_res: p.voxel_res.value(),
            voxel_threshold: p.voxel_threshold.value(),
            voxel_radius: p.voxel_radius.value(),
            voxel_emission: p.voxel_emission.value(),
            voxel_ao: p.voxel_ao.value(),
            voxel_shadow: p.voxel_shadow.value(),
            voxel_quantize: p.voxel_quantize.value(),
            voxel_beat: p.voxel_beat.value(),
            voxel_gi: p.voxel_gi.value(),
            voxel_gi_strength: p.voxel_gi_strength.value(),
            voxel_gi_distance: p.voxel_gi_distance.value(),
            voxel_gi_sky: p.voxel_gi_sky.value(),
            color_cycle: p.color_cycle.value(),
            ripple_intensity: p.ripple_intensity.value(),
            ripple_speed: p.ripple_speed.value(),
            ripple_freq: p.ripple_freq.value(),
            ripple_sharp: p.ripple_sharp.value(),
            ripple_geom: p.ripple_geom.value().to_u32(),
            membrane_weave: p.membrane_weave.value().to_u32(),
            membrane_show_strands: p.membrane_show_strands.value(),
            membrane_arms: p.membrane_arms.value(),
            membrane_arm_build: p.membrane_arm_build.value().to_u32(),
            membrane_close: p.membrane_close.value(),
            membrane_arm_radius: p.membrane_arm_radius.value(),
            rd_intensity: p.rd_intensity.value(),
            rd_feed: p.rd_feed.value(),
            rd_kill: p.rd_kill.value(),
            rd_scale: p.rd_scale.value(),
            rd_albedo_mix: p.rd_albedo_mix.value(),
            particles_tier: p.particles_tier.value().to_u32(),
            particles_count_k: p.particles_count_k.value(),
            particles_grid_res: p.particles_grid_res.value(),
            particles_speed: p.particles_speed.value(),
            particles_lifetime: p.particles_lifetime.value(),
            particles_spawn_radius: p.particles_spawn_radius.value(),
            particles_size: p.particles_size.value(),
            particles_emissive: p.particles_emissive.value(),
            particles_ribbon: p.particles_ribbon.value(),
            particles_ribbon_stretch: p.particles_ribbon_stretch.value(),
            particles_hue_shift: p.particles_hue_shift.value(),
            particles_beat_burst: p.particles_beat_burst.value(),
            particles_drag: p.particles_drag.value(),
            particles_turbulence: p.particles_turbulence.value(),
            particles_alpha: p.particles_alpha.value(),
            particles_hide_generator: p.particles_hide_generator.value(),
            particles_beads: p.particles_beads.value(),
            particles_metallic: p.particles_metallic.value(),
            particles_roughness: p.particles_roughness.value(),
            particles_material: p.particles_material.value().to_u32(),
            particles_shape: p.particles_shape.value().to_u32(),
            particles_ior: p.particles_ior.value(),
            particles_shape_param: p.particles_shape_param.value(),
            particles_beads_rt: p.particles_beads_rt.value(),
            mn_energize: p.mn_energize.value(),
            mn_gain: p.mn_gain.value(),
            mn_knee: p.mn_knee.value(),
            mn_hue: p.mn_hue.value(),
            mn_antenna: p.mn_antenna.value(),
            mn_antenna_len: p.mn_antenna_len.value(),
            mn_dye_inject: p.mn_dye_inject.value(),
            mx_aura_blend: p.mx_aura_blend.value(),
            // Legacy migration inputs — never captured from live params (only read
            // from old preset JSON by `migrate_legacy_fields`).
            mx_trace_b: None,
            mn_aura_field: None,
            mn_force: p.mn_force.value(),
            mn_force_gain: p.mn_force_gain.value(),
            mn_energy_contrast: p.mn_energy_contrast.value(),
            mn_stir_rate: p.mn_stir_rate.value(),
            mn_pump: p.mn_pump.value(),
            mn_swirl_beat: p.mn_swirl_beat.value(),
            mn_pump_scale: p.mn_pump_scale.value(),
            mn_swirl_decay: p.mn_swirl_decay.value(),
            mn_mode_mix: p.mn_mode_mix.value(),
            mn_ring_freq: p.mn_ring_freq.value(),
            mn_hue_cycle: p.mn_hue_cycle.value(),
            ad_drive: p.ad_drive.value(),
            ad_amount: p.ad_amount.value(),
            ad_floor: p.ad_floor.value(),
            ad_multipole: p.ad_multipole.value(),
            ad_spread: p.ad_spread.value(),
            ad_band_hue: p.ad_band_hue.value(),
            ad_stereo: p.ad_stereo.value(),
            ad_pitch: p.ad_pitch.value(),
            ad_wave: p.ad_wave.value(),
            fluid_force: p.fluid_force.value(),
            fluid_vorticity: p.fluid_vorticity.value(),
            fluid_dissipation: p.fluid_dissipation.value(),
            fluid_iters: p.fluid_iters.value(),
            fluid_inflow_decay: p.fluid_inflow_decay.value(),
            // Fluid Ink (#182 Tier 1).
            ink_enabled: p.ink_enabled.value(),
            ink_rate: p.ink_rate.value(),
            ink_radius: p.ink_radius.value(),
            ink_extinction: p.ink_extinction.value(),
            ink_scatter: p.ink_scatter.value(),
            ink_emissive: p.ink_emissive.value(),
            ink_anisotropy: p.ink_anisotropy.value(),
            ink_dissipation: p.ink_dissipation.value(),
            ink_steps: p.ink_steps.value(),
            ink_maccormack: p.ink_maccormack.value(),
            ink_half_res: p.ink_half_res.value(),
            ink_reveal: p.ink_reveal.value(),
            // Fluid medium Tier 2 (#182).
            fl2_boundaries: p.fl2_boundaries.value(),
            fl2_buoyancy: p.fl2_buoyancy.value(),
            fl2_heat_decay: p.fl2_heat_decay.value(),
            fl2_detail: p.fl2_detail.value(),
            fl2_splash: p.fl2_splash.value(),
            fl2_dye_gate: p.fl2_dye_gate.value(),
            fl2_res: p.fl2_res.value(),
            fl2_substeps: p.fl2_substeps.value(),
            // MLS-MPM liquid (#182 Tier 3a).
            liq_enabled: p.liq_enabled.value(),
            liq_count: p.liq_count.value(),
            liq_res: p.liq_res.value(),
            liq_gravity: p.liq_gravity.value(),
            liq_stiffness: p.liq_stiffness.value(),
            liq_viscosity: p.liq_viscosity.value(),
            liq_container: p.liq_container.value(),
            liq_open_top: p.liq_open_top.value(),
            liq_collide: p.liq_collide.value(),
            liq_stir: p.liq_stir.value(),
            liq_density: p.liq_density.value(),
            liq_threshold: p.liq_threshold.value(),
            liq_hue: p.liq_hue.value(),
            liq_sat: p.liq_sat.value(),
            liq_substeps: p.liq_substeps.value(),
            liq_offset_y: p.liq_offset_y.value(),
            liq_shape: p.liq_shape.value().to_u32(),
            liq_reveal: p.liq_reveal.value(),
            fgi_gi: p.fgi_gi.value(),
            fgi_shadow: p.fgi_shadow.value(),
            fgi_receive: p.fgi_receive.value(),
            fgi_sway: p.fgi_sway.value(),
            ca_amount: p.ca_amount.value(),
            ca_sharpness: p.ca_sharpness.value(),
            liq_material: p.liq_material.value().to_u32(),
            liq_metallic: p.liq_metallic.value(),
            liq_roughness: p.liq_roughness.value(),
            liq_ior: p.liq_ior.value(),
            ghost_light: p.ghost_light.value(),
            liq_render: p.liq_render.value().to_u32(),
            liq_absorb: p.liq_absorb.value(),
            liq_glow: p.liq_glow.value(),
            liq_chrome_purity: p.liq_chrome_purity.value(),
            liq_glass_clarity: p.liq_glass_clarity.value(),
            liq_f0: p.liq_f0.value(),
            liq_dispersion: p.liq_dispersion.value(),
            liq_gcaustic: p.liq_gcaustic.value(),
            liq_thin_film: p.liq_thin_film.value(),
            // Hardware RT (#195).
            rt_enable: p.rt_enable.value(),
            rt_shadows: p.rt_shadows.value(),
            rt_shadow_soft: p.rt_shadow_soft.value(),
            rt_shadow_strength: p.rt_shadow_strength.value(),
            rt_shadow_fill: p.rt_shadow_fill.value(),
            rt_reflect: p.rt_reflect.value(),
            rt_reflect_intensity: p.rt_reflect_intensity.value(),
            rt_reflect_rough: p.rt_reflect_rough.value(),
            rt_reflect_reach: p.rt_reflect_reach.value(),
            rt_reflect_shadows: p.rt_reflect_shadows.value(),
            rt_reflect_rays: p.rt_reflect_rays.value(),
            ao_source: p.ao_source.value().to_u32(),
            rt_ao_rays: p.rt_ao_rays.value(),
            rt_gi: p.rt_gi.value(),
            rt_gi_intensity: p.rt_gi_intensity.value(),
            rt_gi_rays: p.rt_gi_rays.value(),
            rt_gi_reach: p.rt_gi_reach.value(),
            rt_gi_shadows: p.rt_gi_shadows.value(),
            rt_temporal: p.rt_temporal.value(),
            rt_temporal_feedback: p.rt_temporal_feedback.value(),
            rt_temporal_beat: p.rt_temporal_beat.value(),
            rt_temporal_variance: p.rt_temporal_variance.value(),
            rt_temporal_accum: p.rt_temporal_accum.value(),
            rt_temporal_clamp: p.rt_temporal_clamp.value(),
            rt_denoise: p.rt_denoise.value(),
            rt_denoise_amount: p.rt_denoise_amount.value(),
            nd_enable: p.nd_enable.value(),
            nd_strength: p.nd_strength.value(),
            nd_seed: p.nd_seed.value(),
            nd_omega: p.nd_omega.value(),
            up_enable: p.up_enable.value(),
            up_sharpen: p.up_sharpen.value(),
            up_seed: p.up_seed.value(),
            neural_enable: p.neural_enable.value(),
            neural_seed_a: p.neural_seed_a.value(),
            neural_seed_b: p.neural_seed_b.value(),
            neural_walk: p.neural_walk.value(),
            neural_omega: p.neural_omega.value(),
            neural_scale: p.neural_scale.value(),
            neural_coord: p.neural_coord.value(),
            neural_iso: p.neural_iso.value(),
            neural_steps: p.neural_steps.value(),
            neural_march: p.neural_march.value(),
            neural_color: p.neural_color.value(),
            neural_walk_rate: p.neural_walk_rate.value(),
            neural_strands_mode: p.neural_strands_mode.value(),
            neural_strands_cols: p.neural_strands_cols.value(),
            neural_strands_rows: p.neural_strands_rows.value(),
            neural_strands_extent: p.neural_strands_extent.value(),
            neural_strands_displace: p.neural_strands_displace.value(),
            // Jewel Box (#80).
            ssr: p.ssr.value(),
            ssr_intensity: p.ssr_intensity.value(),
            ssr_max_roughness: p.ssr_max_roughness.value(),
            ssr_thickness: p.ssr_thickness.value(),
            ssr_steps: p.ssr_steps.value(),
            gi: p.gi.value(),
            gi_intensity: p.gi_intensity.value(),
            gi_falloff: p.gi_falloff.value(),
            glass_dispersion: p.glass_dispersion.value(),
            glass_caustic: p.glass_caustic.value(),
            glass_thin_film: p.glass_thin_film.value(),
            // Post-composite creative FX (#152).
            fx_enabled: p.fx_enabled.value(),
            fx_style: p.fx_style.value().to_u32(),
            fx_style_amt: p.fx_style_amt.value(),
            fx_dof: p.fx_dof.value(),
            fx_dof_focus: p.fx_dof_focus.value(),
            fx_dof_range: p.fx_dof_range.value(),
            fx_chroma: p.fx_chroma.value(),
            fx_vignette: p.fx_vignette.value(),
            fx_grain: p.fx_grain.value(),
            fx_grade_sat: p.fx_grade_sat.value(),
            fx_grade_contrast: p.fx_grade_contrast.value(),
            fx_grade_temp: p.fx_grade_temp.value(),
            fx_grade_gain: p.fx_grade_gain.value(),
            fx_feedback: p.fx_feedback.value(),
            fx_outline: p.fx_outline.value(),
            // Emissive volume surface mode (#152).
            volume_radius: p.volume_radius.value(),
            volume_density: p.volume_density.value(),
            volume_emission: p.volume_emission.value(),
            volume_absorption: p.volume_absorption.value(),
            volume_steps: p.volume_steps.value(),
            // Screen-space GI (#152 Tier 2).
            ssgi: p.ssgi.value(),
            ssgi_intensity: p.ssgi_intensity.value(),
            ssgi_radius: p.ssgi_radius.value(),
            ssgi_rays: p.ssgi_rays.value(),
            // Cast shadows (#152 Tier 3).
            shadow_enabled: p.shadow_enabled.value(),
            shadow_bias: p.shadow_bias.value(),
            shadow_strength: p.shadow_strength.value(),
            // Voxel GI (#152 Tier 3, #10).
            vxgi_enabled: p.vxgi_enabled.value(),
            vxgi_intensity: p.vxgi_intensity.value(),
            vxgi_rays: p.vxgi_rays.value(),
            vxgi_steps: p.vxgi_steps.value(),
            // Reflection look (#163 Tier 1).
            reflect_tint: p.reflect_tint.value(),
            chrome_purity: p.chrome_purity.value(),
            glass_clarity: p.glass_clarity.value(),
            f0_override: p.f0_override.value(),
            // Reflection probe / parallax (#163 Tier 2).
            refl_source: p.refl_source.value().to_u32(),
            refl_box_scale: p.refl_box_scale.value(),
            refl_box_height: p.refl_box_height.value(),
            refl_blend: p.refl_blend.value(),
            // VXGI specular reflections (#163 Tier 3).
            vxgi_spec_strength: p.vxgi_spec_strength.value(),
            vxgi_spec_aperture: p.vxgi_spec_aperture.value(),
            vxgi_spec_reach: p.vxgi_spec_reach.value(),
            vxgi_spec_steps: p.vxgi_spec_steps.value(),
            // Membrane screen-space FX opt-in.
            membrane_fx: p.membrane_fx.value(),
            // Cinematic finishing (#167 Tier 1).
            hal_amount: p.hal_amount.value(),
            hal_threshold: p.hal_threshold.value(),
            hal_width: p.hal_width.value(),
            hal_warmth: p.hal_warmth.value(),
            lf_amount: p.lf_amount.value(),
            lf_ghosts: p.lf_ghosts.value(),
            lf_halo: p.lf_halo.value(),
            lf_streak: p.lf_streak.value(),
            // Emissive cubes as real lights (#167 Tier 3).
            ml_enabled: p.ml_enabled.value(),
            ml_intensity: p.ml_intensity.value(),
            ml_radius: p.ml_radius.value(),
            ml_count: p.ml_count.value(),
            ml_restir: p.ml_restir.value(),
            // Path-tracer dielectric BTDF (#258 Tier 2).
            pt_dielectric: p.pt_dielectric.value(),
            pt_absorb: p.pt_absorb.value(),
            pt_composite: p.pt_composite.value().to_u32(),
            pt_augment: p.pt_augment.value(),
            spectral_enable: p.spectral_enable.value(),
            spectral_abbe: p.spectral_abbe.value(),
            // Photon-mapped caustics (#258 Tier 5).
            pt_caustics: p.pt_caustics.value(),
            pt_caustic_intensity: p.pt_caustic_intensity.value(),
            pt_caustic_radius: p.pt_caustic_radius.value(),

            // #354 — Environment (world layer + sky/IBL).
            terrain_enabled: p.terrain_enabled.value(),
            terrain_height: p.terrain_height.value(),
            terrain_snow: p.terrain_snow.value(),
            terrain_fog: p.terrain_fog.value(),
            terrain_sun_elev: p.terrain_sun_elev.value(),
            terrain_sun_azim: p.terrain_sun_azim.value(),
            terrain_sun_int: p.terrain_sun_int.value(),
            terrain_scroll: p.terrain_scroll.value(),
            terrain_ride: p.terrain_ride.value(),
            terrain_noise: p.terrain_noise.value().to_u32(),
            terrain_palette: p.terrain_palette.value().to_u32(),
            terrain_emissive: p.terrain_emissive.value(),
            terrain_day_speed: p.terrain_day_speed.value(),
            terrain_sun_scene: p.terrain_sun_scene.value(),
            terrain_scatter: p.terrain_scatter.value(),
            terrain_godray: p.terrain_godray.value(),
            terrain_water: p.terrain_water.value(),
            terrain_water_level: p.terrain_water_level.value(),
            terrain_water_hue: p.terrain_water_hue.value(),
            terrain_water_ripple: p.terrain_water_ripple.value(),
            terrain_seed: p.terrain_seed.value(),
            terrain_ridged: p.terrain_ridged.value(),
            terrain_brightness: p.terrain_brightness.value(),
            terrain_haze: p.terrain_haze.value(),
            terrain_steps: p.terrain_steps.value(),
            terrain_octaves: p.terrain_octaves.value(),
            terrain_res: p.terrain_res.value().to_u32(),
            stars_enabled: p.stars_enabled.value(),
            stars_brightness: p.stars_brightness.value(),
            stars_twinkle: p.stars_twinkle.value(),
            stars_twinkle_speed: p.stars_twinkle_speed.value(),
            stars_size: p.stars_size.value(),
            stars_latitude: p.stars_latitude.value(),
            stars_sky_speed: p.stars_sky_speed.value(),
            stars_mag_limit: p.stars_mag_limit.value(),
            stars_saturation: p.stars_saturation.value(),
            stars_sun: p.stars_sun.value(),
            stars_sun_bright: p.stars_sun_bright.value(),
            stars_sun_size: p.stars_sun_size.value(),
            stars_sun_warmth: p.stars_sun_warmth.value(),
            atmos_enabled: p.atmos_enabled.value(),
            atmos_turbidity: p.atmos_turbidity.value(),
            atmos_mie_g: p.atmos_mie_g.value(),
            atmos_sun_int: p.atmos_sun_int.value(),
            atmos_ground_albedo: p.atmos_ground_albedo.value(),
            atmos_exposure: p.atmos_exposure.value(),
            atmos_aerial: p.atmos_aerial.value(),
            atmos_rayleigh: p.atmos_rayleigh.value(),
            clouds_enabled: p.clouds_enabled.value(),
            clouds_coverage: p.clouds_coverage.value(),
            clouds_density: p.clouds_density.value(),
            clouds_base: p.clouds_base.value(),
            clouds_thickness: p.clouds_thickness.value(),
            clouds_steps: p.clouds_steps.value(),
            clouds_detail: p.clouds_detail.value(),
            clouds_drift: p.clouds_drift.value(),
            clouds_hg: p.clouds_hg.value(),
            clouds_absorption: p.clouds_absorption.value(),
            clouds_shadow: p.clouds_shadow.value(),
            clouds_ambient: p.clouds_ambient.value(),
            ocean_enabled: p.ocean_enabled.value(),
            ocean_level: p.ocean_level.value(),
            ocean_wind_speed: p.ocean_wind_speed.value(),
            ocean_wind_dir: p.ocean_wind_dir.value(),
            ocean_amplitude: p.ocean_amplitude.value(),
            ocean_choppiness: p.ocean_choppiness.value(),
            ocean_tile_size: p.ocean_tile_size.value(),
            ocean_foam: p.ocean_foam.value(),
            ocean_glitter: p.ocean_glitter.value(),
            ocean_hue: p.ocean_hue.value(),
            ocean_depth: p.ocean_depth.value(),
            // The loaded .hdr is out-of-band (sidecar); `capture()` fills this,
            // `capture_params_only` leaves it empty (no audio-thread file I/O).
            hdr_path: String::new(),

            // #354 — Settings (renderer / output / capture framing).
            hdr_output: p.hdr_output.value(),
            hdr_knee: p.hdr_knee.value(),
            hdr_wide: p.hdr_wide.value(),
            hdr_vivid: p.hdr_vivid.value(),
            msaa: p.msaa.value().to_u32(),
            render_scale: p.render_scale.value(),
            render_auto: p.render_auto.value(),
            aspect_preset: p.aspect_preset.value().to_u32(),
            out_long_edge: p.out_long_edge.value(),
            out_custom_w: p.out_custom_w.value(),
            out_custom_h: p.out_custom_h.value(),
            letterbox_r: p.letterbox_r.value(),
            letterbox_g: p.letterbox_g.value(),
            letterbox_b: p.letterbox_b.value(),
            frame_guide: p.frame_guide.value(),
            lock_window: p.lock_window.value(),

            // #354 — Audio metering config.
            meter_res: p.meter_res.value().to_u32(),
            meter_weight: p.meter_weight.value().to_u32(),
            meter_averaging: p.meter_averaging.value().to_u32(),
            meter_hud: p.meter_hud.value(),
        }
    }

    /// Fold legacy pre-blend preset keys into the E↔B blends (#feat/maxwell-eb-blend).
    /// Presets saved before this change stored a generator bool (`mx_trace_b`) and,
    /// from #304, an aura enum (`mn_aura_field`: 0 = follow generator, 1 = Electric,
    /// 2 = Magnetic). Map them onto `mx_gen_blend` / `mx_aura_blend` (0 = E … 1 = B) so
    /// a B-traced / Magnetic preset doesn't silently revert to E. Gated on the legacy
    /// generator key being present, so NEW presets (which carry the blends and none of
    /// the legacy keys) are left untouched. Called once per preset on load.
    pub fn migrate_legacy_fields(&mut self) {
        if let Some(trace_b) = self.mx_trace_b.take() {
            self.mx_gen_blend = if trace_b { 1.0 } else { 0.0 };
            // Aura: the #304 enum when present, else (pre-#304) it followed the
            // generator — so an old B-generator preset also gets a B aura.
            self.mx_aura_blend = match self.mn_aura_field.take() {
                Some(1) => 0.0,         // Electric → E
                Some(2) => 1.0,         // Magnetic → B
                _ => self.mx_gen_blend, // Some(0) Follow Generator, or pre-#304 (absent)
            };
        }
    }

    /// Recall — overwrite every parameter through the host setter (full state).
    /// Implemented as the union of the three per-tab applies, so the global and
    /// per-tab recall paths share one field→tab partition and cannot drift.
    /// Recall a **Scene** (#354): the union of the four Scene-component tabs
    /// (Generator / Motion / Environment / Look). Audio / Synth / Settings are
    /// deliberately NOT touched — they have their own presets and never ride in
    /// a Scene. The loaded `.hdr` (`hdr_path`) is restored by the editor's
    /// recall handler, not here (it needs the out-of-band sidecar + `hdr_gen`).
    pub fn apply(&self, p: &OrganicMathParams, s: &ParamSetter) {
        for tab in EditorTab::SCENE {
            self.apply_tab(tab, p, s);
        }
    }

    /// Recall **every** partition (all seven tabs) — the full-state apply used by
    /// ⟲ Reset All (#354). `apply()` is Scene-only, so a hard factory reset must
    /// use this or Audio/Synth/Settings params (pulse/tempo/tonemap/sn_*/metering)
    /// would never return to their defaults.
    pub fn apply_all(&self, p: &OrganicMathParams, s: &ParamSetter) {
        for tab in EditorTab::ALL {
            self.apply_tab(tab, p, s);
        }
    }

    /// Copy `src`'s fields for `tabs` into `self`, leaving every other field as-is
    /// (#354). Used to build a Key Map snapshot as `live_base ⊕ scene_preset`: a
    /// MIDI-held Scene preset overlays only its four Scene tabs onto the live look,
    /// so held notes don't reset Settings/Audio/Synth to a sparse preset's
    /// serde-defaulted values. Mirrors `apply_tab`, but PresetValues→PresetValues.
    pub fn overlay_tabs(&mut self, src: &PresetValues, tabs: &[EditorTab]) {
        macro_rules! op {
            ($t:ident, $f:ident, scalar) => {
                if tabs.contains(&EditorTab::$t) {
                    self.$f = src.$f;
                }
            };
            ($t:ident, $f:ident, enum, $ty:ident) => {
                if tabs.contains(&EditorTab::$t) {
                    self.$f = src.$f;
                }
            };
        }
        for_each_tab_field!(op);
        // NB: `hdr_path` (Environment, out-of-band) is intentionally NOT copied —
        // this stays allocation-free for the audio-thread Key Map merge, and the
        // only consumer (`to_shared`) doesn't carry it. GUI recall restores the
        // `.hdr` via the sidecar in `apply_recall`, not through this overlay.
    }

    /// Recall only the parameters that belong to `tab` (#145), leaving every
    /// other tab's parameters untouched — an override/merge, not a full-state
    /// replace. Sets through the host `ParamSetter` like `apply`, so partial
    /// recall is equally automation-recordable/undoable.
    pub fn apply_tab(&self, tab: EditorTab, p: &OrganicMathParams, s: &ParamSetter) {
        macro_rules! set {
            ($param:expr, $val:expr) => {{
                s.begin_set_parameter($param);
                s.set_parameter($param, $val);
                s.end_set_parameter($param);
            }};
        }
        macro_rules! op {
            ($t:ident, $f:ident, scalar) => {
                if tab == EditorTab::$t {
                    set!(&p.$f, self.$f);
                }
            };
            ($t:ident, $f:ident, enum, $ty:ident) => {
                if tab == EditorTab::$t {
                    set!(&p.$f, $ty::from_u32(self.$f));
                }
            };
        }
        for_each_tab_field!(op);
    }

    /// The (field-name, tab) partition as data — drives the drift-guard test
    /// AND the #354 subset save (which serialized fields belong to each tab).
    pub fn tab_field_list() -> Vec<(&'static str, EditorTab)> {
        let mut v = Vec::new();
        macro_rules! op {
            ($t:ident, $f:ident, scalar) => {
                v.push((stringify!($f), EditorTab::$t));
            };
            ($t:ident, $f:ident, enum, $ty:ident) => {
                v.push((stringify!($f), EditorTab::$t));
            };
        }
        for_each_tab_field!(op);
        v
    }
}

impl PresetValues {
    /// Build the shared-memory snapshot for this preset (used to export it as a
    /// MIDI clip).
    pub fn to_shared(&self) -> crate::ipc::Shared {
        crate::ipc::Shared {
            seq: 0,
            layout_version: crate::ipc::LAYOUT_VERSION,
            loop_count: crate::param_table::pack_loop_count_preset(self),
            rot_amp: crate::param_table::pack_rot_amp_preset(self),
            rot_mod: crate::param_table::pack_rot_mod_preset(self),
            trans_amp: crate::param_table::pack_trans_amp_preset(self),
            trans_mod: crate::param_table::pack_trans_mod_preset(self),
            lighting: crate::param_table::pack_lighting_preset(self),
            scale_amp: self.scale_amp,
            rot_func: self.rot_func,
            trans_func: self.trans_func,
            scale_func: self.scale_func,
            animate: self.animate as u32,
            pulse: self.pulse as u32,
            tempo: self.tempo,
            pulse_depth: 0.0, // reserved (the Pulse Depth knob was removed)
            pbr: crate::param_table::pack_pbr_preset(self),
            hdr_gen: 0,
            transport: [0.0, 0.0, self.tempo, 0.0],
            tempo_sync: self.tempo_sync as u32,
            camera: crate::param_table::pack_camera_preset(self),
            cam_amount: self.cam_amount,
            cam_seq: crate::param_table::pack_cam_seq_preset(self),
            cam_dolly: crate::param_table::pack_cam_dolly_preset(self),
            cam_clock: crate::param_table::pack_cam_clock_preset(self),
            cam_audio: [0.0; 4], // live detection, runtime-only
            cam_frame: crate::param_table::pack_cam_frame_preset(self),
            cam_story: crate::param_table::pack_cam_story_preset(self),
            routing: crate::param_table::pack_routing_preset(self),
            surface_mode: self.surface_mode,
            origin_mode: self.origin_mode,
            bevel: self.bevel,
            maporbit: crate::param_table::pack_maporbit_preset(self),
            // AI-Performer runtime block (#317 T1): not preset-captured; runtime-stamped.
            agent: [0.0; 8],
            fieldsim: crate::param_table::pack_fieldsim_preset(self),
            surface_fx: crate::param_table::pack_surface_fx_preset(self),
            // HDR/MSAA are per-display/quality settings — defaults, not saved.
            hdr_output: self.hdr_output as u32,
            hdr_knee: self.hdr_knee,
            hdr_wide: self.hdr_wide as u32,
            // `Shared.msaa` is the wgpu sample COUNT (1/2/4/8), not the enum
            // ordinal — mirror the live packer (`Msaa::samples()`).
            msaa: Msaa::from_u32(self.msaa).samples(),
            // Path tracer (#200 Tier 4) — per-display, not saved (a heavy reference mode).
            pathtrace_on: 0,
            // Look settings captured by the preset.
            tonemap: self.tonemap,
            bg_tonemap: self.bg_tonemap,
            bg_visible: self.bg_visible as u32,
            bg_intensity: self.bg_intensity,
            env_tint_hue: self.env_tint_hue,
            env_tint_amt: self.env_tint_amt,
            ssao: crate::param_table::pack_ssao_preset(self),
            // Live band envelopes are runtime-only; a preset just carries the
            // pulse-source choice.
            audio: [0.0; 8],
            pulse_source: self.pulse_source,
            speed_pulse: crate::param_table::pack_speed_pulse_preset(self),
            cont_shape: self.cont_shape,
            metaball: crate::param_table::pack_metaball_preset(self),
            voxel: crate::param_table::pack_voxel_preset(self),
            voxel_gi: crate::param_table::pack_voxel_gi_preset(self),
            bio: crate::param_table::pack_bio_preset(self),
            membrane: crate::param_table::pack_membrane_preset(self),
            rd: crate::param_table::pack_rd_preset(self),
            generator: self.generator,
            frenet: crate::param_table::pack_frenet_preset(self),
            dna: crate::param_table::pack_dna_preset(self),
            attr: crate::param_table::pack_attr_preset(self),
            boids: crate::param_table::pack_boids_preset(self),
            bell: crate::param_table::pack_bell_preset(self),
            harm: crate::param_table::pack_harm_preset(self),
            ls: crate::param_table::pack_ls_preset(self),
            cn: crate::param_table::pack_cn_preset(self),
            breath: crate::param_table::pack_breath_preset(self),
            pol: crate::param_table::pack_pol_preset(self),
            maxwell: crate::param_table::pack_maxwell_preset(self),
            acoustic: crate::param_table::pack_acoustic_preset(self),
            acoustic2: crate::param_table::pack_acoustic2_preset(self),
            acoustic3: crate::param_table::pack_acoustic3_preset(self),
            analytical: crate::param_table::pack_analytical_preset(self),
            fieldvol: crate::param_table::pack_fieldvol_preset(self),
            colour: crate::param_table::pack_colour_preset(self),
            sonify: crate::param_table::pack_sonify_preset(self),
            voices: [0.0; 64], // runtime-only
            // #333: measured at runtime, never captured in a preset.
            audiometer: [0.0; 16],
            audiospectrum: [-120.0; 128],
            // #346 Field Chamber: scope frame is runtime-written; the panel look is captured.
            scopewave: [0.0; 260],
            chamber: crate::param_table::pack_chamber_preset(self),
            emissive: crate::param_table::pack_emissive_preset(self),
            splat: crate::param_table::pack_splat_preset(self),
            plexus: crate::param_table::pack_plexus_preset(self),
            plexus2: crate::param_table::pack_plexus2_preset(self),
            plexus_node_mat: crate::param_table::pack_plexus_node_mat_preset(self),
            plexus_edge_mat: crate::param_table::pack_plexus_edge_mat_preset(self),
            plexus3: crate::param_table::pack_plexus3_preset(self),
            plexus4: crate::param_table::pack_plexus4_preset(self),
            splat2: crate::param_table::pack_splat2_preset(self),
            mx_eb: crate::param_table::pack_mx_eb_preset(self),
            plexus_overlay: crate::param_table::pack_plexus_overlay_preset(self),
            // Visible-Mind specimen (#367 T1) is runtime-stamped, never preset-captured.
            mind: [0.0; 8],
            field: crate::param_table::pack_field_preset(self),
            field_gen: 0,
            mapattractor: crate::param_table::pack_mapattractor_preset(self),
            mapattractor2: crate::param_table::pack_mapattractor2_preset(self),
            axon: crate::param_table::pack_axon_preset(self),
            phyl: crate::param_table::pack_phyl_preset(self),
            tessellation: crate::param_table::pack_tessellation_preset(self),
            mandelbulb: crate::param_table::pack_mandelbulb_preset(self),
            creature: crate::param_table::pack_creature_preset(self),
            creature2: crate::param_table::pack_creature2_preset(self),
            creature3: crate::param_table::pack_creature3_preset(self),
            material: crate::param_table::pack_material_preset(self),
            // Runtime-stamped (the material folder is followed via a sidecar counter,
            // like hdr_gen) — not carried through a preset snapshot.
            material_gen: 0,
            material_layer: crate::param_table::pack_material_layer_preset(self),
            material_grad: crate::param_table::pack_material_grad_preset(self),
            material_layer2: crate::param_table::pack_material_layer2_preset(self),
            material_grad2: crate::param_table::pack_material_grad2_preset(self),
            material_derive: crate::param_table::pack_material_derive_preset(self),
            material_live: crate::param_table::pack_material_live_preset(self),
            minimal_surface: crate::param_table::pack_minimal_surface_preset(self),
            synchrotron: crate::param_table::pack_synchrotron_preset(self),
            // Terrain backdrop is a global world/display layer (like HDR/MSAA),
            // not a per-preset look, so presets don't capture it — keep the default.
            terrain: crate::param_table::pack_terrain_preset(self),
            // Render scale / auto-resolution are global display settings too.
            render_scale: self.render_scale,
            render_auto: self.render_auto as u32,
            // The starfield is a global sky layer (like terrain/HDR/MSAA), not a
            // per-preset look, so presets don't capture it — keep the default.
            stars: crate::param_table::pack_stars_preset(self),
            // The physical atmosphere (#100) is a global world layer too — not
            // preset-captured; keep the default.
            atmosphere: crate::param_table::pack_atmosphere_preset(self),
            // Volumetric clouds (#102 A) — a global world layer; not preset-captured.
            clouds: crate::param_table::pack_clouds_preset(self),
            // FFT ocean (#102 B) — a global world layer; not preset-captured.
            ocean: crate::param_table::pack_ocean_preset(self),
            hdr_vivid: self.hdr_vivid,
            particles: crate::param_table::pack_particles_preset(self),
            fluid: crate::param_table::pack_fluid_preset(self),
            // Jewel Box (#80) — captured looks.
            ssr: crate::param_table::pack_ssr_preset(self),
            gi: crate::param_table::pack_gi_preset(self),
            glass_spec: crate::param_table::pack_glass_spec_preset(self),
            kifs: crate::param_table::pack_kifs_preset(self),
            kaleido: crate::param_table::pack_kaleido_preset(self),
            instrument: crate::param_table::pack_instrument_preset(self),
            instrument2: crate::param_table::pack_instrument2_preset(self),
            // #423 atlas: runtime-stamped, not preset-captured → zeros here too.
            atlas: [0.0; 8],
            // #407 Tier A Field Playback clip-load counter: runtime-stamped, not
            // preset-captured → 0 here.
            fieldclip_gen: 0,
            // #407 Tier B: runtime-stamped model-load counter, not preset-captured.
            nca_gen: 0,
            fdtd: crate::param_table::pack_fdtd_preset(self),
            // Capture / production frame (#135) — a per-display setting (like
            // HDR/MSAA/terrain), not a per-preset look; keep the Native default.
            capture: crate::param_table::pack_capture_preset(self),
            // Capture overlay (#135 Phase 2) — per-display too; keep the default.
            overlay: crate::ipc::Shared::default().overlay,
            overlay_gen: 0,
            // Capture decoration: axes + box (#135 P5) — per-display; keep the default.
            axes: crate::ipc::Shared::default().axes,
            // Post-composite creative FX (#152) — captured looks.
            fx: crate::param_table::pack_fx_preset(self),
            volume: crate::param_table::pack_volume_preset(self),
            // Temporal (#152 Tier 2: TAA / motion blur / stochastic) — per-display,
            // not a per-preset look; keep the default (like capture/overlay/axes).
            temporal: crate::ipc::Shared::default().temporal,
            // Screen-space GI (#152 Tier 2) — a captured look.
            ssgi: crate::param_table::pack_ssgi_preset(self),
            shadow: crate::param_table::pack_shadow_preset(self),
            vxgi: crate::param_table::pack_vxgi_preset(self),
            reflect: crate::param_table::pack_reflect_preset(self),
            refl_probe: crate::param_table::pack_refl_probe_preset(self),
            vxgi_spec: crate::param_table::pack_vxgi_spec_preset(self),
            membrane_fx: crate::param_table::pack_membrane_fx_preset(self),
            // Cinematic finishing (#167 Tier 1): halation + lens flares — a captured look.
            finishing: crate::param_table::pack_finishing_preset(self),
            manylight: crate::param_table::pack_manylight_preset(self),
            vecfield: crate::param_table::pack_vecfield_preset(self),
            vecbuild: crate::param_table::pack_vecbuild_preset(self),
            // Fluid Ink (#182 Tier 1) — a captured look.
            fluidvis: crate::param_table::pack_fluidvis_preset(self),
            // Fluid medium Tier 2 (#182) — a captured look.
            fluid2: crate::param_table::pack_fluid2_preset(self),
            // MLS-MPM liquid (#182 Tier 3a) — a captured look.
            liquid: crate::param_table::pack_liquid_preset(self),
            liquid2: crate::param_table::pack_liquid2_preset(self),
            fluidgi: crate::param_table::pack_fluidgi_preset(self),
            caustic: crate::param_table::pack_caustic_preset(self),
            liqmat: crate::param_table::pack_liqmat_preset(self),
            liqmat2: crate::param_table::pack_liqmat2_preset(self),
            rails: crate::param_table::pack_rails_preset(self),
            scenery: crate::param_table::pack_scenery_preset(self),
            terra: crate::param_table::pack_terra_preset(self),
            water: crate::param_table::pack_water_preset(self),
            water2: crate::param_table::pack_water2_preset(self),
            rt: crate::param_table::pack_rt_preset(self),
            // Refractive generator material — a captured look.
            refrmat: crate::param_table::pack_refrmat_preset(self),
            rt2: crate::param_table::pack_rt2_preset(self),
            rt3: crate::param_table::pack_rt3_preset(self),
            rt4: crate::param_table::pack_rt4_preset(self),
            neural: crate::param_table::pack_neural_preset(self),
            aniso: crate::param_table::pack_aniso_preset(self),
            coat: crate::param_table::pack_coat_preset(self),
            neural2: crate::param_table::pack_neural2_preset(self),
            neural3: crate::param_table::pack_neural3_preset(self),
            body: crate::param_table::pack_body_preset(self),
            micro: crate::param_table::pack_micro_preset(self),
            ndenoise: crate::param_table::pack_ndenoise_preset(self),
            emit: crate::param_table::pack_emit_preset(self),
            ssrefr: crate::param_table::pack_ssrefr_preset(self),
            upscale: crate::param_table::pack_upscale_preset(self),
            restir: crate::param_table::pack_restir_preset(self),
            neural_net: crate::param_table::pack_neural_net_preset(self),
            neural_edge: crate::param_table::pack_neural_edge_preset(self),
            maxenergy: crate::param_table::pack_maxenergy_preset(self),
            neural_net2: crate::param_table::pack_neural_net2_preset(self),
            nn_gen: 0,
            creature_gen: 0, // #476 T2b: runtime-written, not preset-captured
            neural_mlp: crate::param_table::pack_neural_mlp_preset(self),
            neural_attn: crate::param_table::pack_neural_attn_preset(self),
            tube: crate::param_table::pack_tube_preset(self),
            neural_surface: crate::param_table::pack_neural_surface_preset(self),
            neural_surface2: crate::param_table::pack_neural_surface2_preset(self),
            brain: crate::param_table::pack_brain_preset(self),
            thinfilm: crate::param_table::pack_thinfilm_preset(self),
            // Path-tracer dielectric BTDF (#258 T2): [enable, absorption, _, _].
            ptglass: [
                self.pt_dielectric as u32 as f32,
                self.pt_absorb,
                self.pt_composite as f32,
                self.pt_augment,
            ],
            tube_profile: self.tube_profile,
            lens: crate::param_table::pack_lens_preset(self),
            spectral: [self.spectral_enable as u32 as f32, self.spectral_abbe, 3.0, 0.0],
            demo: crate::param_table::pack_demo_preset(self),
            audiodip: crate::param_table::pack_audiodip_preset(self),
            audiodip2: crate::param_table::pack_audiodip2_preset(self),
            mxforce: crate::param_table::pack_mxforce_preset(self),
            mxforce2: crate::param_table::pack_mxforce2_preset(self),
            mxforce3: crate::param_table::pack_mxforce3_preset(self),
            pbeads: crate::param_table::pack_pbeads_preset(self),
            matcol: crate::param_table::pack_matcol_preset(self),
            pbeads2: crate::param_table::pack_pbeads2_preset(self),
            // Photon-mapped caustics (#258 T5): the photon budget is per-quality —
            // presets carry the param default (mirrors spectral's literal `3.0`).
            ptcaustic: [
                self.pt_caustics as u32 as f32,
                128.0,
                self.pt_caustic_intensity,
                self.pt_caustic_radius,
            ],
            skyrefl: crate::param_table::pack_skyrefl_preset(self),
            nrc: crate::param_table::pack_nrc_preset(self),
            nrc2: crate::param_table::pack_nrc2_preset(self),
            nrc3: crate::param_table::pack_nrc3_preset(self),
            nrc4: crate::param_table::pack_nrc4_preset(self),
            // #541 S2 T1 mindview spine: **never preset-captured** (#484's
            // default-inert contract — a pane arrangement is a workspace, not a
            // look, and it is runtime-stamped like `mind[8]`/`atlas[8]`). Nothing
            // is added to `PresetValues`, so every existing preset file
            // deserializes unchanged and recalling one leaves the panes alone.
            mindview: [0.0; 8],
            mindview_pane: [0.0; crate::ipc::MINDVIEW_PANES * crate::ipc::MINDVIEW_PANE_SLOTS],
            mindview_gen: 0,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub values: PresetValues,
}

/// Directory where exported `.mid` clips are written. The per-preset `.mid`
/// export button was removed from the presets rail (unused feature); kept —
/// with `write_midi` — in case clip export returns.
#[allow(dead_code)]
pub fn clips_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OrganicMath")
        .join("clips");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Filesystem-safe version of a preset name. (Only used by the retired `.mid`
/// export path — see `clips_dir`.)
#[allow(dead_code)]
pub fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    if s.is_empty() { "preset".into() } else { s }
}

fn presets_path() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OrganicMath");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("presets.json")
}

/// Where the loadable **network gallery** (#226) is installed — the connectome /
/// MLP / attention JSON demos, sitting next to `presets.json` + `clips/` in the
/// app-support store. `native/deploy.sh` copies `native/assets/networks/*.json`
/// here on the Mac, and the editor's "Load Network (JSON)…" dialog opens here so
/// the gallery is one click away. Created (empty) if the deploy hasn't run yet.
pub fn networks_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OrganicMath")
        .join("networks");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Where the **procedural material graph** gallery (#472 Tier 4) is installed —
/// authored `.json` graphs next to `presets.json` in the app-support store.
/// `native/deploy.sh` copies `native/assets/materials/graphs/*.json` here, and the
/// editor's "Load Material Graph (JSON)…" dialog opens here. Mirrors `networks_dir`.
pub fn material_graphs_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OrganicMath")
        .join("materials");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Where the **Creature Engine** body-plan gallery (#476 Tier 2b) is installed —
/// authored `.json` plans sitting next to `presets.json` + `networks/` in the
/// app-support store. `native/deploy.sh` copies `native/assets/creatures/*.json`
/// here on the Mac, and the editor's "Load Creature (JSON)…" dialog opens here so
/// the gallery is one click away. Created (empty) if the deploy hasn't run yet.
/// Mirrors `networks_dir`.
pub fn creatures_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OrganicMath")
        .join("creatures");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Where the **Field Playback** clip gallery (#407 Tier A) is installed — the baked
/// `.bin` field clips (derived offline from PolymathicAI's *The Well*), sitting next
/// to `presets.json` + `networks/` in the app-support store. `native/deploy.sh` copies
/// `native/assets/fields/*.bin` here on the Mac, and the editor's "Load Field Clip…"
/// dialog opens here so the gallery is one click away. Created (empty) if the deploy
/// hasn't run yet. Mirrors `networks_dir`.
pub fn fields_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OrganicMath")
        .join("fields");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Where the loadable **Neural CA model gallery** (Tier B, #407) is installed — the
/// trained NCA weights-JSON files (`nca-default.json`, …), next to `presets.json`
/// and the `networks/` gallery. `native/deploy.sh` copies `native/assets/nca/*.json`
/// here on the Mac, and the editor's "Load NCA Model (JSON)…" dialog opens here so
/// the built-in default is one click away. Created (empty) if the deploy hasn't run.
pub fn nca_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OrganicMath")
        .join("nca");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Parse a preset-list JSON string **leniently**: a strict `Vec<Preset>` parse
/// first, then — if that fails — salvage element-wise so one malformed entry
/// can't wipe the whole list (a whole-list `.ok()` fallback to empty is how the
/// #354 subset-save regression silently blanked every preset). Every
/// `PresetValues` field is `#[serde(default)]`, so a subset object (missing the
/// other tabs' fields) deserializes fine.
fn parse_presets_lenient(s: &str) -> Vec<Preset> {
    if let Ok(v) = serde_json::from_str::<Vec<Preset>>(s) {
        return v;
    }
    match serde_json::from_str::<Vec<serde_json::Value>>(s) {
        Ok(arr) => arr
            .into_iter()
            .filter_map(|v| serde_json::from_value::<Preset>(v).ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub fn load() -> Vec<Preset> {
    let mut presets: Vec<Preset> = std::fs::read_to_string(presets_path())
        .map(|s| parse_presets_lenient(&s))
        .unwrap_or_default();
    // Migrate pre-E↔B-blend presets (mx_trace_b / mn_aura_field → the blends).
    for preset in &mut presets {
        preset.values.migrate_legacy_fields();
    }
    // #187 Tier 3: seed the factory Rails presets once (marker-guarded, so a
    // user deleting/renaming them is respected forever after). The marker
    // drops only after the seeded store persisted — a failed save retries.
    if seed_rails_presets(&mut presets) && save(&presets) {
        mark_rails_seeded();
    }
    presets
}

/// The serialized field names that belong to any of `tabs` (#354). Drives the
/// subset save so a preset's JSON carries only its bucket's params.
fn field_names_for(tabs: &[EditorTab]) -> std::collections::HashSet<&'static str> {
    PresetValues::tab_field_list()
        .into_iter()
        .filter(|(_, t)| tabs.contains(t))
        .map(|(n, _)| n)
        .collect()
}

/// Serialize `presets` to `path`, keeping in each entry's `values` **only** the
/// fields that belong to `tabs` (#354 subset capture). Old full-blob files still
/// load — the dropped keys just fall back to their serde defaults, and recall
/// only touches `tabs` anyway. `hdr_path` (skipped-when-empty, out-of-band) is
/// preserved only for the Environment/Scene subsets that include it.
/// One preset entry as its stored subset JSON: `{name, values}` with `values`
/// filtered to `tabs`' fields (+ `hdr_path` when the Environment bucket is
/// present). Split out from `save_subset` so the round-trip is unit-testable.
fn subset_entry(p: &Preset, tabs: &[EditorTab]) -> serde_json::Value {
    let names = field_names_for(tabs);
    // The loaded .hdr rides `hdr_path` (not in the tab partition); include it in
    // any subset that carries the Environment bucket so the sky is restored.
    let keep_hdr = tabs.contains(&EditorTab::Environment);
    let mut vals = serde_json::to_value(&p.values).unwrap_or(serde_json::Value::Null);
    if let Some(map) = vals.as_object_mut() {
        map.retain(|k, _| names.contains(k.as_str()) || (keep_hdr && k == "hdr_path"));
    }
    serde_json::json!({ "name": p.name, "values": vals })
}

fn save_subset(path: &std::path::Path, presets: &[Preset], tabs: &[EditorTab]) -> bool {
    let arr: Vec<serde_json::Value> = presets.iter().map(|p| subset_entry(p, tabs)).collect();
    match serde_json::to_string_pretty(&arr) {
        Ok(json) => std::fs::write(path, json).is_ok(),
        Err(_) => false,
    }
}

/// Save the **Scene** list (#354): only the four Scene-component tabs' fields.
pub fn save(presets: &[Preset]) -> bool {
    save_subset(&presets_path(), presets, &EditorTab::SCENE)
}

/// Marker file for the one-shot factory-preset seeding (#187 Tier 3). Presence
/// means "already seeded" — deleting/renaming the presets afterwards must NOT
/// re-add them on the next load.
fn rails_seed_marker() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OrganicMath");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("seeded_rails_v2")
}

/// The factory Rails presets (#187 Tier 3): 5 finished rides, one per
/// archetype + two look variants, so the compose-by-MIDI story (Key Map note →
/// preset, quantized entry on the phrase boundary) works out of the box. Built
/// as deltas on the factory default capture, so everything not listed stays at
/// factory (and future param additions inherit their defaults via serde).
pub fn builtin_rails_presets() -> Vec<Preset> {
    let base = PresetValues::capture(&OrganicMathParams::default());
    // The rides use scenery alone: primary generator off, Zone corridor on.
    let rails_gen = crate::params::GeneratorMode::None.to_u32();
    let mk = |name: &str, f: &dyn Fn(&mut PresetValues)| -> Preset {
        let mut v = base.clone();
        v.generator = rails_gen;
        v.sc_mode = 1; // SceneryMode::Zone — the corridor
        f(&mut v);
        Preset { name: name.into(), values: v }
    };
    vec![
        // The flagship: a glass throat morphing every 2 bars.
        mk("Rails — Crystal Throat", &|v| {
            v.sc_surface = 2; // swept tubes (the scenery membrane is a follow-up)
            v.sc_mat = 2; // Glass
            v.sc_glow = 0.5;
            v.rl_swell = 0.5;
            v.rl_spike = 0.7;
            v.rl_speed = 6.0;
            v.rl_rib_gain = 0.8;
            v.rl_color_flow = 0.08;
        }),
        // A hall-of-mirrors gullet: chrome tubes, corkscrewing.
        mk("Rails — Chrome Gullet", &|v| {
            v.sc_surface = 2;
            v.sc_mat = 1; // Chrome
            v.rl_twist = 0.25;
            v.rl_swell = 0.45;
            v.rl_variance = 0.7;
        }),
        // Cylindrical phyllotaxis as glowing spiral rails.
        mk("Rails — Pinecone Run", &|v| {
            v.rl_archetype = 1; // Phyllo Wall
            v.sc_surface = 2; // Swept Tubes = the actual parastichy arms
            v.sc_glow = 0.6;
            v.rl_rows_beat = 6;
            v.rl_thickness = 0.8;
            v.rl_color_flow = 0.1;
        }),
        // A gate per beat, a MAJOR gate on every bar — the visual metronome.
        mk("Rails — Gate Ritual", &|v| {
            v.rl_archetype = 2; // Rings & Gates
            v.rl_cell_len = 2; // RailCellLen::Bar → minors every beat
            v.sc_surface = 2; // gates as lit tubes
            v.sc_glow = 0.7;
            v.rl_rib_gain = 1.2;
            v.rl_thickness = 0.9;
        }),
        // Nested counter-rotating shells, re-rolled hard every phrase.
        mk("Rails — Tissue Storm", &|v| {
            v.rl_archetype = 3; // Tissue Tube
            v.sc_surface = 0; // cubes
            v.rl_shells = 3;
            v.rl_spike = 0.8;
            v.rl_variance = 0.8;
            v.rl_evolve = 0.6;
        }),
    ]
}

/// One-shot seeding of the factory Rails presets into the global store: runs
/// only while the marker is absent, appends only names that don't already
/// exist. Called from `load()`, which drops the marker only once the appended
/// presets actually persisted — a failed `save` retries the seed next load
/// (#190 review). When nothing needed appending the marker drops here.
fn seed_rails_presets(presets: &mut Vec<Preset>) -> bool {
    let marker = rails_seed_marker();
    if marker.exists() {
        return false;
    }
    // v2 (#187 scenery pivot): the v1 factory presets targeted the retired
    // GeneratorMode::Rails — replace any factory-named entries wholesale so
    // stale ones can't recall a blank generator, then seed the new set.
    let before = presets.len();
    presets.retain(|e| !e.name.starts_with("Rails — "));
    let mut changed = presets.len() != before;
    for p in builtin_rails_presets() {
        if !presets.iter().any(|e| e.name == p.name) {
            presets.push(p);
            changed = true;
        }
    }
    if !changed {
        // Nothing to persist — the one-shot is complete as-is.
        mark_rails_seeded();
    }
    changed
}

/// Drop the one-shot marker — only after the seeded store is safely on disk.
fn mark_rails_seeded() {
    let _ = std::fs::write(rails_seed_marker(), "1");
}

/// Which preset list a save/recall/rename/delete acts on (#145): the full-state
/// global list, or one of the three per-tab lists.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum PresetScope {
    #[default]
    Global,
    Tab(EditorTab),
}

/// Which of the two preset-rail tabs is showing (#145): the full-state Scene
/// list, or the list for the currently-selected editor tab.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum PresetRailTab {
    /// The full **Scene** list (renamed from "Global" in #354).
    #[default]
    Global,
    Tab,
}

/// A pending Update/Delete awaiting the quick "Are you sure?" confirm (#354).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    Update,
    Delete,
}

/// Filesystem slug for a tab's preset file (`presets_<slug>.json`).
///
/// #626 Tier 3: was `EditorTab::file_slug`, an inherent method. `EditorTab` now
/// lives in `organon_core::tabs`, and this is a *persistence* concern rather than
/// tab identity — so it stayed here, beside its one caller, instead of following
/// the type into core and dragging a filesystem opinion in with it.
fn tab_file_slug(tab: EditorTab) -> &'static str {
    match tab {
        EditorTab::Generator => "generator",
        EditorTab::Motion => "motion",
        EditorTab::Environment => "environment",
        EditorTab::Look => "look",
        EditorTab::Audio => "audio",
        EditorTab::Synth => "synth",
        EditorTab::Settings => "settings",
    }
}

fn tab_presets_path(tab: EditorTab) -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OrganicMath");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("presets_{}.json", tab_file_slug(tab)))
}

/// Load one tab's preset list (#145). Each entry is a full `PresetValues`, but
/// recall applies only that tab's subset via `PresetValues::apply_tab`.
pub fn load_tab(tab: EditorTab) -> Vec<Preset> {
    let mut presets: Vec<Preset> = std::fs::read_to_string(tab_presets_path(tab))
        .map(|s| parse_presets_lenient(&s))
        .unwrap_or_default();
    // Migrate pre-E↔B-blend presets (mx_trace_b / mn_aura_field → the blends).
    for preset in &mut presets {
        preset.values.migrate_legacy_fields();
    }
    presets
}

pub fn save_tab(tab: EditorTab, presets: &[Preset]) {
    // #354: store only this tab's fields (subset capture).
    let _ = save_subset(&tab_presets_path(tab), presets, &[tab]);
}

/// User-recorded per-parameter defaults (#131): a sparse overlay over the factory
/// defaults, keyed by each param's nih-plug **id** (`#[id="…"]` string) → recorded
/// **normalized** value (0..1). The per-control ⟲ reset targets the recorded value
/// when one is present, else the param's built-in default. Persisted as its own
/// JSON (independent of the named presets); the id key is stable across sessions
/// (unlike a param's runtime address), so the overlay survives a reload.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Defaults {
    pub map: std::collections::BTreeMap<String, f32>,
}

fn defaults_path() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OrganicMath");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("defaults.json")
}

impl Defaults {
    pub fn load() -> Self {
        std::fs::read_to_string(defaults_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(defaults_path(), json);
        }
    }
}



/// Editor-side preset UI state (held in the egui editor's user state).
#[derive(Default)]
pub struct PresetUi {
    pub presets: Vec<Preset>,
    /// Per-tab preset lists (#145), indexed by `EditorTab::index()`
    /// (Generator/Motion/Look). Each entry is a full `PresetValues`, but recall
    /// applies only that tab's subset (an override/merge). Loaded lazily with
    /// `presets`.
    pub tab_presets: [Vec<Preset>; EditorTab::COUNT],
    pub loaded: bool,
    /// Which top-level UI tab is active (#131; grown to five — `UiTab`).
    pub tab: UiTab,
    /// #483 Tier 1 (Organon Mind only) — the `model_gen` this editor last auto-pointed
    /// the visual at. Organon Mind ships no Generator tab, so a freshly loaded `.gguf`
    /// would have nothing drawing it; the editor sets generator = Neural Network +
    /// topology = Connectome (loaded) once per load. Tracked so the auto-set fires on
    /// the *edge* and never fights a user who changed something afterwards. Always 0 in
    /// full Organon (which never auto-sets — you pick your own generator).
    pub mind_auto_view_gen: u32,
    /// #620 — the `model_gen` an auto-point is currently *trying* to reach, if any.
    ///
    /// Split from `mind_auto_view_gen` because the two answer different questions: that one is
    /// "which load have we finished pointing at", this one is "which load are we still working
    /// on". Before #620 there was only the first, latched *before* the writes — so a single lost
    /// write left the model un-pointed permanently, with reloading the only recovery. The latch
    /// now moves only once the params read back as intended.
    pub mind_auto_view_pending: Option<u32>,
    /// #620 — editor frames spent on the pending auto-point, driving retry and give-up.
    pub mind_auto_view_attempts: u32,
    /// #620 — `(generator, topology)` as they read the moment the pending auto-point began.
    ///
    /// The auto-point's promise is that it never fights a manual change. One-shot writes kept that
    /// for free; retrying does not, so the retry needs to tell "my write has not landed yet" from
    /// "someone else moved this" — and the baseline is what makes those distinguishable. See
    /// `mind_ui::auto_point_externally_changed`, which is per-param precisely because the two
    /// land separately.
    pub mind_auto_view_baseline: Option<(GeneratorMode, NeuralTopology)>,
    /// Which preset-rail tab is showing (#145): Global, or the active editor tab's list.
    pub rail_tab: PresetRailTab,
    pub rename_idx: Option<usize>,
    /// Which list the in-progress rename targets (#145), paired with `rename_idx`.
    pub rename_scope: PresetScope,
    pub rename_buf: String,
    /// A pending Update/Delete awaiting confirmation (#354): (scope, index, kind).
    pub confirm: Option<(PresetScope, usize, ConfirmKind)>,
    /// A beat-quantized recall waiting for its boundary (#354): `(scope, values,
    /// target beat, high-water beat)`. Fired once the host beat crosses `target`;
    /// the high-water mark (the max beat seen while waiting) lets a backward
    /// transport jump (loop / seek) fire it instead of stranding + spinning.
    pub pending_recall: Option<(PresetScope, PresetValues, f32, f32)>,

    // --- #356 Four-Quadrant Performance Controller (editor-side state) ---
    /// The device profile (pad note → quadrant/slot, side-button CCs). Loaded
    /// from `controller.json` lazily, edited by the learn flow, re-saved on change.
    pub perf_layout: crate::controller::PadLayout,
    /// Loaded `perf_layout` from disk yet? (also drains stale mailbox events once).
    pub perf_loaded: bool,
    /// Dirty flag → persist `perf_layout` at end of frame.
    pub perf_dirty: bool,
    /// Learn capture in progress: `Some(next pad index 0..64)` while walking the
    /// grid, `None` when off.
    pub perf_learn: Option<usize>,
    /// Most-recent raw MIDI message seen on the surface (live diagnostic readout).
    pub perf_last_raw: Option<crate::controller::RawMidi>,
    /// Human-readable description of the last surface action (which component/slot
    /// recalled + preset name, or "empty") — makes the behaviour legible.
    pub perf_last_action: Option<String>,
    /// The last-applied (active) **absolute** preset index per quadrant (i.e.
    /// `bank*16 + slot`), for the mirror grid — absolute so the highlight follows
    /// the actual preset across bank pages, not a bank-local slot.
    pub perf_active: [Option<usize>; 4],
    /// A queued (pending-boundary) pad recall: `(component index, absolute preset
    /// index)`. `None` unless a pad recall is currently armed in `pending_recall`.
    pub perf_queued: Option<(usize, usize)>,
    /// Bank offset — pads address slots `bank*16 .. bank*16+16` (◀▶ page it).
    pub perf_bank: usize,
    /// Wall-clock (egui `input.time`) of the last controller frame — a large gap
    /// means the editor was closed, so the mailbox is drained on the next frame to
    /// discard stale presses instead of firing them.
    pub perf_last_frame_time: f64,
    /// Is the Performance Controller window open?
    pub perf_window_open: bool,

    // --- #448 Rotary knob layer (Launch Control XL; editor-side state) ---
    /// The knob-bank configuration (device CCs, mode, pickup, Performer pages).
    /// Loaded from `knobs.json` lazily, edited by the card, re-saved on change.
    pub knob_config: crate::controller::KnobConfig,
    /// Loaded `knob_config` from disk yet?
    pub knob_loaded: bool,
    /// Dirty flag → persist `knob_config` at end of frame.
    pub knob_dirty: bool,
    /// Knob learn in progress: `Some(next knob index 0..24)` while twisting the
    /// bank in row-major order, `None` when off.
    pub knob_learn: Option<usize>,
    /// Per-knob pickup engagement for the CURRENT context (reset whenever the
    /// context key changes, so a re-pointed bank re-engages softly).
    pub knob_engaged: [bool; crate::controller::KNOB_COUNT],
    /// Last normalized CC value seen per knob (crossing detection for pickup).
    pub knob_last_cc: [Option<f32>; crate::controller::KNOB_COUNT],
    /// The context key the engagement state belongs to (mode + tab/generator/
    /// page); a change resets `knob_engaged` / `knob_last_cc`.
    pub knob_context_key: Option<String>,
    /// The ordered param universe for knob targeting, built lazily from
    /// `Params::param_map()`: declaration-ordered `(id, ptr)` list + id → index.
    /// (`ParamPtr`s are stable for the life of the plugin instance — the same
    /// contract `param_map` itself hands to nih-plug's own editors.)
    pub knob_params: Option<(Vec<(String, ParamPtr)>, std::collections::HashMap<String, usize>)>,
    /// Performer-mode slot picker: which slot's assignment popup is open.
    pub knob_assign_open: Option<usize>,
    /// Filter text for the assignment picker.
    pub knob_filter: String,

    /// Overlay string sidecar buffers (#135 P2): handle/watermark + title override,
    /// written to `overlay_sidecar_path()` (+ `overlay_gen` bump) on edit.
    pub overlay_handle: String,
    pub overlay_title: String,
    /// Loaded the overlay sidecar from disk yet?
    pub overlay_loaded: bool,
    /// GUI-side peak-hold/decay for the spectrum bars (the falling-cap look),
    /// decoupled from the audio block rate.
    pub spectrum_peaks: [f32; crate::audio::DISP_BINS],
    /// GUI-side peak-hold for the input level meter.
    pub level_peak: f32,
    /// Calibrated spectrogram/waterfall history (#333 T2): newest row first, each a
    /// snapshot of the RTA band levels (dBFS). Capped in the draw code.
    pub rta_waterfall: Vec<[f32; crate::audio::MAX_RTA_BANDS]>,

    // --- Oscilloscope (#333, s(M)exoscope-style; GUI-side display state) ---
    /// Horizontal zoom = pixels per sample (small = zoomed out/seconds, large =
    /// individual samples). Default ~2 samples per pixel.
    pub scope_px_per_sample: f32,
    /// Vertical zoom: display gain in dB (−60…+60).
    pub scope_amp_db: f32,
    /// Trigger mode: 0 Free, 1 Rising, 2 Falling, 3 Internal.
    pub scope_trigger: u32,
    /// Trigger threshold (−1…+1) for Rising/Falling.
    pub scope_trig_level: f32,
    /// Retrigger hold-off: minimum samples between triggers.
    pub scope_retrigger: f32,
    /// Internal-trigger period (ms) for the Internal mode.
    pub scope_internal_ms: f32,
    /// Only redraw on a trigger event (a more coherent, measurement-stable display).
    pub scope_sync_redraw: bool,
    /// Freeze: hold the current waveform for inspection.
    pub scope_freeze: bool,
    /// DC-kill: remove DC offset (recenter around zero).
    pub scope_dckill: bool,
    /// Channel to display: 0 L, 1 R, 2 Mid (L+R)/2.
    pub scope_channel: u32,
    /// The currently-displayed waveform (updated on trigger; held while frozen or
    /// between triggers when Sync Redraw is on).
    pub scope_display: Vec<f32>,
    /// Whether the scope controls have been seeded with their non-zero defaults.
    pub scope_inited: bool,
    /// Reverse channel to the visual: the live render resolution (for the Output
    /// Resolution readout). Opened lazily once the visual has created the file.
    pub render_feedback: Option<crate::ipc::FeedbackReader>,
    /// #554 Tier 1 — the editor's end of the frame mirror. `None` until first use; opening is
    /// total (a missing ring just never yields a frame), so this carries no error state.
    ///
    /// **#593 Tier 4 — these four fields are full Organon's only.** They are the editor side of
    /// the CPU mirror; Mind's editor draws the world itself, so a mind-edition build carries no
    /// reader, no staging buffer and no texture handle for a frame that is never published.
    /// `PresetUi` derives `Default`, so gating the fields needs nothing else.
    // ── #617 Tier 1 — workstation / immersive view mode ─────────────────────
    //
    // These live here rather than on `EditorCtx` because `editor_ui` already takes
    // `&mut PresetUi` and `&EditorCtx` is shared and immutable. Three of them carry data in
    // opposite directions across one frame boundary, which needs a `&mut` channel.
    /// **false = workstation** (the scene is a bounded pane the interface lays out),
    /// **true = immersive** (the scene fills the window and the interface floats over it —
    /// #593 Tier 4's behaviour, kept as a mode rather than replaced).
    ///
    /// Deliberately **not persisted** in Tier 1: `PresetUi` is transient editor state, and
    /// where the mode should be remembered is open question 2 on #617. It resets to
    /// workstation each launch, which is the default anyway.
    pub immersive: bool,
    /// The rect `editor_ui` reserved for the viewport pane, in **points**. Written by the UI,
    /// read by the host.
    ///
    /// ⚠️ **The host reads this one frame late, and that is inherent.** The scene has to be
    /// rendered *before* the interface that reserves its rect runs, so frame N draws at the
    /// size frame N−1 asked for. On the first frame it is `None` and the host picks a default;
    /// after a resize one frame is drawn at the old size and then it converges. Standard
    /// immediate-mode, invisible at 60 Hz, and the alternative — running the UI twice per
    /// frame to learn the rect first — costs a full layout pass to fix nothing anyone can see.
    pub scene_pane_rect: Option<nih_plug_egui::egui::Rect>,
    /// The egui texture the host rendered the scene into. Written by the host, painted by the
    /// UI. `None` until the first scene frame exists.
    pub scene_texture: Option<nih_plug_egui::egui::TextureId>,
    /// #621 — what this frame's pointer interaction asked the **camera** for, plus which side
    /// owns the drag in flight. Written by `scene_input::scene_viewport` while the UI runs, and
    /// drained by the host immediately after it.
    ///
    /// The fourth field in this group and the same kind of thing: transient view state that
    /// crosses one frame boundary. **Not a parameter and not a preset field** — recalling a
    /// Scene must not move the camera, exactly as it must not change the mode.
    pub scene_input: crate::scene_input::SceneInput,

    #[cfg(not(feature = "mind-edition"))]
    pub frame_reader: Option<crate::frame_ring::FrameRingReader>,
    /// The uploaded mirror frame. Kept across frames so a repaint with no *new* frame redraws
    /// the existing texture instead of re-uploading ~0.9 MB.
    #[cfg(not(feature = "mind-edition"))]
    pub frame_tex: Option<nih_plug_egui::egui::TextureHandle>,
    /// Scratch for the readback copy, reused so a 60 Hz repaint does not allocate.
    #[cfg(not(feature = "mind-edition"))]
    pub frame_buf: Vec<u8>,
    /// Frames since the last attempt to re-open a closed mirror. The visual can start *after*
    /// the editor, so a closed ring must be retried — but at ~1 Hz, not 60: with no visual
    /// running that would be an open + mmap + munmap syscall burst forever (the same throttle
    /// `bin/visual.rs` applies to re-opening the `Shared` reader).
    #[cfg(not(feature = "mind-edition"))]
    pub frame_retry: u32,
    // --- Performance status bar (#277) ---
    /// Whether the bottom performance / diagnostics status bar is showing.
    pub perf_open: bool,
    // #554 Tier 1 — there is deliberately no `viewport_open`. The viewport is native to the
    // editor window rather than a pane you opt into, so there is no open/closed state to carry:
    // it is drawn under the tab bar every frame and `Shared.mindview[3]` is requested
    // unconditionally. The field existed only while the viewport shipped behind a toggle.
    /// #551 Tier 2 — the saved theme library, loaded lazily on first panel draw.
    pub theme_lib: Option<crate::theme_config::ThemeLibrary>,
    /// Which library row is being renamed, and its in-progress text (the `rename_idx` idiom).
    pub theme_rename: Option<(usize, String)>,
    /// #551 Tier 1 — is the **UI theme** panel open? Editor-session state, deliberately not
    /// persisted with the theme itself: which panels you had open is not part of a look.
    pub ui_theme_open: bool,
    /// Ring of recent frame-time samples (ms), oldest→newest, for the scrolling
    /// frame-time graph. Sampled from the visual's reported fps on a fixed ~60 Hz
    /// wall clock (see `perf_next_sample_t`) so the graph's time axis is stable
    /// regardless of the editor's repaint rate; capped at `PERF_HIST_LEN`.
    pub perf_hist: Vec<f32>,
    /// egui time (seconds) at which the next `perf_hist` sample may be taken. Gates
    /// the ring to a steady cadence instead of once-per-repaint (which duplicates
    /// samples and warps the "last few seconds" span when repaints outrun frames).
    pub perf_next_sample_t: f64,
    // --- Key Map window state ---
    /// Whether the Key Map window is open.
    pub keymap_open: bool,
    /// Loaded the note→preset map from disk yet?
    pub keymap_loaded: bool,
    /// The editable note→preset mapping (source of truth for the UI; saved to
    /// disk and resolved into the audio-thread `KeyMap` on change).
    pub mapping: crate::keymap::KeyMapping,
    /// Which octave the keyboard is paged to (C0..C5).
    pub keymap_octave: i32,
    /// Set when the mapping or presets changed, so the editor rebuilds + saves
    /// the resolved map once at the end of the frame.
    pub keymap_dirty: bool,

    // --- Mind tab: AI Performer chat/agent (#317 Tier 1) ---
    /// The chat input buffer (the message being typed).
    pub chat_input: String,
    /// A local echo of the conversation (user prompts + agent replies), newest last.
    pub chat_log: Vec<String>,
    /// OpenAI-compatible localhost endpoint + model editable buffers (persisted to the
    /// `organic-math-agent.txt` sidecar). Loaded once from disk on first Mind-tab draw.
    pub agent_endpoint: String,
    pub agent_model: String,
    /// Whether the agent endpoint/model buffers have been seeded from the sidecar.
    pub agent_cfg_loaded: bool,
    /// UI-sync (#317): consumed-line cursor into the agent apply channel
    /// (`organic-math-agent-apply.txt`), so each agent action is mirrored onto the
    /// params exactly once. Seeded to the file's current length on first drain so a
    /// prior session's lines are never replayed.
    pub agent_apply_cursor: usize,
    pub agent_apply_seeded: bool,

    // --- Mind tab: #367 Tier 2b embedded-runtime prompt/reply (Runtime sub-section) ---
    /// The prompt being typed for the embedded `organic-math-mind-runtime` completion.
    pub mind_prompt_input: String,
    /// The streaming reply, polled from the runtime's reply sidecar
    /// (`organic-math-mind-reply.txt`) each repaint while a completion is in flight.
    pub mind_reply: String,
    /// #367 Tier 2 UX — the Mind console command line (the stdin REPL input the plugin
    /// forwards to the embedded runtime: `gen` / `load` / `temp` / `status` / …).
    pub mind_cmd_input: String,

    // --- Mind tab: #482 Tier 1 live-telemetry dashboard ---
    /// The editor's OWN reader of the activation ring (`mind_ring.rs`). mmap reads
    /// are non-destructive, so this sits alongside the visual's reader for free —
    /// no new IPC, no `Shared` change. Opened lazily once the writer exists.
    pub mind_reader: Option<crate::mind_ring::MindRingReader>,
    /// The Mind dashboard display state (peak-hold caps, scrolling effort scope,
    /// auto-gain, tokens/sec). Fed one `MindFrame` per repaint while streaming.
    pub mind_viz: crate::mind_viz::MindViz,

    // --- Mind tab: #423 Tier 1 the atlas (Design Space card) ---
    /// Selected built-in hardware-profile index (into `math::builtin_hardware_profiles`).
    pub atlas_profile_idx: usize,
    /// Chosen context length (tokens) the design-space traffic is stated at.
    pub atlas_ctx_tokens: u32,
    /// A hardware profile loaded from JSON via the "Load Hardware Profile…" rail;
    /// when present it overrides the built-in selection for the scan.
    pub atlas_custom_profile: Option<crate::math::HardwareProfile>,
    /// Whether the atlas card's defaults (profile idx + context) have been seeded.
    pub atlas_seeded: bool,

    // --- Intelligent preset names (#425) ---
    /// When set, saving a preset asks the locally-connected LLM to name it from what
    /// the scene actually is (generator / surface / material / look) instead of leaving
    /// the provisional `Preset N`. Keys off a configured endpoint, NOT on the agent
    /// being engaged (`agent_on`); no reachable model → the provisional name stands.
    /// Defaults ON — seeded once on the first presets-rail draw (`auto_name_seeded`).
    pub auto_name_presets: bool,
    /// Whether `auto_name_presets` has been seeded to its default-on value yet.
    pub auto_name_seeded: bool,
    /// In-flight auto-name requests: `(id, scope, provisional_name)`. The reply patches
    /// the preset whose name still equals `provisional_name` in that scope. `id` is the
    /// `name_gen` value carried in the request JSON and echoed in the reply.
    pub name_pending: Vec<(u32, PresetScope, String)>,
}

#[cfg(test)]
mod preset_io_tests {
    use super::*;
    use crate::params::OrganicMathParams;

    /// #354 regression: the subset save drops each tab's *non-owned* fields. If a
    /// dropped field lacked a `#[serde(default)]`, the whole file failed to
    /// deserialize and `load()` silently returned an EMPTY list — every preset
    /// vanished on reopen. Every subset must strict-parse and round-trip to a
    /// non-empty list, keeping the bucket's own fields.
    #[test]
    fn subset_presets_survive_a_save_load_round_trip() {
        let mut v = PresetValues::capture(&OrganicMathParams::default());
        v.loop_count_x = 3; // Generator — had no serde default before the fix
        v.ambient = 0.5; // Look — had no serde default before the fix
        v.tempo = 141.0; // Settings — had no serde default before the fix
        v.terrain_enabled = true; // Environment
        let preset = Preset { name: "P".into(), values: v };

        let scopes: [&[EditorTab]; 7] = [
            &EditorTab::SCENE,
            &[EditorTab::Generator],
            &[EditorTab::Look],
            &[EditorTab::Environment],
            &[EditorTab::Settings],
            &[EditorTab::Audio],
            &[EditorTab::Synth],
        ];
        for tabs in scopes {
            let json = serde_json::to_string(&vec![subset_entry(&preset, tabs)]).unwrap();
            // The bug: this strict parse used to FAIL when the subset omitted a
            // no-default field, so load()'s `.ok()` blanked the whole store.
            let strict: Result<Vec<Preset>, _> = serde_json::from_str(&json);
            assert!(strict.is_ok(), "subset for {tabs:?} must strict-parse");
            let back = parse_presets_lenient(&json);
            assert_eq!(back.len(), 1, "subset for {tabs:?} must reload one preset");
            assert_eq!(back[0].name, "P");
        }

        // The owning tab keeps the value; a non-owning tab drops+defaults it.
        let gen = parse_presets_lenient(
            &serde_json::to_string(&vec![subset_entry(&preset, &[EditorTab::Generator])]).unwrap(),
        );
        assert_eq!(gen[0].values.loop_count_x, 3, "Generator keeps loop_count_x");
        let look = parse_presets_lenient(
            &serde_json::to_string(&vec![subset_entry(&preset, &[EditorTab::Look])]).unwrap(),
        );
        assert_eq!(look[0].values.ambient, 0.5, "Look keeps ambient");
        assert_eq!(look[0].values.loop_count_x, 0, "Look drops+defaults loop_count_x");

        // An Environment subset preserves the loaded .hdr reference.
        let mut ve = preset.clone();
        ve.values.hdr_path = "/tmp/x.hdr".into();
        let env = parse_presets_lenient(
            &serde_json::to_string(&vec![subset_entry(&ve, &[EditorTab::Environment])]).unwrap(),
        );
        assert_eq!(env[0].values.hdr_path, "/tmp/x.hdr", "Environment keeps hdr_path");
    }

    /// One malformed entry must not wipe the rest of the list.
    #[test]
    fn lenient_parse_salvages_a_bad_entry() {
        let json = r#"[{"name":"ok","values":{}},{"name":"bad","values":42}]"#;
        let out = parse_presets_lenient(json);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "ok");
    }

    /// #354 (Key Map fix): `overlay_tabs(SCENE)` takes the source's Scene-tab
    /// fields but keeps the base's non-Scene fields — so a MIDI-held Scene preset
    /// overlays only its four tabs onto the live look, never resetting
    /// Settings/Audio/Synth to a sparse preset's defaults.
    #[test]
    fn overlay_tabs_takes_scene_keeps_the_rest() {
        let mut base = PresetValues::capture(&OrganicMathParams::default());
        base.loop_count_x = 7; // Generator (Scene) — should be overwritten
        base.tempo = 155.0; // Settings (non-Scene) — should be kept
        base.meter_hud = true; // Audio (non-Scene) — should be kept

        let mut src = PresetValues::capture(&OrganicMathParams::default());
        src.loop_count_x = 2;
        src.tempo = 90.0;
        src.meter_hud = false;

        base.overlay_tabs(&src, &EditorTab::SCENE);
        assert_eq!(base.loop_count_x, 2, "Scene field takes the preset's value");
        assert_eq!(base.tempo, 155.0, "Settings field keeps the live base value");
        assert_eq!(base.meter_hud, true, "Audio field keeps the live base value");
    }

    /// #541 S2 T1 — the mindview spine is **not** preset-captured (#484's
    /// default-inert contract: a pane arrangement is a workspace, not a look), and
    /// the proof is that a preset written **before** the block existed still loads.
    ///
    /// The "old preset" here is the real thing: `{}` — a values object with none of
    /// today's fields, which is exactly what a file from an older build looks like
    /// to serde. It must deserialize, and recalling it must leave the panes inert
    /// rather than resetting a live arrangement to a sparse file's defaults.
    #[test]
    fn old_presets_load_and_leave_the_mindview_spine_inert() {
        let out = parse_presets_lenient(r#"[{"name":"from an older build","values":{}}]"#);
        assert_eq!(out.len(), 1, "a pre-mindview preset must still load");
        assert_eq!(out[0].name, "from an older build");

        // Nothing was added to PresetValues, so the snapshot a preset produces is
        // all-zero across the reservation — Single grid, pane 0 = today's scene.
        let s = out[0].values.to_shared();
        assert!(s.mindview.iter().all(|&v| v == 0.0), "grid header must stay inert");
        assert!(s.mindview_pane.iter().all(|&v| v == 0.0), "pane block must stay inert");
        assert_eq!(s.mindview_gen, 0);
        assert_eq!(s.mindview_pane_count(), 1, "one viewport, as today");

        // Same for a preset captured from live params, and it must agree with the
        // params packer byte-for-byte (neither side writes the reservation).
        let p = OrganicMathParams::default();
        let from_params = p.to_shared();
        let from_preset = PresetValues::capture(&p).to_shared();
        assert_eq!(from_preset.mindview, from_params.mindview);
        assert_eq!(from_preset.mindview_pane, from_params.mindview_pane);
        assert_eq!(from_preset.mindview_gen, from_params.mindview_gen);

        // And a full save→load round trip of a modern preset is unaffected.
        let preset = Preset { name: "P".into(), values: PresetValues::capture(&p) };
        let json = serde_json::to_string(&vec![preset]).unwrap();
        let back = parse_presets_lenient(&json);
        assert_eq!(back.len(), 1);
        assert!(back[0].values.to_shared().mindview_pane.iter().all(|&v| v == 0.0));
    }
}
