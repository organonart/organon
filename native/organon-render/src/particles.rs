//! Particle Aura (GitHub #81, Tiers 1 & 2): a GPU cloud of luminous motes
//! advected through the active generator's velocity field and drawn additively
//! into the linear HDR scene buffer (so it blooms + tonemaps with everything
//! else). Owns the particle storage, the coarse velocity-grid + node buffers,
//! the advection compute pipeline, and the additive billboard render pipeline.
//!
//! Included by `render.rs` via `#[path]` (like `post`/`metaball`), so it compiles
//! only into the visual binary. The CPU builds the velocity grid (analytic where
//! the generator exposes a field, splatted from node motion otherwise — both in
//! `math.rs::VelGrid`) and hands it here each frame; this module never touches
//! the algorithm, only its GPU plumbing. Off by default → zero work, identical
//! image.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};

/// Compute workgroup size (motes per group). Matches `@workgroup_size(64)`.
const WG: u32 = 64;

/// One mote: position + life, last velocity + max life, tint + random seed.
/// Lives only on the GPU (seeded by `cs_init`, stepped by `cs_advect`); the
/// layout mirrors `particles.wgsl::Particle` (3× vec4 = 48 bytes).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Particle {
    pos: [f32; 4],
    vel: [f32; 4],
    col: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SimU {
    grid_min: [f32; 4], // xyz, w = dt
    grid_max: [f32; 4], // xyz, w = time
    grid_res: [u32; 4], // xyz res, w = particle count
    p0: [f32; 4],       // speed, lifetime, spawn_radius, drag
    p1: [f32; 4],       // turbulence, max_step, node_count, _
    p2: [f32; 4],       // frame_seed, _, _, _
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DrawU {
    view_proj: [[f32; 4]; 4],
    cam_right: [f32; 4],
    cam_up: [f32; 4],
    params: [f32; 4],  // size, emissive, alpha, ribbon
    params2: [f32; 4], // ribbon_stretch, hue_shift, energize(0/1), energy_gain
    params3: [f32; 4], // energy_knee, energy_hue, energy_contrast, energy_hue_cycle
    // #298 Tier 1 beads — the scene's PBR/IBL context (consumed by fs_bead only).
    cam_pos: [f32; 4],    // xyz = camera world pos, w = prefilter mip count
    key_light: [f32; 4],  // xyz = dir TO key light, w = intensity
    fill_light: [f32; 4], // xyz = dir TO fill light, w = intensity
    env: [f32; 4],        // exposure, env_intensity, env_rotation, ambient_mul
    env_tint: [f32; 4],   // xyz = env tint, w unused
    bead: [f32; 4],       // beads on(0/1), metallic, roughness, emissive scale
    bead2: [f32; 4],      // #298 Tier 2: material, shape, ior, shape_param
    bead_hsv: [f32; 4],   // #305 Tier 1: effective hue, saturation, value, _
    skyrefl: [f32; 4],    // #305 Tier 2: enable, cover, drift phase, strength
    // PBR text T6 (#217): coaxial glass capsule — core fraction (0 = off), Beer–
    // Lambert density, reserved, reserved. Tail-appended; `fs_capsule` alone reads it.
    capsule: [f32; 4],
}

/// The scene's PBR/IBL shading context handed to the bead draw each frame (built in
/// `render.rs` from the cube `Uniforms` + the live IBL). Unused by the additive spark
/// path. Mirrors the fields fs_bead reads out of `DrawU`.
pub struct ParticleShade {
    pub cam_pos: Vec3,
    pub prefilter_mips: f32,
    /// xyz = world dir TO the key/fill light (unit), w = intensity.
    pub key_light: Vec4,
    pub fill_light: Vec4,
    /// exposure, env_intensity, env_rotation (rad), ambient_mul.
    pub env: Vec4,
    pub env_tint: Vec3,
    /// #305 Tier 2: live-sky cloud reflection [enable, cover, drift phase, strength].
    pub skyrefl: Vec4,
}

/// Everything the visual hands the particle system each frame. The velocity grid
/// (`vel_grid`, length `grid_res.x·y·z`) and `nodes` (respawn anchors) are built
/// CPU-side in `bin/visual.rs` from the active generator.
pub struct ParticlesFrame<'a> {
    pub enabled: bool,
    pub count: u32,
    pub grid_res: [u32; 3],
    pub grid_min: Vec3,
    pub grid_max: Vec3,
    pub vel_grid: &'a [Vec4],
    pub nodes: &'a [Vec4],
    /// Unscaled world → clip (motes live in true world space, not breath-scaled).
    pub view_proj: Mat4,
    pub cam_right: Vec3,
    pub cam_up: Vec3,
    pub dt: f32,
    pub time: f32,
    pub frame_seed: u32,
    pub speed: f32,
    pub lifetime: f32,
    pub spawn_radius: f32,
    pub drag: f32,
    pub turbulence: f32,
    pub max_step: f32,
    pub size: f32,
    pub emissive: f32,
    pub alpha: f32,
    pub ribbon: bool,
    pub ribbon_stretch: f32,
    pub hue_shift: f32,
    /// Maxwell field energization (#247 Tier 1): when set, each mote samples the field
    /// **energy density** riding in the velocity grid's `w` channel and glows by it
    /// (log/soft-knee tone-mapped), overriding the speed-based ember colour — the
    /// fluorescent-tube demo. Set only for the Maxwell generator on the Lite tier.
    pub energize: bool,
    pub energy_gain: f32,
    pub energy_knee: f32,
    pub energy_hue: f32,
    /// #248 near-core contrast: sampled energy is raised to this power before the
    /// tone-map so the high-energy core stands out. 1 = the original flat look.
    pub energy_contrast: f32,
    /// #248 hue-cycle phase (turns): added to the energized motes' hue so the vortex
    /// cycles through the colour wheel with the beat. 0 = the fixed ember hue.
    pub energy_hue_cycle: f32,
    /// Force a re-seed of all motes (first enable / particle count change).
    pub reseed: bool,
    /// Aura-Fluid tier: when set, the motes ride the persistent Navier–Stokes field
    /// (stepped in `render.rs`) instead of the raw source grid, and `simulate` binds
    /// that external velocity buffer rather than uploading `vel_grid`.
    pub fluid: bool,
    /// Fluid solver controls (used only when `fluid`).
    pub fluid_params: super::FluidParams,
    /// #298 Tier 1: draw the motes as opaque sphere-impostor **beads** shaded by the
    /// shared IBL + key/fill, instead of the additive spark billboards. Off = sparks.
    pub beads: bool,
    /// Bead PBR material (only read when `beads`): metalness + roughness.
    pub bead_metallic: f32,
    pub bead_roughness: f32,
    /// #298 Tier 2: bead material (0 Standard / 1 Chrome / 2 Glass / 3 Refractive),
    /// impostor shape (0 Sphere / 1 Ellipsoid / 2 Teardrop / 3 RoundedBox / 4 Dice),
    /// Glass/Refractive IOR, and the non-sphere shape stretch/roundness amount.
    pub bead_material: u32,
    pub bead_shape: u32,
    pub bead_ior: f32,
    pub bead_shape_param: f32,
    /// #305 Tier 1: bead material HSV — effective hue (base + cycle·beat), saturation,
    /// value. [0, 1, 1] → byte-identical.
    pub bead_hue: f32,
    pub bead_sat: f32,
    pub bead_val: f32,
    /// Material Emissive (HDR): the bead emits its own hue × this value. 0 = off.
    pub bead_emissive: f32,
}

