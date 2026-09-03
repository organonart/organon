//! Shared-memory bridge between the plugin (writer) and the visual window
//! (reader). The plugin writes the live parameter snapshot every process block;
//! the visual reads it each frame. A memory-mapped file keeps it lock-free and
//! dependency-light — values are control-rate, so an occasional torn read of a
//! single float is visually irrelevant.

use bytemuck::{Pod, Zeroable};
use std::fs::OpenOptions;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

/// Flat, `Pod` parameter snapshot. Vec3s are padded to vec4 for clean alignment.
/// Funcs/flags are u32. The visual reconstructs `ParamValues` + `FuncName` from
/// these. Angle is intentionally absent — the visual owns the animation clock.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Shared {
    pub seq: u32,
    pub layout_version: u32,
    pub loop_count: [f32; 4], // x, y, z, q
    pub rot_amp: [f32; 4],    // x, y, z = rotation amp; w = continuous-rotation flag (0/1)
    /// x, y, z = per-axis rotation SPEED (angle advance ∝ speed · inc_scale per
    /// frame); w = inc_scale (the global speed). The animation clock reads these;
    /// `draw_tissue` no longer uses a rotation offset.
    pub rot_mod: [f32; 4],
    pub trans_amp: [f32; 4],
    pub trans_mod: [f32; 4],
    pub lighting: [f32; 8], // ambient(IBL mult), key, fill, key elev, key azim, glow, opacity, material_type
    pub scale_amp: f32,
    pub rot_func: u32,
    pub trans_func: u32,
    pub scale_func: u32,
    pub animate: u32,
    pub pulse: u32,
    pub tempo: f32,
    /// Reserved (was the Pulse Depth auto-pump, now removed). Kept to preserve the
    /// struct layout + the clip CC map (slot 26); always written 0.
    pub pulse_depth: f32,
    // --- PBR / IBL (appended; existing field offsets unchanged) ---
    /// [metallic, roughness, exposure, env_intensity, env_rotation_deg, bloom,
    /// bloom_threshold, glass_ior]
    pub pbr: [f32; 8],
    /// Bumped by the plugin/visual when a new .hdr is chosen; the visual watches
    /// it and re-runs IBL precompute. 0 = no .hdr yet → procedural sky.
    pub hdr_gen: u32,
    /// Live host transport, written by the plugin each block for tempo-sync:
    /// `[playing(0/1), pos_beats (wrapped mod 1024 to keep f32 precision),
    /// host_tempo_bpm, host_has_tempo(0/1)]`. The visual PLL-locks its beat
    /// clock to `pos_beats` when `tempo_sync` is on and the host supplies a
    /// tempo while playing; otherwise it free-runs off the manual `tempo`.
    pub transport: [f32; 4],
    /// 1 = lock the beat clock to host transport when available (was `_pad`).
    pub tempo_sync: u32,
    /// Auto-orbit camera motion: `[path_id, base_speed (cycles/beat), kick
    /// (per-beat velocity impulse), damping (per-beat velocity retention,
    /// 0..1)]`. `path_id` 0 = Off (manual drag only). The visual integrates a
    /// phase + angular velocity from these, kicked on each beat of the PLL clock.
    pub camera: [f32; 4],
    /// Overall amplitude of the camera path swing (0..1).
    pub cam_amount: f32,
    /// Two pulse→param modulation slots: `[a_target_id, a_depth, b_target_id,
    /// b_depth]`. Each slot adds `depth · span(target) · env` to its target
    /// param each frame, where `env` is the decaying beat impulse and `span` is
    /// a per-target musical amount. `target_id` 0 = None. Active only while
    /// `pulse` is on; depth is bipolar (−1..1).
    pub routing: [f32; 4],
    /// Surface render mode: 0 = Original (independent cubes), 1 = Flow-aligned
    /// (each cube oriented + stretched toward its successor so they connect into
    /// ribbons/tubes). (Appended; existing field offsets unchanged.)
    pub surface_mode: u32,
    /// Additive surface-shading modifiers, layered on top of whatever
    /// `material_type` is active (they do NOT replace it):
    /// `[sss_amount, sss_distortion, sss_power, irid_amount, irid_scale,
    /// irid_shift, _, _]`. Translucency is a Barré-Brisebois back-scatter lobe
    /// (light glows through the surface when backlit); iridescence is a
    /// view-angle thin-film spectral tint. Both inert at amount 0.
    /// (Appended; existing field offsets unchanged.)
    pub surface_fx: [f32; 8],
    /// True-HDR (macOS EDR) output request: 0 = SDR, 1 = HDR. The visual
    /// edge-detects changes and swaps the swapchain + tonemap accordingly. Lets
    /// the editor's Renderer checkbox drive HDR (the visual's **H** key still works
    /// standalone). (Appended; existing field offsets unchanged.)
    pub hdr_output: u32,
    /// HDR highlight roll-off knee (0..1). Where the composite shoulder starts
    /// compressing highlights toward the EDR headroom. (Appended.)
    pub hdr_knee: f32,
    /// Wide-gamut HDR colorspace: 0 = extendedLinearSRGB (Rec.709), 1 = the wide
    /// extendedLinearITUR_2020 (Rec.2020) container. The visual re-asserts the metal
    /// layer colorspace when this changes. Pairs with `hdr_vivid` (the gamut-expansion
    /// amount); on its own (vivid = 0) it's a colour-accurate Rec.2020 conversion.
    /// (Appended; field renamed from `hdr_p3` — same offset/type, wire-compatible.)
    pub hdr_wide: u32,
    /// SDR tone-map operator id (0 ACES, 1 AgX, 2 Reinhard, 3 Neutral). (Appended.)
    pub tonemap: u32,
    /// Tone-map operator id (same space) for the ENVIRONMENT backdrop only, so the
    /// HDR panorama can use a gentler curve (e.g. AgX) than the geometry. Applied in
    /// both SDR and true-HDR output (in HDR, geometry uses the headroom shoulder but
    /// the backdrop still uses this), so the skybox looks consistent across
    /// displays. (Appended.)
    pub bg_tonemap: u32,
    /// MSAA sample count (1/2/4/8). The visual rebuilds the scene pipelines +
    /// multisample targets when this changes. (Appended.)
    pub msaa: u32,
    /// Background visible (1) or black (0); IBL lighting is unaffected. (Appended.)
    pub bg_visible: u32,
    /// Background brightness multiplier (skybox only). (Appended.)
    pub bg_intensity: f32,
    /// Environment tint hue in degrees + amount (0..1 saturation). Tints the IBL
    /// lighting and background; works with a loaded HDR or the procedural sky.
    /// (Appended.)
    pub env_tint_hue: f32,
    pub env_tint_amt: f32,
    /// Depth-prepass SSAO: `[enabled(0/1), radius (world units), intensity, bias]`.
    /// When disabled the visual skips the depth prepass + AO passes entirely and
    /// the composite uses AO = 1 (default look unchanged). (Appended.)
    pub ssao: [f32; 4],
    /// Audio-reactive band envelopes from the plugin's input analysis (attack/
    /// release-smoothed on the audio thread): `[sub, bass, low_mid, mid, high,
    /// rms_level, _, _]`. `[5]` (#248 Tier 1) is the smoothed broadband RMS input
    /// level (post-gain, ~1 at full scale) — the loudness envelope that drives the
    /// audio-dipole's amplitude. All 0 when audio-reactive is off or there's no
    /// input signal. The visual can swap `audio[1]` (bass) in as its pulse source.
    /// (Appended.)
    pub audio: [f32; 8],
    /// Pulse source: 0 = synthetic beat clock, 1 = audio bass envelope. Selects
    /// what drives the pulse routing + the exposure/glow pump. (Appended.)
    pub pulse_source: u32,
    /// Speed Pulse — a logarithmic kick to the global rotation speed with its own
    /// attack/decay envelope: `[amount (decades), attack_ms, decay_ms, _]`. The
    /// visual multiplies the global speed by `10^(env·amount)`, so a hit knocks the
    /// spin up by whole powers of 10 and falls back. `amount = 0` is inert.
    /// (Appended.)
    pub speed_pulse: [f32; 4],
    /// Continuous-mode wave depth (0..1): how strongly `rot_func` shapes the
    /// winding *velocity* (always forward, mean-preserving). 0 = constant spin
    /// (default look). No effect in pendulum mode. (Appended.)
    pub cont_shape: f32,
    /// Metaball mode look: `[radius, threshold, smoothness, _]`. Only used when
    /// `surface_mode` = 3 (Metaball): the node set is baked into a 3D field and
    /// raymarched as one smooth skin. `radius` = per-node influence (world units,
    /// must exceed node spacing for blobs to fuse), `threshold` = the iso level,
    /// `smoothness` = falloff-edge sharpness. (Appended.)
    pub metaball: [f32; 4],
    /// Bioluminescence — colour-over-time effects, both free-running:
    /// `[cycle_speed, ripple_intensity, ripple_speed, ripple_freq, ripple_sharp,
    /// ripple_geom, _, _]`.
    /// - `cycle_speed` (cycles/sec, signed): advances a phase added to the palette
    ///   sweep so the gradient flows along the strand/field. Inert at 0; no effect
    ///   with the Native palette (nothing to cycle).
    /// - The rest drive a travelling HDR **emissive ripple** (the cone-jelly pulse),
    ///   computed analytically per fragment from world position: `ripple_intensity`
    ///   (linear HDR peak, 0 = off), `ripple_speed` (signed), `ripple_freq` (number
    ///   of bands), `ripple_sharp` (band tightness), `ripple_geom` (0 = radial shells
    ///   from the field centre, 1 = axial wavefront along +Y). (Appended.)
    pub bio: [f32; 8],
    /// Membrane mode (`surface_mode` = 4): `[weave, show_strands, arms, close]`.
    /// `weave` selects which grid axis/axes to loft sheets across (see
    /// `MembraneWeave`: 0 = Auto/dominant, 1/2/3 = X/Y/Z, 4 = Web/all).
    /// `show_strands` (0/1) also draws the boundary strands as swept tubes under
    /// the membrane. `arms` (0/1) = Skin-Arms mode: skin each strand as its own
    /// closed capped finger with gaps between arms (the volume-render hull) instead
    /// of one continuous shell; the build path (Impostor/Mesh) rides `membrane_fx[1]`.
    /// `close` (0/1) bridges the loft seam when the strand grid wraps a full 360°.
    /// (Appended; `arms`/`close` reuse the two previously-reserved slots.)
    pub membrane: [f32; 4],
    /// Reaction–diffusion (Turing) surface patterning:
    /// `[feed, kill, scale, intensity, albedo_mix, _, _, _]`. A Gray–Scott sim runs
    /// on a tiling texture; the lit shaders sample it triplanar as crawling HDR
    /// emissive dapple (`intensity`, 0 = off) and optional pigment (`albedo_mix`).
    /// `feed`/`kill` morph spots ↔ stripes ↔ maze. (Appended.)
    pub rd: [f32; 8],
    /// Which generative algorithm builds the node field (see `params::GeneratorMode`):
    /// 0 = Original (the classic cube-field). The visual dispatches on this to pick
    /// the node producer; everything downstream (surface/material/look) is shared.
    /// (Appended; existing field offsets unchanged.)
    pub generator: u32,
    /// Frenet–Serret generator params (used when `generator` = 1): `[strands,
    /// nodes, step_ds, kappa, kappa_amp, kappa_freq, tau, tau_amp, tau_freq,
    /// spread, thickness, func_id]`. κ/τ = base + amp·func(freq·s + phase); the
    /// phase is the global animation clock. (Appended.)
    pub frenet: [f32; 12],
    /// DNA double-helix params (used when `generator` = 2): `[form, bp, bp_per_turn,
    /// rise_Å, radius_Å, groove_deg, left(0/1), sigma, super_radius_Å, seed,
    /// thickness, twist_breathe, _, _, _, _]`. Form 0/1/2/3 = A/B/Z/Custom (A/B/Z
    /// preset the geometry table). Supercoiling obeys L = T + W. (Appended.)
    pub dna: [f32; 16],
    /// Strange-attractor params (used when `generator` = 3): `[field, seeds, seed_val,
    /// spread, dt_mult, trail, head_speed, scale, thickness, _, _, _]`. Field
    /// 0/1/2/3 = Lorenz/Aizawa/Thomas/Halvorsen. The visual owns the stateful per-
    /// seed heads + trail ring buffers it integrates. (Appended.)
    pub attr: [f32; 12],
    /// Spherical-harmonic params (used when `generator` = 4): three (mode_idx, amp,
    /// freq) slots + `[radius, theta_res, phi_res, thickness]`:
    /// `[m0,a0,f0, m1,a1,f1, m2,a2,f2, radius, theta, phi, thickness, _, _, _]`.
    /// Displacement = Σ ampₖ·cos(freqₖ·phase)·Yₗᵐ. (Appended.)
    pub harm: [f32; 16],
    /// L-system params (used when `generator` = 5): `[system, depth, angle_deg, step,
    /// sway_amp_deg, sway_freq, grow, thickness, _, _, _, _]`. System 0/1/2/3 =
    /// Fern/Bush/Tree/Seaweed. Sway animates the turn off the global clock; grow is
    /// the unfurl growth-front fraction. (Appended.)
    pub ls: [f32; 12],
    /// Curl-noise flow params (used when `generator` = 6): `[seeds, seed_val, spread,
    /// scale, steps, dt, flow, bound, thickness, _, _, _]`. Particles advected
    /// through the curl of a noise potential (divergence-free); `flow` evolves the
    /// noise in time off the global clock; `bound` pulls toward the origin. (Appended.)
    pub cn: [f32; 12],
    /// Universal "breath" — a pulse-driven uniform scale of the whole scene about its
    /// centre, applied at the camera/view level so it works for *every* generator and
    /// surface mode (the structure swells against a fixed sky). `[amount, attack_ms,
    /// decay_ms, _]`. `amount` = scale depth at a full pulse (a full pulse swells the
    /// scene by × (1 + amount); 0 = inert). Driven by the same pulse envelope (beat or
    /// audio), smoothed with its own attack/decay. (Appended.)
    pub breath: [f32; 4],
    /// Circular-polarization radiation-field params (used when `generator` = 7):
    /// `[rings, spokes, samples, ray_len, k, amp, falloff, handed(0/1), spread_deg,
    /// swirl, show_b(0/1), thickness, _, _, _, _]`. A (rings×spokes) lattice of rays
    /// (cone half-angle `spread_deg` about +Y); each ray is the rotating E-field
    /// helix `E ∝ (1/r)[cos·ê₁ + sin·ê₂]`, with the perpendicular B helix when
    /// `show_b`. ωt + the swirl precession ride the global clock. (Appended.)
    pub pol: [f32; 16],
    /// Maxwell radiation-field params (used when `generator` = 8): `[lines(0/1),
    /// gen_blend(E↔B, 0..1), source_count, dipoles(0/1), separation, phase_offset, swirl,
    /// near, k, amp, r_min, thickness, rings, spokes, samples, ray_len, spread_deg,
    /// seeds_per_source, max_steps, step_ds, bound, _, _, _]`. Real source fields
    /// (point charges / oscillating dipoles, superposed, retarded time): `lines` 0 =
    /// a (θ,φ) lattice drawing the E/B tip (Grid), 1 = field-line streamlines
    /// (Streamlines). ωt + swirl ride the global clock. (Appended.)
    pub maxwell: [f32; 24],
    /// Phyllotaxis / golden-angle params (used when `generator` = 9): `[surface,
    /// count, divergence_deg, radius_scale, parastichy, height, growth, breathe_amp,
    /// breathe_freq, rot_speed, thickness, _, _, _, _, _]`. Golden-angle node
    /// placement (`surface` 0 disk / 1 cone / 2 Fibonacci sphere / 3 log-spiral
    /// shell); the `parastichy` spiral families are the strands (Grid). Rotation +
    /// radius breathing ride the global clock. (Appended.)
    pub phyl: [f32; 16],
    /// Mandelbulb generator params (used when `generator` = 10): `[power, iters,
    /// scale (world radius), detail (raymarch steps), spin (auto-rotation off the
    /// global clock), morph (animated φ phase off the global clock), colour
    /// (orbit-trap intensity), bailout]`. Unlike the other generators this builds
    /// no nodes — the visual flags a raymarch render path (a sibling of Metaball)
    /// that distance-estimates the fractal per pixel and shades it with the shared
    /// PBR/IBL stack. (Appended.)
    pub mandelbulb: [f32; 8],
    /// Infinite terrain backdrop — a raymarched landscape drawn behind *any*
    /// generator (not a generator itself; a toggleable world layer). When on it
    /// replaces the skybox as the background and a synthetic fly-camera rides over
    /// the mountains while the orbit camera frames the generator in the sky:
    /// `[enabled(0/1), height, snow_level(0..1), fog_density, sun_elev_deg,
    /// sun_azim_deg, sun_intensity, scroll_speed, ride_height, noise_type, seed,
    /// ridged(0/1), brightness, haze, march_steps, march_octaves, res_divisor(1/2/4/8),
    /// palette, emissive, day_speed, water_on(0/1), water_level(0..1 of height),
    /// water_hue, water_ripple, scatter, godray, sun_lights_scene(0/1), _, _, _, _, _]`.
    /// `noise_type`/`seed` pick the noise tile; `palette` recolours rock/veg/snow;
    /// `emissive` adds HDR glow (lava/biolume per palette); `day_speed` cycles the
    /// sun; water_* add a reflective sea; `scatter`/`godray` are atmospherics;
    /// `sun_lights_scene` makes the terrain sun light the generator; `march_*`/
    /// `res_divisor` are the perf dials. (Appended; grown 24→32.)
    pub terrain: [f32; 32],
    /// Global output render-resolution scale (0.25..1.0): the whole scene + post
    /// chain render at `swapchain·scale` and the composite upscales to native. The
    /// manual target; ignored when `render_auto` is on. (Appended.)
    pub render_scale: f32,
    /// Dynamic resolution: 1 = the visual auto-adjusts the render scale toward a
    /// 60 FPS target; 0 = use `render_scale` as-is. (Appended.)
    pub render_auto: u32,
    /// HDR starfield (Yale Bright Star Catalog) — a global display layer drawn
    /// behind any generator, fading in as the day-cycle sun sets. The fixed
    /// equatorial star vectors are rotated into world space by R(latitude, sidereal-
    /// time) and drawn as additive HDR point sprites; a companion HDR sun disc rides
    /// the same day-cycle sun direction:
    /// `[enabled(0/1), brightness, twinkle_amt, twinkle_speed, star_size(px),
    /// latitude_deg, sky_rot_speed, mag_limit, saturation, sun_enabled(0/1),
    /// sun_brightness(HDR), sun_size_deg, sun_warmth, _, _, _]`.
    /// `mag_limit` thins the field (lower = only the brightest); `latitude` sets the
    /// celestial-pole height; `sky_rot_speed` wheels the sky over time. The sun's
    /// direction comes from the terrain sun elevation/azimuth (so it shares the day
    /// cycle); `sun_warmth` tints it from white (0) to deep orange (1). (Appended.)
    pub stars: [f32; 16],
    /// Particle Aura (#81): a GPU cloud of luminous motes advected through the
    /// active generator's velocity field (analytic where available, splatted from
    /// node motion otherwise). Off by default → image identical. Layout:
    /// `[tier(0 Off / 1 Lite / 2 Fluid), count_thousands, grid_res, speed, lifetime_s,
    /// spawn_radius, size, emissive, ribbon(0/1), ribbon_stretch, hue_shift,
    /// beat_burst(0..1), drag(0..1), turbulence(0..1), alpha, hide_generator(0/1)]`.
    /// `hide_generator` skips drawing the generator geometry (it still stirs the
    /// motes) so only the particles show. `grid_res` is
    /// the coarse velocity-grid resolution (perf dial); `count_thousands` ×1000 =
    /// particle count (perf dial); ribbon stretches each mote along its velocity
    /// for motion-blur streaks. (Appended; existing offsets unchanged.)
    pub particles: [f32; 16],
    /// Aura-Fluid (#81 showpiece) — used when `particles[0]` (tier) = 2 (Fluid). A
    /// real GPU Stable-Fluids solver evolves a PERSISTENT velocity grid that the
    /// motes ride: the generator's node motion is injected as a momentum source, the
    /// fluid advects + pressure-projects itself divergence-free (manufacturing swirl)
    /// + vorticity-confines (re-injecting the small eddies), so vortices shed off the
    /// moving structure and persist in its wake. Layout:
    /// `[force, vorticity, dissipation, jacobi_iters, inflow_decay, _, _, _]`.
    /// `force` = how hard node motion stirs the fluid; `vorticity` = confinement
    /// strength (the "more eddies / more curl" dial); `dissipation` = velocity damping
    /// per second (viscosity-ish; keeps the sim stable + the wake fading); `jacobi_iters`
    /// = pressure-solve iterations (quality/perf); `inflow_decay` = how fast the
    /// injected source velocity is forgotten so old wake dominates. (Appended.)
    pub fluid: [f32; 8],
    // --- "Jewel Box" look upgrade (#80): surface-to-surface light transport ---
    /// Part A — Inter-cube reflections (screen-space reflections):
    /// `[enabled(0/1), intensity, max_roughness (cutoff), thickness (world),
    /// max_steps, stride (px), _, _]`. SSR marches the HDR scene colour + the
    /// single-sample depth prepass to reflect neighbouring cubes; on a miss it
    /// leaves the IBL/env reflection the material shader already wrote (no seam).
    /// All inert when `enabled = 0` (the prepass + SSR passes are skipped and the
    /// composite adds nothing). (Appended.)
    pub ssr: [f32; 8],
    /// Part B — Bounced GI (irradiance probe volume): `[enabled(0/1), intensity,
    /// falloff, _]`. A coarse 6³ probe grid is filled CPU-side from the field's
    /// node positions + emissive tints (each probe gathers nearby nodes' colour,
    /// distance-weighted) and uploaded as a uniform; the cube shader trilinearly
    /// samples it and adds coloured diffuse bounce into the ambient term. Inert at
    /// `intensity = 0`. (Appended.)
    pub gi: [f32; 4],
    /// Part C — Spectral glass: `[dispersion, caustic, thin_film, spectral_samples]`.
    /// Dispersion splits the Glass refraction into wavelength-offset IORs (rainbow
    /// fringing); caustic boosts focused bright spots seen through the body;
    /// thin_film adds a physically-motivated interference tint at grazing angles.
    /// `dispersion = 0` reproduces today's single-IOR glass exactly. (Appended.)
    pub glass_spec: [f32; 4],
    /// Voxel mode look (`surface_mode` = 5): `[grid_res, threshold, radius,
    /// _reserved, emission, ao, shadow, quantize, beat→threshold,
    /// trails (reserved), _, _]`. The node set is splatted into a 3D occupancy grid
    /// and DDA-raymarched as crisp grid-snapped cubes (flat face shading, voxel AO,
    /// soft shadows, palette posterize). `grid_res` is the perf dial; `radius` sets
    /// strand thickness; `beat→threshold` pumps the fill on the pulse. (Appended;
    /// existing offsets unchanged.)
    pub voxel: [f32; 12],
    /// Voxel GI (#89), used only in Voxel mode: `[enabled(0/1), strength, max_dist
    /// (0..1 fraction of the structure size), sky/ambient fill]`. When enabled the
    /// visual builds a mip pyramid of the splatted field each frame and cone-traces
    /// it for bounced colour (emissive voxels bleed onto neighbours + the world).
    /// Default off → the #88 voxel look is byte-identical. (Appended.)
    pub voxel_gi: [f32; 4],
    /// Kaleidoscopic Fractal (KIFS) generator params (used when `generator` = 11):
    /// `[sectors, fold_c, iterations, iter_rot, spin, breathe, zoom, tunnel(0/1),
    /// rays, ring_gain, glow, hue, pattern_id, palette_id, color_speed, warp, flow,
    /// env(0/1), petals, contrast, sharp, _, _, _]`. N-fold kaleidoscopic symmetry
    /// feeding a selectable fractal engine (`pattern_id`: inversion / mandelbox /
    /// sierpinski / log-spiral / kleinian), drawn as a fullscreen field — flat or a
    /// receding tunnel (`flow` = forward speed) — with a selectable colour palette
    /// (`palette_id`) cycled at `color_speed`. `warp` adds a domain-warp swirl;
    /// `env` bakes the flat field into the IBL to light other generators. Builds no
    /// nodes — its own fullscreen render path. (Appended.)
    pub kifs: [f32; 30],
    /// Boids / flocking params (used when `generator` = 12): `[count, perception,
    /// separation, sep_w, align_w, cohere_w, max_speed, max_force, trail, bounds,
    /// goal_w, thickness, seed, sim_speed, scale, _]`. The first STATEFUL
    /// generator — the visual owns the agent sim (pos/vel + per-agent trail ring
    /// buffers) and advances it with a fixed-dt accumulator; only the param block
    /// crosses IPC. Goal pull is beat-pulsed when Pulse is on. Slots 15+ carry the
    /// creature form: `[…, form, size, banking, _, _, _, _, _, _]` — `form` (0 =
    /// Surface/normal mode, 1..4 = Fish/Bird/Manta/Dart) overrides the surface mode
    /// with a per-agent creature mesh oriented by velocity. (Appended.)
    pub boids: [f32; 24],
    /// Soft-body bell params (#99; a stateful *mode* on the spherical-harmonic
    /// generator = 4): `[physical(0/1), stroke_depth, iters, damping, theta_max,
    /// sim_speed, _, _]`. When `physical` is on, the visual runs an XPBD `BellSim`
    /// (the jellyfish bell genuinely contracts + recoils) instead of the closed-
    /// form `harmonic_strands`; it reuses harm radius/θ-res/φ-res/thickness for
    /// the grid. The sim state lives visual-side; only this block crosses IPC.
    /// The contraction stroke is beat-driven (the pulse envelope). (Appended.)
    pub bell: [f32; 8],
    /// Physically based atmosphere (#100) — a global world layer (like terrain/
    /// stars; NOT preset-captured). `[enabled(0/1), turbidity, mie_g, sun_intensity,
    /// ground_albedo, exposure, aerial_strength, rayleigh]`. When enabled, a Nishita
    /// single-scattering sky is BAKED into the IBL env (so cubes are lit by the real
    /// sky at the real sun angle) and the terrain pass derives its sky + aerial
    /// perspective from the same integral. The sun direction rides the terrain sun
    /// elevation/azimuth (the day cycle). `turbidity` = aerosol density (haze + sun
    /// halo); `mie_g` = forward-scatter anisotropy; `aerial_strength` scales the
    /// terrain haze; `rayleigh` scales the blue. Default off → image unchanged.
    /// (Appended; existing offsets unchanged.)
    pub atmosphere: [f32; 8],
    /// Volumetric clouds (#102, Part A) — a raymarched cloud layer in the terrain
    /// pass replacing the flat value-noise sheet. A global world layer (like terrain/
    /// stars/atmosphere; NOT preset-captured). Layout:
    /// `[enabled(0/1), coverage(0..1), density, base_alt(world y),`
    /// ` thickness, march_steps, detail(erosion 0..1), drift_speed,`
    /// ` hg(forward-scatter g 0..0.95), absorption, shadow_strength(on terrain 0..1),`
    /// ` ambient(sky fill)]`. Density is a coverage/erosion fBm; a light march toward
    /// the sun gives self-shadowing (silver linings); HG phase + Beer extinction. Only
    /// drawn while the **terrain backdrop** is on (it lives in the terrain sky). Off by
    /// default → image unchanged. (Appended; existing offsets unchanged.)
    pub clouds: [f32; 12],
    /// FFT (Tessendorf) ocean (#102, Part B) — a statistical wind-wave ocean
    /// replacing the pooled reflective water; a global world layer (NOT preset-
    /// captured). Layout: `[enabled(0/1), level(world y), wind_speed, wind_dir_deg,
    /// amplitude, choppiness, tile_size(world units/tile), foam, glitter, hue(0..1),
    /// depth_absorption, _]`. The wave field is an inverse-FFT of a Phillips spectrum
    /// synthesized CPU-side (`ocean.rs`) into a tiling normal/height/foam texture the
    /// terrain water shader samples; the shader adds Fresnel sky reflection, depth
    /// absorption, sun glitter + foam. **Setting `enabled` with the terrain backdrop
    /// OFF gives an infinite ocean-only world** (the terrain pass still draws the sky
    /// + sea). Off by default → image unchanged. (Appended; offsets unchanged.)
    pub ocean: [f32; 12],
    /// HDR gamut-expansion amount (0..1, #119). Only meaningful when `hdr_wide` is on
    /// and the EDR surface is active. 0 = colour-accurate (Rec.709 → Rec.2020, colours
    /// unchanged); 1 = full gamut stretch (the Rec.709 spectrum is pushed out to the
    /// Rec.2020 primaries → far more saturated, mimicking + exceeding what a wide-gamut
    /// projector's SDR mode does for free). Per-display (NOT preset-captured). (Appended.)
    pub hdr_vivid: f32,
    /// Tessellation generator (#121; used when `generator` = 13). Layout:
    /// `[family, depth, scale, thickness, view, height, height_mode, _, _, _, _, _,
    ///   _, _, _, _]` — `family` (0 = Penrose P3), `depth` = inflation levels (tile
    /// count ≈ 10·φ^(2·depth)), `scale` = overall size, `thickness` = edge-tube
    /// radius (Edges view); `view` (0 = Edges, 1 = Filled, 2 = Extruded prisms),
    /// `height` = extrusion height as a fraction of size (world height = height·scale),
    /// `height_mode` (0 = uniform, 1 = by tile type, 2 = radial); `beat_infl` =
    /// beat inflation-breathe amount, `ripple_amt`/`ripple_freq` = per-tile beat
    /// ripple (Phase 3); `construct` (0 = inflation, 1 = cut-and-project), `phason`
    /// = acceptance-window orbit amount (continuous flips), `grid_n` = multigrid
    /// grid range (Phase 4); `ammann` = Ammann-bar overlay amount, `hyp_p`/`hyp_q`
    /// = the hyperbolic {p,q} family parameters (follow-up). A node/geometry
    /// generator (no own
    /// shader): Edges → rods; Filled/Extruded ride the membrane mesh, so PBR /
    /// Chrome / Glass all apply. (Appended; existing offsets unchanged.)
    pub tessellation: [f32; 16],
    /// Minimal surfaces / TPMS generator (#127; used when `generator` = 14). Layout:
    /// `[family, scale, cells, iso, thickness, twist, steps, color, beat_iso, _, _,
    ///   _, _, _, _, _]` — `family` (0 = Gyroid, 1 = Schwarz P, 2 = Schwarz D),
    /// `scale` = world radius, `cells` = surface periods across the structure
    /// (channel count), `iso` = isolevel (the surface is F = iso; sweeping it
    /// breathes the channels), `thickness` = soap-film wall half-width, `twist` =
    /// domain twist about the vertical (radians/unit height), `steps` = raymarch
    /// budget, `color` = channel-band colour intensity, `beat_iso` = beat →
    /// isolevel breathe amount. A raymarched implicit-isosurface generator with its
    /// own shader (`minimal.wgsl`) — no nodes/surface modes; shares the PBR/IBL/HDR/
    /// glass/iridescence stack like the Mandelbulb. (Appended; existing offsets
    /// unchanged.)
    pub minimal_surface: [f32; 16],
    /// Capture / production frame (#135 Phase 1). Layout:
    /// `[aspect, long_edge, custom_w, custom_h, r, g, b, frame_guide, lock_window,
    ///   _, _, _]` — `aspect` (0 = Native → render straight to the window; 1 = 9:16,
    /// 2 = 16:9, 3 = 1:1, 4 = 4:5, 5 = 21:9, 6 = Custom), `long_edge` = output
    /// long-edge px (short edge from the aspect; **0 = match the display**, the
    /// visual substitutes the window's longest side so no downscale), `custom_w`/`custom_h` = px when
    /// aspect = Custom, `r`/`g`/`b` = letterbox bar colour (linear 0..1),
    /// `frame_guide` / `lock_window` = 0/1 flags. A per-display capture setting
    /// (not preset-captured), like HDR/MSAA. (Appended; existing offsets unchanged.)
    pub capture: [f32; 12],
    /// Capture overlay style (#135 Phase 2). Layout:
    /// `[enabled, opacity, scale, show_title, show_desc, show_formula, show_readouts,
    ///   show_handle, panel_r, panel_g, panel_b, panel_opacity, text_r, text_g, text_b,
    ///   _]` — `enabled` master on/off, `opacity` whole-overlay alpha, `scale` font/zone
    /// scale, the five `show_*` per-zone flags, `panel_*` the readout-panel fill colour +
    /// alpha, `text_*` the default (non-symbol) text colour. The per-symbol colours + the
    /// formula image + the live readout values are program-supplied (`overlay_meta.rs`),
    /// not in the IPC. A per-display capture setting (not preset-captured), like `capture`.
    /// (Appended; existing offsets unchanged.)
    pub overlay: [f32; 16],
    /// Bumped by the editor when the overlay string sidecar (custom title / handle) is
    /// rewritten; the visual edge-detects it and re-reads `overlay_sidecar_path()` —
    /// mirrors `hdr_gen`. (Appended.)
    pub overlay_gen: u32,
    /// Capture decoration — 3-D axes + wireframe volume (#135 Phase 5). Layout:
    /// `[ax_on, ax_len, ax_ticks, ax_labels, ax_opacity, box_on, box_extent, box_subdiv,
    ///   box_r, box_g, box_b, box_opacity, ax_thick, _, _, _]` — `ax_on` master XYZ axes
    /// toggle, `ax_len` axis length, `ax_ticks`/`ax_labels` flags, `ax_opacity` alpha,
    /// `ax_thick` axis tube radius; `box_on` wireframe box, `box_extent` half-size,
    /// `box_subdiv` grid divisions per wall, `box_*` wall-grid colour + alpha. Axes are
    /// **shaded tubes with conical arrowheads** (triangle surface); the box is gridded
    /// **back walls only** (hidden-line "room"); both in the scene pass (shares camera +
    /// depth). Axis colours conventional (X red, Y green, Z blue); labels project through the
    /// overlay text pass. Per-display (not preset-captured). (Appended.)
    pub axes: [f32; 16],
    /// Synchrotron radiation generator (#150). Layout:
    /// `[radius, beta, charges, grid, extent, near, amp, thickness, rmin, perp,
    ///   view, line_seeds, line_steps, line_ds, line_bound, vol_layers,
    ///   reveal, invert, invert_radius, tilt, precess, _, _, _]` — `radius` orbit R,
    /// `beta` = v/c, `charges` bunched charges on the ring, `grid` samples/axis,
    /// `extent` plane half-size, `near` velocity-term weight (0 = radiation only),
    /// `amp` arrow gain, `thickness` rod radius, `rmin` source clamp, `perp` (>0.5)
    /// sample the plane ⟂ the orbit. `view` (0 = arrows / 1 = field lines / 2 = volume).
    /// **Field-line view:** `line_seeds` (Fibonacci-sphere seeds), `line_steps` (max RK4
    /// steps), `line_ds` (step length), `line_bound` (escape radius). **Volume view:**
    /// `vol_layers` (depth slices the arrow plane is extruded into; in-plane reuses
    /// `grid`/`extent`). **Arrow/volume legibility (#150 P5):** `reveal` (cull arrows
    /// below this soft-saturated |E|, both arrow + volume views), `invert` (>0.5 =
    /// sphere-invert the volume's display positions, turning it inside-out),
    /// `invert_radius` (the inversion sphere c). **3-D orbit motion (#150 P6):** `tilt`
    /// (orbit-plane tilt, **radians** off XY) + `precess` (the tilted plane's normal
    /// cones around Z at this rate) — the source tumbles in 3-D instead of one plane.
    /// Slots 21–23 reserved. (Appended; existing offsets unchanged.)
    pub synchrotron: [f32; 24],
    /// Post-composite creative FX (#152, Tier 1). Layout:
    /// `[enabled, style, style_amt, dof_amount, dof_focus, dof_range, chroma,
    ///   vignette, grain, grade_sat, grade_contrast, grade_temp, grade_gain,
    ///   feedback, outline_thresh, _]` — `enabled` master on/off (off → the whole FX
    /// pass is skipped, the image is byte-identical), `style` NPR mode (0 None / 1
    /// Toon / 2 Outline / 3 Halftone / 4 Dither / 5 Pixelate), `style_amt` its
    /// strength, `dof_*` depth-of-field, `chroma`/`vignette`/`grain` lens FX,
    /// `grade_*` the colour grade (sat/contrast/temp/gain; neutral at 1/1/0/1),
    /// `feedback` echo-trail persistence. A **Look** (preset-captured). Applied on
    /// the composited image by `fx.wgsl` (composite.wgsl untouched). (Appended.)
    pub fx: [f32; 16],
    /// Emissive volume surface mode (#152, Tier 1). Layout:
    /// `[radius, density, emission, absorption, steps, _, _, _]` — reuses the
    /// metaball field bake but raymarches it as a glowing participating medium
    /// (`metaball.wgsl::fs_volume`) instead of an isosurface. Selected by
    /// `SurfaceMode::Volume` (id 6). A **Surface** look (preset-captured, Generator
    /// tab). (Appended; existing offsets unchanged.)
    pub volume: [f32; 8],
    /// Temporal pass (#152 Tier 2: TAA + motion blur). Layout:
    /// `[taa_enabled, taa_blend, taa_sharpen, mb_enabled, mb_amount, mb_samples,
    ///   stochastic, _]` — `taa_enabled` temporal AA, `taa_blend` current-frame
    /// weight (lower = more history), `taa_sharpen` post-blend sharpen; `mb_enabled`
    /// + `mb_amount` (shutter) + `mb_samples` motion blur; `stochastic` = dither-
    /// discard order-independent glass (the OIT item — needs TAA to resolve). A
    /// per-display/quality setting (NOT preset-captured), like MSAA. Applied on the
    /// composited image by `temporal.wgsl` (composite.wgsl untouched). (Appended.)
    pub temporal: [f32; 8],
    /// Screen-space GI (#152 Tier 2). Layout: `[enabled, intensity, radius, rays]` —
    /// one diffuse bounce gathered from the depth prepass + scene colour
    /// (`ssgi.wgsl`), added by the composite like SSR. A captured **Look**.
    /// (Appended; existing offsets unchanged.)
    pub ssgi: [f32; 4],
    /// Cast shadows (#152 Tier 3). Layout: `[enabled, bias, strength, _]` — a single
    /// world-space depth map from the key light, PCF-sampled in `cube.wgsl` (group 4)
    /// to darken the key light's direct term where occluded. `bias` kills shadow acne;
    /// `strength` (0..1) fades the shadow. Instanced path only. A captured **Look**.
    /// (Appended; existing offsets unchanged.)
    pub shadow: [f32; 4],
    /// Voxel GI (#152 Tier 3, #10). Layout: `[enabled, intensity, rays, steps]` — the
    /// node field is voxelized into a 32³ colour volume and marched (world-space) to
    /// add a bounce into the HDR buffer (`vxgi.rs`/`vxgi.wgsl`). Unlike SSGI it sees
    /// off-screen/occluded emitters. Noisy at low rays → pairs with TAA. Instanced
    /// path only. A captured **Look**. (Appended; existing offsets unchanged.)
    pub vxgi: [f32; 4],
    /// Reflection controls (#163 Tier 1): `[reflect_tint, chrome_purity,
    /// glass_clarity, f0_override]`. All 0 by default → today's look is byte-identical.
    /// - `reflect_tint` mixes the RGB-cube palette INTO the reflection (0 = each
    ///   material's existing behaviour; up = tint/saturate/override the reflection).
    /// - `chrome_purity` (0..1) drives Chrome toward a **pure neutral mirror** (sharp,
    ///   untinted, high reflectance) — 0 = today's chrome.
    /// - `glass_clarity` (0..1) drives Glass toward **colourless clear glass** (crisp
    ///   Fresnel rim, minimal body tint, sharper refraction) — 0 = today's glass.
    /// - `f0_override` (0..1) lifts Standard's dielectric reflectance toward a mirror
    ///   without forcing metallic = 1 — 0 = today's metallic-driven F0.
    /// A captured **Look**. (Appended; existing offsets unchanged.)
    pub reflect: [f32; 4],
    /// Reflection probe / parallax (#163 Tier 2): `[source_id, box_scale,
    /// box_height_scale, blend]`. `source_id` 0 = **EnvOnly** (today — the reflection
    /// is a pure direction lookup into the infinitely-distant env map, so it depends
    /// only on a face's *orientation*), 1 = **Parallax** (box-projected: the reflection
    /// ray is intersected against the field's AABB — scaled by `box_scale` in XZ and
    /// `box_height_scale` in Y — so the reflection also shifts with a cube's *position*,
    /// killing the "painted-on sky" flatness). `blend` (0..1) mixes the corrected
    /// direction with the infinite one. Reuses the existing prefiltered env map — no
    /// new textures/passes. `source_id = 0` → today's look byte-identical. A captured
    /// **Look**. (Appended; existing field offsets unchanged.)
    pub refl_probe: [f32; 4],
    /// VXGI **specular** cone tracing (#163 Tier 3): `[strength, aperture, reach_frac,
    /// steps]`. Adds a *reflection* cone to the existing VXGI pass — from each pixel's
    /// reconstructed world position + normal it reflects the view ray and cone-marches
    /// the SAME voxel volume, so cubes reflect the actual scene (other cubes, off-screen
    /// emitters — no screen-edge dropout), added into the HDR buffer. `strength` 0 = off
    /// (today's look). `aperture` (0..1) widens the cone → glossier/blurrier; `reach_frac`
    /// scales the march distance by the scene diagonal; `steps` is the perf/quality dial.
    /// Requires the VXGI master toggle on (shares its voxelize + gather pass); works on
    /// the instanced path only. A captured **Look**. (Appended; existing offsets unchanged.)
    pub vxgi_spec: [f32; 4],
    /// Membrane screen-space FX opt-in: `[enabled, arm_build, arm_radius, _]`.
    /// `arm_build` = the Skin-Arms build path (0 = capsule Impostors, 1 = welded
    /// Mesh; see `membrane[2]`). `arm_radius` = the Skin-Arms capsule radius (0 =
    /// auto per-node thickness). The single-sample depth
    /// prepass normally rasterizes only the instanced cube/tube geometry, so all
    /// screen-space effects that reconstruct world position from it (VXGI diffuse +
    /// specular, SSAO, SSR, SSGI, DoF, TAA) are inert in **Membrane** surface mode. When
    /// `enabled` (0 = off, today's look), the membrane mesh is also drawn into the prepass,
    /// so those effects light up on the membrane too — at the cost of an extra depth draw
    /// (a perf escape hatch: leave it off to keep membrane exactly as it is now). A
    /// captured **Look**. (Appended; existing offsets unchanged.)
    pub membrane_fx: [f32; 4],
    /// Cinematic finishing (#167 Tier 1) — post-composite, inside the FX pass:
    /// `[hal_amount, hal_threshold, hal_width, hal_warmth, lf_amount, lf_ghosts,
    /// lf_halo, lf_streak]`.
    /// - **Halation** (`hal_*`): the warm chromatic bleed film gets *around* bright
    ///   highlights (a wide, red-weighted, tinted halo — distinct from bloom).
    ///   `hal_amount = 0` → off.
    /// - **Lens flares** (`lf_*`): screen-space ghosts + halo + anamorphic streak keyed
    ///   off the bright points. `lf_amount = 0` → off.
    /// Both live in `fx.wgsl` and only act when the **Post FX** master is on; at amount 0
    /// they add nothing. A captured **Look**. (Appended; existing offsets unchanged.)
    pub finishing: [f32; 8],
    /// Emissive cubes as real lights (#167 Tier 3): `[enabled, intensity, radius_frac,
    /// count]`. The visual picks the brightest `count` nodes (by colour luminance) and
    /// uploads them as point lights (group 3 binding 1); `cube.wgsl::many_lights` loops
    /// them and adds real Cook-Torrance direct lighting, so a glowing cube throws a crisp
    /// specular glint + a coloured diffuse pool onto its neighbours. `radius_frac` is the
    /// falloff radius as a fraction of the scene diagonal; `intensity` scales the emitted
    /// radiance. `enabled = 0` → count uploaded as 0, the loop adds nothing (today's look).
    /// Instanced path only. A captured **Look**. (Appended; existing offsets unchanged.)
    pub manylight: [f32; 4],
    /// Vector-field plotter generator (#173, Tier 1). Layout: `[preset, grid_x,
    /// grid_y, grid_z, extent, field_scale, amp, thickness, mag_map, tint_mode,
    /// evolve, z_lift, reveal, …reserved]`. `preset` picks the function bank
    /// entry (`math::vecfield_eval`); the `grid_*` lattice fills ±`extent` (an
    /// axis at 1 collapses to the central plane — the 2-D textbook plot);
    /// `field_scale` is the domain scale k in F(k·p); `mag_map` (0 soft / 1 log /
    /// 2 uniform) maps |F| to arrow length; `tint_mode` (0 magnitude / 1
    /// direction) picks the colour source; `evolve` rigidly rotates the field
    /// domain off the clock *and* drives the axisymmetric presets' intrinsic
    /// motion (rotation/source/saddle/dipole/helix — invariant under the
    /// rotation, so they animate physically instead); `z_lift` extends the
    /// planar classics into 3-D
    /// (Fz += lift·sin z); `reveal` culls arrows below a soft-saturated |F|.
    /// **Tier 2 (field lines), slots 13–22:** `view` (0 arrows / 1 field lines /
    /// 2 both — faint arrows under the lines), `seed_mode` (0 lattice / 1 random
    /// / 2 ring / 3 plane / 4 |F|-weighted), `line_seeds`, `line_steps` (a
    /// bidirectional line splits them half each way), `line_ds`, `bidir` (>0.5 =
    /// trace both directions and join through the seed), `line_color` (0 local
    /// |F| / 1 sweep along the line), `flow` (a brightness/thickness pulse that
    /// marches downstream; 0 = off), `flow_speed` (cycles per clock unit),
    /// `line_thickness`. Slot 23 reserved (Tier 3's builder gets its own block).
    /// (Appended; existing offsets unchanged.)
    pub vecfield: [f32; 24],
    /// Vector-field **function builder** (#173 Tier 3) — active when
    /// `vecfield[0]` = 12 (the Custom bank entry). Layout: 9 terms × 6 slots
    /// (`[func, gain, a, b, c, phase]`, ordered x1 x2 x3 y1 y2 y3 z1 z2 z3),
    /// each term = `gain·func(a·x + b·y + c·z + phase)` and each component of
    /// F is its 3 terms summed; then `[54]` the field operator (0 direct /
    /// 1 gradient ∇φ with φ = the Fx row / 2 curl ∇×A with A = the triple /
    /// 3 Helmholtz blend) and `[55]` the blend mix (0 = gradient … 1 = curl).
    /// `func` ids: 0 Off / 1 Const / 2 Linear / 3 Square / 4 Cube / 5 Abs /
    /// 6 Sin / 7 Cos / 8 Gauss / 9 Inverse (soft 1/u). Decoded by
    /// `math::VecBuildSpec::from_slots`; operators are central-difference, in
    /// the scaled domain q = k·p. Slots 56–63 reserved. (Appended; existing
    /// offsets unchanged.)
    pub vecbuild: [f32; 64],
    /// Fluid Ink (#182 Tier 1) — "the medium IS the image": an RGB dye field on
    /// the Aura-Fluid solver, injected at the generator's nodes (coloured by
    /// their live tints) and rendered as a lit volumetric into the HDR buffer.
    /// Enabling it runs the fluid solver even when the Particle Aura tier isn't
    /// Fluid. Layout: `[enabled, rate, radius, extinction, scatter, emissive,
    /// anisotropy, dissipation, steps, maccormack, half_res, reveal]`.
    /// - `rate` = dye injected per second at each node; `radius` = the splat
    ///   ball radius in grid cells.
    /// - `extinction` = Beer–Lambert σ; `scatter` scales the key-light HG
    ///   in-scatter (+ IBL-irradiance ambient); `emissive` makes the dye glow
    ///   (bioluminescent ink); `anisotropy` = HG g (−back / +forward scatter).
    /// - `dissipation` fades the dye on its own clock; `maccormack` (0/1) picks
    ///   the error-corrected sharp advection; `steps` = march step budget;
    ///   `half_res` (0/1) marches at half resolution + depth-aware upsample.
    /// - `reveal` = a soft density threshold the march culls below (like the
    ///   vector-field reveal): chips away the dilute haze crust so the dense
    ///   vortex filaments inside show through. 0 = march everything.
    /// `enabled = 0` → every dye/ink pass is skipped (byte-identical default).
    /// A captured **Look**. (Appended; existing offsets unchanged.)
    pub fluidvis: [f32; 12],
    /// Fluid medium, Tier 2 (#182) — "a medium with substance". Layout:
    /// `[boundaries, buoyancy, heat_decay, detail, splash, dye_gate, res, substeps]`.
    /// - `boundaries` (0/1): voxelize node occupancy into the grid and enforce
    ///   moving no-slip walls in advection + the pressure solve — wakes shed off
    ///   the structure, flow channels through lattices.
    /// - `buoyancy` (signed): vertical force per unit heat; heat is injected
    ///   with the dye and cools at `heat_decay` — smoke rises, ink sinks.
    /// - `detail` (0..1): render-time curl-noise perturbation of the ink march,
    ///   scaled by local |vorticity| (a coarse grid reads finer than it is).
    /// - `splash`: radial momentum impulse on each beat (× the pulse envelope);
    ///   `dye_gate` (0..1) fades dye injection toward beat-gated (1 = ink only
    ///   on the pulse).
    /// - `res`: sim grid override (0 = follow the aura's grid dial, else 8..128
    ///   — an honest perf dial, 128³ is heavy); `substeps` (1..4) splits the
    ///   frame dt for fast stirs (full solver cost per substep).
    /// All zero (substeps 1) → byte-identical Tier-1 behaviour. A captured
    /// **Look**. (Appended; existing offsets unchanged.)
    pub fluid2: [f32; 8],
    /// MLS-MPM liquid (#182 Tier 3a) — a free-surface liquid the generator
    /// churns, rendered through the metaball isosurface path (set Material =
    /// Glass for water; `hide_generator` gives the pure pool). Layout:
    /// `[enabled, count_k, grid_res, gravity, stiffness, viscosity,
    /// container, open_top, collide, stir, density, threshold, hue, sat,
    /// reset_gen, substeps]`.
    /// - `count_k` = particles ×1000 (capped 300k); `grid_res` = sim grid per
    ///   axis (16..96). Both PERF dials (a reseed on change).
    /// - `gravity` (world/s², **default 0** — weightless until dialled up, so
    ///   the pool doesn't immediately drain to the floor), `stiffness` (EOS),
    ///   `viscosity` (0..1 APIC damping) — the feel of the liquid.
    /// - `container` = tank half-extent (world units, centred on the smoothed
    ///   field centre); `open_top` (0/1) skips the ceiling clamp.
    /// - `collide` (0/1): generator nodes become moving no-slip obstacles that
    ///   churn the pool; `stir` scales the velocity they impose.
    /// - `density` scales the splatted iso field; `threshold` is the surface
    ///   iso level; `hue`/`sat` colour the liquid's albedo.
    /// - `reset_gen`: a live counter (the editor's "reset pool" button, NOT a
    ///   param / not preset-captured — stamped in `process()` like `hdr_gen`);
    ///   the visual reseeds the pool when it changes.
    /// - `substeps` (1..4): sim substeps per frame (stability, full cost each).
    /// `enabled = 0` → no sim dispatch, no draw (byte-identical default). A
    /// captured **Look**. (Appended; existing offsets unchanged.)
    pub liquid: [f32; 16],
    /// Liquid follow-ups (#182 T3a, second block — `liquid[16]` is full):
    /// `[offset_y, shape, reveal, _]`.
    /// - `offset_y` shifts the tank vertically off the smoothed field centre
    ///   (world units, ±) — pool the liquid below the generator or float it.
    /// - `shape`: 0 box, 1 sphere (free-slip shell — a curved bowl), 2
    ///   cylinder, 3 boundless (NO hard wall: a soft absorbing shell fades
    ///   outward motion + pulls strays back — the liquid trails off).
    /// - `reveal` 0..1: a soft spherical render window on the density (the
    ///   ink's reveal, spatially) — the isosurface closes into blobby
    ///   trailing edges instead of wall-flattened planes. 0 = off.
    /// A captured **Look**. (Appended.)
    pub liquid2: [f32; 4],
    /// Fluid light coupling (#182 Tier 4 — "one world, one light"):
    /// `[gi, shadow, receive, sway]`, all 0 = off (byte-identical default).
    /// - `gi` (0..2): inject the ink dye's radiance/occupancy and the liquid's
    ///   occupancy into the **VXGI bounce volume** — glowing ink tints the
    ///   bounce light on nearby cubes; the liquid occludes and colours GI.
    /// - `shadow` (0..1): the dye density attenuates the **key light on scene
    ///   geometry** via a light-space transmittance LUT (half a deep shadow
    ///   map) — the smoke casts shadow.
    /// - `receive` (0/1): the ink march samples the **scene shadow map**, so
    ///   geometry shades the smoke.
    /// - `sway` (0..1): two-way coupling — fluid velocity (ink grid or MPM
    ///   liquid) is sampled at the generator's nodes (GPU readback) and drives
    ///   a per-node displacement spring on the drawn instances: the structure
    ///   sways because of the water it stirs (the deferred #99 back-reaction).
    /// A captured **Look**. (Appended.)
    pub fluidgi: [f32; 4],
    /// Liquid caustics (#182 Tier 4): `[amount, sharpness, _, _]`.
    /// Key-light rays refract through the liquid's isosurface (field-gradient
    /// normals) and splat a light-space caustic texture projected onto
    /// geometry beneath — `amount` 0 = off (byte-identical), `sharpness`
    /// focuses the pattern (default 1). A captured **Look**. (Appended.)
    pub caustic: [f32; 4],
    /// Liquid material + ghost light (#182 T4 follow-up):
    /// `[material, metallic, roughness, ior, ghost, _, _, _]`.
    /// - `material`: 0 = **use the scene material** (byte-identical default),
    ///   1 = Standard, 2 = Chrome, 3 = Glass — a SEPARATE material for the
    ///   liquid, overriding the scene's type/metallic/roughness/IOR on the
    ///   liquid draw only (the shared fine dials — chrome purity, glass
    ///   clarity, dispersion… — apply on top and now work on the metaball path).
    /// - `metallic`/`roughness`/`ior`: the liquid's own values (used when
    ///   `material` ≠ 0). Water ≈ [3, 0, 0.05, 1.33].
    /// - `ghost` (0/1): a hidden generator keeps LIGHTING — probe GI, VXGI
    ///   injection and the emissive-cube point lights stay live under
    ///   `hide_generator`, so the invisible structure becomes a pure
    ///   GI/light emitter for the fluid.
    /// `[5]` = render mode (0 = isosurface, 1 = **refractive** — the #182 T3b
    /// see-through pass: Snell refraction of the resolved scene, measured
    /// thickness, Beer–Lambert absorption, Fresnel split), `[6]` = absorption
    /// strength, `[7]` = the liquid's own glow.
    /// A captured **Look**. (Appended.)
    pub liqmat: [f32; 8],
    /// Liquid material, second block (#182 T4 follow-up — the FULL material
    /// set, mirroring the scene's fine dials; used when `liqmat[0]` ≠ 0):
    /// `[chrome_purity, glass_clarity, f0_override, dispersion, glass_caustic,
    /// thin_film, _, _]`. A captured **Look**. (Appended.)
    pub liqmat2: [f32; 8],
    /// Z0NE rails generator (#187, Tier 1) — the beat-parametrized infinite
    /// corridor. Layout: `[speed, bore, cell_len, _reserved(change_every,
    /// Tier 3), variance, seed, ring_n, rows_per_beat, horizon, rib_gain,
    /// thickness, lobes_max, spike, twist, swell, fade, color_flow,
    /// archetype, diverge, shells, parastichy, …reserved]`. `speed` = world
    /// units per beat (the rail coordinate is the visual's PLL beat clock, so
    /// cell/beat boundaries are crossed exactly on the beat at any speed);
    /// `cell_len` is the `RailCellLen` ordinal (1/2/4/8/16 beats per morph
    /// cell); `horizon` = beats of corridor ahead (the perf dial); `rib_gain`
    /// scales the integer-beat rib rows that flash with the beat clock.
    /// **Tier 2 (slots 17–20):** `archetype` (`RailArchetype` ordinal — 0
    /// Throat / 1 Phyllo Wall / 2 Rings & Gates / 3 Tissue Tube; **Tier 4
    /// appends** 4 Tiling Liner / 5 Flow Media / 6 Waveguide), `diverge`
    /// (phyllo divergence °), `shells` (tissue), `parastichy` (phyllo).
    /// **Tier 3:** slot [3] = `change_every` (`RailChangeEvery` ordinal →
    /// 4/8/16/32/64 beats — the quantized-transition boundary lattice + the
    /// evolve period) and slot [21] = `evolve` (0..1 per-phrase re-roll).
    /// Decoded by `math::RailsSpec::from_slots`. (Appended; existing offsets
    /// unchanged.)
    pub rails: [f32; 24],
    /// Hardware ray tracing (#195): `[enable, debug_view, shadows, shadow_soft,
    /// shadow_strength, shadow_fill, _, _]`.
    /// - `enable` (0/1, Tier 0): build the BLAS/TLAS over the instanced field
    ///   each frame (the foundation every RT effect traces against; cost in
    ///   `Feedback.tlas_ms`). A captured **Look**, silently inert on machines
    ///   without ray-query support.
    /// - `debug_view`: 0 = Off, 1 = Normals, 2 = Instance index, 3 = Hit distance —
    ///   a fullscreen ray-query visualization drawn OVER the final frame to verify
    ///   the TLAS matches the raster scene. **Per-display** (not preset-captured),
    ///   like HDR/MSAA.
    /// - `shadows` (0/1, Tier 1): one traced ray per pixel toward the key light
    ///   supersedes the PCF shadow map (implies the TLAS build); `shadow_soft` =
    ///   the light's angular size (TAA resolves the jittered penumbra),
    ///   `shadow_strength` = the darkening mix, `shadow_fill` (0/1) = a second
    ///   ray shadows the fill light too. All captured **Looks**. (Appended.)
    pub rt: [f32; 8],
    /// Refractive generator optics: `[absorption, overlay, blend, _]`.
    /// `absorption` (`mat_type` = 3, and the overlay's murk): the liquid's
    /// see-through optics brought to the node materials — the cube shader
    /// Beer–Lambert-attenuates the glass transmission over the measured chord
    /// through each instance along the refracted ray, with the node's own
    /// albedo as the surviving colour (mirrors `liq_absorb`). `overlay` (0/1)
    /// + `blend` (0..1): the refraction-overlay checkbox — the same refracted
    /// transmission woven INTO Standard/Chrome/Glass on top of their own
    /// shading (Standard's body goes glassy, Chrome opens face-on by Fresnel,
    /// Glass gains the murk); both inert at overlay 0. Captured **Looks**.
    /// (Appended after #195's `rt[8]` on re-merge; existing offsets unchanged.)
    pub refrmat: [f32; 4],
    /// Hardware-RT reflections (#195 Tier 2): `[enable, intensity,
    /// max_roughness, reach, hit_shadows, _, _, _]`.
    /// - `enable` (0/1): trace `reflect(v, n)` per pixel against the TLAS into
    ///   the SSR/reflection buffer — cubes reflect the ACTUAL scene (neighbours,
    ///   off-screen emitters), no screen-edge dropout. Supersedes the SSR march
    ///   while on; a miss falls back to the env reflection with no seam.
    ///   Implies the TLAS build.
    /// - `intensity`: the confidence-weight scale (SSR's dial).
    /// - `max_roughness`: cutoff above which the env/IBL reflection stands.
    /// - `reach`: ray length as a multiple of the scene diagonal.
    /// - `hit_shadows` (0/1): trace a key-light shadow ray at each reflection
    ///   hit — reflections contain shadows.
    /// #195 Tier 3 rides the spare slots:
    /// - `[5]` = AO source (0 = GTAO screen-space, 1 = ray-traced): under the
    ///   Ambient Occlusion enable/radius dials, RT fills the SAME raw-AO
    ///   target with short cosine-weighted hemisphere rays (no haloing,
    ///   off-screen occluders count); blur/composite/spec-occlusion unchanged.
    ///   Implies the TLAS build; falls back to GTAO without ray-query support.
    /// - `[6]` = RT AO rays per pixel (1–16; TAA integrates).
    /// - `[7]` = RT **reflection** rays per pixel (1–16; 1 = the original
    ///   single-ray look). Each RT effect now has its own ray budget:
    ///   reflections here, AO in `[6]`, GI in `rt3[2]`.
    /// All captured **Looks**. (Appended AFTER the #201 `refrmat[4]` on
    /// re-merge; existing offsets unchanged.)
    pub rt2: [f32; 8],
    /// The Scenery layer (#187 pivot) — the concurrent generated-scenery
    /// category with its OWN material/surface, independent of the primary
    /// generator. Layout: `[mode, surface, mat_type, metallic, roughness,
    /// glow, opacity, ior, palette, sss, sss_dist, sss_pow, irid, irid_scale,
    /// irid_shift, _reserved]`. `mode` = `SceneryMode` ordinal (0 None /
    /// 1 Zone — the corridor; the geometry itself still reads `rails[24]`);
    /// `surface` = `ScenerySurface` (0 cubes / 1 flow-aligned rods / 2 swept
    /// tubes); the material/FX slots patch a second `Uniforms` for the
    /// scenery draw. (Appended after #195 T2's `rt2[8]` on the main
    /// re-merge; existing offsets unchanged.)
    pub scenery: [f32; 16],
    /// Hardware-RT diffuse global illumination (#195 Tier 4, Option B):
    /// `[enable, intensity, rays, reach, hit_shadows, _, _, _]`.
    /// - `enable` (0/1): gather one indirect bounce per pixel against the TLAS
    ///   into the SSGI buffer — real inter-cube colour bleed incl. off-screen
    ///   emitters (SSGI only sees on-screen neighbours). Supersedes the SSGI
    ///   march while on; a miss leaves the scene's own IBL ambient (no seam).
    ///   Implies the TLAS build.
    /// - `intensity`: the gathered-radiance scale.
    /// - `rays`: cosine-hemisphere rays per pixel (1–4; TAA integrates).
    /// - `reach`: gather ray length as a multiple of the scene diagonal.
    /// - `hit_shadows` (0/1): trace a key-light shadow ray at each hit, so the
    ///   bounced light is itself shadowed.
    /// #200 Tier 4½ part 2 rides the spare slots:
    /// - `[5]` = RT denoise (0/1): edge-aware à-trous over the RT reflection +
    ///   GI buffers (in place, before the composite reads them) — cleans the
    ///   1–4-spp grain without crossing depth/highlight edges; reflections
    ///   roughness-adaptive. Off = raw jitter.
    /// - `[6]` = denoise amount (0..1, blend toward the filtered result).
    /// All captured **Looks**. (Appended.)
    pub rt3: [f32; 8],
    /// RT temporal accumulator (#200 Tier 4½ parts 3 + 4): `[enable, feedback,
    /// beat_relax, variance_on, max_accum, clamp_gamma, _, _]`.
    /// - `enable` (0/1): reproject + accumulate the RT reflection + GI buffers
    ///   across frames (camera reprojection + neighborhood clamp) — the
    ///   temporal half of the denoiser. The RT pass writes a raw buffer and the
    ///   accumulator writes the SSR/SSGI view. Off = raw jitter.
    /// - `feedback`: history weight (0..0.98; higher = smoother, more lag). In
    ///   the variance path this is the adaptive ceiling.
    /// - `beat_relax`: how much a PLL beat kick drops the history weight (the
    ///   visual multiplies the live beat pulse in), so history relaxes on the
    ///   beat instead of smearing across the kick.
    /// - `variance_on` (0/1, part 4): swap the fixed feedback + raw box clamp for
    ///   history-length-adaptive blending + a luminance σ-clamp (true SVGF).
    /// - `max_accum`: max accumulated-sample count the adaptive blend ramps to.
    /// - `clamp_gamma`: σ-clamp width γ (history luma clamped to μ ± γσ).
    /// All captured **Looks**. (Appended.)
    pub rt4: [f32; 8],
    /// Neural shading foundation (#200 Tier 0): `[enable, seed_a, seed_b, walk,
    /// omega, _, _, _]`. The compact control surface for the tiny MLP
    /// (`math.rs::mlp_eval` / `mlp.wgsl`) — a whole network travels as two
    /// integer seeds (stored as f32) + a walk `t` (latent morph A→B) + `omega`
    /// (SIREN feature scale). **Ships dark** (`enable = 0`, nothing samples it
    /// yet — Tier 1's neural-field generator is the first consumer). Captured
    /// **Looks**. (Appended.)
    pub neural: [f32; 8],
    /// Anisotropy (#214 Tier 1): `[amount, rotation_rad, overlay_enable,
    /// overlay_blend]`. The elliptical-GGX streak for the `Anisotropic` material
    /// (`lighting[7]` = 4) and the overlay on Standard/Chrome. `amount` 0 →
    /// isotropic (byte-identical). A captured **Look**. (Appended.)
    pub aniso: [f32; 4],
    /// Axon Waveguide generator (#218 Tiers 1–4; used when `generator` = 18):
    /// `[count, length, bundle_radius, samples, thickness, node_spacing, node_dip,
    /// pulse_speed, pulse_width, stagger, splay, seed, mode, mode_amount, bend,
    /// curve, tortuosity, dti, dispersion, polarization, _, _, _, _]`. A bundle of
    /// myelinated-axon fibres (Vogel-disc-
    /// packed within `bundle_radius`, along +Y) drawn as swept tubes: periodic
    /// Ranvier-node constrictions (`node_spacing`/`node_dip`) + a travelling emissive
    /// "action potential" pulse (`pulse_speed`/`pulse_width`, per-fibre `stagger`)
    /// that rides the global clock; `splay` fans the bundle. **Tier 2:** `mode`
    /// (`AxonMode` ordinal — LP01/LP11/LP21/LP02/LP31/LP12) lights the bundle
    /// cross-section with the LP guided-mode intensity, blended by `mode_amount`
    /// (0 = uniform). Streamlines topology (one tube per fibre) — view in Swept
    /// Tubes + Glass/Refractive for the waveguide look. **Tier 3 (slot 14):**
    /// `bend` scatters the edge-riding guided modes — they leak along the fibre
    /// and flare at the Ranvier nodes — while the LP01 core survives (drives the
    /// optics only; 0 = coherent). **Brain tract (slots 15–17):** `curve` bends
    /// the straight bundle into a broad C-shaped white-matter arc (corpus-callosum
    /// / fasciculus sweep), `tortuosity` adds per-fibre undulation (real axons
    /// aren't parallel), and `dti` cross-fades the colour to the diffusion-MRI
    /// tractography look (fibre direction → RGB). **Tier 4 (slots 18–19):**
    /// `dispersion` chirps the travelling pulse into a chromatic spread (warm
    /// trailing, cool leading edge), and `polarization` adds a coherence shimmer —
    /// clean on the surviving core, scrambled to noise on the leaking fibres. Both
    /// 0 = the Tier-3 look. Slots 20–23 reserved. (Appended after #200's `neural[8]`
    /// + #214's `aniso[4]` on the main re-merge; existing offsets unchanged.)
    pub axon: [f32; 24],
    /// Surface lobes (#214 Tier 2): `[clearcoat, clearcoat_rough, clearcoat_overlay,
    /// sheen_overlay, sheen, sheen_rough, sheen_tint, _]`. The `Clearcoat` (5) /
    /// `Velvet` (6) materials + the Standard/Chrome overlays. All lobes off →
    /// byte-identical. A captured **Look**. (Appended after #218's `axon[24]` on the
    /// main re-merge.)
    pub coat: [f32; 8],
    /// Terra scenery landform (#206 Tier 2): `[form, ridge, channel, width,
    /// steep, terrace, rough, meander, water_level, water_on, clearance,
    /// noise_freq, _, _, _, _]`. Active when `scenery[0]` = 2 (SceneryMode::
    /// Terra). Timing/window params come from the shared rails block (speed,
    /// cell_len, change_every, variance, seed, evolve, horizon, rows/beat,
    /// fade, bore-as-scale); this block is the landform shape and quantizes on
    /// the bar via the same latch. Decoded by `math::TerraSpec::from_slots`.
    /// (Appended after #214's `coat[8]` on the main re-merge.)
    pub terra: [f32; 16],
    /// Neural field generator (#200 Tier 1, used when `generator` = 19): the
    /// raymarched-isosurface controls. `[world_scale, coord_scale, iso, steps,
    /// march_relax, color_intensity, walk_rate, _]`. The network identity + walk
    /// + omega ride the `neural[8]` block above. All captured **Looks**.
    /// (Appended — re-seated to the true tail after `terra` on the main re-merge.)
    pub neural2: [f32; 8],
    /// Neural field — **strand form** (#200 Tier 1b): `[strands_mode(0/1), strands,
    /// nodes, extent, displace, _, _, _]`. When `strands_mode` ≠ 0 the neural
    /// generator samples the MLP on a `strands × nodes` grid and DISPLACES the
    /// nodes (Grid topology → every Surface mode + Material + membrane apply)
    /// instead of raymarching. Captured **Looks**. (Appended.)
    pub neural3: [f32; 8],
    /// Body optics (#214 Tier 3): `[sss_thickness, sss_radius, interior_scatter, _]`.
    /// Drives real-thickness translucency (Beer–Lambert over the measured chord) +
    /// the Glass/Refractive interior in-scatter glow + the `Subsurface` material.
    /// All 0 → byte-identical. A captured **Look**. (Re-seated to the true tail
    /// after `neural3` on the main re-merge; existing offsets unchanged.)
    pub body: [f32; 4],
    /// Microstructure (#214 Tier 4): `[glitter, glitter_density, glitter_sharpness,
    /// diffraction, diffraction_freq, retro, _, _]`. Sparse per-facet glitter +
    /// grating-rainbow diffraction (holo-foil over Chrome) + retroreflection glow,
    /// woven into Standard/Chrome and resolved by TAA/blue noise. All amounts 0 →
    /// byte-identical. A captured **Look**. (Re-seated to the true tail after `body`
    /// on the main re-merge.)
    pub micro: [f32; 8],
    /// Terra water surface (#206 Tier 3): `[mat_type, roughness, ior, opacity,
    /// glow, ripple_amp, ripple_freq, _]`. The channel water floor's OWN
    /// material (a third scenery uniform set) + ripple. A **Look** applied
    /// instantly (the water LEVEL lives in the latched `terra` block, so it
    /// quantizes with the landform; the material does not). Active only in
    /// SceneryMode::Terra with `terra[9]` (water_on) set. (Re-seated to the true tail after #214's
    /// `micro[8]` on the main re-merge.)
    pub water: [f32; 8],
    /// Terra water PHYSICS (#206 dedicated water material): `[absorb, glitter,
    /// reflect, _, _, _, _, _]`. The physically-real shading dials the cube
    /// shader applies to the water surface (flagged via `sss.w = 3`) so it reads
    /// as water regardless of the tunnel material: `absorb` = Beer–Lambert depth
    /// darkening toward the deep colour, `glitter` = sun-sparkle on the ripples,
    /// `reflect` = extra grazing reflectivity on top of the IOR Fresnel. A
    /// **Look**. (Appended.)
    pub water2: [f32; 8],
    /// Path tracer (#200 Tier 4) editor toggle — a **per-display** flag (like
    /// `hdr_output`, NOT preset-captured). The editor's Ray-Tracing-card checkbox
    /// writes it; the visual edge-detects it into its local `pathtrace_on` (the
    /// **P** key toggles the same local state — last-touched-wins). 0 = off.
    /// (Appended scalar — re-seated to the tail after `water2` on the main re-merge.)
    pub pathtrace_on: u32,
    /// Neural denoiser (#200 Tier 5a): the kernel-predicting RT denoiser controls.
    /// `[enable, net_strength, seed, omega, _, _, _, _]`. When `enable`, the RT
    /// reflection / GI denoise step routes through a seeded-MLP-modulated
    /// bilateral instead of the classical à-trous; `net_strength = 0` reproduces
    /// the classical filter exactly. Off = classical (Tier 4½). Captured **Looks**.
    /// (Appended — re-seated to the true tail after `pathtrace_on` on the main re-merge.)
    pub ndenoise: [f32; 8],
    /// Spectral emission (#214 Tier 5 pt 1): `[fluorescence, fluor_hue,
    /// incandescence, temperature_K]`. Fluorescence re-emits the env's absorbed
    /// short-wavelength light at a hue; incandescence adds a blackbody glow by
    /// temperature. Both woven into every material's `emissive`; amounts 0 →
    /// byte-identical. A captured **Look**. (Re-seated to the true tail after
    /// `ndenoise` on the main re-merge.)
    pub emit: [f32; 4],
    /// Screen-space refraction (#214 Tier 5 pt 2): `[strength, displace, _, _]`.
    /// A post pass so the Refractive material shows the displaced RESOLVED SCENE
    /// behind it (neighbours / world), not just the env. `strength` 0 → the pass
    /// isn't dispatched (byte-identical). A captured **Look**. (Re-seated to the
    /// true tail after `emit` on the main re-merge.)
    pub ssrefr: [f32; 4],
    /// Learned upscaler (#200 Tier 5c): the composite's DRS upscale controls.
    /// `[enable, sharpen, seed, _, _, _, _, _]`. When `enable` AND the composite is
    /// upscaling (`render_scale < 1`), the plain bilinear scene fetch becomes an
    /// HDR-safe content-adaptive sharpen reconstruction whose per-pixel gain rides a
    /// Tier-0 seeded MLP. `enable = 0` (or full render scale) = bilinear, byte-
    /// identical. Captured **Looks**. (Appended — re-seated to the true tail after
    /// `ssrefr` on the main re-merge.)
    pub upscale: [f32; 8],
    /// ReSTIR many-lights (#200 Tier 5d): `[enable, _, _, _]`. When `enable`, the
    /// emissive-cube light set (#167 T3) is chosen by weighted reservoir sampling
    /// (every cube gets a luminance-proportional chance over time) instead of a hard
    /// brightest-`count` cap. `enable = 0` = brightest-N, byte-identical. Captured
    /// **Look**. (Appended — re-seated to the true tail after `upscale` on the main re-merge.)
    pub restir: [f32; 4],
    /// Neural Network generator (#226 Tier 1, used when `generator` = 20): the
    /// graph + geometry controls. `[topology, nodes, connectivity, rewire_or_radius,
    /// layers, seed, extent, node_size, node_glow, edge_thickness, edge_bow,
    /// edge_samples, pulse_speed, pulse_width, _, _]`. `topology`: 0 random-geometric,
    /// 1 layered feed-forward, 2 ring lattice, 3 Watts–Strogatz small-world. Decoded
    /// by `math::neural_net_strands` / `neural_graph`. Slots 14–15 reserved for Tier 2
    /// (signal propagation). All captured **Looks**. (Re-seated to the true tail after
    /// `restir` on the main re-merge.)
    pub neural_net: [f32; 16],
    /// Neural Network edges/somas (#226 Tier 1.5): `[edge_fibres, bundle_radius,
    /// node_dip, ranvier_nodes, dendrite, dendrite_count, _, _]`. `edge_fibres > 1`
    /// renders each graph edge as a myelinated fibre BUNDLE (the #218 axon tract at
    /// edge scale — Vogel-packed fibres, Ranvier-node constrictions, staggered pulse)
    /// instead of one tube; `dendrite > 0` sprouts an arbor from each soma. Both inert
    /// at the defaults (fibres 1, dendrite 0) → the Tier-1 geometry. Captured **Looks**.
    /// (Appended after `neural_net`.)
    pub neural_edge: [f32; 8],
    /// Maxwell field energization (#247): light the Particle Aura by the real EM
    /// **energy density** `½(|E|²+|B|²)` of the dipole/charge field — the fluorescent-
    /// tube demo. `[energize(0/1), gain, knee, hue, antenna_len(T2), antenna(0/1)(T2),
    /// dye_inject(T3), aura_blend(E↔B, 0..1)]`. Slot 7's aura E↔B blend is INDEPENDENT
    /// of the generator's `maxwell[1]` blend: it drives both the aura's traced field
    /// direction AND the energy density the glow reads (`(1−t)|E|²+t|B|²`), so the
    /// glow follows the selected field. When `energize` and the generator is Maxwell + Particle
    /// Aura Lite, the velocity grid carries the field energy in its `w` channel; each
    /// mote still advects along the (normalized) field DIRECTION but glows by the local
    /// energy MAGNITUDE, log/soft-knee tone-mapped for the 1/r⁶ near-field range —
    /// bright in the strong zones, dark in the nulls. In the **Aura-Fluid** tier the
    /// energy instead comes from the NS solver's own `cs_energy` pass (½|u|²+½|ω|²).
    /// `gain` scales brightness, `knee` is the soft HDR ceiling, `hue` the base ember
    /// colour. Slots 4–6 are reserved for Tier 2 (finite-antenna standing wave) + Tier 3.
    /// `energize` 0 → inert (byte-identical). A captured **Look**. (Re-seated to the
    /// true tail after `neural_edge` on the main re-merge.)
    pub maxenergy: [f32; 8],
    /// Neural Network signal propagation (#226 Tier 2): the activation-cascade sim
    /// dials. `[fire_mode, threshold, conduction_speed, refractory, decay, deposit,
    /// stim_rate, motes]`. `fire_mode`: 0 off (Tier-1 free-running pulse), 1
    /// wavefront, 2 oscillation, 3 stimulus. Stepped by `math::NeuralSim` on the
    /// beat clock; off → byte-identical to Tier 1. Captured **Looks**. (Re-seated to
    /// the true tail after `maxenergy` on the main re-merge.)
    pub neural_net2: [f32; 8],
    /// Neural Network connectome load counter (#226 Tier 3). Bumped by the editor's
    /// "Load Connectome…" button after it writes the JSON path to the connectome
    /// sidecar; the visual edge-detects it and re-reads + ingests the file (a live
    /// runtime counter — NOT a param / not preset-captured — stamped in `process()`
    /// like `hdr_gen`). (Appended after `neural_net2`.)
    pub nn_gen: u32,
    /// Neural Network MLP look (#226 Tier 4): `[sign_colour, sparsify, layer_gap,
    /// input_drive, _, _, _, _]`. Used when `neural_net[0]` topology = MLP (6): a loaded
    /// trained network laid out layer-by-layer, edges = signed weights (sign→colour,
    /// |w|→thickness), nodes lit by a live forward pass. Captured **Looks**. (Appended
    /// after `nn_gen`.)
    pub neural_mlp: [f32; 8],
    /// Neural Network attention look (#226 Tier 5): `[layer, head, threshold, tokens,
    /// reveal_rate, sweep_rate, ring, _]`. Used when `neural_net[0]` topology =
    /// Attention (7): a transformer's self-attention tensor (a real forward pass from
    /// the JSON sidecar, or a stylized causal synthesis) rendered as a triangular
    /// attention graph — tokens are nodes, causal attention edges carry A_ij, nodes lit
    /// by incoming attention. `reveal_rate` grows the attended token set over beats
    /// (token-by-token generation); `sweep_rate` cycles the head/layer. Captured
    /// **Looks**. (Appended after `neural_mlp`.)
    pub neural_attn: [f32; 8],
    /// Contiguous (welded) Swept-Tubes look: `[weld(0/1), end_cap(0/1), cap_round,
    /// cap_bevel]`. Only used when `surface_mode` = 2 (Swept Tubes). `weld` swaps the
    /// per-segment open cylinders for one smooth swept mesh per strand; `end_cap`
    /// closes the strand ends, shaped by `cap_round` (0 flat disc → 1 dome) and
    /// `cap_bevel` (0 rounded → 1 chamfer). (Appended after `neural_attn`.)
    pub tube: [f32; 4],
    /// Neural Tissue surface (#260 Tiers 1–4): the "living neural tissue" surface +
    /// closed anatomical primitives (soma cell bodies, capped capsule edges,
    /// synaptic boutons) + grown neuron morphology (dendritic arbors + axon). Slot
    /// map — Tier 1 fills `[0..5]`, Tier 2 fills `[5..10]`, Tier 3 fills `[10..13]`,
    /// Tier 4 (final) fills `[13..16]` (never re-order these):
    /// `[soma_size, soma_shape, bouton_size, membrane_sss, membrane_irid,
    /// dendrite_density(T2), dendrite_length(T2), dendrite_taper(T2),
    /// neuron_type(T2), spines(T2),
    /// myelin_amount(T3), ranvier_spacing(T3), sheath_scale(T3),
    /// synapse_cleft(T4), synapse_glow(T4), synapse_vesicles(T4)]`.
    /// `soma_shape` is an anisotropy hint (a teardrop/pyramidal silhouette);
    /// `membrane_sss`/`membrane_irid` drive the waxy translucent membrane look via
    /// the shared Surface-FX SSS/iridescence path (0 = inert). Tier 2: each soma
    /// grows a deterministic bifurcating dendritic tree (radius tapering to the
    /// tips) + a hillock axon ending in terminal boutons; `dendrite_density = 0`
    /// keeps it inert (bare soma, byte-identical to Tier 1). Tier 3: each edge
    /// becomes a **myelinated axon** — fatty internode-sheath capsules separated by
    /// thin Ranvier-node constrictions, thick tracts fanned into fibre bundles, the
    /// action potential conducting **saltatorily** (the bright internode jumps
    /// node-to-node with the Tier-2 pulse); `myelin_amount = 0` keeps plain capsule
    /// edges (byte-identical to Tiers 1/2). **Tier 4 (the final tier): the living
    /// synapse.** `synapse_cleft` (0 = inert) pulls each terminal bouton back off
    /// the post-synaptic membrane so a visible **synaptic cleft** gap opens;
    /// `synapse_vesicles` (0 = off) emits a deterministic **neurotransmitter vesicle
    /// burst** — a few tiny short-lived instances crossing the cleft — on each spike
    /// arrival (the Tier-2/3 cascade deposit event, `edge_pulse ≥ 0.82`), a pure
    /// function of sim state (no per-frame flicker); `synapse_glow` (0 = inert)
    /// lights each soma's **cytoplasmic interior** from within, scaled by its live
    /// activation (the finalized neural material's activation-tied glow, on top of
    /// the `membrane_sss`/`membrane_irid` waxy-membrane path). Captured **Looks**.
    /// (Appended after `tube`.)
    pub neural_surface: [f32; 16],
    /// Neural Tissue tissue-context (#260 Tier 4, the final tier — the network sits
    /// in tissue, not a void). Tail block appended after `neural_surface` (its
    /// `[13..16]` filled by Tier 4's synapse dials, so the context knobs spill here).
    /// `[glia, capillary, _, _, _, _, _, _]` — all 0 = off (byte-identical default):
    /// `glia` sprouts faint sparse **astrocyte scaffolding** (short branching stubs
    /// off a seeded subset of somata, count scaling with the dial); `capillary`
    /// routes a few dim wandering **capillary threads** across the tissue volume.
    /// Both are emitted into the existing CAPSULE sub-batch (no new pipeline);
    /// deterministic (seeded by node/thread index, never time). Slots `[2..8]`
    /// reserved (extracellular-medium fog + wet-fresnel membrane rim are shader-side
    /// follow-ups). Captured **Looks**. (Tail-appended after `neural_surface`.)
    pub neural_surface2: [f32; 8],
    /// Brain model (#275): dials for the `NeuralTopology::Brain` layout — two mirrored
    /// cerebral hemispheres of folded cortex split by a longitudinal fissure, a
    /// cerebellum + brainstem, wired short-range local cortex (Tiers 2–4 fill the
    /// reserved tail with tracts / corpus callosum / parcellation / focal stimulation).
    /// `[fold_depth, fold_freq, hemi_gap, local_k, cerebellum,  | assoc_tracts (T2),
    /// callosum (T2), subcortical (T2),  | region_labels (T3), target_region (T3/T4),
    /// stim_amount (T4), stim_radius (T4), stim_rate (T4),  | signal_swell, reserved×3]`.
    /// Tier 1 uses `[0..5]`; the rest fill with Tiers 2–4. Only active when NN topology =
    /// Brain model. Captured **Looks**. (Tail-appended after `neural_surface2`.)
    pub brain: [f32; 16],
    /// Physical thin-film interference (#258 Tier 1): a real soap-film / bubble
    /// iridescence model layered onto the Glass material and the Foam/Bubble
    /// raymarch. `[film_thickness, film_thickness_var, film_ior, film_drainage]`.
    /// `film_thickness` = base film thickness in nanometres (0 → the model is
    /// disabled and the shader falls through to the existing cosine-hack
    /// `thin_film_tint` / iridescence path, so the default look is byte-identical);
    /// `film_thickness_var` = noise-marbling amount on the thickness; `film_ior` =
    /// the film's refractive index; `film_drainage` = gravity-drainage gradient
    /// (thin at the top → thick at the bottom, along world-space up). Evaluated as a
    /// wavelength-resolved Airy interference summation → RGB reflectance. Captured
    /// **Looks**. (Appended after `neural_mlp`.)
    pub thinfilm: [f32; 4],
    /// Path-tracer dielectric BTDF (#258 Tier 2): `[pt_dielectric_enable, absorption,
    /// _, _]`. When `pt_dielectric_enable > 0.5` the hardware-RT path tracer grows
    /// from diffuse-only to a two-interface dielectric (Fresnel reflect/transmit
    /// split, refract on entry AND exit, TIR) for the Glass/Refractive materials and
    /// a perfect mirror for Chrome; `absorption` sets the Beer–Lambert σ scale for
    /// rays travelling INSIDE the medium (σ = (1 − albedo) × absorption). Both 0 →
    /// the tracer stays diffuse-only (byte-identical). Captured **Looks**. (Appended
    /// after `neural_mlp`.)
    pub ptglass: [f32; 4],
    /// Welded-tube cross-section shape: 1 = circle (original), 0 = sharp square
    /// (flat faces + hard edges → welded flow-aligned cubes), between = rounded
    /// square whose corner-bevel radius = this value. Only used when
    /// `surface_mode` = 2 (Swept Tubes) with Contiguous on. (Tail-appended after
    /// `ptglass`.)
    pub tube_profile: f32,
    /// Lens generator (#258 Tier 3): analytic double-convex / plano-convex lens
    /// body, raymarched as an SDF (intersection of two spheres, or one sphere with
    /// a half-space), shaded through the shared Glass/PBR path so the Tier-2
    /// dielectric tracer makes it focus. Layout `[focal, aperture, thickness,
    /// plano, scale, steps, _, _]`: `focal` = focal-length/curvature dial (sphere
    /// radii derived lensmaker-style), `aperture` = clear-aperture radius (fraction
    /// of scale), `thickness` = centre half-thickness (fraction of scale), `plano` =
    /// 0 biconvex / 1 plano-convex, `scale` = world size, `steps` = sphere-trace
    /// budget. Inert appended tail (default generator unchanged). Captured **Looks**.
    pub lens: [f32; 8],
    /// Spectral light transport (#258 Tier 4): `[spectral_on, abbe, secondaries, _]`.
    /// `spectral_on` 0 → the RGB path tracer (byte-identical); >0 → the hero-wavelength
    /// spectral integrator (glass/lens disperse at a per-λ Cauchy IOR set by the Abbe
    /// number). `secondaries` = extra stratified wavelengths per pixel. Path-tracer only.
    /// (Tail-appended after `lens`.)
    pub spectral: [f32; 4],
    /// Demo scene bench (#288): the hand-authored reference-scene dials, live only
    /// when `GeneratorMode::Demo` is selected (byte-identical otherwise — all-zero
    /// scene id = Cornell box, but the generator gate means the block is inert until
    /// Demo is chosen). `[scene, scale, inner_objects, static_cam, light, roughness,
    /// count, spin]`: `scene` = the `DemoScene` discriminant; `scale` = overall scene
    /// scale; `inner_objects` (0/1) = draw the hero objects inside the box; `static_cam`
    /// (0/1) = hold the fixed reference framing (the visual gates the auto-orbit off);
    /// `light` = emitter / key intensity; `roughness` = the scene's smooth-material
    /// roughness; `count` = pyramid rows / grid side / light count; `spin` = turntable
    /// rotation on the beat clock. The scene emits **explicit instanced geometry** with
    /// per-primitive mesh + material sub-batches (see `math::demo_scene`); it inherits
    /// the whole PBR/IBL/shadow/TLAS/path-trace stack for free. Captured **Generator**.
    /// (Tail-appended after `spectral`.)
    pub demo: [f32; 8],
    /// Audio-driven dipole radiation (#248): the live music modulates the Maxwell
    /// generator's source and the field's radiated energy is rendered via the #247
    /// energization stack. Tier 1 fills `[0..3]` = `[drive_on, amount, floor]`:
    /// with `drive_on` the broadband RMS envelope (`audio[5]`) scales the dipole's
    /// **drive amplitude** — E and B scale linearly, so the energy cloud `½(|E|²+|B|²)`
    /// breathes **quadratically** with the music's dynamics. `amount` = RMS → drive
    /// gain, `floor` = the idle drive on silence (0 = the field goes dark between
    /// notes). Honest mapping, declared: audio drives the SOURCE's parameters; the
    /// field math stays the real retarded dipole radiation — we never render the
    /// 20 Hz–20 kHz carrier itself. Tier 2 fills `[3..6]` = `[multipole, spread,
    /// band_hue]`: with `multipole` on the five FFT band envelopes drive **distinct
    /// multipole moments** (band b → an order-b binomial dipole array — the
    /// multipole expansion is the spherical-harmonic series, so the spectrum
    /// becomes the field's spatial mode structure); `spread` compresses the honest
    /// per-band wavelength ratio; `band_hue` = colour-by-band blend for the energy
    /// dye + band geometry. `[6..8]` reserved for Tier 3 (stereo/pitch/beat). Off
    /// by default → byte-identical. Captured **Motion** (it's an audio-reactivity
    /// coupling, like the pulse routing). (Tail-appended after `demo`.)
    pub audiodip: [f32; 8],
    /// Field-force particle drive (#248): `[force_on, force_gain, energy_contrast,
    /// stir_rate]`. With `force_on`, the Maxwell energization stirs the medium by the
    /// field instead of sliding along field lines at constant speed: **Lite** by the E
    /// **force** (`AnalyticField::force` — charges pushed, strong near the core,
    /// reversing with the oscillation); **Fluid** by the solenoidal **azimuthal
    /// circulation** (`AnalyticField::stir` — the dipole's B swirl the incompressible
    /// projection keeps) reversed by a slow, watchable `stir_rate` (Hz) so the fluid
    /// visibly sloshes back and forth (it low-passes the raw field clock into a steady
    /// flow otherwise). `force_gain` scales it; `energy_contrast` (>1, applied at display
    /// time) sharpens the near-core glow. Off / gain 1 / contrast 1 → byte-identical.
    /// Captured **Look**. (Tail-appended after `audiodip`.)
    pub mxforce: [f32; 4],
    /// Acoustic pump + beat coupling for the field-force drive (#248): `[pump_amount,
    /// beat_spin_force, pump_scale, swirl_slowdown]`. The **beat** (PLL pulse envelope,
    /// needs Pulse on) drives two audio-reactive motions on top of the force drive:
    /// `pump_amount` is a longitudinal **axial pump** (`AnalyticField::pump` — the dipole
    /// expands in/out along its axis, a speaker pushing air, a punchy per-beat velocity
    /// impulse), and `beat_spin_force` makes the swirl a **turbine** — each beat kicks its
    /// angular momentum in ONE direction (via the `swirl_spin` integrator in the visual),
    /// which coasts down at `swirl_slowdown` (1/s) between beats; 0 = the manual
    /// `stir_rate` reversal. `pump_scale` = the pump's core size. Captured **Look**.
    /// (Tail-appended after `mxforce`.)
    pub mxforce2: [f32; 4],
    /// **Beat mode** crossfade for the field-force drive (#248): `[mode_mix, ring_freq,
    /// _, _]`. `mode_mix ∈ [−1,+1]` blends two beat engines (both run every frame; the
    /// visual crossfades their outputs): **−1** = the **turbine** + independent pump (the
    /// beat spins angular momentum up, it coasts down); **+1** = the **coupled E↔B dynamo**
    /// (`math::em_cavity_step` — the beat kicks a struck cavity, energy *rings* between the
    /// axial pump (E/current mode) and the swirl (B mode), the LC analog of Faraday+Ampère);
    /// **0** = an even blend. `ring_freq` (Hz) = the dynamo's E↔B exchange rate; the kick
    /// reuses `beat_spin_force`, the ring-down reuses `swirl_slowdown`. Captured **Look**.
    /// (Tail-appended after `mxforce2`.)
    pub mxforce3: [f32; 4],
    /// **Shaded particle beads (#298 Tier 1):** turn the additive spark motes into
    /// **sphere-impostor droplets** that bear the shared split-sum IBL + key/fill
    /// lighting. `[beads, metallic, roughness, _, _, _, _, _]`: `beads` (0/1) swaps the
    /// additive spark draw for the opaque bead impostor (front-hemisphere normal +
    /// `frag_depth` reconstructed from the billboard, depth-write on, so the droplets
    /// occlude each other and the scene); `metallic`/`roughness` are the beads' PBR
    /// material (the scene's own metallic/roughness stay the generator's). The
    /// energization glow + hue cycle survive as the beads' emissive term. `beads = 0`
    /// → the additive sparks, byte-identical. Captured **Look**. (Tail-appended after
    /// `mxforce3`.)
    pub pbeads: [f32; 8],
    /// Audio-dipole Tier 3 volumetric visualizer (#248): `[wave, _, _, _]`. `wave` =
    /// **waveform shells** — the recent loudness history modulates the baked field
    /// energy **radially** (newest at the source, older toward the rim), so a loud
    /// moment radiates outward as a bright shell through the energy cloud (the waveform
    /// along the radial axis, as retarded amplitude). The other two Tier-3 mappings ride
    /// the reserved `audiodip[6..8]` = [stereo lean, pitch → rate]. Off by default →
    /// byte-identical. Captured **Motion**. (Tail-appended after `pbeads`.)
    pub audiodip2: [f32; 4],
    /// **Per-material Hue/Saturation/Value (#305 Tier 1)** for the two cube-shader
    /// materials. `[0..4]` = the **generator** material `[hue, hue_cycle, saturation,
    /// value]`; `[4..8]` = the **scenery / environment** material (same layout). `hue`
    /// offsets the palette-derived colour (a hue rotation → cycles the palette),
    /// `hue_cycle` auto-advances the hue over the beat clock, `saturation`/`value`
    /// default to 1 and only lower. `[0, 0, 1, 1]` per material → byte-identical.
    /// Captured **Look**. (Tail-appended after `audiodip2` on the #308↔main merge.)
    pub matcol: [f32; 8],
    /// **Bead Hue/Saturation/Value (#305 Tier 1)**: `[hue, hue_cycle, saturation,
    /// value]` for the shaded particle beads (same semantics as `matcol`, applied to
    /// the bead albedo). `[0, 0, 1, 1]` → byte-identical. Captured **Look**.
    /// (Tail-appended after `matcol`.)
    pub pbeads2: [f32; 4],
    /// Photon-mapped caustics (#258 Tier 5): `[enable, photons_k, intensity, radius]`.
    /// `enable` 0 → no photon pass, the path tracer is byte-identical. > 0 → each
    /// frame `rt_caustic` light-traces `photons_k`·1000 photons from the key light
    /// through the tracer's exact specular chain (dielectric split, entry+exit
    /// refraction, TIR, Beer–Lambert, the analytic lens, per-λ Cauchy when spectral
    /// is on) and splats their deposits into a screen-space map the tracer adds to
    /// its progressive accumulation — so a lens focal spot / prism rainbow ON a
    /// surface resolves in ~1 frame instead of thousands. `intensity` scales the
    /// map (a Look); `radius` = gather (KDE) blur radius in pixels (a Look);
    /// `photons_k` is a per-quality budget (not preset-captured). Path-tracer only.
    /// (Tail-appended after `pbeads2` on the #295↔main merge.)
    pub ptcaustic: [f32; 4],
    /// **Live-sky cloud reflections (#305 Tier 2)**: `[enable, cloud_cover,
    /// cloud_speed, cloud_strength]`. When `enable`, a drifting procedural cloud layer
    /// modulates the **sharp environment reflection** (`sample_prefiltered`) on every
    /// reflective cube-shader material + the beads, so chrome/glass droplets show
    /// moving clouds instead of a frozen sky. `cloud_cover` = how much of the dome is
    /// cloud, `cloud_speed` = drift (turns/beat, phase baked in the visual),
    /// `cloud_strength` = the reflection brightness swing. `enable = 0` → byte-
    /// identical. Captured **Look**. (Tail-appended after `ptcaustic` on the #309↔main merge.)
    pub skyrefl: [f32; 4],
    /// Camera shot sequencer (#307 Tier 1): `[enabled(0/1), bars_per_shot,
    /// order(0 Series / 1 Random), transition(0 Glide / 1 Cut)]`. When enabled the
    /// visual cycles the auto-orbit moves on each `bars_per_shot`-bar downbeat
    /// instead of holding the single `camera[0]` path; `transition` picks whether it
    /// glides (crossfades the move offsets over `cam_clock[2]` bars) or cuts. Off
    /// (default) → the single `camera` path behaves exactly as before. Captured
    /// **Motion**. (Tail-appended after `skyrefl` on the #314↔main merge.)
    pub cam_seq: [f32; 4],
    /// Decoupled dolly (#307 Tier 1): `[period_bars, depth(0..1), wave(0 Sine /
    /// 1 Triangle / 2 Ease), beats_per_bar]`. An in/out radius breath on its own
    /// musical period, independent of the orbit speed — so a slow wide orbit can
    /// still breathe. `beats_per_bar` rides here so the visual derives the bar clock
    /// (`beat_pos / beats_per_bar`) in one place. `depth = 0` (default) → inert,
    /// today's framing unchanged. Captured **Motion**. (Tail-appended.)
    pub cam_dolly: [f32; 4],
    /// Beat/bar clock feel (#307 Tier 1): `[tempo_source(0 Host / 1 Audio /
    /// 2 Manual), beat_momentum(0/1), transition_bars, _]`. `tempo_source` selects
    /// what BPM the beat clock free-runs at (Host transport / detected from audio /
    /// the manual dial); `beat_momentum` gates the per-beat velocity kick (off =
    /// smooth cinematic motion that doesn't wiggle with the audio); `transition_bars`
    /// is the glide length. Defaults (Host, momentum on) reproduce today's behaviour.
    /// Captured **Motion**. (Tail-appended.)
    pub cam_clock: [f32; 4],
    /// Audio-detected tempo (#307 Tier 1): `[bpm, confidence(0..1), _, _]`. Written
    /// by the plugin's `process()` from the analyzer's onset autocorrelation (NOT a
    /// param — live detection, like `transport`/`audio`). The visual uses it as the
    /// clock BPM when `cam_clock[0] == 1` (Audio); on a breakdown (confidence drops)
    /// the last good BPM is held. 0 when audio-reactive is off. (Tail-appended.)
    pub cam_audio: [f32; 4],
    /// Camera framing axes + sequencer richness (#307 Tier 2): `[roll_deg, fov_deg,
    /// fov_dolly, hold_prob, phrase_lock(0/1), seq_mix, _, _]`. `roll` rolls the
    /// up-vector (dutch tilt); `fov_deg` is the base vertical FOV (45 = today);
    /// `fov_dolly` couples FOV to the dolly breath (Hitchcock zoom); `hold_prob` is the
    /// chance the sequencer repeats a shot; `phrase_lock` snaps the move phase to a
    /// canonical facing on each shot boundary; `seq_mix` (0..1) blends the always-on
    /// orbit-cam (`camera[0]` path) with the sequencer's move (0 = fully orbit-cam /
    /// organic-math, 1 = fully sequencer). Defaults (roll 0, FOV 45, seq_mix 1) → the
    /// original Tier-2 framing. Captured **Motion**. (Tail-appended after `cam_audio`.)
    pub cam_frame: [f32; 8],
    /// Camera storyboard (#307 Tier 3): an authored, saveable playlist of shots that
    /// overrides the auto sequencer when `[0]` is on. Header `[enabled, count, mode(0
    /// Series / 1 Random / 2 Shuffle / 3 Weighted), seed, next_gen, _, _, _]` then 4
    /// shot slots at `8 + k*4` = `[path, bars, radius_scale, _]`. `next_gen` is a
    /// live counter bumped by the editor's "next shot" button (filled by `process()`,
    /// not a param) that the visual edge-detects to advance at the next bar. Off
    /// (default) → the Tier-1/2 sequencer/single-path is unchanged. Captured
    /// **Motion**. (Tail-appended after `cam_frame`.)
    pub cam_story: [f32; 24],
    /// **Neural radiance cache — live (#256 Tier 0)**: `[enable, confidence,
    /// learn_rate, omega, terminate_bounce, train_steps, seed, _]`. The #200 Tier-6
    /// `RadianceCache` (a trainable SIREN `(pos, dir) → radiance`) made live. When
    /// `enable`, the visual trains the cache each frame (`train_steps` samples of the
    /// analytic environment light field — the bake-first bootstrap; GPU path-traced
    /// samples are the on-Mac follow-up), uploads its 419 weights to the path
    /// tracer, and **short paths terminate into a cache query** at bounce depth
    /// ≥ `terminate_bounce` (`nrc.wgsl`) instead of tracing on — infinite-bounce GI
    /// at short-path cost. `confidence` (0..1) is how much of the cached radiance is
    /// trusted at termination (the confidence blend: a cold/wrong cache can only
    /// *lose* GI, never corrupt the image — the raw trace is the fallback below it).
    /// `omega` = SIREN frequency, `seed` = cache init. `enable = 0` → no query, no
    /// upload; the path tracer is byte-identical. Captured **Look**. (Tail-appended
    /// after `cam_story`; LAYOUT_VERSION 0x0255→0x0256.)
    pub nrc: [f32; 8],
    /// **Neural radiance cache — RT-stack synergies (#256 Tier 1)**: `[guide_on,
    /// guide_candidates, firefly_on, firefly_clamp]`. Both make the path tracer
    /// *cheaper*, using the live Tier-0 cache. `guide_on` = **NRC-guided importance
    /// sampling**: the diffuse bounce direction is chosen by resampled importance
    /// sampling (RIS) over `guide_candidates` cosine candidates weighted by the
    /// cache's predicted radiance, so paths stop wasting themselves on dark
    /// directions → faster convergence at equal quality (unbiased — the RIS reweight
    /// folds into throughput). `firefly_on` = **firefly suppression at the source**:
    /// a per-sample outlier is clamped toward the cache mean (the cache is the
    /// expected value) at `firefly_clamp × expected`, killing fireflies before the
    /// denoiser. Both need the Tier-0 cache live (`nrc[0]`); the visual only arms
    /// them when it is. `guide_on = 0 && firefly_on = 0` (default) → the tracer is
    /// byte-identical. Captured **Look**. (Tail-appended after `nrc`; LAYOUT_VERSION
    /// 0x0256→0x0257.)
    pub nrc2: [f32; 4],
    /// **Neural radiance cache — light-field uses (#256 Tier 2)**: `[gi_on,
    /// gi_strength, reflect_terminate, _]`. Because the cache knows the radiance
    /// everywhere, it can light more than the primary path-traced cubes. `gi_on` =
    /// **supersede the DDGI probe grid**: the visual fills the 6³ SH probe volume
    /// (`gi.rs`, cube-shader group 3) by querying the continuous cache
    /// (`math::compute_gi_probes_from_cache`) instead of the discrete node
    /// integration — a learned, continuous bounce field, and because the ink / fluid
    /// march bind the SAME probe buffer they get lit consistently for free (no shader
    /// change). `gi_strength` scales that bounce (the cache-GI's own intensity, used
    /// in place of the Bounced-GI card's). `reflect_terminate` = **cheap lit
    /// reflections**: a Chrome/Glass secondary ray in the path tracer terminates into
    /// a cache query → reflections of the *lit* neighbours + off-screen light, not
    /// just the environment map. Needs the Tier-0 cache live (`nrc[0]`). All 0
    /// (default) → the discrete probe grid + env-only reflections, byte-identical.
    /// Captured **Look**. (Tail-appended after `nrc2`; LAYOUT_VERSION 0x0257→0x0258.)
    pub nrc3: [f32; 4],
    /// **Neural radiance cache — hard transport + volumetrics (#256 Tier 3)**:
    /// `[volume_on, volume_density, volume_steps, volume_strength, caustic_on,
    /// caustic_gain, _, _]`. The cache pays off most on the light paths that are
    /// otherwise rare or expensive. `volume_on` = **volumetric in-scattering /
    /// god-rays**: the path tracer marches the primary camera ray through a
    /// participating medium (density `volume_density`, `volume_steps` steps, scaled
    /// by `volume_strength`), querying the cache for the in-scattered radiance at each
    /// step → single-scatter glow / haze without a separate volumetric march.
    /// `caustic_on` = **cached caustics**: at the primary diffuse hit the tracer adds
    /// `caustic_gain ×` the cache's radiance arriving along the mirror direction — the
    /// focused high-energy light a camera-first path can't find through a specular
    /// chain, amortized by the cache. Both feed the existing bloom + HDR chain, so
    /// they pulse with the beat for free. Need the Tier-0 cache live (`nrc[0]`); path-
    /// tracer (RGB path) only. All 0 (default) → the tracer is byte-identical.
    /// Captured **Look**. (Tail-appended after `nrc3`; LAYOUT_VERSION 0x0258→0x0259.)
    pub nrc4: [f32; 8],
    /// Acoustic-field generator (#325, Duo-Field N1; used when `generator` = 23):
    /// `[source_kind, k, near, amp, separation, r_min, blend(pressure↔velocity),
    /// norm_field(0/1), rings, spokes, samples, ray_len, spread_deg, thickness,
    /// aura_blend(pressure↔velocity), beat_pump]`. The scalar pressure drives the
    /// geometry (a breathing multipole shell on a (θ,φ) lattice) and the vector
    /// particle-velocity drives the aura (glowing by the acoustic energy density);
    /// `blend`/`aura_blend` are the independent pressure↔velocity swaps for the two
    /// channels. Audio (#325 Tier 3) rides the shared `audiodip`/`audio` spine +
    /// `beat_pump`. Off by default / byte-identical unless selected. Captured
    /// **Generator**. (Tail-appended after `nrc4`; LAYOUT_VERSION 0x0259→0x025A.)
    pub acoustic: [f32; 16],
    /// Acoustic Tier 4 (#325, used when `generator` = 23): `[model(0 Radiating /
    /// 1 Cavity), cav_nx, cav_ny, cav_nz, cav_morph, cav_scale, intensity, _]`.
    /// **Cavity** swaps the radiating multipole for a rectangular standing-wave
    /// eigenmode `(nx,ny,nz)` in a box of half-extent `cav_scale` — its pressure
    /// nodal planes are the 3-D Chladni figures; `cav_morph` > 0 walks the modes on
    /// the beat. **Intensity** > 0 makes the aura advect motes along the acoustic
    /// intensity `I = p·u` (the energy-flux "third channel"), glowing by `|p·u|`.
    /// Model = Radiating + intensity 0 (default) → byte-identical to Tiers 1–3.
    /// Captured **Generator**. (Tail-appended after `acoustic`; LAYOUT_VERSION 0x025A→0x025B.)
    pub acoustic2: [f32; 8],
    /// Acoustic Tier 5 (#325, cavity 3-D + audio): `[cav_tween, audio_x, audio_y,
    /// audio_z, _, _, _, _]`. `cav_tween` (0..1) softens the beat mode-walk — 0 =
    /// the old hard cut, up = the nodal planes glide between mode sets (holds, then
    /// smoothsteps to the next set each beat). `audio_{x,y,z}` are per-axis gains:
    /// with the audio drive on, the broadband level lifts each axis's mode number
    /// independently (louder → denser nodal planes on that axis), so the cavity
    /// breathes in 3-D with the music. All 0 (default) → byte-identical to Tier 4.
    /// Captured **Generator**. (Tail-appended after `acoustic2`; LAYOUT_VERSION 0x025B→0x025C.)
    pub acoustic3: [f32; 8],
    /// Duo-Field synthesis (#339 Tier 1) — the "Sound" card the visual reads to
    /// draw the listener gizmos it can *see* sitting in the field it hears:
    /// `[synth_on, play_mode(0 gen/1 inst/2 duet), source_kind, vis_pivot(Hz),
    /// vis_anchor(rate Hz), vis_slope, vis_k_slope, vis_quantize(0 free/1 oct/
    /// 2 beat), probeL x, probeL y, probeL z, probeR x, probeR y, probeR z,
    /// probe0_rides_camera, _]`. The DSP itself lives in the plugin's `process()`
    /// (it reads params directly); this block is only what the picture needs.
    /// Off by default / byte-identical when `synth_on = 0`. Captured **Look**.
    /// (Re-seated to the true tail after `acoustic3` on the #336 merge; LAYOUT_VERSION
    /// 0x025C→0x025D.)
    pub sonify: [f32; 16],
    /// Runtime-written voice bank (#339 Tier 1) — stamped by `process()` each block
    /// like `transport`/`audio`, NOT param-packed. Per voice (stride 8, 8 voices):
    /// `[gate, k_vis, rate_vis, drive, x, y, z, reserved-for-MPE]`. The visual
    /// appends each sounding note to the acoustic generator's source stack so you
    /// *see what you play*, rendered at the lensed rate the eye enjoys.
    pub voices: [f32; 64],
    /// Calibrated metering (#333 Tiers 1–2), written each block by the plugin from
    /// the `LoudnessMeter` (measured, not param-derived): `[momentary_LUFS,
    /// short_LUFS, integrated_LUFS, LRA, true_peak_dBTP, correlation, L_dBFS, R_dBFS,
    /// M_dBFS, S_dBFS, hud_on(0/1), band_count, mode(oct denom; 0=linear), band0_Hz,
    /// _, _]`. dB values are floored at −120 (no −∞ in the snapshot). `hud_on` mirrors
    /// the `meter_hud` param so the visual can draw the numeric HUD; `[11..14]` are the
    /// header for `audiospectrum` below. All 0 by default (no audio) → HUD off.
    /// (Tail-appended after `voices` on the #339 merge; LAYOUT_VERSION 0x025D→0x025E.)
    pub audiometer: [f32; 16],
    /// Calibrated RTA (#333 Tier 2): the measured fractional-octave (or linear-FFT)
    /// band levels in **dBFS** (a full-scale sine reads 0 dBFS in its band), floored
    /// at −120. First `audiometer[11]` entries valid; for the octave modes band `i`'s
    /// centre = `audiometer[13]·2^(i/audiometer[12])` — so the visual can wrap the
    /// calibrated bars onto a field with real frequency/dB labels (Tier 3). Written
    /// each block by the plugin; all −120 by default (silence).
    /// (Tail-appended after `audiometer`; LAYOUT_VERSION 0x025E→0x025F.)
    pub audiospectrum: [f32; 128],
    /// Analyzer / Calibrated instrument mode (#333 Tier 3): `[mode(0 Expressive /
    /// 1 Calibrated), target_lufs, floor_lufs, tp_ceiling_dBTP, corr_alarm,
    /// reference_hud(0/1), _, _]`. In **Calibrated** mode the visual drives the
    /// Duo-Field from the *measured* loudness (`audiometer`/`audiospectrum`) via a
    /// reproducible dB law instead of the expressive gain·RMS — the picture becomes
    /// comparable across sessions. `reference_hud` draws the delivery-target +
    /// true-peak/phase alarms. Expressive (default) → byte-identical. Captured
    /// **Look**. (Tail-appended after `audiospectrum`; LAYOUT_VERSION 0x025F→0x0260.)
    pub analytical: [f32; 8],
    /// Field Volume (#348) — extends `SurfaceMode::Volume` from the node-baked
    /// metaball to a calibrated density cloud (kills the scraggle). Layout:
    /// `[source, smooth, exposure_db, calibrate, gain, _, _, _]`.
    /// - `source` (`FieldVolSource`): 0 = **Legacy** (today's node point-set
    ///   metaball bake — byte-identical), 1 = **Auto** (field generators
    ///   Maxwell/Acoustic → the analytic field-energy bake, every other node
    ///   generator → a smoothed node bake), 2 = **FieldBaked** (force the analytic
    ///   field-energy bake), 3 = **SmoothedNode** (force the smoothed node bake).
    /// - `smooth` = smoothing-kernel width scale for the node bake (multiplies the
    ///   metaball radius; 1 = neutral).
    /// - `exposure_db` = Tier-2 exposure in dB added to the cloud brightness (0 =
    ///   neutral).
    /// - `calibrate` (0/1) = key the density/emission gain to the **calibrated**
    ///   loudness `calibrated_drive(LUFS)²` (from `audiometer`/`analytical`) instead
    ///   of the plain `volume[]` dials (audio-optional: 0 = the plain dials).
    /// - `gain` = extra density/emission multiplier (1 = neutral).
    /// `source = 0` (default) → today's Volume byte-identical. Captured **Look**.
    /// (Tail-appended after `analytical`; part of the #348/#349 LAYOUT_VERSION 0x0260→0x0261.)
    pub fieldvol: [f32; 8],
    /// Calibrated colour (#349) — a cross-cutting tint law: colour that MEANS a
    /// measured level, sampled from a legend-backed perceptual LUT, applied once in
    /// `draw_tissue`'s per-node tint + the shared shader tint so EVERY surface type
    /// inherits it. Layout: `[mode, lo_db, hi_db, lut, source, amount, _, _]`.
    /// - `mode` (`ColourMode`): 0 = **Aesthetic** (today's tint — HSV/palette/
    ///   RGB-cube, byte-identical), 1 = **Calibrated** (tint = LUT sampled at
    ///   `db_to_colour_t(level, lo_db, hi_db)`).
    /// - `lo_db`/`hi_db` = the dB window mapped across the LUT (e.g. −60..0 dBFS).
    /// - `lut` (`CalLut`): 0 Turbo / 1 Viridis / 2 Inferno / 3 Magma (perceptually
    ///   uniform — equal colour steps = equal dB steps).
    /// - `source` (`CalColourSource`): 0 = **Auto** (field generators Maxwell/
    ///   Acoustic → per-band dBFS from `audiospectrum`, every other generator →
    ///   momentary LUFS from `audiometer[0]`), 1 = **Band**, 2 = **Lufs**.
    /// - `amount` (0..1) = blend of the calibrated tint over the aesthetic tint.
    /// `mode = 0` (default) → today's colour byte-identical. Captured **Look**.
    /// (Tail-appended after `fieldvol`; part of the #348/#349 LAYOUT_VERSION 0x0260→0x0261.)
    pub colour: [f32; 8],
    /// Field Chamber (#346 Tier 1) — the triggered, downsampled **oscilloscope
    /// display frame** the plugin publishes each block from the `ScopeRing` (the
    /// visual is a separate process and can't see the ring directly). Layout:
    /// `[0] = valid sample count (≤256), [1] = sample_rate (Hz), [2] = channel
    /// (0 L / 1 R / 2 Mid), [3] = trigger_locked (0/1); [4..4+n] = amplitude
    /// samples in −1..1` (raw normalized; the visual applies the chamber AMP gain).
    /// A rising/falling-edge trigger + linear downsample stabilizes the wall trace
    /// (mirrors how `audiospectrum` + `audiometer[11..14]` publish the RTA). Written
    /// each block by the plugin; all 0 by default (silence). Runtime-written, NOT
    /// param-packed / NOT preset-captured. (Re-seated to the true tail after `colour`
    /// on the main merge; part of LAYOUT_VERSION 0x0261→0x0262.)
    pub scopewave: [f32; 260],
    /// Field Chamber (#346 Tier 1) — the two analyzer **panels** hung on the box's
    /// back walls (rear −Z = time/oscilloscope, right +X = frequency/spectrum), so
    /// the Duo-Field sits inside a time × frequency frame. Layout (all captured
    /// **Look**): `[0] = panels_on, [1] = panel_style (0 Flat / 1 Impostor),
    /// [2] = rear_on (scope), [3] = right_on (spectrum), [4] = opacity (0..1),
    /// [5] = fill (0..1 wall inset), [6] = scope_amp (vertical gain),
    /// [7] = db_floor (spectrum, dBFS), [8] = material_type (Tier 2 impostor),
    /// [9] = metallic, [10] = roughness, [11] = wall_relative (0 fixed world axes /
    /// 1 camera-relative back walls), [12] = thickness (ribbon/bar radius),
    /// [13] = emissive, [14] = db_top (spectrum, dBFS), [15] = reserved`. Panels are
    /// drawn only on **back-facing** walls (reusing `AxesConfig.eye`) so they never
    /// occlude the field. `panels_on = 0` draws nothing → byte-identical. The rear
    /// reads `scopewave`, the right reads `audiospectrum`. The wall scope's own
    /// TIME/trigger/channel are separate plugin-side params (they drive the
    /// `scopewave` publish, so the visual doesn't need them). Captured **Look**.
    /// (Re-seated to the true tail after `scopewave`; part of LAYOUT_VERSION 0x0261→0x0262.)
    pub chamber: [f32; 16],
    /// Material **Emissive** (HDR self-emission in the surface's own colour):
    /// `[mat_emissive, sc_emissive, pbead_emissive, _]`. Each emits that surface's
    /// resolved HSV colour × the value, added on top of `glow` and pushed into HDR
    /// so the geometry blooms in its OWN hue instead of washing to white. Routed
    /// through spare uniform components (cube `env_tint.w`, bead `bead_hsv.w`) — no
    /// GPU struct change. Captured **Look**; all 0 → byte-identical. (Appended;
    /// LAYOUT_VERSION 0x0262→0x0263.)
    pub emissive: [f32; 4],
    /// Gaussian Splatting surface (`SurfaceMode::Splat` = 8): the node set is
    /// rendered as a cloud of anisotropic 3-D Gaussians (the 3DGS *primitive* used
    /// for forward synthesis — no reconstruction). Each node's model matrix becomes
    /// a splat: translation → centre μ, the 3×3 rot·scale columns → the covariance
    /// basis, tint → colour. Layout:
    /// `[radius (world × on the node axes), opacity (0..1), falloff (Gaussian
    /// exponent scale — higher = tighter core), mode (0 = Tier 1 additive/unlit,
    /// 1 = Tier 2 sorted-alpha/IBL-lit 2DGS disks), cutoff (discard below this
    /// weight), aniso (extra stretch of the node's non-uniform axes; 1 = as-is),
    /// scatter (Tier 3 — sub-splats sprayed per node, 1 = one/node), jitter (Tier 3 —
    /// how far the sub-splats spread, as a fraction of node size)]`. Only consumed when
    /// `surface_mode` = 8; reuses `instances`/`tints`, so there is no new geometry
    /// payload. **Tier 2 lit splats honour the Material card** (`amb[1]`/`amb[2]`):
    /// Chrome mirrors + Glass/Refractive refract the environment, like the cubes.
    /// Captured **Generator** (a surface-shape block, like `metaball` + `surface_mode`);
    /// `radius = 0` draws nothing → any other surface mode is byte-identical. (Appended;
    /// LAYOUT_VERSION 0x0263→0x0264; Tier 3 filled the two reserved slots [6]/[7].)
    pub splat: [f32; 8],
    /// Plexus surface mode (ordinal 9): a proximity "web" over whatever node cloud
    /// the active generator produced — each node wired to its nearest neighbours by
    /// thin struts, with a marker at each node. `[radius_mul, max_links, strut_mul,
    /// marker_mul]`. `radius_mul`/`strut_mul`/`marker_mul` are unitless multipliers
    /// of the field's characteristic node spacing, so the look is scale-invariant
    /// across generators. Inert unless `surface_mode == 9`. (Appended after `splat`;
    /// LAYOUT_VERSION 0x0264→0x0265.)
    pub plexus: [f32; 4],
    /// Plexus Tier 2 (impostor rendering): `[impostor_on, edges_on, node_radius_mul,
    /// edge_radius_mul]`. When `impostor_on`, the instanced-cube web is replaced by
    /// GPU impostors — nodes as analytic **sphere** impostors, edges as **capsule**
    /// (tube) impostors — each with its OWN material (see `plexus_node_mat` /
    /// `plexus_edge_mat`). Radii are × node spacing. (Appended; LAYOUT_VERSION 0x0264→0x0265.)
    pub plexus2: [f32; 4],
    /// Plexus node impostor material: `[mat_type, metallic, roughness, ior, hue,
    /// saturation, value, emissive]` — full independent PBR/HSV control of the node
    /// spheres, shaded by the same split-sum IBL + key/fill as everything else.
    pub plexus_node_mat: [f32; 8],
    /// Plexus edge impostor material: same layout as `plexus_node_mat`, fully
    /// independent of it — chrome nodes on glass filaments, emissive nodes on matte
    /// struts, whatever.
    pub plexus_edge_mat: [f32; 8],
    /// Plexus Tier 3 (beat-driven signal propagation): `[signal_on, speed, gain,
    /// width]`. A bright activation shell radiates outward from the web's centre on
    /// the beat clock (`speed` = shells per beat), boosting the emissive of the node
    /// + edge impostors it passes over (`gain`, `width`) — the web "fires" to the
    /// music. Rides the Tier-2 impostor path. (Appended; LAYOUT_VERSION 0x0265→0x0266.)
    pub plexus3: [f32; 4],
    /// Plexus Tier-1 shape morph: `[node_shape, edge_shape, _, _]`. `node_shape`
    /// morphs the node markers cube (0) → rounded cube → sphere (1); `edge_shape`
    /// morphs the strut cross-section sharp square (0) → circle (1). Drives two
    /// procedurally-morphed meshes drawn as node/strut sub-batches. Only affects the
    /// Tier-1 instanced path (Tier-2 impostors are already round). (Appended; LAYOUT_VERSION
    /// 0x0267→0x0268.)
    pub plexus4: [f32; 4],
    /// Splat Tier 3 look extension: `[solid, _, _, _]`. **Solidity** (0..1) remaps the
    /// Gaussian weight toward a flat-topped, sharp-edged **opaque disc** (a super-Gaussian
    /// edge): 0 = the soft Gaussian (unchanged), 1 = a hard disc. This is what turns the
    /// splat cloud from soft bokeh into a compact opaque *surface* — overlapping opaque
    /// discs occlude instead of blurring. Only used when `surface_mode == 8`; `solid = 0`
    /// → byte-identical. (Appended after `plexus4`; LAYOUT_VERSION 0x0268→0x0269.)
    pub splat2: [f32; 4],
    /// Maxwell **E↔B phase** (near↔far induction dial): `[eb_phase_deg, _, _, _]`.
    /// `eb_phase_deg` ∈ [0, 90] offsets the tempo-locked B-swirl reversal relative to
    /// the E oscillation clock — `osc = cos(maxdip_phase − φ)`. **0° = far-field**
    /// (radiation zone: E and B in phase, reverse together — the E↔B-lock default); **90°
    /// = near-field induction** (the swirl is in quadrature — B peaks at E's zero-crossing,
    /// as `∂B/∂t ∝ ∇×E` demands close to the source). Only active with Tempo Sync +
    /// Maxwell fluid force-drive; 0 = byte-identical to the plain lock. (Appended after
    /// `splat2`; LAYOUT_VERSION 0x0269→0x026A.)
    pub mx_eb: [f32; 4],
    /// Plexus **overlay**: `[overlay_on, shell_scale, shell_thickness, shell_bins]`.
    /// When `overlay_on`, the plexus web is drawn as an OUTER SHELL wrapped around
    /// whatever OTHER surface is active (Metaball, etc.) — like the Particle Aura /
    /// Water overlays, it reads the generator's node cloud non-destructively instead
    /// of replacing it. `math::outer_shell` bins the cloud by direction and keeps the
    /// `shell_thickness` farthest per `shell_bins²` directional cell (the rind, not
    /// the full volume), then `shell_scale` (≥ 1) grows it outward into a cage. The
    /// shell reuses ALL the standalone Plexus look params (`plexus`/`plexus2`/
    /// `plexus3`/`plexus4` + node/edge materials), so it keeps every feature — Tier-1
    /// shape-morph markers/struts, Tier-2 impostors + independent materials, Tier-3
    /// beat signal. Off (`overlay_on = 0`) → byte-identical. (Appended after `mx_eb`;
    /// LAYOUT_VERSION 0x026A→0x026B.)
    pub plexus_overlay: [f32; 4],
    /// Field Engine (#381 Tier 1) live coefficients: `[kind, preset, scale, extent,
    /// a, b, density, gain, thickness, _]`. `kind` = `FieldKind` (0 Auto / 1 Scalar /
    /// 2 Vector / 3 Complex); `preset` = `FieldPreset` gallery index (7 = Custom =
    /// the sidecar program); `a`/`b` are host-mappable coefficients bound to the
    /// program variables `a`/`b`; `density` = field-line seeds / lattice resolution.
    /// The program TEXT itself rides the `organic-math-field.txt` sidecar +
    /// `field_gen` (below), NOT this block. Captured **Generator**; selected only
    /// when `generator == 24`. Slot `[9]` is reserved for a Tier-2 coefficient.
    /// (Appended after `plexus_overlay`; LAYOUT_VERSION 0x026B→0x026C.)
    pub field: [f32; 10],
    /// Field Engine program-load counter (#381 Tier 1) — the GUI bumps it after
    /// writing the field sidecar; the visual edge-detects it and recompiles the
    /// program (exactly like `hdr_gen` / `nn_gen`). Runtime-written by `process()`
    /// from an atomic; the param default is 0. (Appended after `field`.)
    pub field_gen: u32,
    /// Density-Map Attractor generator (#380 Tier 1; used when `generator` = 25):
    /// `[kind (MapKind: 0 = Complexus), a, b, points_k (thousands), warmup, scale
    /// (world half-extent of the map's [-1,1] box), size (per-point marker), intensity
    /// (HDR tint gain), a_drive, b_drive]`. The generator iterates the discrete
    /// complex-holomorphic map `x' = sin(x²−y²+a)`, `y' = cos(2xy+b)` for many points and
    /// emits the visited set as a node cloud (`instances`/`tints`) → an additive density
    /// "fire" (best in `SurfaceMode::Splat` + bloom). `a_drive`/`b_drive` ∈ [0,1] set how
    /// much the animation clock (`gen_phase`) sweeps `a`/`b` — 0 = static (the byte-identical
    /// default), 1 = full-rate sweep; independent, so unequal drives trace a Lissajous path
    /// through (a,b) space and the pattern morphs on its own. Captured **Generator**; off by
    /// default / byte-identical unless selected. (Slots [8]/[9] grow the block; tail-appended
    /// after `field_gen`; LAYOUT_VERSION 0x026C→0x026D.)
    pub mapattractor: [f32; 10],
    /// Origin mode for the Original cube-field (`OriginMode`): 0 = **Corner**
    /// (grid corner at the origin — the historical look, byte-identical) / 1 =
    /// **Centered** (each axis's loop index re-centred to `idx − (count−1)/2`, so
    /// the field is point-symmetric about the origin and every arm/sheet pivots off
    /// its own centre). Read by `draw_tissue` / `build_swept_tubes` / `draw_membrane`
    /// / `cube_field_strands` (Original generator only). `0` (default) → today's
    /// geometry byte-identical. Captured **Generator**. (Tail-appended after
    /// `mapattractor`; LAYOUT_VERSION 0x026D→0x026E.)
    pub origin_mode: u32,
    /// Density-Map Attractor **parameter orbit** (#380 Tier 2; used with `generator`
    /// = 25): `[mode (MapOrbitMode: 0 Off / 1 Linear / 2 Lissajous), loop_beats,
    /// Ra, Rb, fa, fb, psi (rad), free_rate]`. Turns the static Tier-1 field into the
    /// morphing animation by walking `(a, b)` (centred on `mapattractor[1]`/`[2]`)
    /// around a **closed loop** in parameter space. **Lissajous:** `a = a0 + Ra·sin(2π·fa·φ)`,
    /// `b = b0 + Rb·sin(2π·fb·φ + ψ)` — integer `fa`/`fb` ⇒ the loop closes seamlessly.
    /// The loop phase `φ` is driven by the **PLL beat clock** (`φ = beat_pos / loop_beats`,
    /// so one loop = `loop_beats` beats) while the host plays, and free-runs on
    /// `gen_phase · free_rate` otherwise. **Linear** (default) reproduces the Tier-1
    /// ramp (`a += a_drive·gen_phase`) — byte-identical with the default drives 0; **Off**
    /// holds `(a, b)` static. `math::map_attractor_effective_ab` is the shared evaluator
    /// (renderer + overlay inset). Captured **Generator**. (Tail-appended after
    /// `origin_mode`; LAYOUT_VERSION 0x026E→0x026F.)
    pub maporbit: [f32; 8],
    /// **AI Performer** runtime block (#317 Tier 1): `[agent_on, chat_gen, plan_gen,
    /// release_gen, _, _, _, _]`. Runtime-stamped by the plugin's `process()` from
    /// editor-thread atomics (the `hdr_gen`/`nn_gen` pattern) — NOT a `param_block!`
    /// entry and NOT preset-captured. `agent_on` (0/1) reflects whether the Mind card
    /// has been engaged; `chat_gen` bumps when the editor writes a new chat message to
    /// the `organic-math-chat.txt` sidecar; `plan_gen` bumps when a phrase-plan JSON is
    /// written to `organic-math-plan.txt` (the debug executor path); `release_gen` bumps
    /// on "Release agent" (clears all agent holds in the visual). `name_gen` (slot 4,
    /// #425 intelligent preset names) bumps when the editor saves a preset with
    /// auto-naming on — it writes the scene identity to `organic-math-namereq.txt`, and
    /// the visual asks the local model for a name and writes it to
    /// `organic-math-namereply.txt`. Naming keys off a configured endpoint, NOT
    /// `agent_on`, so it works without the chat engaged. The agent runtime +
    /// its OpenAI-compatible localhost client live in the VISUAL process (`agent.rs`);
    /// the endpoint URL + model name ride the `organic-math-agent.txt` sidecar, not this
    /// block. All zeros → the feature is inert. (Tail-appended after `maporbit`;
    /// LAYOUT_VERSION 0x026F→0x0270.)
    pub agent: [f32; 8],
    /// Visible-Mind block (#367) — a **runtime-stamped** counter block (NOT params,
    /// NOT preset-captured), like `hdr_gen` / `nn_gen`:
    /// `[mind_on, model_gen, topo_mode, prompt_gen, temp, ctx, rate, fullattn]`.
    /// *Tier 1 (specimen):* `mind_on` (0/1) = a model is loaded; `model_gen` bumps
    /// when the editor picks a `.gguf` (the path rides the `organic-math-model.txt`
    /// sidecar) — the visual edge-detects it, parses the GGUF header, builds the
    /// architecture topology, and feeds the same `neural_loaded` slot the connectome
    /// JSON path fills; `topo_mode` (0 = architecture skeleton, 1 = Live streaming)
    /// selects the activation-ring live mode.
    /// *Tier 2b (embedded runtime):* the last five slots drive the optional
    /// `organic-math-mind-runtime` bin (built `--features embedded-llm`): `prompt_gen`
    /// bumps when the Mind card's "Generate" writes the prompt sidecar
    /// (`organic-math-mind-prompt.txt`) — the runtime edge-detects it and runs one
    /// completion, streaming per-token activation frames into the mind ring + decoded
    /// text into the reply sidecar (`organic-math-mind-reply.txt`); `temp` = sampling
    /// temperature, `ctx` = context length (tokens), `rate` = token-rate cap
    /// (tokens/sec, 0 = uncapped), `fullattn` (0/1) = flash-attention OFF (so the
    /// per-head attention tap can read weights). All zeros by default → the feature is
    /// inert until a model is loaded. (Tail-appended after `agent`; LAYOUT_VERSION 0x0270→0x0271.)
    pub mind: [f32; 8],
    /// Density-Map Attractor **Tier 3** extras (#380; used with `generator` = 25):
    /// `[c, d, color (MapColor: 0 StepSpeed / 1 IterIndex / 2 JacobianStretch), _]`.
    /// `c`/`d` are the third/fourth map coefficients (Clifford / de Jong / Pickover use
    /// all four; Complexus / Gumowski–Mira / Hopalong ignore the ones they don't read);
    /// they are static this tier (the beat orbit walks `a`/`b` only). `color` picks how
    /// `math::map_attractor_field` derives each splat's tint coordinate — `StepSpeed`
    /// (default) → byte-identical. Slot `[3]` is reserved. Captured **Generator**.
    /// (Re-seated after `mind` on the #389/#390 merge; LAYOUT_VERSION 0x0271→0x0272.)
    pub mapattractor2: [f32; 4],
    /// Field Engine **Tier 3** time-marched PDE sim (#381; used when `generator`
    /// = 24 and `[0]` != 0): `[preset (PdePreset: 0 Off / 1 Heat / 2 Wave /
    /// 3 Schrodinger / 4 Gray-Scott), D (diffusion / wave-speed / kinetic), time_scale
    /// (beat-delta -> sim-time multiplier), feed (Gray-Scott F), kill (Gray-Scott k),
    /// potential (Schrodinger harmonic-trap strength), forcing (audio source amp),
    /// res (grid resolution 16..128; 0 = default 64)]`. When `preset` != Off the visual
    /// marches a `math::FieldSim` on a periodic CPU grid (CFL-clamped explicit stepper)
    /// off the PLL beat clock and renders the live grid state through the lattice
    /// glyphs (scalar) / |psi|^2+phase (complex) instead of the static analytic field.
    /// `preset = Off` (default) -> Tier 1/2 behaviour is byte-identical. Captured
    /// **Generator**. (Re-seated after `mapattractor2` on the #393 merge; LAYOUT_VERSION 0x0272->0x0273.)
    pub fieldsim: [f32; 8],
    /// Scene Kaleidoscope (#361 Tier 1) — a post-stage kaleidoscopic fold of the
    /// resolved HDR scene (the live PBR render of ANY generator + surface), run
    /// before the bloom/tonemap composite. Layout:
    /// `[enabled(0/1), sectors, mode(0 FullFrame / 1 Wedge), spin, roll, zoom,
    /// center_x, center_y, mix, twist, tint_hue(deg), tint_amt, seam, _, _, _]`.
    /// N-fold kaleidoscopic symmetry folds each output pixel's screen coordinate and
    /// samples the scene there — reflected shards are real, moving, lit geometry.
    /// `mode` 0 = each slice shows the whole frame mirror-tiled (swimmy), 1 = classic
    /// optical kaleidoscope (identical slices). `spin`/`roll` rotate the fold (spin ×
    /// the animation clock, so it rides Speed/beat); `zoom`/`center_*` frame the
    /// source; `twist` adds a log-polar spiral; `tint_*` hue-grade the fold; `seam`
    /// supersamples the mirror seams; `mix` crossfades against the untouched scene.
    /// `enabled = 0` → the HDR buffer is untouched (byte-identical). Captured
    /// **Look**. (Re-seated to the true tail after `fieldsim` on the #363↔main sync;
    /// LAYOUT_VERSION 0x0273→0x0274.)
    pub kaleido: [f32; 16],
    /// Quantitative instrumentation (#391 Tier 1) — placeable field probes + energy
    /// ledger + Poynting-flux surface, read from the SAME kernels the visual draws so
    /// the numbers and the picture cannot disagree (Maxwell/Acoustic/Cavity generators
    /// only). Layout:
    /// `[hud_on, probe_on, probe_x, probe_y, probe_z, ledger_on, ledger_half,
    ///   ledger_res, flux_on, flux_x, flux_y, flux_z, flux_size, flux_axis(FluxAxis:
    ///   0 X / 1 Y / 2 Z / 3 Radial), flux_res, csv_log]`. The visual samples
    /// `math::AnalyticField::probe` at the probe point, integrates the energy ledger
    /// (`math::field_energy_ledger`) and the flux patch (`math::poynting_flux_through_plane`),
    /// draws a numeric HUD panel (gated by `hud_on`), and appends CSV rows to
    /// `ipc::probe_csv_path()` while `csv_log` is on. `hud_on = 0` (default) → the HUD is
    /// not drawn and nothing is written; render is byte-identical. Captured **Look**.
    /// (Tail-appended after `kaleido`; LAYOUT_VERSION 0x0274→0x0275.)
    pub instrument: [f32; 16],
    /// Instrumentation HUD **presentation** (#391 Tier 1 follow-up): the rounded
    /// backing panel + overall size + dock corner, so the read-out is legible over the
    /// render. Layout: `[panel_opacity, panel_bevel (0 square … 1 pill), hud_scale,
    /// dock (HudDock: 0 TL / 1 BL / 2 TR / 3 BR), _, _, _, _]`. The visual draws the HUD
    /// on a `overlay::draw_hud_panel` rounded rect (opacity + bevel), scales the font +
    /// panel by `hud_scale`, and docks it to the chosen corner of the letterbox rect.
    /// Only affects the HUD when `instrument[0]` (hud_on) is set. Captured **Look**.
    /// (Tail-appended after `instrument`; LAYOUT_VERSION 0x0275→0x0276.)
    pub instrument2: [f32; 8],
    /// #423 Tier 1 — **The atlas** (resource-aware inference geometry). A
    /// **runtime-stamped** control block (NOT a `param_block!` entry, NOT
    /// preset-captured — the `mind`/`agent`/`model_gen` pattern): the editor scans a
    /// model library + a hardware profile, writes the derived design points to the
    /// `organic-math-atlas.json` sidecar, and bumps `atlas[0]`; the visual
    /// edge-detects the counter, reads the sidecar, builds the design-space
    /// constellation into `neural_loaded`, and (when `atlas[2]`) draws the roofline
    /// inset. Layout: `[atlas_gen, on, roofline_on, _, _, _, _, _]`. All zeros by
    /// default → inert (no models scanned → today's VST3, byte-for-byte).
    /// (Tail-appended after `instrument2`; LAYOUT_VERSION 0x0276→0x0277.)
    pub atlas: [f32; 8],
    /// Field Playback (#407 Tier A) clip-load counter — a runtime-stamped `*_gen`
    /// counter (the `field_gen`/`nn_gen`/`hdr_gen` pattern, NOT a param, NOT
    /// preset-captured). The editor's "Load Field Clip…" button writes the chosen
    /// `.bin` path to `field_clip_sidecar_path()` and bumps this; the visual
    /// edge-detects it and (re)loads the `math::FieldClip`. Only consumed when the
    /// Field Engine generator's `fieldsim[0]` == `PdePreset::Playback`. Default 0.
    /// (Tail-appended after `atlas`; LAYOUT_VERSION 0x0277→0x0278.)
    pub fieldclip_gen: u32,
    /// Neural CA (learned surrogate, Tier B #407) model-load counter — a
    /// **runtime-stamped** field (the `field_gen`/`nn_gen` pattern), NOT a param and
    /// NOT preset-captured. The editor's "Load NCA Model (JSON)…" button writes the
    /// chosen weights-JSON path to `nca_sidecar_path()` and bumps this; the visual
    /// edge-detects it and (re)loads `math::NcaWeights` (falling back to
    /// `NcaWeights::builtin_default()` when the sidecar is empty/missing/malformed, so
    /// the Neural CA source always renders). Read only when the Field Engine's
    /// `fieldsim[0]` (`PdePreset`) == NeuralCa; 0 by default → inert.
    /// (Tail-appended after `fieldclip_gen`; LAYOUT_VERSION 0x0278→0x0279.)
    pub nca_gen: u32,
    /// FDTD Maxwell solver (#412 Tier 3, Phase 0) — a real-time CPU Yee-lattice
    /// stepper that marches Maxwell's curl equations on a grid, so the field
    /// *propagates* (retardation is emergent) instead of being handed its closed-form
    /// value. A toggle on the Maxwell generator; feeds the Volume surface's energy
    /// cloud (`math::FdtdGrid::fill_volume` → `field_vol_grid`). Layout:
    /// `[fdtd_on, resolution (grid cells/axis), source_mode (0 Pulse / 1 CW),
    ///   frequency (source ω, rad per animation-time unit), drive (source amplitude),
    ///   substeps (CFL sub-steps/frame), boundary_cells (sponge thickness), extent
    ///   (domain half-size, world units)]`. `fdtd_on = 0` (default) → the analytic
    /// Maxwell path is untouched (byte-identical). Captured **Generator**.
    /// (Tail-appended after `nca_gen`; LAYOUT_VERSION 0x0279→0x027A.)
    pub fdtd: [f32; 8],
    /// Node bevel (0 = sharp cube / 0.5 = wide rounded cube / 1 = sphere): rounds
    /// the Original + Flow-Aligned cube geometry via the cube shader's rounded-box
    /// vertex morph (`Uniforms.shape.x`). 0 → today's sharp cube (byte-identical).
    /// Captured **Generator**. (Tail-appended after `fdtd`; LAYOUT_VERSION
    /// 0x027A→0x027B.)
    pub bevel: f32,
    /// Creature Engine params (#476 Tier 1; used when `generator` = 26 =
    /// `GeneratorMode::Creature`): `[form, scale, detail(steps), swim_rate,
    /// warp_amp, warp_freq, rim, glow_scale]`. `form` selects one of the built-in
    /// body plans (0 jelly / 1 ribbon-swimmer / 2 paddle-finned predator); the
    /// geometry is built CPU-side in the visual (`math::creature_body_plan`), so
    /// only these scalars ride the wire. Captured **Generator**. (Tail-appended
    /// after `bevel`; LAYOUT_VERSION 0x027B→0x027C.)
    pub creature: [f32; 8],
    /// Creature Engine Tier 2a params (#476, the metachronal wave): `[wave_speed,
    /// wave_freq, wave_sharp, wave_amount, _, _, _, _]`. A travelling band of light
    /// running along the body axis, its phase advanced off the global Speed clock
    /// (rides the beat). `wave_amount = 0` → base glow, the Tier-1 look. Captured
    /// **Generator**. (Tail-appended after `creature`; LAYOUT_VERSION 0x027C→0x027D.)
    pub creature2: [f32; 8],
    /// Creature body-plan load counter (#476 Tier 2b). Bumped by the editor when a
    /// creature-JSON file is chosen (its path is written to `creature_sidecar_path`);
    /// the visual edge-detects the change, reads the file, and replaces the built-in
    /// body plan via `math::parse_creature_spec` (the `nn_gen`/connectome pattern).
    /// **Runtime-stamped, NOT a param, NOT preset-captured.** (Tail-appended after
    /// `creature2`; LAYOUT_VERSION 0x027D→0x027E.)
    pub creature_gen: u32,
    /// Creature anatomy overlay (#476 Tier 2c): `[overlay_on, opacity, brightness, _]`.
    /// A projected diagram over the creature — the spine, a cross-section ring per
    /// body segment, and a vector per limb — drawn as additive glowing lines,
    /// two-pass depth-tested so landmarks dim behind the translucent body. `overlay_on
    /// = 0` → no diagram (byte-identical). Captured **Generator**. (Tail-appended after
    /// `creature_gen`; part of the merged LAYOUT_VERSION 0x027F→0x0280.)
    pub creature3: [f32; 4],
    /// Procedural / texture-mapped materials (#472 Tier 1): the renderer samples a
    /// real PBR **texture set** (albedo / normal / roughness / metallic / AO /
    /// height) for the generator cubes instead of the scalar-uniform path. Layout:
    /// `[on, projection_mode (0 triplanar / 1 world-planar XZ / 2 object-planar),
    ///   scale (world→UV frequency), normal_strength, ao_strength, rough_scale,
    ///   metal_scale, _]`. `on = 0` (default) → the scalar-uniform PBR path is
    /// untouched (byte-identical). Captured **Look** (serde defaults for old
    /// presets). (Tail-appended after `creature3`; part of merged 0x027F→0x0280.)
    pub material: [f32; 8],
    /// Material texture-set load counter (#472 Tier 1) — a **runtime-stamped**
    /// counter (the `hdr_gen` / `nca_gen` pattern), NOT a param and NOT
    /// preset-captured. The editor's "Load Material…" button writes the chosen
    /// material **folder** path to `material_sidecar_path()` and bumps this; the
    /// visual edge-detects it and (re)loads the six PNG channel maps from that
    /// folder into the GPU material texture set (missing maps fall back to a neutral
    /// 1×1 default). 0 by default → the built-in neutral set → inert.
    /// (Tail-appended after `material`; part of merged 0x027F→0x0280.)
    pub material_gen: u32,
    /// Procedural material — **single-layer noise graph** (#472 Tier 2). When
    /// `material_layer[16]` (procedural on) is set, the visual **bakes** a noise
    /// field into the routed channel's texture with a compute pass (`material_bake.
    /// wgsl`), superseding the Tier-1 PNG load for that channel. One layer for now
    /// (Tier 3 stacks N). Layout: `[kind (MatNoise 0..15), channel (MatChannel:
    /// 0 albedo / 1 roughness / 2 metallic / 3 height / 4 AO / 5 emissive), scale
    /// (world→UV tiles), rotation (rad), offset_x, offset_y, octaves, lacunarity,
    /// gain, warp (domain-warp amount), contrast, gamma, remap_lo, remap_hi,
    /// invert (0/1), seed, procedural (0 off → Tier-1/scalar path untouched,
    /// byte-identical), bake_res (px)]`. Captured **Look**. (Tail-appended after
    /// `material_gen`; LAYOUT_VERSION 0x027D→0x027E.)
    pub material_layer: [f32; 18],
    /// Procedural albedo **gradient** (#472 Tier 2): the two colour stops the baked
    /// noise scalar maps between when the routed channel is Albedo (linear RGB).
    /// Layout: `[lo_r, lo_g, lo_b, _, hi_r, hi_g, hi_b, _]`. Ignored for scalar
    /// channels. Captured **Look**. (Tail-appended after `material_layer`; part of
    /// LAYOUT_VERSION 0x027D→0x027E.)
    pub material_grad: [f32; 8],
    /// Procedural material — **overlay layer 2** (#472 Tier 3). Same 16 base params
    /// as `material_layer[0..15]`, but `[16] = enabled (0/1)` and `[17] = blend_mode`
    /// (BlendMode: 0 Normal / 1 Add / 2 Multiply / 3 Overlay / 4 Screen / 5 Min /
    /// 6 Max / 7 Height) — the layer composites onto the base layer's output for the
    /// **same channel** (`[1]`). `enabled = 0` (default) → the Tier-2 single-layer
    /// path is untouched. Captured **Look**. (Tail-appended after `material_grad`;
    /// LAYOUT_VERSION 0x0280→0x0281.)
    pub material_layer2: [f32; 18],
    /// Overlay layer 2 albedo gradient (same layout as `material_grad`). Captured
    /// **Look**. (Part of LAYOUT_VERSION 0x0280→0x0281.)
    pub material_grad2: [f32; 8],
    /// Procedural material — **derived maps** (#472 Tier 3, the correlation
    /// principle). After the explicit channels bake, the visual derives a normal
    /// and/or AO map from the height field (or albedo luminance) so the set stays
    /// self-consistent. Layout: `[derive_normal (0/1), derive_ao (0/1),
    /// normal_source (0 height field / 1 albedo luminance), normal_strength,
    /// ao_strength, ao_radius (texels), _, _]`. All-zero derive flags (default) →
    /// no derived maps → the Tier-2 path is untouched. Captured **Look**.
    /// (Tail-appended after `material_layer3`; part of LAYOUT_VERSION 0x0280→0x0281.)
    pub material_derive: [f32; 8],
    /// Procedural material — **live / animation + displacement** (#472 Tier 5). The
    /// bake is live, so the material can flow/evolve; the baked **height** can drive
    /// vertex displacement. Layout: `[anim_on (0/1), anim_speed, anim_mode (AnimMode:
    /// 0 Drift / 1 Evolve / 2 Rotate), flow_x, flow_y, displace (height→vertex amount,
    /// 0 = off), audio_drive (RESERVED — the deferred #472 Tier-5 audio hook, inert
    /// until built), _]`. The visual injects a time term into the baked layers'
    /// offset/rotation (throttled ~30 Hz), and `cube.wgsl`'s vertex stage offsets the
    /// generator cubes along their normal by the sampled height. `anim_on = 0` +
    /// `displace = 0` (default) → the Tier-4 path is untouched (byte-identical).
    /// Captured **Look**. (Tail-appended after `material_derive`; LAYOUT_VERSION
    /// 0x0282→0x0283.)
    pub material_live: [f32; 8],

    // ═════════════════════════════════════════════════════════════════════════
    // #541 S2 Tier 1 — the **mindview** spine: the pane→lens selector.
    //
    // Reservation, not implementation (the Phase-B `MindFrame` precedent). The
    // three fields below are assigned **together, before the compositor exists**,
    // so WS-A (#484 — the pane grid), WS-C (#507/#505 — the lenses) and the #484
    // Tier-1 inside camera do not each append their own selector and disagree
    // about what "which view is pane 2 showing" means.
    //
    // **Nothing writes them yet**: `params.rs`/`preset.rs` pack zeros, so every
    // slot is 0 and the visual keeps drawing exactly what it draws today.
    //
    // Runtime-stamped, **NOT preset-captured** — matching the rest of the mind
    // block (`mind[8]`, `atlas[8]`, the `*_gen` counters). A pane layout is a
    // workspace arrangement, not a look; #484's default-inert contract says so
    // explicitly. Nothing is added to `PresetValues`, so old presets round-trip
    // untouched.
    // ═════════════════════════════════════════════════════════════════════════
    /// #541 S2 T1 — mindview **grid header** (the whole-window part of the
    /// selector). Layout:
    /// `[grid, focus, link_group, _, _, _, _, _]`.
    ///
    /// - `grid` — how the window tiles: `0` **Single** (today, byte-identical),
    ///   `1` 2-up horizontal, `2` 2-up vertical, `3` Quad. See
    ///   [`Shared::mindview_pane_count`] for the pane count each implies.
    /// - `focus` — which pane index owns keyboard focus and the pointer when a
    ///   gesture is not already captured (`PointerRouter`'s future consumer).
    ///   `0` = pane 0, i.e. today's only viewport.
    /// - `link_group` — WS-E linked selection: `0` = panes select independently
    ///   (today), `1` = all panes share one selection. Reserved semantics; the
    ///   selection *payload* is a separate contract, not this block.
    /// - `[3]` **mirror** (#554 T1) — does an editor want the **embedded viewport**'s frames?
    ///   `0` = publish nothing, non-zero = publish. The visual edge-reads it each frame and
    ///   creates/drops its `frame_ring` (in `organic-math-native`) writer accordingly; its own window is
    ///   unaffected either way.
    ///
    ///   **Not a user-facing toggle.** It shipped as one and stopped being one: the viewport is
    ///   native to the editor window rather than a pane you opt into, so the editor stamps `1`
    ///   unconditionally while it is running. The slot survives because the *cross-process*
    ///   question is still real — the visual has no other way to know an editor exists to mirror
    ///   into, and a projector-only session should not pay for a readback nobody reads. Turning
    ///   it off again when the pane is merely hidden (wrong tab, offscreen) is #554 Tier 3.
    ///
    ///   **Why here rather than a new field.** This block is by its own definition "the
    ///   whole-window part of the selector", and whether the window mirrors its scene into
    ///   the editor is exactly that. #541 Tier 1 reserved `[3..8]` for this family of
    ///   state, so spending one of those slots costs **no `LAYOUT_VERSION` movement and no
    ///   `Shared` growth** — which is the whole point of having made the reservation.
    /// - `[4..8]` reserved, written 0.
    ///
    /// All-zero (the default) = Single grid, pane 0 focused, no linking, no mirror → the
    /// single-viewport behaviour that exists today. (Tail-appended after
    /// `material_live`; LAYOUT_VERSION 0x0283→0x0284.)
    pub mindview: [f32; 8],
    /// #541 S2 T1 — the **per-pane** selection, row-major
    /// `[pane * MINDVIEW_PANE_SLOTS + slot]` for `pane < MINDVIEW_PANES`. Read and
    /// written through [`Shared::mind_pane`] / [`Shared::set_mind_pane`] so pane
    /// indexing lives in exactly one place. Per-pane slots:
    ///
    /// - `[0]` **lens** — *what subject* the pane shows: `0` **Scene** (whatever
    ///   the visual already renders — today's behaviour, and the reason all-zero
    ///   is inert), `1` Specimen skeleton (#367 T1), `2` Embedding galaxy
    ///   (#507 T1), `3` Residual trajectory (#507 T2), `4` Logit-lens wall
    ///   (#507 T3), `5` Expert bank (#505 T2), `6` Attention river (topo 7,
    ///   #484 T4), `7` Feature ticker (#409), `8` Analytics (#482). Higher values
    ///   are unassigned — a reader must treat an unknown lens as `0`.
    /// - `[1]` **layout** — *which sculpture* the subject is embedded in
    ///   (#484 T2): `0` Stacked slices (today), `1` Concentric shells, `2` Helix,
    ///   `3` Matrix wall. Unknown ⇒ `0`.
    /// - `[2]` **camera** — `0` external orbit (today), `1` inside the residual
    ///   stream (#484 T1), `2` follow the focused pane's camera.
    /// - `[3]` **detail** — one lens-defined scalar (e.g. the layer a logit-lens
    ///   wall pins, or a galaxy point budget). `0` = the lens's own default.
    /// - `[4..8]` reserved per pane, written 0.
    ///
    /// **Why four panes and a stride.** #484 Tier 3 / PRD §12 WS-A specify a
    /// 1 / 2-up / quad grid, so four is the ceiling the product actually asks
    /// for; the stride means a fifth pane is a mechanical `mindview_pane2[…]`
    /// append reusing the same decode, not a re-layout of this one.
    /// (Tail-appended after `mindview`; part of LAYOUT_VERSION 0x0283→0x0284.)
    pub mindview_pane: [f32; MINDVIEW_PANES * MINDVIEW_PANE_SLOTS],
    /// #541 S2 T1 — saved-**layout** load counter (the `hdr_gen` / `nn_gen` /
    /// `material_gen` pattern): the editor writes a pane arrangement to
    /// [`mindview_layout_path`] and bumps this; the compositor edge-detects the
    /// change and reloads. **Runtime-stamped, NOT a param, NOT preset-captured.**
    /// `0` = no saved layout has been loaded → the default arrangement.
    /// (Tail-appended after `mindview_pane`; part of LAYOUT_VERSION 0x0283→0x0284.)
    pub mindview_gen: u32,

    // ═════════════════════════════════════════════════════════════════════════
    // organon#217 T3 — PBR text look controls (`doc/pbr_text_engine.md` §14).
    //
    // T1 drew the glyph ring's tiles from one `const`, `glyph_ring::GlyphLook::DEFAULT`;
    // these three blocks lift that look, the held camera and T6's capsule core onto the
    // param chain so a preset can carry them. ⚠️ Every default below is **exactly the
    // constant it replaces** (invariant #4): `world.rs::glyph_look_from` on a default
    // `Shared` reproduces `GlyphLook::DEFAULT` field for field, pinned by test, so a
    // preset saved before T3 renders the grid it rendered yesterday.
    // ═════════════════════════════════════════════════════════════════════════
    /// organon#217 T3 — the **glyph look**. Layout, all in **cell units** (§5.1 —
    /// "express depth in cell units, never pixels"; `[0]` is the one world-unit anchor
    /// and everything else scales with it):
    /// `[cell_w, depth, gap, gain, faceplate, back_r, back_g, back_b,
    ///   margin, back_depth, default_fg, bevel, crown, profile, dark_tiles, _]`.
    ///
    /// - `[0]` **cell_w** — world units per column (1.0).
    /// - `[1]` **depth** — full-block extrusion, in column widths (0.18).
    /// - `[2]` **gap** — tile back face → backplane front face, the contact-shadow well (0.06).
    /// - `[3]` **gain** — emission gain in **SDR-white units** (§4; 3.0). ⚠️ The harness
    ///   (`legibility.rs`) measures linear light; this is the one colour convention here.
    /// - `[4]` **faceplate** — the near-black dielectric tint, one grey level (0.03).
    /// - `[5..8]` **backplane** RGB tint (0.06, 0.06, 0.065 — a shade lighter, faintly cool).
    /// - `[8]` **margin** — backplane beyond the grid, in column widths (1.5).
    /// - `[9]` **back_depth** — backplane thickness, in column widths (0.25).
    /// - `[10]` **default_fg** — grey used for a cell with a symbol but no fg colour (0.75).
    /// - `[11]` **bevel** — the tiles' rounded-box morph, `cube.wgsl::round_local` (0 =
    ///   sharp tile, exactly what T1 drew: it rode `Shared.bevel`, whose default is 0).
    ///   ⚠️ Its **own** lane, not `Shared.bevel`: that one is a Generator-bucket surface
    ///   control for the field's cubes, and a *Look* preset must carry the whole glyph
    ///   look; and on a 1×2×0.18 tile the same number rounds a different shape.
    /// - `[12]` **crown** — §5.1's face curvature: a per-fragment dome normal across each
    ///   tile face so light moves across the flat 95 % (0 = flat, today). Normal-only, no
    ///   geometry: the silhouette, the depth prepass and the RT/path-trace hit shading
    ///   are untouched.
    /// - `[13]` **profile** — T9's emission-profile strength, `cube.wgsl::tile_profile`
    ///   through `Uniforms.shape.z` while a ring is live (0 = flat, exactly the even glow
    ///   T1 drew; `tile_profile` is bit-for-bit 1.0 at zero). `glyph_profile`.
    /// - `[14]` **dark_tiles** — `> 0.5` gives every symbol-less cell a dark quarter-depth
    ///   tile at zero emission (`glyph_ring::LowerOptions::dark_tiles`; 0 = only lit cells
    ///   get tiles, T1). A flag on an `f32` lane, spelled 0/1 like `glyph_cam[0]`.
    ///   `glyph_dark_tiles`.
    /// - `[15]` reserved, written 0.
    ///
    /// Captured **Look**. (Tail-appended after `mindview_gen`; LAYOUT_VERSION 0x0285→0x0286.
    /// `[13]`/`[14]` were reserved lanes of that layout, written 0 until T9's wire took
    /// them — no layout move.)
    pub glyph: [f32; 16],
    /// organon#217 T3 — the **held camera** for a live glyph ring. Layout:
    /// `[hold, tilt_deg, zoom, _, _, _, _, _]`.
    ///
    /// - `[0]` **hold** — non-zero: while a ring is live the camera is an *absolute* rig
    ///   (the `substrate_rig` shape — centre, yaw 0, pitch = tilt, distance fitted to the
    ///   grid's bounds and the frame's FOV, roll 0), so the auto-orbit, the drag orbit and
    ///   the AABB follow are all bypassed and `pt_moved` is false every frame — which is
    ///   what lets T5's converge-on-hold actually converge. `0` = today: the ring inherits
    ///   whatever the orbit rig is doing, including the cube field's default distance.
    /// - `[1]` **tilt** — camera pitch in degrees (0 = straight on; a few degrees gives
    ///   §5.1's letterpress). `[2]` **zoom** — multiplier on the fitted distance (1 = the
    ///   grid fills the frame edge to edge; < 1 closer, > 1 further).
    /// - `[3..8]` reserved, written 0.
    ///
    /// Captured **Motion** (it is a camera). (Tail-appended after `glyph`; part of
    /// LAYOUT_VERSION 0x0285→0x0286.)
    pub glyph_cam: [f32; 8],
    /// organon#217 T6/T3 — the **coaxial capsule core** (`doc/pbr_text_engine.md` §11 route
    /// 1): `[core_frac, absorb, _, _]`. `[0]` is the inner emissive capsule's radius as a
    /// fraction of the outer (0 = off, pixel-identical to the pre-T6 frame); `[1]` the
    /// Beer–Lambert density per outer radius. Reaches `ParticleSystem::set_capsule_core`
    /// through the render frame's `Surface.capsule_core`. ⚠️ `ORGANON_CAPSULE_CORE` still
    /// **overrides** it when set — the seed stays, because no CPU test can prove the GPU
    /// draw read the param; a GPU session retires it once it has looked. Captured **Look**.
    /// (Tail-appended after `glyph_cam`; part of LAYOUT_VERSION 0x0285→0x0286.)
    pub capsule: [f32; 4],
}

