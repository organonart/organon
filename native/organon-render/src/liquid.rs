//! MLS-MPM liquid (#182 Tier 3) — the sim host for `liquid.wgsl`.
//!
//! Owns the particle/grid/collider buffers + the five compute pipelines
//! (P2G → grid → G2P per substep; density splat + field resolve per frame).
//! The density field is written into a `MetaField`'s 3D texture, so the
//! EXISTING metaball isosurface raymarch renders the liquid surface with the
//! full material stack (Glass = water) — route A of the tier, zero new
//! shading code. Seeding reuses `math::MpmSim::new` (the unit-tested CPU
//! mirror), so the GPU liquid starts from the exact tested distribution;
//! P2G's fixed-point integer atomics keep the GPU sim deterministic too.
//!
//! Included by `render.rs` via `#[path]` (visual binary only).

use bytemuck::{Pod, Zeroable};
use glam::{Vec3, Vec4};

use organon_core::math;

const WG: u32 = 64;
/// Particle count hard cap (the buffer is sized to the request, capped here).
pub const MAX_LIQUID_PARTICLES: usize = 300_000;

/// Matches `LiquidU` in liquid.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LiquidU {
    res: [u32; 4],
    fres: [u32; 4],
    box0: [f32; 4],
    box1: [f32; 4],
    phys: [f32; 4],
    misc: [f32; 4],
    color: [f32; 4],
}

/// Matches `Particle` in liquid.wgsl (5 × vec4 = 80 bytes).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuParticle {
    pos_j: [f32; 4],
    vel: [f32; 4],
    c0: [f32; 4],
    c1: [f32; 4],
    c2: [f32; 4],
}

/// Per-frame liquid controls (from `Shared.liquid`).
#[derive(Clone, Copy)]
pub struct LiquidParams {
    pub gravity: f32,   // world units / s²
    pub stiffness: f32, // EOS
    pub viscosity: f32, // 0..1 APIC damping
    pub open_top: bool,
    pub collide: bool,
    pub stir: f32,        // collider velocity gain
    pub splat_scale: f32, // density → iso-field units
    pub color: Vec3,      // liquid albedo (the field's rgb)
    pub substeps: u32,    // 1..4
    /// Container shape (mirrors `math::MpmParams::shape`): 0 box, 1 sphere,
    /// 2 cylinder, 3 boundless (soft absorbing shell, no hard wall).
    pub shape: u32,
    /// Render reveal 0..1: a soft spherical window on the splatted density,
    /// so the surface trails off instead of flattening against the walls.
    pub reveal: f32,
    /// Reseed counter (the editor's "reset pool" button via `Shared.liquid[14]`):
    /// any change from the last-seen value re-pours the pool from the seed
    /// distribution — the way back once gravity has drained it to the floor.
    pub reset_gen: u32,
}

pub struct LiquidSim {
    ubo: wgpu::Buffer,
    parts: wgpu::Buffer,
    grid_a: wgpu::Buffer,
    grid_v: wgpu::Buffer,
    colliders: wgpu::Buffer,
    dens_a: wgpu::Buffer,
    // #247 Tier 3: per-field-cell HDR ember glow (energy → liquid). Uploaded only
    // when energization is injecting; the shader ignores it when `fres.w = 0`.
    glow: wgpu::Buffer,
    p2g: wgpu::ComputePipeline,
    grid: wgpu::ComputePipeline,
    g2p: wgpu::ComputePipeline,
    splat: wgpu::ComputePipeline,
    resolve: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
    bind: Option<wgpu::BindGroup>,
    // Current allocation / seeding key.
    count: usize,
    res: u32,
    field_res: u32,
    seeded: bool,
    // Last-seen reset counter; a change forces a reseed (see LiquidParams).
    reset_gen: u32,
}