pub struct ParticleSystem {
    // Compute (advect)
    init_pipeline: wgpu::ComputePipeline,
    advect_pipeline: wgpu::ComputePipeline,
    compute_bgl: wgpu::BindGroupLayout,
    compute_bind: wgpu::BindGroup,
    sim_ub: wgpu::Buffer,
    particle_buf: wgpu::Buffer,
    particle_cap: usize,
    velgrid_buf: wgpu::Buffer,
    velgrid_cap: usize,
    node_buf: wgpu::Buffer,
    node_cap: usize,
    // Draw (additive billboards)
    draw_shader: wgpu::ShaderModule,
    draw_layout: wgpu::PipelineLayout,
    draw_pipeline: wgpu::RenderPipeline,
    draw_ub: wgpu::Buffer,
    draw_bind: wgpu::BindGroup,
    // Draw (opaque sphere-impostor beads, #298 Tier 1): a second pipeline that also
    // binds the IBL group (group 1) and writes depth.
    bead_layout: wgpu::PipelineLayout,
    bead_pipeline: wgpu::RenderPipeline,
    // Depth-only bead draw for the FX depth prepass (#298 Tier 3). Single-sample
    // (the FX prepass depth is always 1×; the shared route only triggers at MSAA 1×),
    // so it never needs a sample-count rebuild.
    bead_depth_pipeline: wgpu::RenderPipeline,
    // Membrane Skin-Arms capsule impostors (Stage 2): per-arm-segment capsule
    // billboards, IBL-shaded like beads but per-instance geometry + strand tint.
    arm_pipeline: wgpu::RenderPipeline,
    arm_depth_pipeline: wgpu::RenderPipeline,
    arm_draw_ub: wgpu::Buffer,
    arm_draw_bind: wgpu::BindGroup,
    arm_buf: wgpu::Buffer,
    arm_cap: usize,
    arm_count: u32,
    arm_drew: bool,
    // Plexus impostors (#plexus Tier 2): two self-contained capsule batches — nodes
    // as A≈B degenerate capsules (= analytic spheres), edges as capsules — each with
    // its OWN DrawU material context (independent node vs edge materials). Both reuse
    // the validated `arm_pipeline` / `arm_depth_pipeline` (same capsule shaders); only
    // the instance data + material uniform differ. Doesn't touch the membrane-arm path.
    plex_node_ub: wgpu::Buffer,
    plex_node_bind: wgpu::BindGroup,
    plex_node_buf: wgpu::Buffer,
    plex_node_cap: usize,
    plex_node_count: u32,
    plex_edge_ub: wgpu::Buffer,
    plex_edge_bind: wgpu::BindGroup,
    plex_edge_buf: wgpu::Buffer,
    plex_edge_cap: usize,
    plex_edge_count: u32,
    plex_drew: bool,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
    // Live frame state
    count: u32,
    drew_this_frame: bool,
    beads: bool,
    // PBR text T6 (#217): the coaxial-glass knob for every capsule draw (arms + both
    // plexus batches) — [core fraction, absorption density]. Defaults [0, 0] = inert.
    // Not a param-chain value (look controls are T3): `set_capsule_core` is the
    // render-side API, and `ORGANON_CAPSULE_CORE` seeds it so a GPU session can
    // look before any control exists.
    capsule_core: [f32; 2],
}