/// #541 S2 T1 — panes the mindview selector can address.
///
/// Four, because #484 Tier 3 and PRD §12 WS-A specify a **1 / 2-up / quad** grid
/// and quad is four. Deliberately not over-reserved: `mindview_pane` is indexed by
/// a documented stride, so a wider grid is a fresh appended block using the same
/// decode rather than a renumbering of this one.
pub const MINDVIEW_PANES: usize = 4;

/// #541 S2 T1 — selector slots stored per pane (4 assigned + 4 reserved).
///
/// The reserve is deliberate: a lens that later needs a second scalar (a filter, a
/// palette override, an opacity for an overlaid lens) must not force a
/// `LAYOUT_VERSION` bump on a block whose whole purpose is to stop three
/// workstreams fighting over offsets.
pub const MINDVIEW_PANE_SLOTS: usize = 8;

/// One pane's decoded selection — the unit WS-A multiplexes and WS-C reads.
///
/// `Default` is `lens = Scene`, `layout = stacked`, `camera = external orbit`,
/// `detail = 0`, i.e. **exactly what the visual draws today**; that is the whole
/// zero-is-absent contract expressed as a type.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct MindPane {
    /// `mindview_pane[p*S + 0]` — which subject (see the field docs).
    pub lens: u32,
    /// `mindview_pane[p*S + 1]` — which spatial embedding (#484 T2).
    pub layout: u32,
    /// `mindview_pane[p*S + 2]` — external orbit / inside / follow-focus.
    pub camera: u32,
    /// `mindview_pane[p*S + 3]` — one lens-defined scalar; 0 = the lens default.
    pub detail: f32,
}