impl LiquidSim {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("liquid"),
            source: wgpu::ShaderSource::Wgsl(include_str!("liquid.wgsl").into()),
        });
        let ubo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("liquid-ubo"),
            size: std::mem::size_of::<LiquidU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sto = |b: u32, ro: bool| wgpu::BindGroupLayoutEntry {
            binding: b,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: ro },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("liquid-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                sto(1, false), // particles
                sto(2, false), // grid atomics
                sto(3, false), // grid velocity
                sto(4, true),  // colliders
                sto(5, false), // density atomics
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba16Float,
                        view_dimension: wgpu::TextureViewDimension::D3,
                    },
                    count: None,
                },
                sto(7, true), // #247 T3: energy → liquid ember glow
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("liquid-pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let mk = |label: &str, entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&pl),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        LiquidSim {
            ubo,
            parts: make_buf(device, "liquid-parts", 80, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST),
            grid_a: make_buf(device, "liquid-grid-a", 16, wgpu::BufferUsages::STORAGE),
            grid_v: make_buf(device, "liquid-grid-v", 16, wgpu::BufferUsages::STORAGE),
            colliders: make_buf(device, "liquid-colliders", 16, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST),
            dens_a: make_buf(device, "liquid-dens-a", 4, wgpu::BufferUsages::STORAGE),
            glow: make_buf(device, "liquid-glow", 16, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST),
            p2g: mk("liquid-p2g", "cs_p2g"),
            grid: mk("liquid-grid", "cs_grid"),
            g2p: mk("liquid-g2p", "cs_g2p"),
            splat: mk("liquid-splat", "cs_splat_density"),
            resolve: mk("liquid-resolve", "cs_resolve_field"),
            bgl,
            bind: None,
            count: 0,
            res: 0,
            field_res: 0,
            seeded: false,
            reset_gen: 0,
        }
    }

    /// The grid-velocity buffer (vec4/cell, xyz = velocity in GRID units/s) —
    /// the #182 T4 sway pass samples it (× cell size for world units).
    pub fn grid_v(&self) -> &wgpu::Buffer {
        &self.grid_v
    }
    /// Current sim grid resolution (0 before the first `ensure`).
    pub fn res(&self) -> u32 {
        self.res
    }

    /// (Re)allocate + reseed when the particle count or grid resolution
    /// changes. Seeding reuses the unit-tested CPU mirror (`math::MpmSim`),
    /// so the GPU liquid starts from the exact tested distribution.
    #[allow(clippy::too_many_arguments)]
    fn ensure(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        count: usize,
        res: u32,
        field_res: u32,
        field_view: &wgpu::TextureView,
        field_gen: &mut u64,
    ) {
        let count = count.clamp(1, MAX_LIQUID_PARTICLES);
        if self.count == count && self.res == res && self.field_res == field_res && self.seeded {
            return;
        }
        self.count = count;
        self.res = res;
        self.field_res = field_res;
        self.seeded = true;
        *field_gen = field_gen.wrapping_add(1);

        let n = (res as usize).pow(3);
        let fn_ = (field_res as usize).pow(3);
        self.parts = make_buf(
            device,
            "liquid-parts",
            (count * std::mem::size_of::<GpuParticle>()) as u64,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        self.grid_a = make_buf(device, "liquid-grid-a", (n * 16) as u64, wgpu::BufferUsages::STORAGE);
        self.grid_v = make_buf(device, "liquid-grid-v", (n * 16) as u64, wgpu::BufferUsages::STORAGE);
        self.colliders = make_buf(
            device,
            "liquid-colliders",
            (n * 16) as u64,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        self.dens_a = make_buf(device, "liquid-dens-a", (fn_ * 4) as u64, wgpu::BufferUsages::STORAGE);
        // #247 T3: one HDR ember colour per FIELD cell (rgb glow, w unused).
        self.glow = make_buf(
            device,
            "liquid-glow",
            (fn_ * 16) as u64,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );

        // Deterministic seed (fixed — presets recall the same pool every time).
        let sim = math::MpmSim::new(count, 0x11D0, res as usize);
        let init: Vec<GpuParticle> = (0..count)
            .map(|i| GpuParticle {
                pos_j: [sim.pos[i].x, sim.pos[i].y, sim.pos[i].z, 1.0],
                vel: [0.0; 4],
                c0: [0.0; 4],
                c1: [0.0; 4],
                c2: [0.0; 4],
            })
            .collect();
        queue.write_buffer(&self.parts, 0, bytemuck::cast_slice(&init));

        self.bind = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("liquid-bind"),
            layout: &self.bgl,
            entries: &[
                ent(0, &self.ubo),
                ent(1, &self.parts),
                ent(2, &self.grid_a),
                ent(3, &self.grid_v),
                ent(4, &self.colliders),
                ent(5, &self.dens_a),
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(field_view),
                },
                ent(7, &self.glow),
            ],
        }));
    }

    /// Step the liquid (substeps × P2G/grid/G2P) and splat the density into
    /// `field_view` (the liquid MetaField's texture). `colliders` is a `res³`
    /// occupancy grid (xyz = world velocity, w = solid) or empty when the
    /// generator isn't colliding. `field_gen` is bumped whenever the bind
    /// group is rebuilt (the caller keys nothing on it today; kept for parity
    /// with the other epoch patterns).
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        count: usize,
        res: u32,
        field_res: u32,
        field_view: &wgpu::TextureView,
        field_gen: &mut u64,
        cmin: Vec3,
        cmax: Vec3,
        colliders: &[Vec4],
        // #247 Tier 3: per-FIELD-cell HDR ember glow (energy → liquid); empty = off.
        glow: &[Vec4],
        dt: f32,
        p: &LiquidParams,
    ) {
        let res = res.clamp(16, 96);
        // "Reset pool": a bumped counter re-pours the seed distribution.
        if p.reset_gen != self.reset_gen {
            self.reset_gen = p.reset_gen;
            self.seeded = false;
        }
        self.ensure(device, queue, count, res, field_res, field_view, field_gen);
        let n = (res as usize).pow(3);
        let count = self.count;

        let collide = p.collide && colliders.len() >= n;
        if collide {
            queue.write_buffer(&self.colliders, 0, bytemuck::cast_slice(&colliders[..n]));
        }
        // #247 T3: upload the ember glow only when present (fn_ field cells); the
        // shader gate (`fres.w`) keeps the buffer inert otherwise.
        let fn_ = (field_res as usize).pow(3);
        let glow_on = glow.len() >= fn_;
        if glow_on {
            queue.write_buffer(&self.glow, 0, bytemuck::cast_slice(&glow[..fn_]));
        }

        // World cell size; gravity converts into grid units. dt splits across
        // substeps (MPM wants small steps; each substep is the full pipeline).
        let h = ((cmax.x - cmin.x) / res as f32).max(1e-5);
        // MPM wants each substep small for stability (<= MAX_DT_SUB), but the
        // substeps must also cover the whole frame dt or the liquid drifts
        // behind real time after a hitch. So the user's `substeps` dial is a
        // quality FLOOR and we add more as needed to keep each step stable while
        // integrating the full frame — bounded so a hitch can't spiral the count.
        const MAX_DT_SUB: f32 = 1.0 / 30.0;
        // Cap the frame dt: past ~100 ms drop time rather than spiral the cost.
        let frame_dt = dt.clamp(0.0, 1.0 / 10.0);
        let floor = p.substeps.clamp(1, 4);
        let substeps = ((frame_dt / MAX_DT_SUB).ceil() as u32).max(floor).min(8);
        let dt_sub = frame_dt / substeps as f32;
        let u = LiquidU {
            res: [res, res, res, count as u32],
            // fres.w = #247 T3 energy-glow flag (0 = off → byte-identical resolve).
            fres: [field_res, field_res, field_res, glow_on as u32],
            box0: [cmin.x, cmin.y, cmin.z, h],
            box1: [cmax.x, cmax.y, cmax.z, dt_sub],
            phys: [p.gravity / h, p.stiffness, p.viscosity, p.open_top as u32 as f32],
            misc: [p.stir, collide as u32 as f32, p.splat_scale, p.shape as f32],
            color: [p.color.x, p.color.y, p.color.z, p.reveal.clamp(0.0, 1.0)],
        };
        queue.write_buffer(&self.ubo, 0, bytemuck::bytes_of(&u));

        let bind = self.bind.as_ref().unwrap();
        let pgroups = (count as u32).div_ceil(WG);
        let ggroups = (n as u32).div_ceil(WG);
        let fgroups = ((field_res as u32).pow(3)).div_ceil(WG);
        {
            let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("liquid-sim"),
                timestamp_writes: None,
            });
            cp.set_bind_group(0, bind, &[]);
            for _ in 0..substeps {
                cp.set_pipeline(&self.p2g);
                cp.dispatch_workgroups(pgroups, 1, 1);
                cp.set_pipeline(&self.grid);
                cp.dispatch_workgroups(ggroups, 1, 1);
                cp.set_pipeline(&self.g2p);
                cp.dispatch_workgroups(pgroups, 1, 1);
            }
            // Once per frame: splat the particle density into the iso field.
            cp.set_pipeline(&self.splat);
            cp.dispatch_workgroups(pgroups, 1, 1);
            cp.set_pipeline(&self.resolve);
            cp.dispatch_workgroups(fgroups, 1, 1);
        }
    }
}

fn ent(binding: u32, buf: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource: buf.as_entire_binding() }
}

fn make_buf(device: &wgpu::Device, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(16),
        usage,
        mapped_at_creation: false,
    })
}