impl ParticleSystem {
    pub fn new(
        device: &wgpu::Device,
        ibl_layout: &wgpu::BindGroupLayout,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("particles"),
            source: wgpu::ShaderSource::Wgsl(include_str!("particles.wgsl").into()),
        });

        // --- Compute: SimU(0) + particles rw(1) + velgrid ro(2) + nodes ro(3) ---
        let compute_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particles-compute-layout"),
            entries: &[
                ubo_entry(0, wgpu::ShaderStages::COMPUTE),
                storage_entry(1, false),
                storage_entry(2, true),
                storage_entry(3, true),
            ],
        });
        let compute_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particles-compute-pl"),
            bind_group_layouts: &[Some(&compute_bgl)],
            immediate_size: 0,
        });
        let init_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("particles-init"),
            layout: Some(&compute_layout),
            module: &shader,
            entry_point: Some("cs_init"),
            compilation_options: Default::default(),
            cache: None,
        });
        let advect_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("particles-advect"),
            layout: Some(&compute_layout),
            module: &shader,
            entry_point: Some("cs_advect"),
            compilation_options: Default::default(),
            cache: None,
        });

        let sim_ub = uniform_buf(device, "particles-sim-ub", std::mem::size_of::<SimU>());
        let particle_cap = 1usize;
        let particle_buf = make_particle_buf(device, particle_cap);
        let velgrid_cap = 1usize;
        let velgrid_buf = make_storage_buf(device, "particles-velgrid", velgrid_cap);
        let node_cap = 1usize;
        let node_buf = make_storage_buf(device, "particles-nodes", node_cap);
        let compute_bind = make_compute_bind(
            device, &compute_bgl, &sim_ub, &particle_buf, &velgrid_buf, &node_buf,
        );

        // --- Draw: DrawU(0); the particle buffer rides in as an instance vbuf ---
        let draw_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particles-draw-layout"),
            entries: &[ubo_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT)],
        });
        let draw_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particles-draw-pl"),
            bind_group_layouts: &[Some(&draw_bgl)],
            immediate_size: 0,
        });
        let draw_ub = uniform_buf(device, "particles-draw-ub", std::mem::size_of::<DrawU>());
        let draw_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particles-draw-bind"),
            layout: &draw_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: draw_ub.as_entire_binding() }],
        });
        let draw_pipeline = make_draw_pipeline(
            device, &shader, &draw_layout, color_format, depth_format, sample_count,
        );

        // --- Bead draw (#298 Tier 1): group0 = DrawU (reuse draw_bgl), group1 = IBL ---
        let bead_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particles-bead-pl"),
            bind_group_layouts: &[Some(&draw_bgl), Some(ibl_layout)],
            immediate_size: 0,
        });
        let bead_pipeline = make_bead_pipeline(
            device, &shader, &bead_layout, color_format, depth_format, sample_count,
            "vs_bead", "fs_bead",
        );
        // Depth-only prepass bead pipeline (#298 Tier 3): group 0 = DrawU (draw_layout),
        // single-sample, no colour target. Always 1× (see the field comment).
        let bead_depth_pipeline =
            make_bead_depth_pipeline(device, &shader, &draw_layout, depth_format, "vs_bead", "fs_bead_depth");

        // --- Membrane Skin-Arms capsule impostors (Stage 2): same bead layout
        // (group0 DrawU + group1 IBL) + instance vbuf, capsule entry points. Its
        // own DrawU (arm material/tint context) + growable instance buffer. ---
        let arm_pipeline = make_bead_pipeline(
            device, &shader, &bead_layout, color_format, depth_format, sample_count,
            "vs_capsule", "fs_capsule",
        );
        let arm_depth_pipeline = make_bead_depth_pipeline(
            device, &shader, &draw_layout, depth_format, "vs_capsule", "fs_capsule_depth",
        );
        let arm_draw_ub = uniform_buf(device, "particles-arm-ub", std::mem::size_of::<DrawU>());
        let arm_draw_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particles-arm-bind"),
            layout: &draw_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: arm_draw_ub.as_entire_binding() }],
        });
        let arm_cap = 1usize;
        let arm_buf = make_particle_buf(device, arm_cap);

        // Plexus impostor batches (Tier 2): two DrawU contexts + instance buffers,
        // reusing draw_bgl + arm_pipeline. Start at cap 1; grown on first upload.
        let plex_node_ub = uniform_buf(device, "plex-node-ub", std::mem::size_of::<DrawU>());
        let plex_node_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("plex-node-bind"),
            layout: &draw_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: plex_node_ub.as_entire_binding() }],
        });
        let plex_node_buf = make_particle_buf(device, 1);
        let plex_edge_ub = uniform_buf(device, "plex-edge-ub", std::mem::size_of::<DrawU>());
        let plex_edge_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("plex-edge-bind"),
            layout: &draw_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: plex_edge_ub.as_entire_binding() }],
        });
        let plex_edge_buf = make_particle_buf(device, 1);

        ParticleSystem {
            init_pipeline,
            advect_pipeline,
            compute_bgl,
            compute_bind,
            sim_ub,
            particle_buf,
            particle_cap,
            velgrid_buf,
            velgrid_cap,
            node_buf,
            node_cap,
            draw_shader: shader,
            draw_layout,
            draw_pipeline,
            draw_ub,
            draw_bind,
            bead_layout,
            bead_pipeline,
            bead_depth_pipeline,
            arm_pipeline,
            arm_depth_pipeline,
            arm_draw_ub,
            arm_draw_bind,
            arm_buf,
            arm_cap,
            arm_count: 0,
            arm_drew: false,
            plex_node_ub,
            plex_node_bind,
            plex_node_buf,
            plex_node_cap: 1,
            plex_node_count: 0,
            plex_edge_ub,
            plex_edge_bind,
            plex_edge_buf,
            plex_edge_cap: 1,
            plex_edge_count: 0,
            plex_drew: false,
            color_format,
            depth_format,
            sample_count,
            count: 0,
            drew_this_frame: false,
            beads: false,
            capsule_core: capsule_core::from_env(),
        }
    }

    /// PBR text T6 (#217): set the coaxial-glass core for every capsule impostor draw.
    /// `core_frac` is the inner emissive capsule's radius as a fraction of the outer
    /// (clamped to [0, 1]; **0 = off, pixel-identical to the pre-T6 frame**), `absorb`
    /// the Beer–Lambert density per outer radius (≥ 0; 0 = a clear shell). Only
    /// Glass/Refractive capsules read it. Takes effect at the next `set_arms` /
    /// `set_plexus` upload.
    pub fn set_capsule_core(&mut self, core_frac: f32, absorb: f32) {
        self.capsule_core = capsule_core::lanes(core_frac, absorb);
    }

    fn capsule_lanes(&self) -> [f32; 4] {
        [self.capsule_core[0], self.capsule_core[1], 0.0, 0.0]
    }

    /// Rebuild the additive draw pipeline for a new MSAA sample count (the motes
    /// render into the multisampled scene target). No-op if unchanged.
    pub fn set_sample_count(&mut self, device: &wgpu::Device, n: u32) {
        let n = n.max(1);
        if n == self.sample_count {
            return;
        }
        self.sample_count = n;
        self.draw_pipeline = make_draw_pipeline(
            device,
            &self.draw_shader,
            &self.draw_layout,
            self.color_format,
            self.depth_format,
            n,
        );
        self.bead_pipeline = make_bead_pipeline(
            device,
            &self.draw_shader,
            &self.bead_layout,
            self.color_format,
            self.depth_format,
            n,
            "vs_bead",
            "fs_bead",
        );
        self.arm_pipeline = make_bead_pipeline(
            device,
            &self.draw_shader,
            &self.bead_layout,
            self.color_format,
            self.depth_format,
            n,
            "vs_capsule",
            "fs_capsule",
        );
    }

    /// Upload the velocity grid + respawn nodes + per-frame uniforms, grow the
    /// buffers as needed, and run the compute passes (init on reseed, then
    /// advect). Call once per frame BEFORE the scene pass. Disabled / empty →
    /// nothing runs and `draw` becomes a no-op.
    pub fn simulate(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        f: &ParticlesFrame,
        // Fluid tier: the evolved Navier–Stokes velocity field (res³ vec4) the motes
        // sample instead of this system's own (CPU-uploaded) velocity grid.
        external_velgrid: Option<&wgpu::Buffer>,
        // #298 Tier 1: the scene PBR/IBL context the bead draw shades with.
        shade: &ParticleShade,
    ) {
        self.drew_this_frame = false;
        let count = f.count;
        if !f.enabled || count == 0 || f.vel_grid.is_empty() {
            self.count = 0;
            return;
        }

        // Grow GPU buffers (power-of-two) when the demand exceeds capacity. Always
        // rebuild the compute bind group (cheap) so it points at the right velocity
        // grid — our own (Lite) or the fluid's (Fluid).
        if count as usize > self.particle_cap {
            self.particle_cap = (count as usize).next_power_of_two();
            self.particle_buf = make_particle_buf(device, self.particle_cap);
        }
        if external_velgrid.is_none() && f.vel_grid.len() > self.velgrid_cap {
            self.velgrid_cap = f.vel_grid.len().next_power_of_two();
            self.velgrid_buf = make_storage_buf(device, "particles-velgrid", self.velgrid_cap);
        }
        let node_len = f.nodes.len().max(1);
        if node_len > self.node_cap {
            self.node_cap = node_len.next_power_of_two();
            self.node_buf = make_storage_buf(device, "particles-nodes", self.node_cap);
        }
        let velgrid = external_velgrid.unwrap_or(&self.velgrid_buf);
        self.compute_bind = make_compute_bind(
            device,
            &self.compute_bgl,
            &self.sim_ub,
            &self.particle_buf,
            velgrid,
            &self.node_buf,
        );

        // Upload the field (Lite only — in Fluid mode the solver already wrote it) +
        // respawn anchors.
        if external_velgrid.is_none() {
            queue.write_buffer(&self.velgrid_buf, 0, bytemuck::cast_slice(f.vel_grid));
        }
        if !f.nodes.is_empty() {
            queue.write_buffer(&self.node_buf, 0, bytemuck::cast_slice(f.nodes));
        }

        let sim = SimU {
            grid_min: [f.grid_min.x, f.grid_min.y, f.grid_min.z, f.dt],
            grid_max: [f.grid_max.x, f.grid_max.y, f.grid_max.z, f.time],
            grid_res: [f.grid_res[0], f.grid_res[1], f.grid_res[2], count],
            p0: [f.speed, f.lifetime, f.spawn_radius, f.drag],
            p1: [f.turbulence, f.max_step, f.nodes.len() as f32, 0.0],
            // p2.y = energize (#247): only then does cs_advect sample the grid's `w`
            // energy channel (a fluid grid's `w` is not energy — Tier 3 handles that).
            p2: [f.frame_seed as f32, if f.energize { 1.0 } else { 0.0 }, 0.0, 0.0],
        };
        queue.write_buffer(&self.sim_ub, 0, bytemuck::bytes_of(&sim));

        let en = if f.energize { 1.0 } else { 0.0 };
        // Bead emissive scale: keep the vortex glow present but let the PBR dominate
        // (a look constant; tune on-Mac). Ribbons are meaningless for beads → forced off.
        let draw = DrawU {
            view_proj: f.view_proj.to_cols_array_2d(),
            cam_right: [f.cam_right.x, f.cam_right.y, f.cam_right.z, 0.0],
            cam_up: [f.cam_up.x, f.cam_up.y, f.cam_up.z, 0.0],
            params: [f.size, f.emissive, f.alpha, if f.ribbon { 1.0 } else { 0.0 }],
            params2: [f.ribbon_stretch, f.hue_shift, en, f.energy_gain],
            params3: [f.energy_knee, f.energy_hue, f.energy_contrast, f.energy_hue_cycle],
            cam_pos: [shade.cam_pos.x, shade.cam_pos.y, shade.cam_pos.z, shade.prefilter_mips],
            key_light: shade.key_light.to_array(),
            fill_light: shade.fill_light.to_array(),
            env: shade.env.to_array(),
            env_tint: [shade.env_tint.x, shade.env_tint.y, shade.env_tint.z, 0.0],
            bead: [if f.beads { 1.0 } else { 0.0 }, f.bead_metallic, f.bead_roughness, 0.5],
            bead2: [f.bead_material as f32, f.bead_shape as f32, f.bead_ior, f.bead_shape_param],
            bead_hsv: [f.bead_hue, f.bead_sat, f.bead_val, f.bead_emissive],
            skyrefl: shade.skyrefl.to_array(),
            capsule: [0.0; 4], // beads/sparks never read it
        };
        queue.write_buffer(&self.draw_ub, 0, bytemuck::bytes_of(&draw));
        self.beads = f.beads;

        let groups = count.div_ceil(WG);
        {
            let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("particles-sim-pass"),
                timestamp_writes: None,
            });
            // Reseed (first enable / count change) before stepping, so motes start
            // distributed near the geometry rather than at uninitialized memory.
            if f.reseed || self.count != count {
                cp.set_pipeline(&self.init_pipeline);
                cp.set_bind_group(0, &self.compute_bind, &[]);
                cp.dispatch_workgroups(groups, 1, 1);
            }
            cp.set_pipeline(&self.advect_pipeline);
            cp.set_bind_group(0, &self.compute_bind, &[]);
            cp.dispatch_workgroups(groups, 1, 1);
        }

        self.count = count;
        self.drew_this_frame = true;
    }

    /// Draw the motes into the active scene render pass (after the geometry, before
    /// post). No-op if `simulate` ran nothing this frame. `ibl_bind` is the shared IBL
    /// group (bound only for the opaque bead path); the additive spark path ignores it.
    pub fn draw<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, ibl_bind: &'a wgpu::BindGroup) {
        if !self.drew_this_frame || self.count == 0 {
            return;
        }
        if self.beads {
            // #298 Tier 1: opaque sphere-impostor droplets, PBR-shaded by the IBL.
            rp.set_pipeline(&self.bead_pipeline);
            rp.set_bind_group(0, &self.draw_bind, &[]);
            rp.set_bind_group(1, ibl_bind, &[]);
        } else {
            rp.set_pipeline(&self.draw_pipeline);
            rp.set_bind_group(0, &self.draw_bind, &[]);
        }
        rp.set_vertex_buffer(0, self.particle_buf.slice(..));
        // 6 verts per mote (two triangles), one instance per particle.
        rp.draw(0..6, 0..self.count);
    }

    /// #298 Tier 3: draw the beads (depth only) into the single-sample FX depth
    /// prepass, so the screen-space effects (SSAO / SSR / SSGI / DoF / TAA) that
    /// reconstruct from that depth see the droplets as first-class scene geometry.
    /// No-op unless beads are on and `simulate` ran this frame. `DrawU` (group 0) is
    /// already uploaded by `simulate`, so only the pipeline + vbuf are set here.
    pub fn draw_depth<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>) {
        if !self.drew_this_frame || self.count == 0 || !self.beads {
            return;
        }
        rp.set_pipeline(&self.bead_depth_pipeline);
        rp.set_bind_group(0, &self.draw_bind, &[]);
        rp.set_vertex_buffer(0, self.particle_buf.slice(..));
        rp.draw(0..6, 0..self.count);
    }

    /// Membrane Skin-Arms (Stage 2): upload the per-segment capsule impostors + the
    /// shading context for this frame. `caps` is one `ArmInstance` per arm segment
    /// (A + radius, B, strand tint). Empty `caps` clears the arm draw (no-op next
    /// draw). Call every frame; the DrawU carries the arm Material / metallic /
    /// roughness / IOR so the capsules shade through the SAME PBR/IBL as the cubes.
    #[allow(clippy::too_many_arguments)]
    pub fn set_arms(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        caps: &[ArmInstance],
        view_proj: Mat4,
        cam_right: Vec3,
        cam_up: Vec3,
        shade: &ParticleShade,
        material: f32,
        metallic: f32,
        roughness: f32,
        ior: f32,
        glow: f32,
        hsv: Vec4,
    ) {
        self.arm_drew = !caps.is_empty();
        self.arm_count = caps.len() as u32;
        if caps.is_empty() {
            return;
        }
        let draw = DrawU {
            view_proj: view_proj.to_cols_array_2d(),
            cam_right: [cam_right.x, cam_right.y, cam_right.z, 0.0],
            cam_up: [cam_up.x, cam_up.y, cam_up.z, 0.0],
            // params.y = Material Glow → fs_capsule's `emissive = albedo · glow`.
            params: [1.0, glow, 1.0, 0.0],
            params2: [0.0, 0.0, 0.0, 0.0],
            params3: [0.0, 0.0, 0.0, 0.0],
            cam_pos: [shade.cam_pos.x, shade.cam_pos.y, shade.cam_pos.z, shade.prefilter_mips],
            key_light: shade.key_light.to_array(),
            fill_light: shade.fill_light.to_array(),
            env: shade.env.to_array(),
            env_tint: [shade.env_tint.x, shade.env_tint.y, shade.env_tint.z, 0.0],
            bead: [1.0, metallic, roughness, 0.5],
            bead2: [material, 0.0, ior, 0.0],
            bead_hsv: hsv.to_array(),
            skyrefl: shade.skyrefl.to_array(),
            capsule: self.capsule_lanes(),
        };
        queue.write_buffer(&self.arm_draw_ub, 0, bytemuck::bytes_of(&draw));

        // Grow the instance buffer if needed (double until it fits).
        if caps.len() > self.arm_cap {
            let mut cap = self.arm_cap.max(1);
            while cap < caps.len() {
                cap *= 2;
            }
            self.arm_cap = cap;
            self.arm_buf = make_particle_buf(device, self.arm_cap);
        }
        queue.write_buffer(&self.arm_buf, 0, bytemuck::cast_slice(caps));
    }

    /// Draw the Skin-Arms capsule impostors into the scene pass (opaque, PBR/IBL-
    /// shaded, depth-written). No-op unless `set_arms` uploaded any this frame.
    pub fn draw_arms<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, ibl_bind: &'a wgpu::BindGroup) {
        if !self.arm_drew || self.arm_count == 0 {
            return;
        }
        rp.set_pipeline(&self.arm_pipeline);
        rp.set_bind_group(0, &self.arm_draw_bind, &[]);
        rp.set_bind_group(1, ibl_bind, &[]);
        rp.set_vertex_buffer(0, self.arm_buf.slice(..));
        rp.draw(0..6, 0..self.arm_count);
    }

    /// Depth-only Skin-Arms draw for the FX depth prepass (mirrors `draw_depth`).
    pub fn draw_arms_depth<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>) {
        if !self.arm_drew || self.arm_count == 0 {
            return;
        }
        rp.set_pipeline(&self.arm_depth_pipeline);
        rp.set_bind_group(0, &self.arm_draw_bind, &[]);
        rp.set_vertex_buffer(0, self.arm_buf.slice(..));
        rp.draw(0..6, 0..self.arm_count);
    }

    /// Plexus Tier 2: upload the node (sphere) + edge (tube) capsule impostors and
    /// their TWO independent material contexts. `nodes`/`edges` are `ArmInstance`s
    /// (node spheres are A≈B degenerate capsules built caller-side). Empty slices
    /// clear that batch. Call every frame; no-op draw unless something was uploaded.
    #[allow(clippy::too_many_arguments)]
    pub fn set_plexus(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        nodes: &[ArmInstance],
        edges: &[ArmInstance],
        view_proj: Mat4,
        cam_right: Vec3,
        cam_up: Vec3,
        shade: &ParticleShade,
        node_mat: PlexMat,
        edge_mat: PlexMat,
    ) {
        self.plex_node_count = nodes.len() as u32;
        self.plex_edge_count = edges.len() as u32;
        self.plex_drew = !nodes.is_empty() || !edges.is_empty();
        if !self.plex_drew {
            return;
        }
        // Read the knob before the closure: `mk` must not borrow `self`, which the
        // buffer-growth arms below mutate while `mk` is still alive.
        let capsule = self.capsule_lanes();
        let mk = |m: &PlexMat| DrawU {
            capsule,
            view_proj: view_proj.to_cols_array_2d(),
            cam_right: [cam_right.x, cam_right.y, cam_right.z, 0.0],
            cam_up: [cam_up.x, cam_up.y, cam_up.z, 0.0],
            params: [1.0, m.glow, 1.0, 0.0], // params.y = glow → fs_capsule emissive
            params2: [0.0, 0.0, 0.0, 0.0],
            params3: [0.0, 0.0, 0.0, 0.0],
            cam_pos: [shade.cam_pos.x, shade.cam_pos.y, shade.cam_pos.z, shade.prefilter_mips],
            key_light: shade.key_light.to_array(),
            fill_light: shade.fill_light.to_array(),
            env: shade.env.to_array(),
            env_tint: [shade.env_tint.x, shade.env_tint.y, shade.env_tint.z, 0.0],
            bead: [1.0, m.metallic, m.roughness, 0.5],
            bead2: [m.material, 0.0, m.ior, 0.0],
            bead_hsv: m.hsv.to_array(),
            skyrefl: shade.skyrefl.to_array(),
        };
        if !nodes.is_empty() {
            queue.write_buffer(&self.plex_node_ub, 0, bytemuck::bytes_of(&mk(&node_mat)));
            if nodes.len() > self.plex_node_cap {
                let mut cap = self.plex_node_cap.max(1);
                while cap < nodes.len() {
                    cap *= 2;
                }
                self.plex_node_cap = cap;
                self.plex_node_buf = make_particle_buf(device, self.plex_node_cap);
            }
            queue.write_buffer(&self.plex_node_buf, 0, bytemuck::cast_slice(nodes));
        }
        if !edges.is_empty() {
            queue.write_buffer(&self.plex_edge_ub, 0, bytemuck::bytes_of(&mk(&edge_mat)));
            if edges.len() > self.plex_edge_cap {
                let mut cap = self.plex_edge_cap.max(1);
                while cap < edges.len() {
                    cap *= 2;
                }
                self.plex_edge_cap = cap;
                self.plex_edge_buf = make_particle_buf(device, self.plex_edge_cap);
            }
            queue.write_buffer(&self.plex_edge_buf, 0, bytemuck::cast_slice(edges));
        }
    }

    /// Draw the plexus impostors (edges then nodes) into the scene pass — opaque,
    /// PBR/IBL-shaded, depth-written. No-op unless `set_plexus` uploaded any.
    pub fn draw_plexus<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, ibl_bind: &'a wgpu::BindGroup) {
        if !self.plex_drew {
            return;
        }
        rp.set_pipeline(&self.arm_pipeline);
        rp.set_bind_group(1, ibl_bind, &[]);
        if self.plex_edge_count > 0 {
            rp.set_bind_group(0, &self.plex_edge_bind, &[]);
            rp.set_vertex_buffer(0, self.plex_edge_buf.slice(..));
            rp.draw(0..6, 0..self.plex_edge_count);
        }
        if self.plex_node_count > 0 {
            rp.set_bind_group(0, &self.plex_node_bind, &[]);
            rp.set_vertex_buffer(0, self.plex_node_buf.slice(..));
            rp.draw(0..6, 0..self.plex_node_count);
        }
    }

    /// Depth-only plexus-impostor draw for the FX depth prepass.
    pub fn draw_plexus_depth<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>) {
        if !self.plex_drew {
            return;
        }
        rp.set_pipeline(&self.arm_depth_pipeline);
        if self.plex_edge_count > 0 {
            rp.set_bind_group(0, &self.plex_edge_bind, &[]);
            rp.set_vertex_buffer(0, self.plex_edge_buf.slice(..));
            rp.draw(0..6, 0..self.plex_edge_count);
        }
        if self.plex_node_count > 0 {
            rp.set_bind_group(0, &self.plex_node_bind, &[]);
            rp.set_vertex_buffer(0, self.plex_node_buf.slice(..));
            rp.draw(0..6, 0..self.plex_node_count);
        }
    }
}