impl Shared {
    /// #554 T1 — is the **embedded viewport** (frame mirror) on? `mindview[3]`.
    ///
    /// One place decodes this slot, for the same reason `mind_pane` exists: a bare
    /// `mindview[3] != 0.0` scattered across two processes is how a reserved slot quietly
    /// acquires two meanings.
    pub fn mindview_mirror(&self) -> bool {
        self.mindview[3] != 0.0
    }

    /// The grid mode (`mindview[0]`), clamped to a known value. Unknown ⇒ Single,
    /// so a snapshot from a newer writer degrades to today's single viewport
    /// rather than to an undrawable tiling.
    pub fn mindview_grid(&self) -> u32 {
        let g = self.mindview[0];
        if g.is_finite() && g >= 1.0 && g <= 3.0 {
            g as u32
        } else {
            0
        }
    }

    /// How many panes the current grid actually shows: Single 1, 2-up 2, Quad 4.
    pub fn mindview_pane_count(&self) -> usize {
        match self.mindview_grid() {
            1 | 2 => 2,
            3 => MINDVIEW_PANES,
            _ => 1,
        }
    }

    /// Which pane has focus (`mindview[1]`), clamped into the live pane range —
    /// a focus index past the visible panes is a writer bug, not a reason for the
    /// reader to index out of bounds.
    pub fn mindview_focus(&self) -> usize {
        let f = self.mindview[1];
        let n = self.mindview_pane_count();
        if f.is_finite() && f >= 1.0 {
            (f as usize).min(n - 1)
        } else {
            0
        }
    }