/// One plexus impostor material context (Tier 2). Mirrors the DrawU material lanes
/// so node and edge materials are fully independent.
#[derive(Clone, Copy)]
pub struct PlexMat {
    pub material: f32,
    pub metallic: f32,
    pub roughness: f32,
    pub ior: f32,
    pub glow: f32,
    pub hsv: Vec4,
}

/// One membrane arm-segment capsule impostor (Stage 2): endpoint A + radius, B,
/// and the strand tint. Layout matches the capsule vertex attributes (3× vec4).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct ArmInstance {
    /// A.xyz (world) + capsule radius (w).
    pub a_r: [f32; 4],
    /// B.xyz (world) + unused.
    pub b: [f32; 4],
    /// Strand tint rgb + unused.
    pub color: [f32; 4],
}

fn ubo_entry(binding: u32, vis: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: vis,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform_buf(device: &wgpu::Device, label: &str, size: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn make_particle_buf(device: &wgpu::Device, cap: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("particles-buf"),
        size: (cap * std::mem::size_of::<Particle>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn make_storage_buf(device: &wgpu::Device, label: &str, cap: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (cap * std::mem::size_of::<[f32; 4]>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn make_compute_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sim_ub: &wgpu::Buffer,
    particle_buf: &wgpu::Buffer,
    velgrid_buf: &wgpu::Buffer,
    node_buf: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("particles-compute-bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: sim_ub.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: particle_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: velgrid_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: node_buf.as_entire_binding() },
        ],
    })
}

fn make_draw_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    // The particle buffer rides in as a per-instance vertex buffer: pos / vel /
    // col at locations 0–2 (3× vec4 = 48-byte stride).
    let instance_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Particle>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &[
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 0, shader_location: 0 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16, shader_location: 1 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 32, shader_location: 2 },
        ],
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("particles-draw-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_particle"),
            buffers: &[Some(instance_layout)],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_particle"),
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                // Additive: the motes glow and roll off in the HDR post chain.
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        // Depth-tested against the scene (occluded by geometry) but no write, so
        // the additive motes don't block each other.
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: sample_count, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

/// #298 Tier 3: the depth-only bead pipeline for the FX depth prepass. Same
/// per-instance particle vbuf + `vs_bead`, but a fragment (`fs_bead_depth`) that
/// writes only the impostor's `frag_depth` (no colour target). Single-sample, depth
/// write on with a `Less` test (matching the prepass), so the droplets land in the
/// depth the screen-space effects reconstruct from.
fn make_bead_depth_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    depth_format: wgpu::TextureFormat,
    vs_entry: &str,
    fs_entry: &str,
) -> wgpu::RenderPipeline {
    let instance_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Particle>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &[
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 0, shader_location: 0 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16, shader_location: 1 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 32, shader_location: 2 },
        ],
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("particles-bead-depth-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vs_entry),
            buffers: &[Some(instance_layout)],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fs_entry),
            targets: &[], // depth only
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: 1, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

/// #298 Tier 1: the opaque sphere-impostor bead pipeline. Same per-instance particle
/// vbuf as the spark draw, but shades with the IBL (group 1), writes `frag_depth`
/// (the bulged hemisphere), and depth-writes opaquely so the droplets occlude each
/// other + the scene. No blend (replace).
fn make_bead_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
    vs_entry: &str,
    fs_entry: &str,
) -> wgpu::RenderPipeline {
    let instance_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Particle>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &[
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 0, shader_location: 0 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16, shader_location: 1 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 32, shader_location: 2 },
        ],
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("particles-bead-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vs_entry),
            buffers: &[Some(instance_layout)],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fs_entry),
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend: None, // opaque
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        // Opaque: depth-write on, so droplets occlude each other and the scene.
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        // #305 Tier 3: alpha-to-coverage — fs_bead outputs the silhouette coverage as
        // alpha, and MSAA dithers it into sub-pixel edge samples (both colour + depth),
        // so the impostor rim stops aliasing. No-op at 1× (single sample); needs MSAA on.
        multisample: wgpu::MultisampleState {
            count: sample_count,
            alpha_to_coverage_enabled: sample_count > 1,
            ..Default::default()
        },
        multiview_mask: None,
        cache: None,
    })
}