    /// Decode pane `i`'s selection. Out-of-range `i` yields [`MindPane::default`]
    /// (= today's scene), so a caller iterating a wider grid than this build knows
    /// about still renders something honest.
    pub fn mind_pane(&self, i: usize) -> MindPane {
        if i >= MINDVIEW_PANES {
            return MindPane::default();
        }
        let b = i * MINDVIEW_PANE_SLOTS;
        let u = |v: f32| if v.is_finite() && v > 0.0 { v as u32 } else { 0 };
        MindPane {
            lens: u(self.mindview_pane[b]),
            layout: u(self.mindview_pane[b + 1]),
            camera: u(self.mindview_pane[b + 2]),
            detail: self.mindview_pane[b + 3],
        }
    }

    /// Encode pane `i`'s selection. Out-of-range `i` is a no-op (the reservation's
    /// reserved slots `[4..8]` are left untouched for whoever claims them).
    pub fn set_mind_pane(&mut self, i: usize, p: MindPane) {
        if i >= MINDVIEW_PANES {
            return;
        }
        let b = i * MINDVIEW_PANE_SLOTS;
        self.mindview_pane[b] = p.lens as f32;
        self.mindview_pane[b + 1] = p.layout as f32;
        self.mindview_pane[b + 2] = p.camera as f32;
        self.mindview_pane[b + 3] = p.detail;
    }
}