/// PBR text T6 (#217) — the CPU twin of `particles.wgsl`'s coaxial-glass arithmetic,
/// kept the way this tree keeps a Rust mirror beside a shader so the invariants can
/// be pinned without a GPU: the ray–capsule interval (`capsule_interval`), the
/// clamped Beer–Lambert transmittance (`capsule_transmittance`), the gate that makes
/// core fraction 0 inert, WGSL `refract()`'s zero-on-TIR contract, and the
/// `ORGANON_CAPSULE_CORE` seed. ⚠️ Mirror, not import: the WGSL is the shipped code,
/// and a test here proves the *arithmetic*, not the picture. Runs under
/// `cargo test -p organon-render`. The mirrors are reached only from the tests
/// (production reads the knob, the GPU does the arithmetic), hence the allow.
#[cfg_attr(not(test), allow(dead_code))]
mod capsule_core {
    /// The uniform lanes for a requested knob: core fraction clamped to [0, 1] (a
    /// core wider than the shell is not a core), density clamped to ≥ 0.
    pub fn lanes(core_frac: f32, absorb: f32) -> [f32; 2] {
        let c = if core_frac.is_finite() { core_frac.clamp(0.0, 1.0) } else { 0.0 };
        let a = if absorb.is_finite() { absorb.max(0.0) } else { 0.0 };
        [c, a]
    }