// Bumped: this merge appends BOTH main's PR #276 tube[4] AND #260 T1
// neural_surface[16] after neural_attn — a new layout, so a stale reader must reject
// it (0x0250 was each block's independent bump; 0x0251 marks the combined layout).
// 0x0252 then tail-appends `tube_profile: f32` (PR #276 follow-up: welded cross-section).
// 0x0253 tail-appends the #307 cinematic-camera blocks (cam_seq/cam_dolly/cam_clock/
// cam_audio) after `audiodip2` — a new layout, so a stale reader must reject it.
// 0x0254 tail-appends #307 Tier 2 `cam_frame[8]` (roll/FOV/framing) after `cam_audio`.
// 0x0255 tail-appends #307 Tier 3 `cam_story[24]` (storyboard) after `cam_frame`.
// 0x0256 tail-appends #256 Tier 0 `nrc[8]` (live neural radiance cache) after `cam_story`.
// 0x0257 tail-appends #256 Tier 1 `nrc2[4]` (NRC-guided sampling + firefly clamp) after `nrc`.
// 0x0258 tail-appends #256 Tier 2 `nrc3[4]` (cache GI supersedes DDGI + lit reflections) after `nrc2`.
// 0x0259 tail-appends #256 Tier 3 `nrc4[8]` (cache volumetrics + cached caustics) after `nrc3`.
// 0x025A tail-appends #325 `acoustic[16]` (acoustic Duo-Field generator) after `nrc4`
// (re-seated to the true tail on the main merge — it was 0x0256/after cam_story on its branch).
// 0x025B tail-appends #325 Tier 4 `acoustic2[8]` (cavity Chladni modes + intensity flux) after `acoustic`.
// 0x025C tail-appends #325 Tier 5 `acoustic3[8]` (cavity 3-D tween + per-axis audio breathe) after `acoustic2`.
// 0x025D tail-appends #339 Tier 1 `sonify[16]` (Duo-Field synthesis Sound card) + the
// runtime-written `voices[64]` block (played-note radiators), re-seated after `acoustic3`.
// 0x025E tail-appends #333 Tiers 1–2 `audiometer[16]` (calibrated LUFS/dBTP/LRA/correlation) after `voices`.
// 0x025F tail-appends #333 Tier 2 `audiospectrum[128]` (calibrated RTA band levels, dBFS) after `audiometer`.
// 0x0260 tail-appends #333 Tier 3 `analytical[8]` (Analyzer/Calibrated instrument mode) after `audiospectrum`.
// 0x0261 tail-appends #348 `fieldvol[8]` (Field Volume density-cloud source/exposure) AND #349
//        `colour[8]` (calibrated cross-cutting tint law) after `analytical` — one bump, both blocks.
// 0x0262 tail-appends #346 Tier 1 the Field Chamber blocks — `scopewave[260]` (triggered
//        oscilloscope display frame, runtime-written) + `chamber[16]` (the analyzer-panel look,
//        captured Look), re-seated to the true tail after `colour` on the main merge.
// 0x0263 tail-appends #349 Material Emissive `emissive[4]` after `chamber`.
// 0x0264 tail-appends the Gaussian Splatting surface `splat[8]` (SurfaceMode::Splat = 8;
//        the 3DGS primitive for forward synthesis — anisotropic Gaussians from the node
//        set, reusing instances/tints; captured Look) after `emissive`.
// 0x0265 tail-appends the Plexus surface `plexus[4]` (proximity-web controls) after `splat`
//        (re-seated after the splat merge; Plexus takes ordinal 9 since Splat took 8).
// 0x0266 tail-appends Plexus Tier 2 `plexus2[4]` + `plexus_node_mat[8]` + `plexus_edge_mat[8]`
//        (impostors + independent node/edge materials) after `plexus`.
// 0x0267 tail-appends Plexus Tier 3 `plexus3[4]` (beat-driven signal propagation) after
//        `plexus_edge_mat`.
// 0x0268 tail-appends the Plexus Tier-1 shape morph `plexus4[4]` (node cube→sphere +
//        edge square→circle) after `plexus3`.
// 0x0269 tail-appends the Splat Tier 3 look extension `splat2[4]` (`[solid, _, _, _]` —
//        solidity remaps the Gaussian toward an opaque disc) after `plexus4`.
// 0x026A tail-appends the Maxwell E↔B phase dial `mx_eb[4]` after `splat2`.
// 0x026B tail-appends the Plexus overlay `plexus_overlay[4]` (outer-shell web wrapped
//        around another surface — overlay_on / shell_scale / thickness / bins) after `mx_eb`.
// 0x026C tail-appends the #381 Tier 1 Field Engine `field[10]` (live coefficients)
//        + `field_gen: u32` (program-load counter) after `plexus_overlay`.
// 0x026D tail-appends the Density-Map Attractor `mapattractor[10]` (#380 Tier 1;
//        slots [8]/[9] = a_drive/b_drive, animation → parameter A/B) after `field_gen`.
// 0x026E tail-appends `origin_mode: u32` (Original cube-field Corner/Centered origin) after `mapattractor`.
// 0x026F tail-appends the Density-Map Attractor `maporbit[8]` (#380 Tier 2 beat-locked
//        parameter orbit — mode/loop_beats/Ra/Rb/fa/fb/psi/free_rate) after `origin_mode`.
// 0x0270 tail-appends the #317 Tier 1 AI-Performer runtime block `agent[8]`
//        ([agent_on, chat_gen, plan_gen, release_gen, …]; runtime-stamped, not a param)
//        after `maporbit`.
// 0x0271 tail-appends the #367 Tier 1 visible-mind `mind[8]` (runtime-stamped
//        mind_on / model_gen / topo_mode; the GGUF specimen) after `agent`.
// 0x0272 tail-appends the Density-Map Attractor `mapattractor2[4]` (#380 Tier 3 —
//        [c, d, color, _]; extra map coefficients + colour-by-dynamics mode) after `mind`
//        (re-seated after `mind` on the #389/#390 merge).
// 0x0273 tail-appends the #381 Tier 3 Field Engine `fieldsim[8]` (time-marched PDE:
//        preset/D/time_scale/feed/kill/potential/forcing/res) after `mapattractor2`
//        (re-seated after `mapattractor2` on the #393 merge).
// 0x0274 tail-appends #361 Tier 1 the Scene Kaleidoscope `kaleido[16]` (post-stage
//        kaleidoscopic fold of the resolved HDR scene, captured Look) after `fieldsim`
//        (re-seated to the true tail on the #363↔main sync).
// 0x0275 tail-appends #391 Tier 1 the Quantitative Instrumentation `instrument[16]`
//        (placeable field probes + energy ledger + Poynting-flux surface + CSV export,
//        captured Look; inert unless hud_on) after `kaleido`.
// 0x0276 tail-appends #391 Tier 1 the instrumentation-HUD presentation `instrument2[8]`
//        (rounded backing panel opacity/bevel + overall size + dock corner, captured Look)
//        after `instrument`.
// 0x0277 tail-appends #423 Tier 1 the atlas `atlas[8]` (runtime-stamped design-space
//        control: gen counter + on/roofline toggles; the editor scans a model library
//        into the atlas sidecar, the visual builds the constellation + roofline inset)
//        after `instrument2`. Not preset-captured; all-zero default = inert.
// 0x0278 tail-appends the #407 Tier A Field Playback `fieldclip_gen: u32` (a runtime-
//        stamped clip-load counter — the visual (re)loads a baked `FieldClip` when it
//        changes) after `atlas`.
// 0x0279 tail-appends the Neural CA (learned surrogate, Tier B #407) `nca_gen: u32`
//        (runtime-stamped model-load counter, NOT a param) after `fieldclip_gen`.
// 0x027A tail-appends #412 Tier 3 Phase 0 the FDTD Maxwell solver `fdtd[8]` (CPU Yee
//        stepper toggle on the Maxwell generator, feeds the Volume energy cloud,
//        captured Generator) after `nca_gen`.
// 0x027B tail-appends the node bevel `bevel: f32` (rounds the Original + Flow-Aligned
//        cube geometry cube→sphere via the cube shader's rounded-box vertex morph,
//        captured Generator; 0 = today's sharp cube) after `fdtd`.
// 0x027C tail-appends the Creature Engine `creature[8]` (#476 Tier 1: SDF-raymarched
//        synthetic sea creatures — form + scale + detail + swim + warp + rim + glow;
//        captured Generator) after `bevel`.
// 0x027D tail-appends `creature2[8]` (#476 Tier 2a: the metachronal wave — speed,
//        freq, sharp, amount; a beat-driven band of light along the body; captured
//        Generator) after `creature`.
// 0x027E tail-appends `creature_gen: u32` (#476 Tier 2b: JSON body-plan load counter,
//        runtime-stamped like nn_gen, NOT a param) after `creature2`.
// 0x0280 (main) is the MERGE of two independent 0x027F layouts: the creature branch
//        tail-appended `creature3[4]` (#476 Tier 2c anatomy overlay — on, opacity,
//        brightness; captured Generator), while the #472 Tier 1 procedural-material
//        foundation tail-appended `material[8]` (on + projection + scale; the per-map
//        quality knobs were dropped, material[3..6] reserved) AND the runtime-stamped
//        `material_gen: u32` texture-set load counter (`hdr_gen` pattern). Order after
//        `creature_gen`: `creature3[4]`, `material[8]`, `material_gen`.
// 0x0281 tail-appends the #472 Tier 2 procedural single-layer noise graph:
//        `material_layer[18]` (noise kind + channel + transform + fractal/warp +
//        remap + procedural toggle + bake res; captured Look, procedural = 0 →
//        byte-identical) + `material_grad[8]` (albedo gradient stops) after `material_gen`.
// 0x0282 tail-appends the #472 Tier 3 procedural layer stack: an overlay layer
//        `material_layer2[18]` + `material_grad2[8]` (enabled=0 default → Tier-2 path
//        untouched, blend modes) and the derived-maps block `material_derive[8]`
//        (normal/AO ← height or albedo; all-off default). Captured Look. After
//        `material_grad`. This is the MERGE of main's 0x0280 (with `creature3`) and the
//        #472 Tier 2/3 stack — a distinct version so a reader built for either rejects it.
// 0x0283 tail-appends the #472 Tier 5 live block `material_live[8]` (animation
//        enable/speed/mode + flow + height→vertex displace + a reserved audio-drive
//        slot; anim off + displace 0 default → byte-identical). Captured Look. After
//        `material_derive`.
// 0x0284 tail-appends the #541 S2 Tier 1 **mindview spine** — the pane→lens selector
//        that WS-A (#484) will multiplex and WS-C (#507/#505) will read: `mindview[8]`
//        (grid / focus / link_group + reserve), `mindview_pane[32]` (4 panes × 8 slots:
//        lens / layout / camera / detail + reserve), and the runtime-stamped
//        `mindview_gen: u32` (saved-layout load counter, the `hdr_gen` pattern). A
//        RESERVATION: nothing writes it, all-zero = Single grid + pane 0 showing the
//        scene the visual already draws → byte-identical. Runtime-stamped, NOT
//        preset-captured (matching `mind[8]`/`atlas[8]`). After `material_live`.
// 0x0285 changes NO field and NO offset — `Shared` is still 8512 bytes. What it
//        bumps is the *meaning of `seq`* (#618 Tier 0a): it is now a seqlock counter
//        (odd = a write is in flight, even = committed) rather than a plain
//        monotonic tick, and `Writer::write` publishes it after the body instead of
//        as part of one bulk copy. A version bump is the only honest way to signal
//        that, because the change is invisible to a size or offset check: an OLD
//        writer beside a NEW reader would park `seq` on an odd value half the time
//        and the reader would reject every frame. Bumping makes a mixed pair fail
//        the way mixed pairs already fail (defaults, loudly) instead of going blank
//        for reasons nothing explains. Close and reopen the visual after this lands.
// 0x0286 tail-appends the organon#217 T3 **PBR text look controls**: `glyph[16]` (the
//        glyph ring's look — cell width, extrusion, gap, emission gain, faceplate,
//        backplane tint / margin / depth, default fg, the tiles' own bevel, the face
//        crown; captured Look), `glyph_cam[8]` (hold / tilt / zoom — the held, fitted
//        camera a live ring needs for T5's converge-on-hold to converge; captured
//        Motion) and `capsule[4]` (T6's coaxial-glass core fraction + absorption;
//        captured Look), after `mindview_gen`. Every default is the constant it
//        replaces — `GlyphLook::DEFAULT`, bevel 0, crown 0, hold off, core 0 — so a
//        ring session and a no-ring session both render byte-identically to 0x0285.
//        Shared 8512 → 8624 bytes.
pub const LAYOUT_VERSION: u32 = 0x0_2_8_6; // "om" sentinel

/// Copy into `dst` every lane in which `mine` disagrees with `base`, leaving the rest alone.
///
/// **What it is for.** A front-end that owns only *part* of a look — Organon Console drawing
/// one of Organon's editor panels — has to put that part on top of a snapshot somebody else
/// composed, without a hand-written list of which lanes the panel happens to touch. Such a
/// list is the thing that rots: a param added to the panel would keep working in the editor
/// and quietly stop reaching the world here, with nothing to say so.
///
/// `base` is what the panel's own values *were* before anyone touched them, `mine` is what
/// they are now, and the difference between the two is exactly the set of lanes the panel has
/// an opinion about. Nothing else is named, so nothing else can be forgotten.
///
/// 🚨 **`base == mine` writes nothing, and that is the point.** An untouched panel is
/// byte-inert over any `dst` whatsoever — the repo's "new capability defaults to inert" rule
/// made structural rather than checked. [`overlay_changed_is_inert_when_nothing_moved`] pins
/// it.
///
/// ⚠️ **Lane granularity, not byte granularity.** A changed `f32` differs in one to four of
/// its bytes; copying only the *differing* bytes would splice two floats together and produce
/// a value neither side ever held. [`Shared`] is `Pod` and every one of its fields is a `u32`,
/// an `f32`, or an array of them, so a 4-byte word **is** a lane and `bytemuck` can hand us
/// the whole struct as `[u32]` — which is why this can be a dozen lines rather than a visitor
/// over ~200 fields. [`shared_is_a_whole_number_of_lanes`] is the guard on that assumption;
/// if `Shared` ever grows a field of another width, it fails rather than corrupting one.
///
/// ⚠️ **[`Shared::seq`] and [`Shared::layout_version`] are lanes like any other**, and they
/// are safe here for a reason worth stating rather than relying on: both are equal in `base`
/// and `mine` whenever the two come from the same build's packers, so neither can ever be in
/// the differing set. The alternative — special-casing them — would be a second place to
/// remember, and `seq` is stamped by [`Writer::write`] after this runs anyway.
pub fn overlay_changed(dst: &mut Shared, base: &Shared, mine: &Shared) {
    let base: &[u32] = bytemuck::cast_slice(std::slice::from_ref(base));
    let mine: &[u32] = bytemuck::cast_slice(std::slice::from_ref(mine));
    let dst: &mut [u32] = bytemuck::cast_slice_mut(std::slice::from_mut(dst));
    for i in 0..dst.len() {
        if base[i] != mine[i] {
            dst[i] = mine[i];
        }
    }
}

impl Default for Shared {
    /// The web app's helix defaults, so the visual shows something sensible
    /// before the plugin has written anything.
    fn default() -> Self {
        Shared {
            seq: 0,
            layout_version: LAYOUT_VERSION,
            loop_count: [1.0, 1.0, 3.0, 48.0],
            rot_amp: [6.0, 6.0, 8.0, 0.0],
            // per-axis rotation speed x,y,z + inc_scale (global) in w
            rot_mod: [0.6, 0.8, 1.0, 0.01],
            trans_amp: [1.0, 1.0, 2.0, 0.0],
            trans_mod: [1.6, 0.0, 9.0, 0.0],
            lighting: [1.0, 2.2, 0.6, 35.0, 40.0, 0.2, 1.0, 0.0],
            scale_amp: 0.2,
            rot_func: 0,
            trans_func: 0,
            scale_func: 3,
            animate: 1,
            pulse: 0,
            tempo: 120.0,
            pulse_depth: 0.6,
            // metallic 0, roughness 0.35, exposure 1, env_intensity 1, rot 0
            pbr: [0.0, 0.35, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            hdr_gen: 0,
            // not playing, phase 0, fallback 120bpm, no host yet
            transport: [0.0, 0.0, 120.0, 0.0],
            tempo_sync: 1,
            // path Off, gentle drift, modest kick, snappy decay
            camera: [0.0, 0.05, 0.08, 0.4],
            cam_amount: 1.0,
            // both routing slots off
            routing: [0.0, 0.0, 0.0, 0.0],
            surface_mode: 0, // Original
            // both modifiers off (amount 0); distortion/power/scale carry sane
            // defaults so dialing the amount up reads well immediately.
            surface_fx: [0.0, 0.3, 4.0, 0.0, 2.0, 0.0, 0.0, 0.0],
            hdr_output: 1,   // HDR on by default (visual falls back to SDR if unsupported)
            hdr_knee: 0.8,   // roll-off starts at 0.8× SDR white
            hdr_wide: 1,     // wide gamut on by default → Rec.2020 (0 = Rec.709)
            tonemap: 0,      // ACES (geometry)
            bg_tonemap: 1,   // AgX (environment backdrop — gentler on HDR panoramas)
            msaa: 4,         // 4× MSAA
            bg_visible: 1,
            bg_intensity: 1.0,
            env_tint_hue: 40.0,
            env_tint_amt: 0.0, // no tint
            ssao: [0.0, 1.5, 1.0, 0.025], // off
            audio: [0.0; 8],              // no signal
            pulse_source: 0,              // synthetic beat clock
            speed_pulse: [0.0, 5.0, 350.0, 0.0], // inert (amount 0)
            cont_shape: 0.0,                     // constant spin
            // radius 1.3 (> unit node spacing, so neighbours fuse), iso 0.6, soft edge
            metaball: [1.3, 0.6, 1.0, 0.0],
            // cycle off; ripple off (intensity 0) but speed/freq/sharp carry sane
            // defaults so raising intensity reads well immediately; geom radial.
            bio: [0.0, 0.0, 0.3, 1.0, 2.0, 0.0, 0.0, 0.0],
            // weave Auto; strands hidden.
            membrane: [0.0, 0.0, 0.0, 0.0],
            // RD off (intensity 0); spot-forming feed/kill, low scale (visible band).
            rd: [0.037, 0.06, 0.02, 0.0, 0.0, 0.0, 0.0, 0.0],
            generator: 0, // Original
            // Frenet: clean helix bundle (24 strands × 200 nodes, κ 0.35, τ 0.12,
            // amps 0 = no modulation, func Sin=0). Matches params defaults.
            frenet: [24.0, 200.0, 0.12, 0.35, 0.0, 1.0, 0.12, 0.0, 1.0, 0.15, 0.25, 0.0],
            // DNA: relaxed B-DNA, 48 bp, seq seed 1 (matches params defaults).
            dna: [
                1.0, 48.0, 10.5, 3.32, 10.0, 144.0, 0.0, 0.0, 24.0, 1.0, 0.16, 0.0, 0.0, 0.0, 0.0,
                0.0,
            ],
            // Attractor: Lorenz, 6 seeds, seed 1, spread 0.6, dt×1, trail 300,
            // head speed 1, scale 1, thickness 0.12 (matches params defaults).
            attr: [0.0, 6.0, 1.0, 0.6, 1.0, 300.0, 1.0, 1.0, 0.12, 0.0, 0.0, 0.0],
            // Harmonic: Y₂₀(0.5) + Y₃₀(0.25) pulsing bell, 48×64 grid (matches params).
            harm: [
                4.0, 0.5, 1.0, 8.0, 0.25, 0.5, 6.0, 0.0, 0.75, 6.0, 48.0, 64.0, 0.12, 0.0, 0.0, 0.0,
            ],
            // L-system: depth-4 fern, 25°, step 0.5, no sway, fully grown (matches params).
            ls: [0.0, 4.0, 25.0, 0.5, 0.0, 1.0, 1.0, 0.08, 0.0, 0.0, 0.0, 0.0],
            // Curl-noise: 12 particles, 200-step streamlines, free flow (matches params).
            cn: [12.0, 1.0, 4.0, 0.3, 200.0, 0.08, 1.0, 0.0, 0.1, 0.0, 0.0, 0.0],
            // Breath inert (amount 0); quick attack, slow heartbeat release.
            breath: [0.0, 8.0, 400.0, 0.0],
            // Polarization: 4×20 ray lattice, 64-node E helices, 150° bloom, no B
            // (matches params defaults) — a warm radiating corkscrew field.
            pol: [
                4.0, 20.0, 64.0, 14.0, 1.6, 2.2, 0.0, 0.0, 150.0, 0.0, 0.0, 0.12, 0.0, 0.0, 0.0,
                0.0,
            ],
            // Maxwell: one oscillating dipole on a 5×24 / 48-node lattice over the
            // full sphere → the rotating radiation lobe (matches params defaults).
            maxwell: [
                0.0, 0.0, 1.0, 1.0, 6.0, 1.57, 0.0, 0.0, 1.2, 3.0, 0.6, 0.12, 5.0, 24.0, 48.0,
                12.0, 180.0, 8.0, 200.0, 0.15, 40.0, 0.0, 0.0, 0.0,
            ],
            // Phyllotaxis: a 1500-node sunflower disk, 21 parastichy spirals, gentle
            // spin (matches params defaults).
            phyl: [
                0.0, 1500.0, 137.50776, 1.0, 21.0, 8.0, 2.0, 0.0, 0.5, 0.3, 0.1, 0.0, 0.0, 0.0,
                0.0, 0.0,
            ],
            // Mandelbulb: classic power-8 set, 8 iterations, cube-field size, a
            // 96-step march, gentle spin, no morph, orbit-trap colour, bailout 2.
            mandelbulb: [8.0, 8.0, 150.0, 96.0, 1.0, 0.0, 1.0, 2.0],
            // Terrain off by default. When enabled: peaks ~140 tall, snow above
            // 0.62 of height, gentle fog, a warm low sun, slow drift, riding 55
            // above the land, white-noise tile, not ridged, full brightness; perf
            // dials = 220 march steps, 7 march octaves, full resolution (divisor 1).
            terrain: [
                0.0, 140.0, 0.62, 1.0, 14.0, 145.0, 1.0, 1.0, 55.0, 0.0, 1.0, 0.0, 1.0, 1.0,
                220.0, 7.0, 1.0, 0.0, // 0-17: base + perf + palette
                0.0, 0.0, // emissive, day_speed (off)
                0.0, 0.25, 0.55, 0.4, // water: off, level 0.25, blue hue, ripple 0.4
                0.0, 0.0, // scatter, godray (off)
                1.0, // sun lights the scene when terrain on
                0.0, 0.0, 0.0, 0.0, 0.0, // spare (27-31)
            ],
            render_scale: 0.5, // native default (half-res, composite-upscaled)
            render_auto: 0,    // dynamic resolution off
            // Starfield off by default. When enabled: full brightness, gentle
            // twinkle, ~1.6 px stars, latitude 35°, slow sky drift, mag limit 6.5
            // (most of the catalog), modest colour saturation; sun on, HDR bright,
            // ~0.8° disc, warm. Stars only show once the sun sets (night factor).
            stars: [
                0.0, 1.0, 0.35, 1.5, 1.6, 35.0, 0.02, 6.5, 0.55, 1.0, 6.0, 0.8, 0.5, 0.0, 0.0, 0.0,
            ],
            // Particle Aura off (tier 0). The rest carry sane defaults so turning
            // it on reads well: 200k motes on a 32³ velocity grid, 3 s life, a
            // tight halo around the geometry, gentle beat burst + turbulence.
            particles: [
                0.0,   // tier (Off)
                200.0, // count (thousands) → 200k
                32.0,  // grid resolution
                1.0,   // speed
                3.0,   // lifetime (s)
                0.6,   // spawn radius
                0.06,  // size
                1.0,   // emissive
                0.0,   // ribbon (points)
                0.5,   // ribbon stretch
                0.0,   // hue shift
                0.3,   // beat burst
                0.1,   // drag
                0.2,   // turbulence
                1.0,   // alpha
                0.0,   // hide_generator (off)
            ],
            // Aura-Fluid defaults (used only at tier = Fluid): a moderate stir, strong
            // vorticity confinement (lots of eddies), gentle dissipation, a 24-iter
            // pressure solve, and a slow inflow decay so the wake lingers.
            fluid: [12.0, 6.0, 0.4, 24.0, 1.5, 0.0, 0.0, 0.0],
            // Jewel Box (#80): all three off / neutral by default, so the current
            // look + existing presets are byte-identical.
            ssr: [0.0, 1.0, 0.4, 0.5, 48.0, 2.0, 0.0, 0.0], // off; intensity/cutoff/thickness/steps tunable
            gi: [0.0, 1.0, 1.0, 0.0],                       // off
            glass_spec: [0.0, 0.0, 0.0, 3.0],               // dispersion 0 → today's glass
            // Voxel: additive, 96³, mid threshold, ~unit-thick strands, no glow,
            // full AO, gentle shadows, no posterize, no beat pump. Inert unless
            // surface_mode = 5 (Voxel). Matches params defaults.
            voxel: [96.0, 0.5, 1.0, 0.0, 0.0, 1.0, 0.6, 0.0, 0.0, 0.0, 0.0, 0.0],
            // Voxel GI off; strength 1, cone reach 0.5× the structure diagonal, a
            // little sky fill. Inert unless enabled (mip pyramid not built).
            voxel_gi: [0.0, 1.0, 0.5, 0.2],
            // KIFS: 12-fold symmetry, fold 0.65, 6 iterations, gentle spin/breathe,
            // flat projection, 18 rays, full ring/glow; Inversion engine + Spectral
            // palette, colour speed 0.08, no warp, tunnel flow 0.5, env off, 6
            // petals, contrast 1, sharpness 0.5 (matches params defaults).
            kifs: [
                12.0, 0.65, 6.0, 0.0, 1.0, 0.25, 1.2, 0.0, 18.0, 1.0, 1.0, 0.0, //
                0.0, 0.0, 0.08, 0.0, 0.004, 1.0, 6.0, 1.0, 0.5, 0.0, 0.0, 0.0, //
                // [24]e8_flow [25]relief_h [26]elev [27]steps [28]shine [29]3D-mode(Field)
                0.0, 0.5, 1.25, 96.0, 0.5, 0.0,
            ],
            // Boids: 120 agents, radius-6 cage, 64-frame trail, gentle gather, Fish
            // creature form (size 14, banking 0.6) — matches params defaults.
            boids: [
                120.0, 3.0, 1.2, 1.5, 1.0, 1.0, 3.0, 4.0, 64.0, 6.0, 0.3, 1.5, 1.0, 1.0, 30.0, //
                1.0, 14.0, 0.6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
            // Bell: physical mode off by default (harmonic stays closed-form);
            // depth 0.5, 8 iters, damping 0.99, θmax 1.7, stroke-rate 0.1 (slow).
            bell: [0.0, 0.5, 8.0, 0.99, 1.7, 0.1, 0.0, 0.0],
            // [enabled, turbidity, mie_g, sun_intensity, ground_albedo, exposure,
            //  aerial, rayleigh] — ON by default (the physical sky is the default
            //  environment; an explicitly loaded .hdr still overrides it).
            atmosphere: [1.0, 2.0, 0.76, 22.0, 0.3, 1.0, 1.0, 1.0],
            // [enabled, coverage, density, base_alt, thickness, march_steps, detail,
            //  drift_speed, hg, absorption, shadow_strength, ambient] — off by default.
            clouds: [0.0, 0.5, 1.0, 800.0, 500.0, 48.0, 0.5, 1.0, 0.55, 1.0, 0.7, 0.5],
            // [enabled, level, wind_speed, wind_dir, amplitude, choppiness, tile_size,
            //  foam, glitter, hue, depth, _] — off by default.
            ocean: [0.0, 0.0, 14.0, 45.0, 1.0, 1.0, 600.0, 1.0, 1.0, 0.54, 0.6, 0.0],
            hdr_vivid: 1.0, // full gamut stretch by default (max vividness)
            // Penrose P3, depth 4, radius-8 plane, slim edge rods (matches params).
            // [family, depth, scale, thickness, view(2=Extruded), height(×scale),
            //  height_mode, beat_infl, ripple_amt, ripple_freq, construct, phason,
            //  grid_n, ammann, hyp_p(7), hyp_q(3)]
            tessellation: [0.0, 4.0, 8.0, 0.06, 2.0, 0.25, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 6.0, 0.0, 7.0, 3.0],
            // Minimal surfaces (#127): a balanced gyroid, cube-field size, 6
            // channels, thin soap-film wall, no twist, 160 steps, full colour.
            // [family, scale, cells, iso, thickness, twist, steps, color, beat_iso,
            //  bend, uv_res, extent, bend_phase, turns, form_res, …] — slots 9..13
            //  drive the Phase-2 parametric families; slot 14 is the Phase-3 raymarch
            //  form-resolution divisor (1 = full res).
            minimal_surface: [
                0.0, 150.0, 6.0, 0.0, 0.06, 0.0, 160.0, 1.0, 0.0, 0.0, 96.0, 2.0, 0.0, 1.0, 0.5, 0.0,
            ],
            // Capture: Native (0) → render straight to the window; long edge 0 =
            // match the display (no downscale); default custom 1920×1080; black
            // letterbox; guides/lock off.
            capture: [0.0, 0.0, 1920.0, 1080.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            // Overlay: off by default; opacity 0.9, scale 1.0, all five zones on, a
            // dark translucent panel, white text. Matches the params default.
            overlay: [
                0.0, 0.9, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.04, 0.05, 0.08, 0.55, 0.95, 0.95, 0.97, 0.0,
            ],
            overlay_gen: 0,
            // Axes/box off by default (image unchanged); sensible defaults when enabled
            // ([12] = axis tube thickness/radius).
            axes: [
                0.0, 4.0, 0.0, 1.0, 1.0, 0.0, 4.0, 1.0, 0.5, 0.55, 0.7, 0.5, 0.12, 0.0, 0.0, 0.0,
            ],
            // Synchrotron (#150): mirrors the param defaults (radius 4, β 0.5,
            // 1 charge, 28² grid, ±14 plane, full near-field, gain 1, …; field-line
            // view: view 0=arrows, seeds 64, steps 220, ds 0.12, bound 40; volume
            // view: 7 depth layers; P5 legibility: reveal 0, invert off, radius 8).
            synchrotron: [
                4.0, 0.5, 1.0, 28.0, 14.0, 1.0, 1.0, 0.10, 0.5, 0.0, 0.0, 64.0, 220.0, 0.12, 40.0,
                7.0, 0.0, 0.0, 8.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
            // Post FX off by default (image byte-identical). Sensible values when
            // enabled: style None, 4 toon bands, no DoF/chroma/vignette/grain, a
            // neutral grade (sat=1, contrast=1, temp=0, gain=1), no feedback,
            // outline threshold 0.15. Matches the params defaults.
            fx: [
                0.0, 0.0, 4.0, 0.0, 0.5, 0.25, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.15, 0.0,
            ],
            // Emissive volume: radius 1.5, density 1, emission 1.5, absorption 0.6,
            // 96 steps. Only used when SurfaceMode::Volume (6) is selected.
            volume: [1.5, 1.0, 1.5, 0.6, 96.0, 0.0, 0.0, 0.0],
            // Temporal off by default (image byte-identical). Sensible values when
            // enabled: TAA blend 0.1 (history-heavy), sharpen 0.2; motion blur amount
            // 0.5 over 8 samples; stochastic off. Matches the params defaults.
            temporal: [0.0, 0.1, 0.2, 0.0, 0.5, 8.0, 0.0, 0.0],
            // SSGI off; intensity 1, radius 2, 4 rays when enabled.
            ssgi: [0.0, 1.0, 2.0, 4.0],
            // Shadows off by default; bias 0.0015, full strength when enabled.
            shadow: [0.0, 0.0015, 1.0, 0.0],
            // Voxel GI off; intensity 1, 4 rays, 12 steps when enabled.
            vxgi: [0.0, 1.0, 4.0, 12.0],
            // Reflection controls (#163) all 0 → today's chrome/glass/standard look.
            reflect: [0.0, 0.0, 0.0, 0.0],
            // Reflection probe: source EnvOnly (0 = today's look); box scale/height 1
            // (the field's own AABB), full parallax blend when switched to Parallax.
            refl_probe: [0.0, 1.0, 1.0, 1.0],
            // VXGI specular reflections off (strength 0); aperture 0.2, full reach,
            // 24 march steps when enabled.
            vxgi_spec: [0.0, 0.2, 1.0, 24.0],
            // Membrane screen-space FX on by default (turn off to skip the extra depth pass).
            membrane_fx: [1.0, 0.0, 0.0, 0.0],
            // Cinematic finishing (#167 T1): halation + lens flares both off (amount 0);
            // sensible threshold/width/warmth + ghost/halo/streak balance when raised.
            finishing: [0.0, 0.6, 1.0, 0.6, 0.0, 0.5, 0.4, 0.3],
            // Emissive cubes as lights off (enabled 0); intensity 1, radius 0.5× the
            // scene diagonal, up to 24 brightest cubes when enabled.
            manylight: [0.0, 1.0, 0.5, 24.0],
            // Vector field (#173): parabolic swirl (preset 0) on a 12³ lattice,
            // ±10 box, domain scale 0.5, soft length map, magnitude tint, gentle
            // evolve + 3-D z-lift. Tier 2 tail: arrows view, lattice seeding,
            // 96 bidirectional lines × 160 steps × ds 0.15, |F| colour, flow off,
            // line thickness 0.06. Matches the vf_* param defaults.
            vecfield: [
                0.0, 12.0, 12.0, 12.0, 10.0, 0.5, 1.0, 0.10, 0.0, 0.0, 0.3, 0.6, 0.0, 0.0, 0.0,
                96.0, 160.0, 0.15, 1.0, 0.0, 0.0, 1.0, 0.06, 0.0,
            ],
            // Builder (#173 T3): defaults reproduce the flagship — Fx = y²
            // (Square(y)), Fy = −x² (Square(x), gain −1), Fz = 0.5·sin z — so
            // switching the bank to Custom starts at the reel's field. Off
            // terms keep gain 1 + their own axis as the argument, so enabling
            // a func immediately does something. Direct operator, mix 0.5.
            vecbuild: [
                // Fx: Square(y) ·1 | off (arg x) | off (arg x)
                3.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0,
                0.0, 0.0,
                // Fy: Square(x) ·−1 | off (arg y) | off (arg y)
                3.0, -1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0,
                0.0, 0.0,
                // Fz: Sin(z) ·0.5 | off (arg z) | off (arg z)
                6.0, 0.5, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0,
                1.0, 0.0,
                // operator (0 = direct), Helmholtz mix, reserved
                0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
            // Fluid Ink (#182 Tier 1): off; [rate, radius, extinction, scatter,
            // emissive, anisotropy, dissipation, steps, maccormack, half_res, reveal].
            fluidvis: [0.0, 2.0, 1.5, 4.0, 1.0, 0.6, 0.45, 0.15, 96.0, 1.0, 1.0, 0.3],
            // Fluid medium Tier 2 (#182): everything inert — [boundaries 0,
            // buoyancy 0, heat_decay 0.3, detail 0, splash 0, dye_gate 0,
            // res 0 (= follow the aura grid dial), substeps 1].
            fluid2: [0.0, 0.0, 0.3, 0.0, 0.0, 0.0, 0.0, 1.0],
            // MLS-MPM liquid (#182 Tier 3a): off; [count_k 100, grid 64,
            // gravity 0 (weightless by default), stiffness 3, viscosity 0.05,
            // container 10, open 0, collide 1, stir 1, density 1,
            // threshold 0.35, hue 0.55, sat 0.35, reset_gen 0, substeps 2].
            liquid: [
                0.0, 100.0, 64.0, 0.0, 3.0, 0.05, 10.0, 0.0, 1.0, 1.0, 1.0, 0.35, 0.55, 0.35,
                0.0, 2.0,
            ],
            // Liquid block 2: offset 0 (centred), shape 0 (box), reveal 0 (off).
            liquid2: [0.0; 4],
            // #182 T4 coupling: all off; caustic sharpness 1.
            fluidgi: [0.0; 4],
            caustic: [0.0, 1.0, 0.0, 0.0],
            // Liquid material: follow the scene (0); own dials default to
            // water-ish (metallic 0, roughness 0.05, ior 1.33); ghost off.
            liqmat: [0.0, 0.0, 0.05, 1.33, 0.0, 0.0, 0.5, 0.0],
            liqmat2: [0.0; 8],
            // Z0NE rails (#187): 8 units/beat through a 6-radius bore,
            // 8-beat cells (ordinal 3), variance 0.5, 36-around × 4 rows/beat,
            // 24-beat horizon, beat ribs 0.6, thickness 0.5, up to 8 lobes,
            // spike 0.5, twist 0.1 turns/beat, swell 0.3, 6-beat horizon fade,
            // colour flow 0.05 cycles/beat; Tier 2: Throat archetype, golden
            // divergence, 2 shells, 13 parastichies; Tier 3: change every 8 bars
            // (ordinal 3), evolve 0. Matches the rl_* defaults.
            rails: [
                8.0, 6.0, 3.0, 3.0, 0.5, 0.0, 36.0, 4.0, 24.0, 0.6, 0.5, 8.0, 0.5, 0.1, 0.3,
                6.0, 0.05, 0.0, 137.50776, 2.0, 13.0, 0.0, 0.0, 0.0,
            ],
            // Hardware RT (#195 Tier 0): off, debug view Off.
            rt: [0.0; 8],
            // Refractive material: absorption 1 (only read when mat_type = 3).
            refrmat: [1.0, 0.0, 0.0, 0.0],
            // RT reflections (#195 T2): off; intensity 1, max-rough 0.4,
            // reach 2× diag, hit shadows on (all inert while [0] = 0).
            // T3: AO source GTAO (0), 2 RT rays (inert while source = 0).
            rt2: [0.0, 1.0, 0.4, 2.0, 1.0, 0.0, 16.0, 16.0],
            // Scenery (#187 pivot): off; material defaults mirror the main
            // look (Standard, metallic 0, roughness 0.35, glow 0.2, opacity 1,
            // IOR 1.45, Native palette, FX inert). Matches the sc_* defaults.
            scenery: [
                0.0, 0.0, 0.0, 0.0, 0.35, 0.2, 1.0, 1.45, 0.0, 0.0, 0.3, 4.0, 0.0, 2.0, 0.0,
                0.0,
            ],
            // RT GI (#195 T4): off; intensity 1, 2 rays, reach 2× diag, hit
            // shadows on. #200 T4½ p2: denoise off, amount 1 (inert while off);
            // GI rays default 16.
            rt3: [0.0, 1.0, 16.0, 2.0, 1.0, 0.0, 1.0, 0.0],
            // RT temporal (#200 T4½ p3/p4): off; feedback 0.9, beat relax 0.7;
            // variance off, max 32 samples, clamp γ 3.0 (all inert while [0] = 0).
            rt4: [0.0, 0.9, 0.7, 0.0, 32.0, 3.0, 0.0, 0.0],
            // Neural field (#200 T0): dark; seeds 1/2, no walk, feature scale 4
            // (all inert while [0] = 0).
            neural: [0.0, 1.0, 2.0, 0.0, 4.0, 0.0, 0.0, 0.0],
            // Anisotropy off: amount 0 (isotropic), rotation 0, overlay off, blend 1.
            aniso: [0.0, 0.0, 0.0, 1.0],
            // Axon Waveguide (#218): 24 fibres, length 24, bundle radius 4,
            // 64 samples/fibre, thickness 0.16, Ranvier nodes every 3 (dip 0.55),
            // pulse speed 0.6 / width 0.10, full stagger, no splay, seed 1,
            // mode LP01, mode amount 0; then bend 0.35 (edge scatter), curve 0.6
            // (a C-arc tract), tortuosity 0.3, DTI 0. Matches the ax_* param
            // defaults. Slots 18–23 reserved.
            axon: [
                24.0, 24.0, 4.0, 64.0, 0.16, 3.0, 0.55, 0.6, 0.10, 1.0, 0.0, 1.0, //
                0.0, 0.0, 0.35, 0.6, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
            // Surface lobes: clearcoat 1 / rough 0.1 / overlay off, sheen overlay
            // off, sheen 1 / rough 0.3 / tint 0. Full strength but inert until the
            // pure type or overlay is selected → Standard byte-identical.
            coat: [1.0, 0.1, 0.0, 0.0, 1.0, 0.3, 0.0, 0.0],
            // Terra (#206 Tier 2): a fjord — ridge 2, channel 1, valley width 3,
            // steep 0.7, no terracing, roughness 0.4, meander 0.6, water at −1
            // (on), clearance 1.5, detail 0.15. Matches the terra_* param
            // defaults. Inert until SceneryMode::Terra is selected.
            terra: [
                0.0, 2.0, 1.0, 3.0, 0.7, 0.0, 0.4, 0.6, -1.0, 1.0, 1.5, 0.15, 0.0, 0.0, 0.0, 0.0,
            ],
            // Neural field generator (#200 T1): size 120, detail 1.5, iso 0,
            // 96 steps, march 0.6, colour 0.8, walk rate 0 (static).
            neural2: [120.0, 1.5, 0.0, 96.0, 0.6, 0.8, 0.0, 0.0],
            // Neural strand form (#200 T1b): off (raymarch); 48×48 grid, extent
            // 2.5, displace 1.0 (inert while [0] = 0).
            neural3: [0.0, 48.0, 48.0, 2.5, 1.0, 0.0, 0.0, 0.0],
            // Body optics off: SSS thickness 0 (distortion-only), radius 1, interior 0.
            body: [0.0, 1.0, 0.0, 0.0],
            // Microstructure off: glitter 0 (density 12, sharpness 0.6), diffraction
            // 0 (freq 8), retro 0. Amounts 0 → byte-identical.
            micro: [0.0, 12.0, 0.6, 0.0, 8.0, 0.0, 0.0, 0.0],
            // Terra water (#206 Tier 3): calm glass water — mat Glass (2),
            // roughness 0.06, IOR 1.33, opacity 0.7, no glow, ripple 0.15 @ 0.6.
            water: [2.0, 0.06, 1.33, 0.7, 0.0, 0.15, 0.6, 0.0],
            // Water physics: absorb 1.4 (depth darkening), glitter 0.6 (sun
            // sparkle), reflect 0.15 (grazing lift) — realistic water by default.
            water2: [1.4, 0.6, 0.15, 0.0, 0.0, 0.0, 0.0, 0.0],
            pathtrace_on: 0, // path tracer off by default (per-display; P or the checkbox toggles)
            // Neural denoiser (#200 T5a): off, net 0.5, seed 1, omega 4.
            ndenoise: [0.0, 0.5, 1.0, 4.0, 0.0, 0.0, 0.0, 0.0],
            // Spectral emission off: fluorescence 0 (hue 0.33), incandescence 0
            // (3000K). Amounts 0 → byte-identical.
            emit: [0.0, 0.33, 0.0, 3000.0],
            // Screen-space refraction off: strength 0 (displace 0.5, inert while off).
            ssrefr: [0.0, 0.5, 0.0, 0.0],
            // Learned upscaler (#200 T5c): off; sharpen 0.5, seed 1.
            upscale: [0.0, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            // ReSTIR many-lights (#200 T5d): off (brightest-N).
            restir: [0.0, 0.0, 0.0, 0.0],
            // Neural Network (#226 Tier 1): a small-world ring — topology 3, 48
            // nodes, k = 6, 15% rewired, 4 layers (unused off Layered), seed 1,
            // extent 12, node size 0.5 / glow 1.2, edge thick 0.14 / bow 0.25 /
            // 16 samples, pulse speed 0.5 / width 0.12. Slots 14–15 reserved.
            neural_net: [
                3.0, 48.0, 6.0, 0.15, 4.0, 1.0, 12.0, 0.5, 1.2, 0.14, 0.25, 16.0, 0.5, 0.12, 0.0,
                0.0,
            ],
            // Neural Network edges/somas (#226 Tier 1.5): fibres 1 (single tube →
            // Tier-1 geometry), bundle radius 0.4, Ranvier dip 0.6 over 5 nodes,
            // dendrite 0 (plain blob) with 6 sprouts. Inert until fibres > 1 / dendrite > 0.
            neural_edge: [1.0, 0.4, 0.6, 5.0, 0.0, 6.0, 0.0, 0.0],
            // Maxwell energization off (energize 0): gain 1, knee 4, ember hue 0.08;
            // Tier 2 finite antenna off (slot 5), rod length 6 (slot 4); Tier 3 fluid
            // dye injection off (slot 6). Inert → byte-identical.
            maxenergy: [0.0, 1.0, 4.0, 0.08, 6.0, 0.0, 0.0, 0.0],
            // Tier 2 signal propagation OFF (mode 0 = Tier-1 look); dials seeded:
            // threshold 0.5, conduction 8 units/beat, refractory 1, decay 0.6,
            // deposit 0.6, stim rate 2/beat, motes off.
            neural_net2: [0.0, 0.5, 8.0, 1.0, 0.6, 0.6, 2.0, 0.0],
            nn_gen: 0,
            // Tier 4 MLP look: sign colour 0.8, sparsify 0.05, layer gap 1.0, static input.
            neural_mlp: [0.8, 0.05, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            // Tier 5 attention: layer 0, head 0, edge threshold 0.05, 24 tokens,
            // reveal rate 0.5 tok/beat, sweep off, row layout.
            neural_attn: [0.0, 0.0, 0.05, 24.0, 0.5, 0.0, 0.0, 0.0],
            // welding off (segmented Swept Tubes unchanged); caps on, half-dome round
            tube: [0.0, 1.0, 0.5, 0.0],
            // #260 Tier 1 Neural Tissue: soma size 1.0, no anisotropy, bouton 0.35,
            // membrane SSS/iridescence inert (0). #260 Tier 2 morphology [5..10]:
            // dendrite density 0 (inert — no arbor), reach 1.0, Rall taper 0.62,
            // pyramidal (0), spines 0. Slots [10..13] Tier 3 (myelin off), [13..16]
            // Tier 4 synapse (cleft/glow/vesicles all 0, inert).
            neural_surface: [1.0, 0.0, 0.35, 0.0, 0.0, 0.0, 1.0, 0.62, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            // #260 Tier 4 tissue context: glia + capillary off (byte-identical).
            neural_surface2: [0.0; 8],
            // #275 brain model: folded cerebrum (depth 0.12, ~5 gyri), 0.1 fissure,
            // local k=8, cerebellum 0.14. T2 white matter [5..8]: assoc 0.25, corpus
            // callosum 0.4, subcortical 0.2. T3 [8..10]: highlight off, target 0. T4
            // [10..12]: stim off (0), ~2 pulses/beat. [12] signal swell 0 (anatomy still).
            brain: [0.12, 5.0, 0.1, 8.0, 0.14, 0.25, 0.4, 0.2, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0],
            // Physical thin-film (#258 T1): thickness 0 → the model is OFF and the
            // shader keeps the existing cosine-hack path (byte-identical default).
            // Non-inert example seeds: var 0.3, film IOR 1.33 (soap), drainage 0.5.
            thinfilm: [0.0, 0.3, 1.33, 0.5],
            // Path-tracer dielectric BTDF (#258 T2) OFF: enable 0 (diffuse-only,
            // byte-identical), absorption 0 (no Beer–Lambert). Inert defaults.
            ptglass: [0.0, 0.0, 0.0, 0.0],
            tube_profile: 1.0, // circle (original welded look)
            // Lens (#258 T3): focal 1.0, aperture 0.6, thickness 0.25, biconvex (plano 0),
            // world scale 150, 128 march steps. Inert unless the Lens generator is selected.
            lens: [1.0, 0.6, 0.25, 0.0, 150.0, 128.0, 0.0, 0.0],
            // Spectral (#258 T4): OFF (spectral_on 0 → RGB tracer); Abbe 40 (crown-ish
            // dispersion when enabled), 3 secondary wavelengths.
            spectral: [0.0, 40.0, 3.0, 0.0],
            // Demo scene bench (#288): Cornell box (scene 0), unit scale, inner
            // objects on, fixed reference framing, key 1.0, smooth-metal roughness
            // 0.15, 4 rows/side, still. Inert unless generator = Demo.
            demo: [0.0, 1.0, 1.0, 1.0, 1.0, 0.15, 4.0, 0.0],
            // Audio-driven dipole (#248): drive OFF, amount 1 (unity RMS→drive),
            // floor 0.1 (a dim idle field on silence); T2 multipole OFF, gentle
            // wavelength spread 0.25, colour-by-band 0.7; T3 stereo lean 0.5, pitch
            // rate 0.5 ([6..8]) — matching the `ad_stereo`/`ad_pitch` param defaults.
            audiodip: [0.0, 1.0, 0.1, 0.0, 0.25, 0.7, 0.5, 0.5],
            // Field-force drive (#248): OFF, gain 1, contrast 1 (= current
            // direction-drive + flat energization), stir rate 0.3 Hz → byte-identical
            // until enabled.
            mxforce: [0.0, 1.0, 1.0, 0.3],
            // Acoustic pump + beat coupling (#248): pump 0 (off), beat spin force 0
            // (manual reversal), pump core size 3, spin slowdown 1.5/s → the beat
            // couplings are inert until dialed in.
            mxforce2: [0.0, 0.0, 3.0, 1.5],
            // Beat mode crossfade (#248): −1 = turbine (the original), ring freq 2 Hz.
            mxforce3: [-1.0, 2.0, 0.0, 0.0],
            // Shaded particle beads (#298 Tier 1): OFF (additive sparks), metallic 0.9,
            // roughness 0.2 (shiny chrome/pearl droplets when enabled) → byte-identical
            // until `beads` is turned on.
            pbeads: [0.0, 0.9, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0],
            // Audio Tier 3 (#248): waveform shells off → byte-identical until dialed in.
            audiodip2: [0.0, 0.0, 0.0, 0.0],
            // Per-material HSV (#305 T1): generator + scenery, both identity
            // (hue 0, cycle 0, sat 1, val 1) → byte-identical until dialed.
            matcol: [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0],
            // Bead HSV (#305 T1): identity.
            pbeads2: [0.0, 0.0, 1.0, 1.0],
            // Photon-mapped caustics (#258 T5) OFF: enable 0 (no photon pass,
            // byte-identical tracer); budget 128k photons, intensity 1, gather 2 px
            // seed sensible values for when it's switched on.
            ptcaustic: [0.0, 128.0, 1.0, 2.0],
            // Live-sky cloud reflections (#305 T2): OFF; cover 0.55, drift 0.08/beat,
            // strength 0.7 → inert until enabled.
            skyrefl: [0.0, 0.55, 0.08, 0.7],
            // Cinematic camera (#307 Tier 1): sequencer off, 8-bar shots, Series,
            // Glide; dolly depth 0 (inert); Host tempo, beat momentum on, 1-bar
            // glide; no detected BPM → all byte-identical to today's camera.
            cam_seq: [0.0, 8.0, 0.0, 0.0],
            cam_dolly: [4.0, 0.0, 0.0, 4.0],
            cam_clock: [0.0, 1.0, 1.0, 0.0],
            cam_audio: [0.0, 0.0, 0.0, 0.0],
            // Tier 2 framing: roll 0, FOV 45, no dolly-zoom, no hold, no phrase-lock;
            // seq_mix 1 = fully sequencer (dial down to blend the orbit-cam back in).
            cam_frame: [0.0, 45.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            // Tier 3 storyboard: disabled; the demo playlist matches params defaults
            // (HCircle 8 / Spiral 8 / Figure-8 4 / Boom 8). Off → sequencer unchanged.
            cam_story: [
                0.0, 4.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, // header
                1.0, 8.0, 1.0, 0.0, // shot 0: HCircle, 8 bars, r1.0
                4.0, 8.0, 1.3, 0.0, // shot 1: Spiral, 8 bars, r1.3
                3.0, 4.0, 0.8, 0.0, // shot 2: Figure-8, 4 bars, r0.8
                5.0, 8.0, 1.5, 0.0, // shot 3: Boom, 8 bars, r1.5
            ],
            // Live radiance cache OFF by default (byte-identical). confidence 0.5,
            // lr 0.02, omega 4, terminate at bounce 2, 8 train samples/frame, seed 1.
            nrc: [0.0, 0.5, 0.02, 4.0, 2.0, 8.0, 1.0, 0.0],
            // Cache RT-stack synergies OFF by default (byte-identical). guiding over
            // 4 candidates, firefly clamp at 8× the cache mean.
            nrc2: [0.0, 4.0, 0.0, 8.0],
            // Cache light-field uses OFF by default (byte-identical). GI strength 1,
            // reflect-terminate off.
            nrc3: [0.0, 1.0, 0.0, 0.0],
            // Cache hard transport OFF by default (byte-identical). volume density
            // 0.15, 16 steps, strength 1; caustic gain 1.
            nrc4: [0.0, 0.15, 16.0, 1.0, 0.0, 1.0, 0.0, 0.0],
            // #325 acoustic: a dipole on a 5×24 lattice; geometry = pressure
            // (blend 0), aura = velocity (aura_blend 1); beat pump off. Mirrors the
            // params defaults so the two Default paths agree.
            acoustic: [
                1.0, 1.5, 0.5, 1.5, 1.5, 0.3, 0.0, 0.0, // kind..norm_field
                5.0, 24.0, 48.0, 8.0, 180.0, 0.1, 1.0, 0.0, // rings..beat_pump
            ],
            // #325 Tier 4: Radiating model + intensity 0 → identical to Tiers 1–3;
            // cavity defaults a (2,2,1) mode in a box of half-extent 8. Mirrors params.
            acoustic2: [0.0, 2.0, 2.0, 1.0, 0.0, 8.0, 0.0, 0.0],
            // #325 Tier 5: tween 0.6 (soft mode-glide) + per-axis audio breathe 0 (off).
            // Mirrors params defaults; audio gains 0 → inert until dialled + the drive on.
            acoustic3: [0.6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            // #339 sonify: synth off; generative mode; monopole; lens pivot A2
            // (~110 Hz) rendering at 0.5 Hz, slope 1/3 (compress the keyboard);
            // stereo probe pair ±9 cm out front; camera-ride off. Mirrors the
            // params defaults so the two Default paths agree.
            sonify: [
                0.0, 0.0, 0.0, 110.0, 0.5, 0.34, 0.34, 0.0, // on..quantize
                -0.09, 0.0, 1.2, 0.09, 0.0, 1.2, 0.0, 0.0, // probeL, probeR, cam, _
            ],
            voices: [0.0; 64], // runtime-written each block; no note sounding
            // #333: calibrated meters — silence/off until the plugin writes measured values.
            audiometer: [0.0; 16],
            // #333 T2: calibrated RTA — silence floor until measured.
            audiospectrum: [-120.0; 128],
            // #333 Tier 3: Expressive; streaming targets (−14/−50 LUFS, −1 dBTP). Mirrors params.
            analytical: [0.0, -14.0, -50.0, -1.0, 0.0, 0.0, 0.0, 0.0],
            // #348 Field Volume: Legacy source (0 → today's node-metaball Volume,
            // byte-identical), neutral smoothing 1, exposure 0 dB, calibrate off,
            // gain 1. Mirrors params defaults.
            fieldvol: [0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            // #349 Calibrated colour: Aesthetic (0 → today's tint, byte-identical),
            // dB window −60..0 dBFS, Turbo LUT, Auto source, full amount 1. Mirrors params.
            colour: [0.0, -60.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            // #346 Field Chamber: no scope frame captured yet (runtime-written).
            scopewave: [0.0; 260],
            // #346 Field Chamber: panels off → byte-identical. Sane look defaults so
            // toggling `panels_on` reads well immediately: both walls on, 0.85 opacity,
            // 0.9 wall fill, unity scope amp, −60..0 dBFS spectrum window, Standard
            // material (metallic 0.8 / rough 0.25) for the Impostor style, fixed world
            // axes, thin ribbon, no emissive. Mirrors the params defaults.
            chamber: [
                0.0, 0.0, 1.0, 1.0, 0.85, 0.9, 1.0, -60.0, // on..db_floor
                0.0, 0.8, 0.25, 0.0, 0.03, 0.0, 0.0, 0.0, // material..reserved
            ],
            // Emissive off → byte-identical (glow-only self-emission as before).
            emissive: [0.0; 4],
            // Splat surface (only consumed when surface_mode == 8): Tier 2 lit
            // 2DGS disks by default (radius/opacity/falloff/mode/cutoff/aniso).
            splat: [0.55, 0.85, 1.0, 1.0, 0.003, 1.0, 1.0, 0.35], // …, scatter=1, jitter=0.35
            // Plexus: radius 1.6× spacing, up to 8 links/node, thin struts, small
            // node markers (multipliers of node spacing). Inert unless surface_mode = 9.
            plexus: [1.6, 8.0, 0.07, 0.24],
            // Tier 2 impostors off by default (Tier 1 cubes). Edges on; node sphere
            // radius 0.35× spacing, edge tube radius 0.09× spacing.
            plexus2: [0.0, 1.0, 0.35, 0.09],
            // Node material: near-white, slightly metallic, moderate emissive glow.
            plexus_node_mat: [0.0, 0.1, 0.4, 1.45, 0.0, 0.0, 1.0, 0.6],
            // Edge material: matte, cool-blue tinted, gentle glow — independent of nodes.
            plexus_edge_mat: [0.0, 0.0, 0.6, 1.45, 0.58, 0.4, 1.0, 0.3],
            // Signal propagation off; 1 shell/beat, gain 1.5, width 0.18.
            plexus3: [0.0, 1.0, 1.5, 0.18],
            // Shape morph: node + edge both 1 = sphere nodes / circular struts
            // (0 recovers the old sharp cube / square look).
            plexus4: [1.0, 1.0, 0.0, 0.0],
            splat2: [0.0, 0.0, 0.0, 0.0], // solidity 0 = soft Gaussian (unchanged)
            // E↔B phase 0° = far-field / in-phase (the plain lock) → byte-identical.
            mx_eb: [0.0, 0.0, 0.0, 0.0],
            // Overlay off by default; shell grows 1.15×, keeps the outer 0.2 radial
            // band per directional cell over a 12×12 direction grid.
            plexus_overlay: [0.0, 1.15, 0.2, 12.0],
            // Field Engine: Auto kind, Coulomb preset, k=1, extent 6, a=b=1, density
            // 12, gain 1, thickness 0.12 (mirrors the params defaults). Inert unless
            // generator == 24.
            field: [0.0, 0.0, 1.0, 6.0, 1.0, 1.0, 12.0, 1.0, 0.12, 0.0],
            field_gen: 0,
            // #380 Density-Map Attractor: Complexus, a=b=1.5, 60K points, warmup 50,
            // scale 12, size 0.08, intensity 1 (matches params defaults). Inert unless
            // generator = 25.
            mapattractor: [0.0, 1.5, 1.5, 60.0, 50.0, 12.0, 0.08, 1.0, 0.0, 0.0],
            origin_mode: 0, // Corner — grid corner at the origin (historical look)
            // #380 Tier 2 parameter orbit: Linear (the Tier-1-compatible default →
            // byte-identical field with the mapattractor drives at 0), loop 16 beats,
            // Ra=Rb=1.5, fa=1/fb=2 (a figure-8 Lissajous once the user switches mode),
            // ψ=π/2, free-run rate 0.05.
            maporbit: [1.0, 16.0, 1.5, 1.5, 1.0, 2.0, std::f32::consts::FRAC_PI_2, 0.05],
            agent: [0.0; 8], // AI-Performer runtime block; inert until engaged (#317 T1)
            // Visible-Mind specimen (#367 T1): inert until a model is loaded.
            mind: [0.0; 8],
            // #380 Tier 3: c = d = 1.5 (matches a/b; inert unless the map reads them),
            // colour mode StepSpeed (0 → byte-identical render). Slot [3] reserved.
            mapattractor2: [1.5, 1.5, 0.0, 0.0],
            // #381 Tier 3 Field Engine PDE sim: Off (byte-identical default), D=1,
            // time_scale 1, Gray-Scott feed 0.037 / kill 0.06 (spots), Schrödinger
            // potential 1, forcing 0, res 64 (mirrors the params defaults). Inert
            // unless generator == 24 and preset != Off.
            fieldsim: [0.0, 1.0, 1.0, 0.037, 0.06, 1.0, 0.0, 64.0],
            // Scene Kaleidoscope off (enabled 0) → the HDR buffer is untouched. Sane
            // look defaults so toggling it on reads well immediately: 6-fold, FullFrame,
            // gentle spin, unit zoom, centred, full mix, no twist/tint, mild seam soften.
            kaleido: [
                0.0, 6.0, 0.0, 0.1, 0.0, 1.0, 0.0, 0.0, // enabled..center_y
                1.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, // mix..reserved
            ],
            // #391 Tier 1 instrumentation: HUD off (byte-identical), but sane
            // measurement geometry so toggling it on reads immediately — probe at
            // a point off-axis, a 4-unit ledger box on 12³ samples, a 2-unit flux
            // patch facing +X, CSV logging off.
            instrument: [
                0.0, 1.0, 2.0, 0.0, 0.0, 1.0, 4.0, 12.0, // hud..ledger_res
                1.0, 2.0, 0.0, 0.0, 2.0, 0.0, 16.0, 0.0, // flux_on..csv_log
            ],
            // HUD presentation: a semi-opaque panel with soft-rounded corners at unit
            // size, docked top-left. Match params defaults (the Default→Shared golden).
            instrument2: [0.55, 0.35, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            // #423 atlas: runtime-stamped, inert by default (no models scanned).
            atlas: [0.0; 8],
            // #407 Tier A: no clip loaded yet (runtime-stamped by the plugin).
            fieldclip_gen: 0,
            // Neural CA model-load counter — 0 (no model loaded → builtin default).
            nca_gen: 0,
            // #412 Phase 0: FDTD off (byte-identical). Sane sim defaults so toggling
            // it on runs immediately: 64³ grid, Pulse source, ω 8, unit drive, 4 CFL
            // sub-steps/frame, an 8-cell sponge, domain half-extent 12.
            fdtd: [0.0, 64.0, 0.0, 8.0, 1.0, 4.0, 8.0, 12.0],
            bevel: 0.0,
            // #476 Tier 1 Creature Engine: form 0 (bell jelly), world scale 120,
            // 128 raymarch steps, swim 1×, warp amp 0.06 / freq 4, rim 0.6, glow 1.
            creature: [0.0, 120.0, 128.0, 1.0, 0.06, 4.0, 0.6, 1.0],
            // #476 Tier 2a metachronal wave: speed 1×, 3 bands, sharpness 2.5,
            // amount 0 (off → the Tier-1 look, byte-identical).
            creature2: [1.0, 3.0, 2.5, 0.0, 0.0, 0.0, 0.0, 0.0],
            creature_gen: 0, // #476 Tier 2b: no JSON plan loaded (use the built-in form)
            // #476 Tier 2c anatomy overlay: off (byte-identical), opacity 1, brightness 1.
            creature3: [0.0, 1.0, 1.0, 0.0],
            // #472 Tier 1: materials OFF (byte-identical). [on, projection, scale,
            // reserved×5] — the maps feed the unified pipeline directly, so indices
            // 3–6 (was normal/AO/rough/metal strength) are reserved (0).
            material: [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            // No material folder loaded yet (runtime-stamped by the plugin).
            material_gen: 0,
            // #472 Tier 2: procedural OFF (byte-identical). Sane FBM→albedo defaults
            // so toggling on reads immediately: 5-octave FBM at 4 tiles, lacunarity
            // 2 / gain 0.5, identity contrast/gamma/remap, 512px bake.
            material_layer: [
                3.0, 0.0, 4.0, 0.0, 0.0, 0.0, 5.0, 2.0, // kind..lacunarity
                0.5, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0, // gain..seed
                0.0, 512.0, // procedural (OFF), bake_res
            ],
            // Dark→light gradient (linear RGB) for the albedo route.
            material_grad: [0.04, 0.04, 0.05, 0.0, 0.80, 0.76, 0.70, 0.0],
            // #472 Tier 3: overlay layers 2/3 DISABLED (Tier-2 path untouched). Sane
            // defaults so enabling reads immediately — layer 2 = FBM→roughness (Normal
            // blend), layer 3 = Worley→height (Multiply blend); [16]=enabled, [17]=blend.
            material_layer2: [
                3.0, 1.0, 6.0, 0.0, 0.0, 0.0, 4.0, 2.0, // kind..lacunarity
                0.5, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0, // gain..seed
                0.0, 0.0, // enabled (OFF), blend (Normal)
            ],
            material_grad2: [0.04, 0.04, 0.05, 0.0, 0.80, 0.76, 0.70, 0.0],
            // #472 Tier 3: derived maps OFF (Tier-2 path untouched). normal_source
            // height, unit strengths, 2-texel AO radius so enabling reads immediately.
            material_derive: [0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 0.0, 0.0],
            // #472 Tier 5: animation OFF + no displacement (byte-identical). Sane
            // defaults so enabling reads immediately — Drift at 0.1 speed, flow +x.
            material_live: [0.0, 0.1, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            // #541 S2 T1 mindview spine — the ONE block whose default must be all
            // zeros rather than "sane defaults": zero IS the semantic (Single grid,
            // pane 0 focused, pane 0 showing the scene the visual already draws).
            // Any non-zero default here would silently turn the reservation on.
            mindview: [0.0; 8],
            mindview_pane: [0.0; MINDVIEW_PANES * MINDVIEW_PANE_SLOTS],
            mindview_gen: 0,
            // organon#217 T3: exactly `glyph_ring::GlyphLook::DEFAULT` (T1's one const),
            // plus bevel 0 (T1 rode `Shared.bevel`, default 0) and crown 0 (new, inert).
            // `world::glyph_look_from` pins the round trip.
            glyph: [
                1.0, 0.18, 0.06, 3.0, // cell_w, depth, gap, gain
                0.03, 0.06, 0.06, 0.065, // faceplate, backplane rgb
                1.5, 0.25, 0.75, 0.0, // margin, back_depth, default_fg, bevel
                0.0, 0.0, 0.0, 0.0, // crown, reserved ×3
            ],
            // Hold off (the ring inherits the orbit rig, as T1 did), tilt 0°, zoom 1.
            glyph_cam: [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            // T6's inert gate: core fraction 0 → `shade_bead` exactly as before.
            capsule: [0.0; 4],
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The IPC **namespace** (#483 Tier 1 — the Organon Mind edition fork)
//
// Every mmap + sidecar below lives in `$TMPDIR` under one shared filename prefix.
// Historically that prefix was hard-coded `organic-math`; it is now the *edition's*
// namespace, so an **Organon Mind** session and a full **Organon** session can run
// side by side without stomping each other's snapshot (the one cross-product
// invariant #483 calls out).
//
// Two things resolve it, in order:
//   1. `$ORGANON_IPC_NS` — a runtime override. This is how the **one** visual binary
//      serves both products: the visual is compiled once (feature-off, so its own
//      `EDITION` is `Full`), and the editor that spawns it passes its namespace in
//      the child environment. Same for a hand-run `organic-math-mind-runtime`.
//   2. Otherwise `edition::EDITION.ipc_namespace()` — `organic-math` for full Organon
//      (byte-identical to every path this file has ever produced), `organon-mind` for
//      the Mind edition.
//
// Resolved **once** per process (a `OnceLock`): a mid-run namespace change would tear
// a live writer/reader pair apart, and every path function must agree with itself.
// ─────────────────────────────────────────────────────────────────────────────

/// Accept a namespace only if it is a safe single `$TMPDIR` **filename component**:
/// non-empty ASCII alphanumerics / `-` / `_`. Anything else (a path separator, `..`,
/// a shell metacharacter, whitespace) is rejected and we fall back to the edition's
/// own namespace — an env var must not be able to redirect our mmaps out of `$TMPDIR`.
pub fn sanitize_ns(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || t.len() > 64 {
        return None;
    }
    if !t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    Some(t.to_string())
}

/// This process's IPC namespace — the `$TMPDIR` filename prefix shared by every mmap
/// and sidecar. See the block comment above.
pub fn namespace() -> &'static str {
    static NS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NS.get_or_init(|| {
        std::env::var("ORGANON_IPC_NS")
            .ok()
            .and_then(|raw| sanitize_ns(&raw))
            .unwrap_or_else(|| crate::edition::EDITION.ipc_namespace().to_string())
    })
    .as_str()
}

/// Compose a namespaced `$TMPDIR` path from an explicit namespace. Split out from
/// [`ns_file`] so the namespace fork is unit-testable for **both** editions from a
/// default (feature-off) build.
pub fn ns_file_in(ns: &str, suffix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{ns}-{suffix}"))
}

/// `$TMPDIR/<namespace>-<suffix>` — the one place every IPC filename is built.
pub fn ns_file(suffix: &str) -> PathBuf {
    ns_file_in(namespace(), suffix)
}

/// Compose a path in a namespace **named by a caller**, refusing anything
/// [`sanitize_ns`] would refuse (#191 T1 — "two runtimes, two rings").
///
/// Everything above resolves the namespace from this *process* — `$ORGANON_IPC_NS` or
/// the edition — which is right while a process talks to its own peers, and is the whole
/// reason an Organon session and a Mind session coexist. It is not enough the moment
/// **one** process wants to look at **another** namespace's channel: two model runtimes
/// each write their own activation ring, and whatever compares them has to *name* the
/// ring it means rather than open whichever one its own namespace resolved to.
///
/// `None` — not a silent fallback — is the point of the return type. [`namespace`] falls
/// back to the edition when `$ORGANON_IPC_NS` is junk, because a spawned visual must come
/// up on *something*; a caller that typed a name has made a mistake it wants to hear
/// about, and quietly handing it the local ring would answer a question it did not ask.
/// Same sanitizer either way, so a caller can never reach a `$TMPDIR` path the env var
/// could not.
pub fn ns_file_checked(ns: &str, suffix: &str) -> Option<PathBuf> {
    sanitize_ns(ns).map(|ns| ns_file_in(&ns, suffix))
}

pub fn ipc_path() -> PathBuf {
    ns_file("ipc.bin")
}

/// Sidecar file the plugin/visual write the chosen .hdr path into. The visual
/// reads it when `Shared.hdr_gen` changes. Plain UTF-8 path; read with `.trim()`.
pub fn hdr_sidecar_path() -> PathBuf {
    ns_file("hdr.txt")
}

/// Sidecar file the editor writes the chosen material **folder** path into (#472
/// Tier 1). The visual reads it when `Shared.material_gen` changes, then loads the
/// six PNG channel maps (`albedo/normal/roughness/metallic/ao/height.png`) from
/// that folder into the GPU material texture set. Plain UTF-8 path; read with
/// `.trim()`. Mirrors `hdr_sidecar_path` + `hdr_gen`.
pub fn material_sidecar_path() -> PathBuf {
    ns_file("material.txt")
}

/// Sidecar file the editor writes the chosen connectome-JSON path into (#226 Tier
/// 3). The visual reads it when `Shared.nn_gen` changes, then ingests the file via
/// `math::neural_graph_from_json`. Plain UTF-8 path; read with `.trim()`.
pub fn connectome_sidecar_path() -> PathBuf {
    ns_file("connectome.txt")
}

/// Sidecar file the editor writes the chosen creature-JSON path into (#476 Tier
/// 2b). The visual reads it when `Shared.creature_gen` changes, then builds the
/// body plan via `math::parse_creature_spec`. Plain UTF-8 path; read with `.trim()`.
pub fn creature_sidecar_path() -> PathBuf {
    ns_file("creature.txt")
}

/// Sidecar file the editor writes the chosen `.gguf` model path into (#367 Tier 1,
/// the visible-mind specimen). The visual reads it when `Shared.mind[1]`
/// (`model_gen`) changes, then parses the GGUF header via `gguf::parse_file` and
/// builds the architecture topology. Plain UTF-8 path; read with `.trim()`. Mirrors
/// `connectome_sidecar_path` + `nn_gen`.
pub fn model_sidecar_path() -> PathBuf {
    ns_file("model.txt")
}

/// #147 Tier 3 — the **LoRA adapter directory** the Delta lens measures. The visual
/// reads it when the Mind view (`Shared.mind[2]`) selects the Delta lens, then hands
/// it to `lora::read_adapter_dir` and lights the specimen by what the fine-tune
/// moved. A directory path (holding `adapter_config.json` +
/// `adapter_model.safetensors`), not a file; plain UTF-8, read with `.trim()`.
/// Mirrors `model_sidecar_path` + `model_gen`.
///
/// 📌 A sidecar rather than a `Shared` field for the same reason every other path
/// here is one: `Shared` is append-only and offset-sensitive across a process
/// boundary, and a path is not a control-rate value.
///
/// ✏️ **#147 T3½: `organon mind adapter <PATH>` writes it** (`cli::select_adapter`,
/// `MIND_ARCHITECTURE.md` §2.8.1) — this doc said *"nothing writes it yet"* until
/// then. ⚠️ **The writer must check the directory before writing**, because the
/// reader's failure arm clears the cache key that would suppress a re-read, so an
/// unreadable path here is re-refused on every frame in the visual. With the file
/// empty or absent the lens still says so out loud rather than substituting
/// something else.
pub fn adapter_sidecar_path() -> PathBuf {
    ns_file("adapter.txt")
}

/// #423 Tier 1 — the atlas sidecar: the editor scans a model library + hardware
/// profile, serializes the derived `math::AtlasDoc` (context, KV element size,
/// profile, design points) as JSON here, and bumps `Shared.atlas[0]`. The visual
/// edge-detects the counter, reads + deserializes this, and builds the design-space
/// constellation + roofline inset. Mirrors `model_sidecar_path` + `model_gen`.
pub fn atlas_sidecar_path() -> PathBuf {
    ns_file("atlas.json")
}

/// #541 S2 Tier 1 — the mindview **saved-layout** sidecar. Part of the selector
/// reservation: `Shared.mindview*` carries the *live* arrangement (a few dozen
/// bytes on the hot snapshot), and a named/savable arrangement — WS-A's "savable
/// layouts" — rides this JSON file, edge-detected via `Shared.mindview_gen`. The
/// `atlas_sidecar_path` + `atlas[0]` pattern. **Nothing reads or writes it yet.**
pub fn mindview_layout_path() -> PathBuf {
    ns_file("mindview.json")
}

/// Sidecar file the editor writes a Field Engine program (#381 Tier 1) into: the
/// program TEXT itself (an expression over `x,y,z,t`, e.g. `charge(a,0,0,0)`), NOT
/// a path. The visual reads it when `Shared.field_gen` changes (with the
/// `FieldPreset` = Custom), then recompiles via `math::FieldProgram::compile`.
/// Mirrors `hdr_sidecar_path` + `hdr_gen`. Plain UTF-8; read with `.trim()`.
pub fn field_sidecar_path() -> PathBuf {
    ns_file("field.txt")
}

/// Sidecar file the editor writes the chosen **Field Playback** clip path into (#407
/// Tier A): the `.bin` file path, plain UTF-8. The editor's "Load Field Clip…" button
/// writes it and bumps `Shared.fieldclip_gen`; the visual edge-detects the counter and
/// (re)loads the `math::FieldClip` via `FieldClip::from_bytes`. Mirrors
/// `field_sidecar_path`; read with `.trim()`.
pub fn field_clip_sidecar_path() -> PathBuf {
    ns_file("fieldclip.txt")
}

/// Sidecar file the editor writes the chosen Neural CA weights-JSON **path** into
/// (Tier B, #407). The visual reads it when `Shared.nca_gen` changes, then loads
/// `math::NcaWeights::from_json`, falling back to `builtin_default()` when the file
/// is missing/empty/malformed. Plain UTF-8 path; read with `.trim()`. Mirrors
/// `connectome_sidecar_path` + `nn_gen`.
pub fn nca_sidecar_path() -> PathBuf {
    ns_file("nca.txt")
}

/// Sidecar file for the overlay's variable-length strings (#135 Phase 2): a small
/// JSON `{ "handle": "...", "title": "..." }`. The editor writes it + bumps
/// `Shared.overlay_gen`; the visual edge-detects the counter and re-reads. Mirrors
/// `hdr_sidecar_path` + `hdr_gen`.
pub fn overlay_sidecar_path() -> PathBuf {
    ns_file("overlay.txt")
}

/// AI-Performer (#317 Tier 1) chat sidecar: the user's latest chat message, plain
/// UTF-8. The editor writes it and bumps `Shared.agent[1]` (`chat_gen`); the visual
/// edge-detects the counter and feeds the message to the agent runtime. Mirrors
/// `hdr_sidecar_path` + `hdr_gen`.
pub fn chat_sidecar_path() -> PathBuf {
    ns_file("chat.txt")
}

/// AI-Performer (#317 Tier 1) phrase-plan sidecar: a hand-written JSON plan for the
/// debug executor path (a scriptable phrase sequencer that needs no model). The
/// editor writes it and bumps `Shared.agent[2]` (`plan_gen`); the visual reads +
/// applies it. Mirrors `hdr_sidecar_path` + `hdr_gen`.
pub fn plan_sidecar_path() -> PathBuf {
    ns_file("plan.txt")
}

/// #452: the CLI command channel — the `organon` CLI (external local agents,
/// e.g. Bianca) APPENDS one `agent::CliOp` line per command; the visual
/// self-detects growth each frame (file length, no `Shared` gen counter — the
/// CLI is deliberately never an IPC writer) and dispatches each new line
/// through the Performer's override lane. Same append-and-drain discipline as
/// the apply channel (`agent::apply_drain_plan`).
pub fn cli_cmd_path() -> PathBuf {
    ns_file("cli.txt")
}

/// #452 Tier 3 ("the eyes") — the request channel for `organon snap` / `record`.
/// These commands can't be fire-and-forget CliOps: they need the VISUAL to do GPU
/// work (a single-frame readback → PNG, or drive the recorder) and hand a file path
/// BACK. The CLI stays a non-IPC-writer: it APPENDS one request line (`<nonce> <verb>`)
/// here, the visual drains it (same file-length/cursor discipline as `cli_cmd_path`),
/// acts, and appends the outcome to `eyes_reply_path`. See `cli::EyesReq`.
pub fn eyes_cmd_path() -> PathBuf {
    ns_file("eyes.txt")
}

/// #452 Tier 3 — the reply channel (visual → CLI) for `organon snap` / `record`. The
/// visual APPENDS one line per completed request (`<nonce> ok <text>` / `<nonce> err
/// <text>`); the CLI polls this file for its own nonce, then prints the path (`ok`) or
/// the error (`err`). Append-only text; nonces are unique per invocation.
pub fn eyes_reply_path() -> PathBuf {
    ns_file("eyes-reply.txt")
}

/// AI-Performer (#317 Tier 1) status sidecar, written by the VISUAL (the agent runtime)
/// and read by the editor's Mind card for its readout: line 1 = comma-separated held
/// param ids, line 2 = the agent's last reply text. Reverse of the chat/plan sidecars.
pub fn agent_status_path() -> PathBuf {
    ns_file("agent-status.txt")
}

/// AI-Performer (#317 Tier 1) agent config sidecar: the OpenAI-compatible localhost
/// endpoint URL + model name (two lines, `endpoint\nmodel`), so Ollama / LM Studio /
/// llama.cpp / MLX are interchangeable without a rebuild. Config, NOT a `Shared`
/// float. Written by the Mind card; read by the agent runtime in the visual.
pub fn agent_config_path() -> PathBuf {
    ns_file("agent.txt")
}

/// AI-Performer **UI-sync** sidecar (visual → plugin editor). The visual APPENDS one line
/// per applied agent action so the editor's GUI thread can mirror it onto the real params
/// (moving the sliders / dropdowns) — the plugin can't be driven from the audio thread, and
/// the agent runs in the visual, so the editor drains this on its GUI loop via `ParamSetter`.
/// Append-and-drain (like the chat sidecar): the editor tracks a consumed-line cursor so a
/// param the user then moves isn't re-applied (last-touched-wins). Line grammar:
/// `set <id> <value>` / `gen <index>` / `surf <index>` / `mat <index>` / `release`.
pub fn agent_apply_path() -> PathBuf {
    ns_file("agent-apply.txt")
}

/// Intelligent-preset-names (#425) request sidecar (plugin editor → visual). On save the
/// editor writes a small JSON `agent::NameRequest` (the just-saved scene's identity +
/// a monotonic `id`) here and bumps `Shared.agent[4]` (`name_gen`); the visual
/// edge-detects the counter, reads this file, asks the local model for a name, and writes
/// the answer to `name_reply_path`. Overwritten per request (the chat-sidecar precedent).
pub fn name_request_path() -> PathBuf {
    ns_file("namereq.txt")
}

/// Intelligent-preset-names (#425) reply sidecar (visual → plugin editor), **one file per
/// request `id`** (`organic-math-namereply-<id>.txt`, holding just the name). A per-id file
/// means two saves whose model replies land close together can't clobber each other (a
/// single shared file would keep only the last writer's answer). The editor drains each
/// pending id's file on its GUI loop, applies the name if the preset still carries its
/// provisional label (a user rename in the meantime wins), then deletes the file.
pub fn name_reply_path(id: u32) -> PathBuf {
    ns_file(&format!("namereply-{id}.txt"))
}

/// Quantitative-instrumentation (#391 Tier 1) probe-trace CSV, written by the VISUAL
/// while `Shared.instrument[15]` (`csv_log`) is on: one row per frame via
/// `math::probe_csv_row` (header from `math::probe_csv_header`), appended so a run
/// accumulates a trace anyone can check. Truncated + re-headered when logging is
/// toggled back on. Plain UTF-8.
pub fn probe_csv_path() -> PathBuf {
    ns_file("probe.csv")
}

const SIZE: usize = std::mem::size_of::<Shared>();

/// Byte range of `Shared::seq` — the seqlock counter, and the first field of the
/// struct. Pinned by `seq_and_layout_version_sit_at_the_front` below, because the
/// writer and reader both address it positionally rather than through the type.
const SEQ_OFF: usize = 0;
/// Bytes the writer publishes *separately* from the body: `seq` (0..4) and
/// `layout_version` (4..8). Everything from here to `SIZE` is the payload.
const HDR: usize = 8;

/// How many times [`Reader::read`] re-reads before giving up on a consistent
/// snapshot. A write is one ~8 KB memcpy and happens once per audio block, so the
/// reader loses at most a handful of nanoseconds per retry and realistically never
/// spends more than one. Exhausting this means a writer died mid-write (its odd
/// `seq` is now permanent), which is the one case where returning defaults is right.
const READ_RETRIES: u32 = 8;

/// Writer side (the plugin). Created once, then `write` each block.
///
/// ## The seqlock (#618 Tier 0a)
///
/// `Shared` is 8512 bytes. No machine copies that atomically, so a reader can
/// observe half of one snapshot and half of the next. Before Tier 0a the only
/// guard was `layout_version`, which catches a *mismatched build* and cannot catch
/// a *tear*: both halves of a torn record carry the same version, so the check
/// passes on garbage. `seq` was written on every block and no reader ever looked
/// at it.
///
/// The fix is the standard single-writer seqlock:
///
/// 1. stamp `seq` **odd** (a write is in flight),
/// 2. copy the body,
/// 3. stamp `seq` **even** (committed).
///
/// A reader accepts a snapshot only when it saw the same even `seq` on both sides
/// of its own read. Note that step 2 must *not* include the header, which is why
/// the bulk copy starts at [`HDR`]: `seq` lives at offset 0, so a single
/// `copy_from_slice` of the whole struct would publish the committed counter
/// first and the body afterwards, which is precisely backwards.
///
/// This runs on the audio thread and stays allocation-free: two 4-byte stores and
/// two fences added to a memcpy that was already happening.
pub struct Writer {
    map: memmap2::MmapMut,
    /// Always **even**, and never 0 after the first write. The odd value published
    /// during a write is `seq | 1`, so it is never mistaken for a committed one.
    seq: u32,
}

impl Writer {
    pub fn create() -> io::Result<Writer> {
        Self::create_at(&ipc_path())
    }

    /// `create`, against an explicit path. The namespaced `ipc_path()` is
    /// process-global and `$ORGANON_IPC_NS` resolves once per process (§4.1), so a
    /// test that wants its own map cannot get one by setting an env var — it needs
    /// this. Used by the seqlock tests, which have to run a real writer against a
    /// real reader to prove anything.
    pub(crate) fn create_at(path: &std::path::Path) -> io::Result<Writer> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.set_len(SIZE as u64)?;
        // SAFETY: file is sized to SIZE; we are the sole writer.
        let mut map = unsafe { memmap2::MmapMut::map_mut(&file)? };
        let mut w = Writer { map, seq: 0 };
        // ⚠️ **The initial image is published through the seqlock too**, not slammed
        // in with a bare `copy_from_slice` (#622 review). This path is reachable
        // concurrently: nih-plug calls `initialize()` again on sample-rate and
        // buffer-size changes and on project reload, and the visual's `Reader` may
        // already be attached and polling. An unbracketed init write slips past BOTH
        // guards — `Shared::default().seq` is 0, which is even and never changes
        // during the copy, and its `layout_version` already matches — so a reader
        // would accept a blend of the old file and the fresh defaults. That is the
        // exact failure this tier exists to remove, just on the reset path.
        //
        // ⚠️ **And it RESUMES the counter instead of resetting it.** Publishing the
        // init at 0 was the first attempt and it is wrong: a reader's before/after
        // equality check is only sound while the counter never *repeats* a value.
        // Reset to 0 on every re-initialize and a full cycle (… → 2 → 1 → 0 → 3 → 2)
        // lands back on a value the reader sampled before its body copy, so the check
        // passes over a body that changed underneath it. That is ABA, and it is a
        // torn read wearing the guard's uniform. `re_creating_the_map_under_a_live_
        // reader_never_blends` fails within a second against the reset version.
        //
        // So: continue from whatever is in the file, rounded up to the next even
        // value. `world.rs` reads seq == 0 as "no writer has published yet", and that
        // stays true — if this ran, a writer exists, so a non-zero counter is the
        // honest answer. A never-written channel has no file at all, and `Reader`
        // answers `Shared::default()` (seq 0) for it.
        let resumed = seq_cell(&w.map).load(Ordering::Relaxed);
        w.seq = (resumed | 1).wrapping_add(1);
        if w.seq == 0 {
            w.seq = 2;
        }
        let committed = w.seq;
        w.publish(bytemuck::bytes_of(&Shared::default()), committed);
        Ok(w)
    }

    pub fn write(&mut self, mut s: Shared) {
        // Even, and never 0: `world.rs` reads seq == 0 as "no writer has ever
        // published", and `Reader::is_live` probes for motion in this counter.
        self.seq = self.seq.wrapping_add(2);
        if self.seq == 0 {
            self.seq = 2;
        }
        s.seq = self.seq;
        s.layout_version = LAYOUT_VERSION;
        let committed = self.seq;
        self.publish(bytemuck::bytes_of(&s), committed);
    }

    /// Publish one image under the seqlock: odd → body → even.
    ///
    /// `bytes` must be a full `Shared` image; its first 4 bytes (`seq`) are ignored,
    /// because the counter is written through the atomic view instead. Everything
    /// from byte 4 on — `layout_version` included — is protocol-protected by the
    /// bracket rather than by being atomic itself, which is what lets it stay a
    /// plain copy.
    fn publish(&mut self, bytes: &[u8], committed: u32) {
        debug_assert_eq!(committed % 2, 0, "the committed counter is always even");
        // 1. In flight. Release: everything after this store is ordered behind it.
        seq_cell(&self.map).store(committed | 1, Ordering::Release);
        // 2. Body + layout_version. Disjoint from the counter word, so no plain
        //    access ever aliases the memory the atomic view covers.
        self.map[HDR..SIZE].copy_from_slice(&bytes[HDR..SIZE]);
        self.map[4..HDR].copy_from_slice(&bytes[4..HDR]);
        // 3. Committed. Release: the body above is visible to any reader that sees
        //    this store.
        seq_cell(&self.map).store(committed, Ordering::Release);
    }
}

/// Reader side (the visual). Returns defaults if nothing has been written yet.
pub struct Reader {
    map: Option<memmap2::Mmap>,
}

impl Reader {
    pub fn open() -> Reader {
        Self::open_at(&ipc_path())
    }

    /// `open`, against an explicit path — the reader half of [`Writer::create_at`],
    /// and for the same reason. An absent or too-short file yields a `Reader` with no
    /// mapping, which reads as defaults; that is the state a reader legitimately
    /// starts in, not an error.
    pub(crate) fn open_at(path: &std::path::Path) -> Reader {
        let map = OpenOptions::new()
            .read(true)
            .open(path)
            .ok()
            .and_then(|f| {
                if f.metadata().map(|m| m.len() as usize >= SIZE).unwrap_or(false) {
                    // SAFETY: file is at least SIZE bytes.
                    unsafe { memmap2::Mmap::map(&f).ok() }
                } else {
                    None
                }
            });
        Reader { map }
    }

    /// The newest **consistent** snapshot, or defaults.
    ///
    /// The acquire half of the [`Writer`] seqlock: sample `seq`, read the body,
    /// sample `seq` again, and accept only when both samples are the same even
    /// value. An odd sample means a write is in flight; a changed sample means one
    /// landed mid-read. Either way the bytes in hand are a mixture of two snapshots
    /// and get thrown away rather than rendered.
    ///
    /// Returns `Shared::default()` when nothing is mapped, when the layout version
    /// does not match this build, or when [`READ_RETRIES`] consecutive attempts all
    /// raced — the last of which effectively means the writer died mid-write.
    pub fn read(&self) -> Shared {
        let Some(m) = &self.map else { return Shared::default() };
        for _ in 0..READ_RETRIES {
            // Acquire: pairs with the writer's committing store, so the body it
            // wrote before that store is visible to the copy below.
            let before = seq_cell(m).load(Ordering::Acquire);
            // Odd = a write is in flight. Nothing under it is trustworthy yet.
            if before & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            // Copy from byte 4 on, NOT from byte 0. Reading the counter bytes here
            // as part of the struct would be a plain read of the very word the
            // writer stores to atomically — the same data race, just moved. `seq` is
            // filled from the atomic load instead; `layout_version` rides the body
            // because the bracket protects it.
            let mut s = Shared::zeroed();
            bytemuck::bytes_of_mut(&mut s)[4..SIZE].copy_from_slice(&m[4..SIZE]);
            s.seq = before;
            if seq_cell(m).load(Ordering::Acquire) != before {
                // A write committed while we were reading: the body straddles two
                // snapshots. Try again against the newer one.
                std::hint::spin_loop();
                continue;
            }
            // Consistent. The version check is the *build* guard and stays exactly
            // what it always was — it is not, and never was, a tear guard.
            return if s.layout_version == LAYOUT_VERSION { s } else { Shared::default() };
        }
        Shared::default()
    }

    /// Whether a live snapshot is mapped, its layout matches this build, AND a
    /// writer is actually alive behind it (#452 review finding: a quit Organon
    /// leaves the mmap file in `$TMPDIR` with a matching layout, so the layout
    /// check alone would call a corpse "live"). The plugin's `Writer::write`
    /// bumps `seq` on every audio block, so we probe for MOTION: sample `seq`,
    /// wait briefly, resample — any advance within ~150 ms = alive. The wait
    /// only costs commands talking to a dead/absent Organon; a live one
    /// answers on the first 25 ms tick. (A plugin whose track is NOT
    /// processing audio reads as not-live — the same "track must be
    /// processing" reality every `Shared`-stamped path has.)
    ///
    /// Unaffected by the Tier-0a seqlock: motion is motion whether the counter is
    /// caught mid-write (odd) or committed (even), so this samples the raw word
    /// rather than paying for a consistent snapshot it does not need.
    pub fn is_live(&self) -> bool {
        let Some(m) = &self.map else { return false };
        let version = u32::from_ne_bytes(m[4..HDR].try_into().unwrap_or([0; 4]));
        if version != LAYOUT_VERSION {
            return false;
        }
        let a = seq_cell(m).load(Ordering::Relaxed);
        for _ in 0..6 {
            std::thread::sleep(std::time::Duration::from_millis(25));
            if seq_cell(m).load(Ordering::Relaxed) != a {
                return true;
            }
        }
        false
    }
}

/// The seqlock counter as a real `AtomicU32`, not four bytes read plainly.
///
/// A standalone `fence` orders *atomic* accesses against each other; it establishes
/// nothing against ordinary loads and stores, so bracketing a plain `[u8]` copy with
/// fences is formally a data race however reliably it behaves on x86-64 and ARM64
/// (#622 review). Both sides go through this view instead, which costs the same and
/// is sound. Precedent in-tree: the `apply_gen` seqlock is a real `Arc<AtomicU32>`.
///
/// SAFETY: `SEQ_OFF` is 0, so this is the mapping's base — page-aligned, therefore
/// 4-byte aligned. `seq_and_layout_version_sit_at_the_front` pins `Shared::seq` at
/// that offset. Every access to those four bytes on both sides goes through this
/// function; the body copies start at byte 4 and never overlap it.
#[inline]
fn seq_cell(m: &[u8]) -> &std::sync::atomic::AtomicU32 {
    unsafe { &*(m.as_ptr().add(SEQ_OFF) as *const std::sync::atomic::AtomicU32) }
}

/// Reverse channel (visual → plugin): the live render resolution the visual is
/// actually drawing at, so the editor can show it — especially in auto/DRS mode,
/// where the visual (not the user) picks the scale. Separate mmap file.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub struct Feedback {
    pub seq: u32,
    pub layout_version: u32,
    pub scale: f32,  // effective render scale (0..1)
    pub width: u32,  // render width (px)
    pub height: u32, // render height (px)
    pub fps: f32,    // smoothed frames/sec
    pub out_w: u32,  // production-frame width (px); 0 = Native (= window)
    pub out_h: u32,  // production-frame height (px); 0 = Native (= window)
    // Hardware RT (#195 Tier 0) — appended, like Shared's discipline.
    pub rt_available: u32, // 1 = the device has EXPERIMENTAL_RAY_QUERY (Metal, M3+)
    pub tlas_ms: f32, // smoothed CPU encode+submit ms of the per-frame TLAS rebuild (0 = not building)
    // Neural acceleration (#200 Tier 2) — appended. Detection only for now: the
    // adapter's support for the cooperative-matrix (simdgroup/tensor) fast path +
    // f16, so the editor can report what the machine has. The feature is NOT
    // enabled on the render device yet (enabling experimental coop-matrix + the
    // GFLOPs microbenchmark need the Mac — the ray-query wedge lesson).
    pub coopmat_available: u32, // 1 = adapter offers EXPERIMENTAL_COOPERATIVE_MATRIX
    pub f16_available: u32,     // 1 = adapter offers SHADER_F16
    // Metal interop island (#200 Tier 3) — appended. The startup probe result:
    // whether the island reached a Metal device + (later) its measured tensor-op
    // GFLOPs. `metal_island_available` = 0 until the on-Mac `imp` lands (dark).
    pub metal_island_available: u32, // 1 = island probe reached Metal + self-checked
    pub tensor_gflops: f32,          // measured island MSL-matmul throughput (0 = not run)
    // Path tracer (#200 Tier 4) — appended AFTER the Tier 2/3 accel fields on the
    // main re-merge. `pathtrace_active` = the ground-truth mode is on; `pathtrace_spp`
    // = samples accumulated (resets on camera move).
    pub pathtrace_active: u32,
    pub pathtrace_spp: u32,
    // Workload telemetry (#277 Tier 2) — appended, like Shared's discipline.
    // The status bar reads these into per-stat headroom meters.
    pub instances: u32, // instanced nodes drawn this frame (0 on raymarch paths)
    pub cpu_ms: f32,    // smoothed CPU ms to encode + submit the frame's render passes
                        // (brackets Renderer::render only; excludes the earlier
                        // per-frame node rebuild — the editor labels it "CPU / frame")
    // GPU timing (#277 Tier 3) — appended. `gpu_ms` is the frame's true GPU time
    // from wgpu timestamp queries (read back a frame late, so no CPU stall); 0
    // until the pipeline primes. `gpu_timing_available` = the device offered
    // TIMESTAMP_QUERY (else the editor shows "n/a" and leans on `cpu_ms`).
    pub gpu_ms: f32,
    pub gpu_timing_available: u32,
    // Neural radiance cache (#256 Tier 0) — appended. The number that proves the
    // live cache works: `nrc_loss` is the smoothed mean training loss of the last
    // batch (falls as the cache converges to the light field); `nrc_state` = 0 off,
    // 1 warming (loss still high), 2 converged (loss under the threshold); 0 while
    // the cache is disabled. The editor shows these on the RT/cache card.
    pub nrc_loss: f32,
    pub nrc_state: u32,
}

/// Where the visual writes its last panic (message + backtrace). The plugin
/// spawns the visual with stderr going nowhere, so a crash was invisible —
/// this file is the black box. Overwritten per panic; read it after a crash.
pub fn panic_log_path() -> PathBuf {
    ns_file("panic.txt")
}

pub fn feedback_path() -> PathBuf {
    ns_file("feedback.bin")
}

/// The #367 Tier 2 activation-ring mmap: a SEPARATE channel from `Shared`, carrying
/// per-token model activations from the writer (the synthetic `organic-math-mind-writer`
/// bin now, the embedded runtime later) to the visual. Kept off `Shared` on purpose so
/// Tier 2's model-free slice adds no `Shared` size/LAYOUT_VERSION change. See `mind_ring.rs`.
pub fn mind_ring_path() -> PathBuf {
    ns_file("mind.bin")
}

/// The activation ring of a **named** namespace (#191 T1). `mind_ring_path()` is this
/// with the process's own namespace, and the two are pinned equal by a test.
///
/// This is what lets a base model and its fine-tune run at once: each runtime is started
/// with its own `$ORGANON_IPC_NS` and writes `$TMPDIR/<ns>-mind.bin`, and a reader names
/// the one it wants. `None` for a namespace [`sanitize_ns`] rejects — see
/// [`ns_file_checked`] for why that is not a fallback.
pub fn mind_ring_path_in(ns: &str) -> Option<PathBuf> {
    ns_file_checked(ns, "mind.bin")
}

/// The #430 audio-sample ring mmap: a SEPARATE channel from `Shared`, carrying the
/// plugin's live post-synth stereo output to the visual's in-app recorder. Off `Shared`
/// (a continuous high-rate stream, not a control-rate snapshot); see `audio_ring.rs`.
pub fn audio_ring_path() -> PathBuf {
    ns_file("audio.bin")
}

/// The **glyph ring** mmap (organon#217 T1, `doc/pbr_text_engine.md` §6): a terminal-
/// shaped cell grid carried from a text-effect producer (`organon-glyphs`, linking
/// `ttfx`) to the world, which renders each cell as a lit tile. A SEPARATE channel from
/// `Shared` for the `mind_ring` / `audio_ring` reason — up to a megabyte at the effect's
/// own cadence is neither control-rate nor small, and `Shared`'s offsets are load-bearing
/// across every saved set. See `glyph_ring.rs` for the layout and the orientation rule.
pub fn glyph_ring_path() -> PathBuf {
    ns_file("glyphs.bin")
}

/// The glyph ring of a **named** namespace — the `mind_ring_path_in` twin, same
/// sanitizer, same refusal (`None`, never a fallback to the local ring).
pub fn glyph_ring_path_in(ns: &str) -> Option<PathBuf> {
    ns_file_checked(ns, "glyphs.bin")
}

/// #554 Tier 1 — the **frame mirror** mmap: the visual's rendered frames, carried to the
/// editor so it can draw a live viewport in its own window. A SEPARATE channel from `Shared`
/// for the `mind_ring` / `audio_ring` reason — ~0.9 MB at ~15 Hz is neither control-rate nor
/// small, and `Shared`'s byte offsets are load-bearing across every saved set. Built by
/// `frame_ring::frame_ring_path()`; see `frame_ring.rs` for why the boundary is CPU memory
/// rather than a shared GPU texture.
///
/// #367 Tier 2b prompt sidecar: the plain-UTF-8 prompt the editor's Mind card writes
/// (its "Generate" button) for the embedded `organic-math-mind-runtime` to complete.
/// The runtime reads it when `Shared.mind[3]` (`prompt_gen`) changes. Mirrors
/// `chat_sidecar_path` + `agent[1]`; read with `.trim()` / as-is (multi-line ok).
pub fn mind_prompt_path() -> PathBuf {
    ns_file("mind-prompt.txt")
}

/// #367 Tier 2b reply sidecar: the streaming decoded reply the embedded runtime writes
/// (truncated at prompt start, appended per generated token) so the editor's Mind card
/// can poll + show the model's live output. Written by the RUNTIME, read by the editor
/// (reverse of the prompt sidecar). Plain UTF-8.
pub fn mind_reply_path() -> PathBuf {
    ns_file("mind-reply.txt")
}

const FB_SIZE: usize = std::mem::size_of::<Feedback>();

/// Feedback writer (the visual). Created once, then `write` each frame.
pub struct FeedbackWriter {
    map: memmap2::MmapMut,
    seq: u32,
}

impl FeedbackWriter {
    pub fn create() -> io::Result<FeedbackWriter> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(feedback_path())?;
        file.set_len(FB_SIZE as u64)?;
        // SAFETY: file is sized to FB_SIZE; we are the sole writer.
        let mut map = unsafe { memmap2::MmapMut::map_mut(&file)? };
        map[..FB_SIZE].copy_from_slice(bytemuck::bytes_of(&Feedback::default()));
        Ok(FeedbackWriter { map, seq: 0 })
    }

    pub fn write(&mut self, mut f: Feedback) {
        self.seq = self.seq.wrapping_add(1);
        f.seq = self.seq;
        f.layout_version = LAYOUT_VERSION;
        self.map[..FB_SIZE].copy_from_slice(bytemuck::bytes_of(&f));
    }
}

/// Feedback reader (the plugin editor). `read` returns `None` until the visual has
/// created + written the file; the editor re-opens lazily via `FeedbackReader::open`.
pub struct FeedbackReader {
    map: Option<memmap2::Mmap>,
}

impl FeedbackReader {
    pub fn open() -> FeedbackReader {
        let map = OpenOptions::new()
            .read(true)
            .open(feedback_path())
            .ok()
            .and_then(|f| {
                if f.metadata().map(|m| m.len() as usize >= FB_SIZE).unwrap_or(false) {
                    // SAFETY: file is at least FB_SIZE bytes.
                    unsafe { memmap2::Mmap::map(&f).ok() }
                } else {
                    None
                }
            });
        FeedbackReader { map }
    }

    pub fn is_open(&self) -> bool {
        self.map.is_some()
    }

    pub fn read(&self) -> Option<Feedback> {
        let m = self.map.as_ref()?;
        let f: Feedback = bytemuck::pod_read_unaligned(&m[..FB_SIZE]);
        (f.layout_version == LAYOUT_VERSION).then_some(f)
    }
}

#[cfg(test)]
mod overlay_tests {
    use super::*;

    /// The assumption [`overlay_changed`] rests on: a 4-byte word is exactly one lane. If
    /// `Shared` ever grows a field of another width this fails here, rather than splicing two
    /// halves of different values together somewhere a person has to notice by eye.
    #[test]
    fn shared_is_a_whole_number_of_lanes() {
        assert_eq!(std::mem::size_of::<Shared>() % 4, 0);
        assert_eq!(std::mem::align_of::<Shared>() % 4, 0);
    }

    /// 🚨 The inertness guarantee, and the reason the Console can carry a panel's mirror over
    /// a snapshot somebody else composed without a lane manifest. Nothing moved → nothing is
    /// written, whatever `dst` happens to hold.
    #[test]
    fn overlay_changed_is_inert_when_nothing_moved() {
        let values = Shared::default();
        let mut dst = Shared::default();
        dst.bevel = 0.75;
        dst.lighting[2] = 9.0;
        dst.seq = 41;
        let before = dst;
        overlay_changed(&mut dst, &values, &values);
        assert_eq!(bytemuck::bytes_of(&dst), bytemuck::bytes_of(&before));
    }

    /// Only what moved moves — the lane that changed lands, and a lane `dst` had its own
    /// opinion about survives untouched. This is the composition the Console needs: the
    /// substrate dressing shows through wherever the panel has said nothing.
    #[test]
    fn overlay_changed_carries_the_changed_lane_and_nothing_else() {
        let base = Shared::default();
        let mut mine = base;
        mine.bevel = 0.5;
        let mut dst = base;
        dst.bevel = 0.1;
        dst.lighting[2] = 4.25; // dst's own opinion, on a lane the "panel" never touched
        overlay_changed(&mut dst, &base, &mine);
        assert_eq!(dst.bevel, 0.5);
        assert_eq!(dst.lighting[2], 4.25);
    }

    /// ⚠️ **Lane granularity is what makes this safe, and the check has to be a value that
    /// differs in only *some* of its bytes.** `0.5f32` and `0.5001f32` share their top byte;
    /// a byte-wise overlay would copy the low three and leave the high one, producing a float
    /// neither side ever held. Bit-equality against `mine` is the whole assertion.
    #[test]
    fn a_partially_differing_float_is_copied_whole() {
        let base = Shared::default();
        let mut mine = base;
        mine.bevel = 0.5;
        let mut dst = base;
        dst.bevel = 0.5001;
        overlay_changed(&mut dst, &base, &mine);
        assert_eq!(dst.bevel.to_bits(), 0.5f32.to_bits());
    }

    /// A lane that moved *back* to its starting value has no opinion again — which is the
    /// honest reading of "the difference is the opinion", and worth pinning because the
    /// alternative (a sticky touched-set) is what someone would reach for first.
    #[test]
    fn a_lane_returned_to_base_stops_being_asserted() {
        let base = Shared::default();
        let mine = base;
        let mut dst = base;
        dst.bevel = 0.9;
        overlay_changed(&mut dst, &base, &mine);
        assert_eq!(dst.bevel, 0.9);
    }
}

#[cfg(test)]
mod ns_tests {
    use super::*;

    /// Full Organon's paths are **byte-identical to before the edition fork**. This is
    /// the regression guard on the whole refactor: a running visual, a hand-run
    /// runtime, and every documented `$TMPDIR` filename must all still line up.
    #[test]
    fn full_edition_paths_are_unchanged() {
        let ns = crate::edition::Edition::Full.ipc_namespace();
        let t = std::env::temp_dir();
        assert_eq!(ns_file_in(ns, "ipc.bin"), t.join("organic-math-ipc.bin"));
        assert_eq!(ns_file_in(ns, "mind.bin"), t.join("organic-math-mind.bin"));
        assert_eq!(ns_file_in(ns, "model.txt"), t.join("organic-math-model.txt"));
        assert_eq!(ns_file_in(ns, "hdr.txt"), t.join("organic-math-hdr.txt"));
        assert_eq!(ns_file_in(ns, "panic.txt"), t.join("organic-math-panic.txt"));
        assert_eq!(
            ns_file_in(ns, &format!("namereply-{}.txt", 7)),
            t.join("organic-math-namereply-7.txt")
        );
    }

    /// The point of the fork (#483): Organon and Organon Mind never open the same file,
    /// so the two products can run side by side.
    #[test]
    fn editions_resolve_to_different_files() {
        let full = crate::edition::Edition::Full.ipc_namespace();
        let mind = crate::edition::Edition::Mind.ipc_namespace();
        for suffix in ["ipc.bin", "mind.bin", "audio.bin", "model.txt", "mind-prompt.txt"] {
            assert_ne!(
                ns_file_in(full, suffix),
                ns_file_in(mind, suffix),
                "{suffix} collides across editions"
            );
        }
        assert_eq!(
            ns_file_in(mind, "ipc.bin"),
            std::env::temp_dir().join("organon-mind-ipc.bin")
        );
    }

    /// An env var must never be able to redirect our mmaps out of `$TMPDIR`.
    #[test]
    fn sanitize_ns_rejects_unsafe_namespaces() {
        assert_eq!(sanitize_ns("organon-mind").as_deref(), Some("organon-mind"));
        assert_eq!(sanitize_ns("  organon_mind2 ").as_deref(), Some("organon_mind2"));
        for bad in ["", "   ", "../evil", "a/b", "a\\b", "a b", "a;rm -rf", "a\nb", "naïve"] {
            assert!(sanitize_ns(bad).is_none(), "{bad:?} should be rejected");
        }
        assert!(sanitize_ns(&"x".repeat(65)).is_none(), "over-long namespace should be rejected");
    }

    /// The resolved namespace is itself filename-safe and every path function agrees
    /// with it (they all funnel through `ns_file`).
    #[test]
    fn resolved_namespace_is_consistent() {
        let ns = namespace();
        assert!(sanitize_ns(ns).is_some(), "resolved namespace {ns:?} is not safe");
        assert_eq!(ipc_path(), ns_file_in(ns, "ipc.bin"));
        assert_eq!(mind_ring_path(), ns_file_in(ns, "mind.bin"));
        assert_eq!(namespace(), ns, "namespace must be stable within a process");
    }

    /// #147 T3 — the adapter sidecar is namespaced like every other one, and it is a
    /// file of its own rather than a second meaning for `model.txt`. Sharing that
    /// file would make "which .gguf" and "which adapter" the same string, and the
    /// Delta lens needs both at once (dims from the model, movement from the adapter).
    #[test]
    fn adapter_sidecar_is_namespaced_and_distinct_from_the_model() {
        let ns = namespace();
        assert_eq!(adapter_sidecar_path(), ns_file_in(ns, "adapter.txt"));
        assert_ne!(adapter_sidecar_path(), model_sidecar_path());
        assert_ne!(
            ns_file_in(crate::edition::Edition::Full.ipc_namespace(), "adapter.txt"),
            ns_file_in(crate::edition::Edition::Mind.ipc_namespace(), "adapter.txt"),
        );
    }

    /// #191 T1 — the two-runtime property, stated at the path layer where it starts.
    ///
    /// Base and fine-tune are two processes, each with its own `$ORGANON_IPC_NS`, so
    /// "two rings" is nothing more exotic than two namespaces resolving to two files.
    /// If they ever collided the second runtime would silently overwrite the first's
    /// frames and the diff would be a model against itself — a picture with no error.
    #[test]
    fn two_namespaces_are_two_rings() {
        let base = mind_ring_path_in("mind-base").expect("legal namespace");
        let tuned = mind_ring_path_in("mind-tuned").expect("legal namespace");
        assert_ne!(base, tuned, "two runtimes must not share one ring file");
        assert_eq!(base, std::env::temp_dir().join("mind-base-mind.bin"));
        assert_eq!(tuned, std::env::temp_dir().join("mind-tuned-mind.bin"));
    }

    /// Naming this process's own namespace yields the path the unnamed call already
    /// produces — so `mind_ring_path_in` GENERALIZES `mind_ring_path` rather than being
    /// a second convention that could drift from it.
    #[test]
    fn naming_your_own_namespace_is_the_path_you_already_had() {
        assert_eq!(mind_ring_path_in(namespace()).as_deref(), Some(mind_ring_path().as_path()));
    }

    /// organon#217 T1 — the glyph ring is namespaced like the mind ring, is a file of its
    /// own (a Mind session and an Organon session must never share one), and its named
    /// form generalizes the unnamed one rather than being a second convention.
    #[test]
    fn glyph_ring_is_namespaced_distinct_and_its_named_form_generalizes() {
        let ns = namespace();
        assert_eq!(glyph_ring_path(), ns_file_in(ns, "glyphs.bin"));
        assert_ne!(glyph_ring_path(), mind_ring_path());
        assert_ne!(glyph_ring_path(), audio_ring_path());
        assert_eq!(glyph_ring_path_in(ns).as_deref(), Some(glyph_ring_path().as_path()));
        assert_ne!(
            ns_file_in(crate::edition::Edition::Full.ipc_namespace(), "glyphs.bin"),
            ns_file_in(crate::edition::Edition::Mind.ipc_namespace(), "glyphs.bin"),
        );
        let a = glyph_ring_path_in("glyph-a").expect("legal namespace");
        let b = glyph_ring_path_in("glyph-b").expect("legal namespace");
        assert_ne!(a, b);
        for bad in ["", "../evil", "a/b", "a b"] {
            assert!(glyph_ring_path_in(bad).is_none(), "{bad:?} should be refused");
        }
    }

    /// A caller-supplied namespace must not reach a `$TMPDIR` path the env var could
    /// not — one sanitizer, both doors. And it REFUSES rather than falling back:
    /// handing a typo the local ring would answer a question nobody asked.
    #[test]
    fn a_named_namespace_is_sanitized_and_refused_not_defaulted() {
        for bad in ["", "   ", "../evil", "a/b", "a\\b", "a b", "a;rm -rf", "a\nb", "naïve"] {
            assert_eq!(
                sanitize_ns(bad).is_none(),
                ns_file_checked(bad, "mind.bin").is_none(),
                "{bad:?}: the named door and the env-var door must agree"
            );
            assert!(mind_ring_path_in(bad).is_none(), "{bad:?} should be refused");
        }
        assert!(mind_ring_path_in(&"x".repeat(65)).is_none());
        // Refused, specifically — not quietly redirected to whichever ring is local.
        assert_ne!(mind_ring_path_in("../evil"), Some(mind_ring_path()));
    }

    /// The mindview saved-layout sidecar is namespaced like every other one, so an
    /// Organon Mind session and a full Organon session never share a layout file.
    #[test]
    fn mindview_layout_sidecar_is_namespaced() {
        let ns = namespace();
        assert_eq!(mindview_layout_path(), ns_file_in(ns, "mindview.json"));
        assert_ne!(
            ns_file_in(crate::edition::Edition::Full.ipc_namespace(), "mindview.json"),
            ns_file_in(crate::edition::Edition::Mind.ipc_namespace(), "mindview.json"),
        );
    }
}

/// #541 S2 Tier 1 — the mindview reservation.
///
/// The point of every test here is the same as Phase B's in `mind_ring.rs`: a
/// selector that decodes to the wrong pane has **no symptom**. The plugin and the
/// visual are separate processes reading one mmap by byte offset, so a field
/// inserted instead of appended still compiles, still runs, and just draws the
/// wrong thing in the wrong rectangle. These convert that into failing tests
/// before there is any compositor to get it wrong.
#[cfg(test)]
mod mindview_tests {
    use super::*;

    /// **Zero is absent.** The whole reservation is inert by construction: a
    /// default snapshot is a Single grid, pane 0 focused, pane 0 showing the scene
    /// the visual already draws, and no saved layout. If this ever fails, the
    /// reservation has started doing something — which is a behaviour change
    /// smuggled in as a layout change.
    #[test]
    fn reservation_defaults_to_todays_single_viewport() {
        let s = Shared::default();
        assert!(s.mindview.iter().all(|&v| v == 0.0), "grid header must be all-zero");
        assert!(s.mindview_pane.iter().all(|&v| v == 0.0), "pane block must be all-zero");
        assert_eq!(s.mindview_gen, 0, "no saved layout loaded");

        assert_eq!(s.mindview_grid(), 0, "Single");
        assert_eq!(s.mindview_pane_count(), 1, "one viewport, as today");
        assert_eq!(s.mindview_focus(), 0);
        assert_eq!(s.mind_pane(0), MindPane::default(), "pane 0 = the scene, today's render");
        // Every pane decodes to the inert selection, not just pane 0.
        for i in 0..MINDVIEW_PANES {
            assert_eq!(s.mind_pane(i), MindPane::default(), "pane {i}");
        }
        // MindPane::default() is itself the inert selection, spelled out.
        let d = MindPane::default();
        assert_eq!((d.lens, d.layout, d.camera, d.detail), (0, 0, 0, 0.0));
    }

    /// **N panes, not one.** Four independent selections coexist at their assigned
    /// stride and do not bleed into each other — the property WS-A's pane grid
    /// multiplexes and the reason the block is indexed rather than scalar.
    #[test]
    fn every_pane_carries_its_own_selection() {
        let mut s = Shared::default();
        for i in 0..MINDVIEW_PANES {
            s.set_mind_pane(
                i,
                MindPane {
                    lens: (i + 1) as u32,
                    layout: (MINDVIEW_PANES - i) as u32,
                    camera: (i % 3) as u32,
                    detail: 0.5 * i as f32,
                },
            );
        }
        for i in 0..MINDVIEW_PANES {
            let p = s.mind_pane(i);
            assert_eq!(p.lens, (i + 1) as u32, "pane {i} lens");
            assert_eq!(p.layout, (MINDVIEW_PANES - i) as u32, "pane {i} layout");
            assert_eq!(p.camera, (i % 3) as u32, "pane {i} camera");
            assert_eq!(p.detail, 0.5 * i as f32, "pane {i} detail");
        }
        // The per-pane reserve is genuinely untouched — whoever claims slots 4..8
        // later inherits zeros, not a neighbour's leaked value.
        for i in 0..MINDVIEW_PANES {
            for slot in 4..MINDVIEW_PANE_SLOTS {
                assert_eq!(
                    s.mindview_pane[i * MINDVIEW_PANE_SLOTS + slot],
                    0.0,
                    "pane {i} reserved slot {slot} was written"
                );
            }
        }
        // Writing panes never disturbs the grid header.
        assert!(s.mindview.iter().all(|&v| v == 0.0));
    }

    /// A full writer → mmap → reader round trip through the real `Shared` channel,
    /// proving the appended block survives the wire at its assigned offsets AND
    /// that the pre-existing tail (`material_live`) still reads correctly — an
    /// overlapping append would corrupt the neighbour, not just the new field.
    #[test]
    fn mindview_round_trips_through_the_snapshot() {
        let mut s = Shared::default();
        s.mindview[0] = 3.0; // Quad
        s.mindview[1] = 2.0; // focus pane 2
        s.mindview[2] = 1.0; // panes share a selection
        s.mindview_gen = 9;
        s.set_mind_pane(0, MindPane { lens: 1, layout: 0, camera: 0, detail: 0.0 });
        s.set_mind_pane(2, MindPane { lens: 4, layout: 3, camera: 1, detail: 12.0 });
        let material_live = s.material_live;

        let bytes = bytemuck::bytes_of(&s).to_vec();
        let got: Shared = bytemuck::pod_read_unaligned(&bytes);

        assert_eq!(got.mindview_grid(), 3);
        assert_eq!(got.mindview_pane_count(), MINDVIEW_PANES);
        assert_eq!(got.mindview_focus(), 2);
        assert_eq!(got.mindview[2], 1.0);
        assert_eq!(got.mindview_gen, 9);
        assert_eq!(got.mind_pane(0), MindPane { lens: 1, layout: 0, camera: 0, detail: 0.0 });
        assert_eq!(got.mind_pane(2), MindPane { lens: 4, layout: 3, camera: 1, detail: 12.0 });
        assert_eq!(got.mind_pane(1), MindPane::default(), "an unset pane stays inert");
        assert_eq!(got.material_live, material_live, "the previous tail must be untouched");
    }

    /// Grid → pane count, and the degradation rule: an **unknown** grid from a
    /// newer writer must fall back to today's single viewport, never to an
    /// undrawable tiling. Same for a NaN/negative slot on a torn read.
    #[test]
    fn unknown_or_torn_selector_values_degrade_to_today() {
        let mut s = Shared::default();
        for (raw, grid, panes) in [
            (0.0, 0, 1),
            (1.0, 1, 2),
            (2.0, 2, 2),
            (3.0, 3, MINDVIEW_PANES),
            (4.0, 0, 1),    // a grid this build doesn't know
            (99.0, 0, 1),   // ditto
            (-1.0, 0, 1),   // nonsense
            (f32::NAN, 0, 1),
        ] {
            s.mindview[0] = raw;
            assert_eq!(s.mindview_grid(), grid, "grid for {raw}");
            assert_eq!(s.mindview_pane_count(), panes, "pane count for {raw}");
        }
        // Focus is clamped into the LIVE pane range — a stale focus of 3 while the
        // grid is 2-up must not index a pane that isn't on screen.
        s.mindview[0] = 1.0; // 2-up
        s.mindview[1] = 3.0;
        assert_eq!(s.mindview_focus(), 1, "focus clamps to the visible panes");
        s.mindview[1] = f32::NAN;
        assert_eq!(s.mindview_focus(), 0);
        // Non-finite / negative pane slots decode as the inert selection.
        s.mindview_pane[0] = f32::NAN;
        s.mindview_pane[1] = -3.0;
        s.mindview_pane[2] = f32::INFINITY;
        assert_eq!(s.mind_pane(0).lens, 0);
        assert_eq!(s.mind_pane(0).layout, 0);
        assert_eq!(s.mind_pane(0).camera, 0);
        // An out-of-range pane index is a no-op write and an inert read, not a panic.
        let before = s.mindview_pane;
        s.set_mind_pane(MINDVIEW_PANES, MindPane { lens: 7, ..MindPane::default() });
        // Compare bit patterns, not values. The block was deliberately seeded with NaN
        // a few lines up, and `NaN != NaN`, so `assert_eq!` on these arrays can never
        // pass however correct `set_mind_pane` is — both sides print identically and
        // still compare unequal. Bitwise is also the stronger claim for a "must not
        // touch" assertion: it catches a rewrite that swapped +0.0 for -0.0, which
        // `==` waves through.
        assert!(
            s.mindview_pane
                .iter()
                .zip(before.iter())
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "out-of-range write must not touch the block"
        );
        assert_eq!(s.mind_pane(MINDVIEW_PANES), MindPane::default());
        assert_eq!(s.mind_pane(usize::MAX), MindPane::default());
    }

    /// The reservation is sized as documented — 4 panes × 8 slots — so the stride
    /// arithmetic in `mind_pane`/`set_mind_pane` and the offsets pinned in
    /// `param_table` describe the same block.
    #[test]
    fn reservation_is_the_documented_size() {
        assert_eq!(MINDVIEW_PANES, 4, "quad is the grid ceiling (#484 T3 / PRD WS-A)");
        assert_eq!(MINDVIEW_PANE_SLOTS, 8, "4 assigned + 4 reserved per pane");
        assert_eq!(
            std::mem::size_of_val(&Shared::default().mindview_pane),
            MINDVIEW_PANES * MINDVIEW_PANE_SLOTS * 4
        );
        // A stale reader must reject a snapshot laid out by this build: the append
        // moved the size, so the version had to move with it.
        // 0x0285 (#618 T0a) changed no offset and no size — it re-defined `seq` as a
        // seqlock counter, which a size or offset check cannot see. The mindview
        // append is still the reason this is ≥ 0x0284. 0x0286 (organon#217 T3) appended
        // `glyph` / `glyph_cam` / `capsule` after `mindview_gen`, growing the struct again.
        assert_eq!(LAYOUT_VERSION, 0x0286, "the T3 look-control append sized it (0x0286)");
    }
}

/// #618 Tier 0a — the `Shared` seqlock.
///
/// Before this, `Shared` had no torn-read protection at all and `ARCHITECTURE.md`
/// §6 claimed it did. The `layout_version` check it pointed at catches a mismatched
/// *build*; it cannot catch a *tear*, because both halves of a torn record carry
/// the same version. These tests pin the guard that actually does the job.
#[cfg(test)]
mod seqlock_tests {
    use super::*;

    /// The writer and reader both address `seq` and `layout_version` positionally
    /// (`SEQ_OFF`, `HDR`) rather than through the struct, because the whole point is
    /// to touch them *without* decoding the other 8508 bytes. If either ever stopped
    /// being where this assumes, the seqlock would silently guard the wrong word —
    /// it would still compile, still run, and still publish garbage. So pin it.
    #[test]
    fn seq_and_layout_version_sit_at_the_front() {
        let s = Shared::default();
        let base = &s as *const Shared as usize;
        assert_eq!(&s.seq as *const u32 as usize - base, SEQ_OFF, "seq must be at SEQ_OFF");
        assert_eq!(&s.layout_version as *const u32 as usize - base, 4, "layout_version follows seq");
        assert_eq!(HDR, 8, "header = seq + layout_version");
    }

    /// A committed snapshot carries an EVEN, non-zero `seq`. Odd is reserved for
    /// "a write is in flight" and 0 for "nothing has ever been published"
    /// (`world.rs` reads 0 that way).
    #[test]
    fn committed_snapshots_carry_an_even_nonzero_seq() {
        let dir = std::env::temp_dir().join(format!("organon-seqlock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("even.bin");

        let mut w = Writer::create_at(&path).unwrap();
        let r = Reader::open_at(&path);
        for i in 1..=4u32 {
            let mut s = Shared::default();
            s.hdr_gen = i;
            w.write(s);
            let got = r.read();
            assert_eq!(got.hdr_gen, i, "the snapshot round-trips");
            assert_eq!(got.seq % 2, 0, "committed seq is even, got {}", got.seq);
            assert_ne!(got.seq, 0, "committed seq is never 0 (0 = never written)");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The guard itself: an odd counter means a writer is part-way through the body,
    /// so the reader must refuse the bytes rather than decode them. This is the case
    /// the old `layout_version`-only check waved straight through — note that the
    /// version here is perfectly valid, which is exactly why it proved nothing.
    #[test]
    fn a_write_in_flight_is_refused_not_decoded() {
        let dir = std::env::temp_dir().join(format!("organon-seqlock-tear-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tear.bin");

        let mut w = Writer::create_at(&path).unwrap();
        let mut good = Shared::default();
        good.hdr_gen = 7;
        w.write(good);

        // Simulate a writer stopped between step 1 and step 3: the counter is odd
        // and the body is a mixture. `layout_version` is untouched and correct.
        {
            let f = OpenOptions::new().read(true).write(true).open(&path).unwrap();
            let mut m = unsafe { memmap2::MmapMut::map_mut(&f).unwrap() };
            let stale = seq_cell(&m).load(Ordering::Relaxed);
            seq_cell(&m).store(stale | 1, Ordering::Relaxed);
            let version = u32::from_ne_bytes(m[4..HDR].try_into().unwrap());
            assert_eq!(version, LAYOUT_VERSION, "the tear leaves the version valid — that is the point");
        }

        let r = Reader::open_at(&path);
        let got = r.read();
        assert_eq!(got.hdr_gen, 0, "a torn snapshot must not reach the caller");
        assert_eq!(got.seq, 0, "refused reads return defaults");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The RESET path, which the #622 review caught bypassing the lock entirely.
    ///
    /// `Writer::create_at` used to slam `Shared::default()` in with a bare
    /// `copy_from_slice`, and nih-plug re-invokes `initialize()` (hence
    /// `Writer::create`) on sample-rate and buffer-size changes and on project
    /// reload — while the visual's `Reader` is already attached and polling. Neither
    /// guard caught it: the default's `seq` is 0, which is even and never moves
    /// during the copy, and its `layout_version` already matches, so a reader
    /// accepted a blend of the previous session's bytes and the fresh defaults.
    ///
    /// A reader must only ever see one whole image or the other, never a mixture.
    #[test]
    fn re_creating_the_map_under_a_live_reader_never_blends() {
        use std::sync::atomic::{AtomicBool, Ordering as O};
        use std::sync::Arc;

        let dir = std::env::temp_dir().join(format!("organon-seqlock-init-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("init.bin");

        let witnessed = |s: &Shared| [s.hdr_gen, s.nn_gen, s.field_gen, s.mindview_gen];
        let mut nine = Shared::default();
        nine.hdr_gen = 9;
        nine.nn_gen = 9;
        nine.field_gen = 9;
        nine.mindview_gen = 9;

        Writer::create_at(&path).unwrap().write(nine);

        // Same scheduling guard as the race test above: run until the reader has
        // actually observed published images, not for a fixed iteration count.
        let seen_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let writer_stop = stop.clone();
        let writer_seen = seen_count.clone();
        let p2 = path.clone();
        let recreator = std::thread::spawn(move || {
            for _ in 0..200_000 {
                // Each pass is exactly the `initialize()` → `Writer::create` path.
                Writer::create_at(&p2).unwrap().write(nine);
                if writer_seen.load(O::Relaxed) >= 200 {
                    break;
                }
            }
            writer_stop.store(true, O::Relaxed);
        });

        let r = Reader::open_at(&path);
        let mut seen = 0u32;
        while !stop.load(O::Relaxed) {
            let w = witnessed(&r.read());
            // Two legal images: freshly-created defaults (all zero) or the written
            // one (all nine). Anything else is a blend of the two.
            assert!(w == [0, 0, 0, 0] || w == [9, 9, 9, 9], "blended across a re-create: {w:?}");
            if w == [9, 9, 9, 9] {
                seen += 1;
                seen_count.store(seen, O::Relaxed);
            }
        }
        recreator.join().unwrap();
        assert!(seen > 0, "the reader never saw a published image — test proves nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real thing: a writer hammering the map while a reader reads it. Every
    /// snapshot the reader accepts must be internally consistent — here, all four
    /// witness fields agreeing — never a blend of two generations.
    ///
    /// Run against the OLD code this fails: an 8512-byte `copy_from_slice` is not
    /// atomic, and `pod_read_unaligned` will happily hand back the seam.
    #[test]
    fn a_concurrent_reader_never_observes_a_blend() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let dir = std::env::temp_dir().join(format!("organon-seqlock-race-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("race.bin");

        let mut w = Writer::create_at(&path).unwrap();
        // The writer runs until the READER has seen enough, not for a fixed count:
        // under parallel test load the writer can otherwise burn every iteration
        // before the reader thread is scheduled once, and the "proves nothing" guard
        // fires on scheduling rather than on a defect.
        let seen_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let writer_stop = stop.clone();
        let writer_seen = seen_count.clone();
        let writer = std::thread::spawn(move || {
            // Four witnesses SPREAD ACROSS THE STRUCT, all moving together: hdr_gen
            // sits near the front and mindview_gen is the very last field, so a seam
            // anywhere between them shows up as a disagreement. Witnesses packed side
            // by side would be copied by the same memcpy chunk and prove nothing.
            for i in 1..2_000_000u32 {
                let mut s = Shared::default();
                s.hdr_gen = i;
                s.nn_gen = i;
                s.field_gen = i;
                s.mindview_gen = i;
                w.write(s);
                if writer_seen.load(Ordering::Relaxed) >= 500 {
                    break;
                }
            }
            writer_stop.store(true, Ordering::Relaxed);
        });

        let r = Reader::open_at(&path);
        let mut seen_written = 0u32;
        while !stop.load(Ordering::Relaxed) {
            let s = r.read();
            if s.hdr_gen == 0 {
                continue; // defaults: not yet written, or a refused read
            }
            seen_written += 1;
            seen_count.store(seen_written, Ordering::Relaxed);
            assert_eq!(s.nn_gen, s.hdr_gen, "torn: nn_gen disagrees with hdr_gen");
            assert_eq!(s.field_gen, s.hdr_gen, "torn: field_gen disagrees with hdr_gen");
            assert_eq!(s.mindview_gen, s.hdr_gen, "torn: mindview_gen disagrees with hdr_gen");
            assert_eq!(s.seq % 2, 0, "accepted an in-flight snapshot");
        }
        writer.join().unwrap();
        assert!(seen_written > 0, "the reader never saw a published snapshot — test proves nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