    /// `ORGANON_CAPSULE_CORE="<core_frac>[,<absorb>]"` — the only way to see the
    /// core before T3 wires a control. Unset, empty or unparsable → `[0, 0]` (inert).
    pub fn from_env() -> [f32; 2] {
        std::env::var("ORGANON_CAPSULE_CORE")
            .ok()
            .and_then(|s| parse(&s))
            .unwrap_or([0.0, 0.0])
    }

    pub fn parse(s: &str) -> Option<[f32; 2]> {
        let mut it = s.split(',').map(str::trim);
        let core: f32 = it.next()?.parse().ok()?;
        let absorb: f32 = match it.next() {
            Some(a) if !a.is_empty() => a.parse().ok()?,
            _ => 0.0,
        };
        Some(lanes(core, absorb))
    }

    /// Mirrors `fs_capsule`'s gate: the coaxial path runs only for a Glass (2) /
    /// Refractive (3) material with a non-zero core fraction.
    pub fn active(material: f32, core_frac: f32) -> bool {
        core_frac > 0.0 && material >= 1.5
    }

    type V3 = [f32; 3];
    fn dot(a: V3, b: V3) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }
    fn sub(a: V3, b: V3) -> V3 {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    /// Mirrors `capsule_interval`: (t_in, t_out) along `rd` from `ro` through the
    /// capsule a→b of radius r; t_in > t_out is a miss. Convex union of a finite
    /// cylinder and two spheres → min of entries, max of exits.
    pub fn capsule_interval(ro: V3, rd: V3, a: V3, b: V3, r: f32) -> (f32, f32) {
        let big = 1e30f32;
        let mut t_in = big;
        let mut t_out = -big;
        let ba = sub(b, a);
        let oa = sub(ro, a);
        let baba = dot(ba, ba);
        let bard = dot(ba, rd);
        let baoa = dot(ba, oa);
        let rdoa = dot(rd, oa);
        let oaoa = dot(oa, oa);
        let qa = baba - bard * bard;
        let qb = baba * rdoa - baoa * bard;
        let qc = baba * oaoa - baoa * baoa - r * r * baba;
        let qh = qb * qb - qa * qc;
        if qa > 1e-8 && qh >= 0.0 {
            let sq = qh.sqrt();
            let t1 = (-qb - sq) / qa;
            let t2 = (-qb + sq) / qa;
            let y1 = baoa + t1 * bard;
            let y2 = baoa + t2 * bard;
            if (0.0..=baba).contains(&y1) {
                t_in = t_in.min(t1);
            }
            if (0.0..=baba).contains(&y2) {
                t_out = t_out.max(t2);
            }
        }
        for k in 0..2 {
            let oc = if k == 1 { sub(ro, b) } else { oa };
            let sb = dot(rd, oc);
            let sc = dot(oc, oc) - r * r;
            let sh = sb * sb - sc;
            if sh >= 0.0 {
                let sq = sh.sqrt();
                t_in = t_in.min(-sb - sq);
                t_out = t_out.max(-sb + sq);
            }
        }
        (t_in, t_out)
    }

    /// Mirrors `capsule_transmittance`: Beer–Lambert in the complement of the
    /// albedo, density per outer radius, optical depth clamped at `OD_MAX`.
    pub const OD_MAX: f32 = 6.0;
    pub fn capsule_transmittance(albedo: V3, path: f32, r_outer: f32, density: f32) -> V3 {
        let k = density.max(0.0) / r_outer.max(1e-6);
        let mut out = [0.0f32; 3];
        for c in 0..3 {
            let sigma = (1.0 - albedo[c].clamp(0.0, 1.0)) * k;
            let od = (sigma * path.max(0.0)).min(OD_MAX);
            out[c] = (-od).exp();
        }
        out
    }

    /// Mirrors WGSL `refract(i, n, eta)`: the refracted direction, or the ZERO
    /// vector when `k < 0` (total internal reflection) — the contract the shader's
    /// `dot(rdir, rdir) < 1e-6` guard is written against.
    pub fn refract(i: V3, n: V3, eta: f32) -> V3 {
        let ni = dot(n, i);
        let k = 1.0 - eta * eta * (1.0 - ni * ni);
        if k < 0.0 {
            return [0.0; 3];
        }
        let s = eta * ni + k.sqrt();
        [eta * i[0] - s * n[0], eta * i[1] - s * n[1], eta * i[2] - s * n[2]]
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const A: V3 = [0.0, 0.0, 0.0];
        const B: V3 = [4.0, 0.0, 0.0];
        const R: f32 = 1.0;

        fn near(x: f32, y: f32) -> bool {
            (x - y).abs() < 1e-4
        }

        #[test]
        fn a_degenerate_capsule_is_the_analytic_sphere() {
            // A≈B is how plexus NODES are drawn: the interval must be the sphere's.
            let (t_in, t_out) = capsule_interval([0.0, 0.0, -5.0], [0.0, 0.0, 1.0], A, A, R);
            assert!(near(t_in, 4.0) && near(t_out, 6.0), "got ({t_in}, {t_out})");
        }

        #[test]
        fn a_side_on_ray_enters_and_leaves_through_the_wall() {
            let (t_in, t_out) = capsule_interval([2.0, 0.0, -5.0], [0.0, 0.0, 1.0], A, B, R);
            assert!(near(t_in, 4.0) && near(t_out, 6.0), "got ({t_in}, {t_out})");
        }

        #[test]
        fn a_ray_beside_the_capsule_misses() {
            let (t_in, t_out) = capsule_interval([2.0, 3.0, -5.0], [0.0, 0.0, 1.0], A, B, R);
            assert!(t_in > t_out, "a miss must read t_in > t_out, got ({t_in}, {t_out})");
        }

        #[test]
        fn from_inside_the_exit_is_the_far_wall() {
            // This is how the outer chord is found: origin on the axis, t_in ≤ 0 < t_out.
            let (t_in, t_out) = capsule_interval([2.0, 0.0, 0.0], [0.0, 0.0, 1.0], A, B, R);
            assert!(t_in <= 0.0 && near(t_out, 1.0), "got ({t_in}, {t_out})");
        }

        #[test]
        fn a_ray_along_the_axis_exits_through_the_cap() {
            // qa == 0 here: the wall test yields nothing and the end sphere decides.
            let (t_in, t_out) = capsule_interval(A, [1.0, 0.0, 0.0], A, B, R);
            assert!(near(t_in, -1.0) && near(t_out, 5.0), "got ({t_in}, {t_out})");
        }

        #[test]
        fn an_oblique_exit_near_the_end_is_the_cap_not_the_infinite_wall() {
            // From just inside the B end, heading out at 45°: the infinite cylinder's
            // far root lands beyond B (y out of range) and must be REJECTED, leaving
            // the B sphere's exit — 0.0707 + √0.995 — as the chord end.
            let s = std::f32::consts::FRAC_1_SQRT_2;
            let (_, t_out) = capsule_interval([3.9, 0.0, 0.0], [s, 0.0, s], A, B, R);
            assert!(near(t_out, 1.068208), "got t_out {t_out}");
        }

        #[test]
        fn a_slanted_ray_enters_through_the_end_cap_not_the_disc() {
            // Parallel to the axis, offset 0.5 r, starting 2 r before A: the entry
            // is the sphere at A (t = 2 − √(1 − 0.25)), which the disc-crossing of
            // the cylinder piece must not shadow.
            let (t_in, _) = capsule_interval([-2.0, 0.5, 0.0], [1.0, 0.0, 0.0], A, B, R);
            assert!(near(t_in, 2.0 - 0.75f32.sqrt()), "got t_in {t_in}");
        }

        #[test]
        fn core_fraction_zero_is_inert_for_every_material() {
            for m in [0.0, 1.0, 2.0, 3.0] {
                assert!(!active(m, 0.0), "material {m} must be inert at core 0");
            }
            assert!(active(2.0, 0.3), "Glass with a core must take the coaxial path");
            assert!(active(3.0, 0.3), "Refractive with a core must take the coaxial path");
            assert!(!active(0.0, 0.3) && !active(1.0, 0.3), "opaque materials never do");
            assert_eq!(lanes(0.0, 5.0), [0.0, 5.0]);
            assert_eq!(lanes(1.7, -1.0), [1.0, 0.0], "core clamps to [0,1], density to >= 0");
            assert_eq!(lanes(f32::NAN, f32::INFINITY), [0.0, 0.0]);
        }

        #[test]
        fn a_black_tint_over_a_long_chord_is_dark_not_zero() {
            let t = capsule_transmittance([0.0, 0.0, 0.0], 1e9, R, 1e3);
            let floor = (-OD_MAX).exp();
            for c in t {
                assert!(near(c, floor) && c > 0.0, "channel {c} must sit at the clamp {floor}");
            }
            assert_eq!(capsule_transmittance([0.0; 3], 3.0, R, 0.0), [1.0; 3], "density 0 = clear");
            assert_eq!(capsule_transmittance([1.0; 3], 3.0, R, 9.0), [1.0; 3], "white passes white");
            // A red tube passes red and eats the rest.
            let t = capsule_transmittance([1.0, 0.0, 0.0], 1.0, R, 1.0);
            assert!(near(t[0], 1.0) && near(t[1], (-1.0f32).exp()) && near(t[2], (-1.0f32).exp()));
        }

        #[test]
        fn entry_refraction_cannot_tir_but_the_guard_branch_is_real() {
            // Air -> glass (eta = 1/ior <= 1): k >= 1 - eta^2 >= 0 for every incidence.
            let n = [0.0, 0.0, 1.0];
            for deg in 0..90 {
                let th = (deg as f32).to_radians();
                let i = [th.sin(), 0.0, -th.cos()];
                let r = refract(i, n, 1.0 / 1.5);
                assert!(dot(r, r) > 1e-6, "TIR at {deg} deg on entry is impossible");
            }
            // Glass -> air at grazing does TIR, and the contract is the ZERO vector —
            // which is what the shader's `dot(rdir, rdir) < 1e-6` guard detects.
            let i = [0.9, 0.0, -(1.0f32 - 0.81).sqrt()];
            assert_eq!(refract(i, n, 1.5), [0.0; 3]);
        }

        #[test]
        fn the_env_seed_parses_or_stays_inert() {
            assert_eq!(parse("0.4"), Some([0.4, 0.0]));
            assert_eq!(parse("0.4, 1.5"), Some([0.4, 1.5]));
            assert_eq!(parse("0.4,"), Some([0.4, 0.0]));
            assert_eq!(parse("1.7,-2"), Some([1.0, 0.0]));
            assert_eq!(parse(""), None);
            assert_eq!(parse("core"), None);
            assert_eq!(parse("0.4,abc"), None);
        }
    }
}
