//! Aura-Fluid (#81 showpiece): a GPU Navier–Stokes (Stable Fluids) solver that
//! evolves a PERSISTENT velocity grid the particle system rides. The generator's
//! splatted node motion is injected as a momentum source; the fluid advects itself,
//! confines vorticity, and pressure-projects to divergence-free — so vortices shed
//! off the moving structure and persist in its wake. The kernels live in
//! `fluid.wgsl`; the PDE math is mirrored + unit-tested on the CPU (`math::fluid_*`).
//!
//! Included by `render.rs` via `#[path]` (compiles only into the visual binary).
//! One storage-buffer set; each pass binds the subset it needs. The grid is a CUBE
//! (uniform cell size), built CPU-side in `bin/visual.rs`; `vel_buffer()` hands the
//! evolved field to `ParticleSystem::simulate` as an external velocity grid.

use bytemuck::{Pod, Zeroable};
use glam::{Vec3, Vec4};

const WG: u32 = 64;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FluidU {
    res: [u32; 4],  // xyz res, w = count
    gmin: [f32; 4], // xyz, w = dt
    gmax: [f32; 4], // xyz, w = time
    p0: [f32; 4],   // force, vorticity, dissipation, inflow_decay
    p1: [f32; 4],   // cell size h, dye rate, dye dissipation, boundaries
    p2: [f32; 4],   // #182 T2: buoyancy, heat decay, splash impulse, beat env
}

/// Per-frame fluid controls (from `Shared.fluid` + the #182 T2 `Shared.fluid2`).
#[derive(Clone, Copy)]
pub struct FluidParams {
    pub force: f32,
    pub vorticity: f32,
    pub dissipation: f32,
    pub iters: u32,
    pub inflow_decay: f32,
    // --- #182 Tier 2 (all inert at 0 / substeps 1) ---
    /// No-slip node-occupancy boundaries (the CPU marks cells in `source.w`).
    pub boundaries: bool,
    /// Signed vertical force per heat unit (+ rises / − sinks; 0 = off).
    pub buoyancy: f32,
    /// Heat cool-down rate (1/s).
    pub heat_decay: f32,
    /// Radial beat-splash impulse (0 = off), scaled by `beat_env`.
    pub splash: f32,
    /// The decaying beat envelope this frame (0 while Pulse is off).
    pub beat_env: f32,
    /// Sim substeps per frame (1..4) — stability for fast stirs.
    pub substeps: u32,
    /// Particle-Aura field energization (#247, Fluid tier): after the final projection,
    /// compute the flow's energy density ½|u|² + ½|ω|² per cell and write it into the
    /// velocity grid's `w` channel, so the riding motes glow by it (`cs_energy`). Off →
    /// `w` stays 0 (the pass isn't dispatched), byte-identical.
    pub energize: bool,
}

/// Per-frame dye controls (#182 Tier 1, from `Shared.fluidvis`). The dye rides
/// the same solve; `None` in `step` skips every dye pass (byte-identical off).
#[derive(Clone, Copy)]
pub struct DyeParams {
    pub rate: f32,
    pub dissipation: f32,
    /// MacCormack error-corrected advection (sharp filaments) vs plain
    /// semi-Lagrangian.
    pub maccormack: bool,
    /// #182 T2: the ink march's curl-noise micro-detail needs |ω|, so run the
    /// curl pass even when vorticity confinement is off.
    pub want_curl: bool,
}

pub struct FluidSim {
    uniform: wgpu::Buffer,
    // Storage buffers (recreated on a resolution change).
    vel_a: wgpu::Buffer,
    vel_b: wgpu::Buffer,
    source: wgpu::Buffer,
    div: wgpu::Buffer,
    curl: wgpu::Buffer,
    p_a: wgpu::Buffer,
    p_b: wgpu::Buffer,
    // #182 Tier 1: the RGB dye field (canonical / advect scratch / MacCormack
    // scratch / CPU-splatted injection source).
    dye_a: wgpu::Buffer,
    dye_b: wgpu::Buffer,
    dye_c: wgpu::Buffer,
    dye_src: wgpu::Buffer,
    // Pipelines (built once).
    inject: wgpu::ComputePipeline,
    advect: wgpu::ComputePipeline,
    curl_p: wgpu::ComputePipeline,
    vort: wgpu::ComputePipeline,
    energy_p: wgpu::ComputePipeline, // #247: flow energy density → vel.w (Fluid energization)
    diverge: wgpu::ComputePipeline,
    jacobi: wgpu::ComputePipeline,
    project: wgpu::ComputePipeline,
    dye_inject: wgpu::ComputePipeline,
    dye_advect: wgpu::ComputePipeline,
    dye_mac: wgpu::ComputePipeline,
    // Bind-group layouts (built once).
    bgl_inject: wgpu::BindGroupLayout,
    bgl_advect: wgpu::BindGroupLayout,
    bgl_curlvort: wgpu::BindGroupLayout, // {0 ubo, 1 vel(rw), 5 curl(rw)}
    bgl_div: wgpu::BindGroupLayout,
    bgl_jacobi: wgpu::BindGroupLayout,
    bgl_project: wgpu::BindGroupLayout,
    bgl_dye_inject: wgpu::BindGroupLayout, // {0 ubo, 8 dye_a(rw), 10 dye_src(ro)}
    bgl_dye_advect: wgpu::BindGroupLayout, // {0 ubo, 1 vel(rw), 8 dye_a(rw), 9 dye_b(rw)}
    bgl_dye_mac: wgpu::BindGroupLayout, // {0 ubo, 1 vel(rw), 8, 9, 11 dye_c(rw)}
    // Bind groups (rebuilt on a resolution change).
    bg_inject: Option<wgpu::BindGroup>,
    bg_advect: Option<wgpu::BindGroup>,
    bg_curl: Option<wgpu::BindGroup>,
    bg_vort: Option<wgpu::BindGroup>,
    bg_div: Option<wgpu::BindGroup>,
    bg_jac_ab: Option<wgpu::BindGroup>,
    bg_jac_ba: Option<wgpu::BindGroup>,
    bg_project: Option<wgpu::BindGroup>,
    bg_dye_inject: Option<wgpu::BindGroup>,
    bg_dye_advect: Option<wgpu::BindGroup>,
    bg_dye_mac: Option<wgpu::BindGroup>,
    res: [u32; 3],
    // Bumped whenever the storage buffers are recreated (a resolution change),
    // so external consumers of `dye_buffer()` (the ink pass) rebuild their binds.
    epoch: u64,
    active: bool, // ran last frame (for reset-on-reenable)
    // The dye ran last frame — a fresh ink session starts from clean water
    // (and stale heat can't linger for the buoyancy term).
    dye_active: bool,
}

impl FluidSim {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fluid"),
            source: wgpu::ShaderSource::Wgsl(include_str!("fluid.wgsl").into()),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fluid-uniform"),
            size: std::mem::size_of::<FluidU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let ubo = |b: u32| wgpu::BindGroupLayoutEntry {
            binding: b,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
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
        let mk_bgl = |label: &str, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            })
        };
        // Binding numbers match fluid.wgsl: 0 U, 1 vel_a(rw), 2 vel_b(rw),
        // 3 source(ro), 4 div(rw), 5 curl(rw), 6 p_src(ro), 7 p_dst(rw),
        // 8 dye_a(rw), 9 dye_b(rw), 10 dye_src(ro), 11 dye_c(rw).
        // #182 T2: source(3) rides along wherever `solid()` is called (advect /
        // divergence / jacobi / project), and inject reads dye_a(8) for buoyancy.
        let bgl_inject =
            mk_bgl("fluid-inject", &[ubo(0), sto(1, false), sto(3, true), sto(8, false)]);
        let bgl_advect =
            mk_bgl("fluid-advect", &[ubo(0), sto(1, false), sto(2, false), sto(3, true)]);
        // (source(3) rides along: cs_vorticity's solid() skip reads occupancy.)
        let bgl_curlvort =
            mk_bgl("fluid-curlvort", &[ubo(0), sto(1, false), sto(3, true), sto(5, false)]);
        let bgl_div =
            mk_bgl("fluid-div", &[ubo(0), sto(1, false), sto(3, true), sto(4, false)]);
        let bgl_jacobi = mk_bgl(
            "fluid-jacobi",
            &[ubo(0), sto(3, true), sto(4, false), sto(6, true), sto(7, false)],
        );
        let bgl_project =
            mk_bgl("fluid-project", &[ubo(0), sto(1, false), sto(3, true), sto(6, true)]);
        let bgl_dye_inject =
            mk_bgl("fluid-dye-inject", &[ubo(0), sto(8, false), sto(10, true)]);
        let bgl_dye_advect =
            mk_bgl("fluid-dye-advect", &[ubo(0), sto(1, false), sto(8, false), sto(9, false)]);
        let bgl_dye_mac = mk_bgl(
            "fluid-dye-mac",
            &[ubo(0), sto(1, false), sto(8, false), sto(9, false), sto(11, false)],
        );

        let mk_pipe = |label: &str, bgl: &wgpu::BindGroupLayout, entry: &str| {
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(bgl)],
                immediate_size: 0,
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&pl),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let inject = mk_pipe("fluid-inject-pipe", &bgl_inject, "cs_inject");
        let advect = mk_pipe("fluid-advect-pipe", &bgl_advect, "cs_advect");
        let curl_p = mk_pipe("fluid-curl-pipe", &bgl_curlvort, "cs_curl");
        let vort = mk_pipe("fluid-vort-pipe", &bgl_curlvort, "cs_vorticity");
        // #247: reuses the curlvort layout (vel_a rw + curl) to write energy into vel.w.
        let energy_p = mk_pipe("fluid-energy-pipe", &bgl_curlvort, "cs_energy");
        let diverge = mk_pipe("fluid-div-pipe", &bgl_div, "cs_divergence");
        let jacobi = mk_pipe("fluid-jacobi-pipe", &bgl_jacobi, "cs_jacobi");
        let project = mk_pipe("fluid-project-pipe", &bgl_project, "cs_project");
        let dye_inject = mk_pipe("fluid-dye-inject-pipe", &bgl_dye_inject, "cs_dye_inject");
        let dye_advect = mk_pipe("fluid-dye-advect-pipe", &bgl_dye_advect, "cs_dye_advect");
        let dye_mac = mk_pipe("fluid-dye-mac-pipe", &bgl_dye_mac, "cs_dye_mac");

        FluidSim {
            uniform,
            vel_a: make_buf(device, "fluid-vel-a", 16),
            vel_b: make_buf(device, "fluid-vel-b", 16),
            source: make_buf(device, "fluid-source", 16),
            div: make_buf(device, "fluid-div", 16),
            curl: make_buf(device, "fluid-curl", 16),
            p_a: make_buf(device, "fluid-p-a", 16),
            p_b: make_buf(device, "fluid-p-b", 16),
            dye_a: make_buf(device, "fluid-dye-a", 16),
            dye_b: make_buf(device, "fluid-dye-b", 16),
            dye_c: make_buf(device, "fluid-dye-c", 16),
            dye_src: make_buf(device, "fluid-dye-src", 16),
            inject,
            advect,
            curl_p,
            vort,
            energy_p,
            diverge,
            jacobi,
            project,
            dye_inject,
            dye_advect,
            dye_mac,
            bgl_inject,
            bgl_advect,
            bgl_curlvort,
            bgl_div,
            bgl_jacobi,
            bgl_project,
            bgl_dye_inject,
            bgl_dye_advect,
            bgl_dye_mac,
            bg_inject: None,
            bg_advect: None,
            bg_curl: None,
            bg_vort: None,
            bg_div: None,
            bg_jac_ab: None,
            bg_jac_ba: None,
            bg_project: None,
            bg_dye_inject: None,
            bg_dye_advect: None,
            bg_dye_mac: None,
            res: [0, 0, 0],
            epoch: 0,
            active: false,
            dye_active: false,
        }
    }

    /// The evolved velocity field — handed to `ParticleSystem::simulate` so the motes
    /// ride the fluid instead of the raw source grid.
    pub fn vel_buffer(&self) -> &wgpu::Buffer {
        &self.vel_a
    }

    /// The evolved dye field (#182 Tier 1) — the Fluid Ink pass blits it into a
    /// 3D texture and raymarches it. Zeroed until a dye step has run.
    pub fn dye_buffer(&self) -> &wgpu::Buffer {
        &self.dye_a
    }

    /// The vorticity buffer (xyz = ω, w = |ω|) — the ink march's micro-detail
    /// reads |ω| (#182 Tier 2). Only fresh when the curl pass ran this frame
    /// (vorticity confinement on, or `DyeParams::want_curl`).
    pub fn curl_buffer(&self) -> &wgpu::Buffer {
        &self.curl
    }

    /// Bumped whenever the storage buffers are recreated (resolution change /
    /// reset), so `dye_buffer()` consumers know to rebuild their bind groups.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The grid resolution the sim last ran at (cubic).
    pub fn res(&self) -> [u32; 3] {
        self.res
    }

    /// Call on frames the solver does NOT step (the renderer's run gate is
    /// off). `step` is the only place `dye_active` is otherwise updated, so on
    /// the ink-only path (aura Off) toggling the ink off would leave it stuck
    /// true and the next activation would skip the clean-water clear — old ink
    /// and heat would ghost straight back in.
    pub fn note_idle(&mut self) {
        self.dye_active = false;
    }

    /// (Re)create the storage buffers + bind groups for a resolution. New wgpu
    /// buffers are zero-initialized, which also resets the sim.
    fn ensure(&mut self, device: &wgpu::Device, res: [u32; 3]) {
        if self.res == res && self.bg_inject.is_some() {
            return;
        }
        self.res = res;
        self.epoch = self.epoch.wrapping_add(1);
        let n = (res[0] * res[1] * res[2]).max(1) as usize;
        let v4 = (n * 16) as u64;
        let f1 = (n * 4) as u64;
        self.vel_a = make_buf(device, "fluid-vel-a", v4);
        self.vel_b = make_buf(device, "fluid-vel-b", v4);
        self.source = make_buf(device, "fluid-source", v4);
        self.div = make_buf(device, "fluid-div", f1);
        self.curl = make_buf(device, "fluid-curl", v4);
        self.p_a = make_buf(device, "fluid-p-a", f1);
        self.p_b = make_buf(device, "fluid-p-b", f1);
        self.dye_a = make_buf(device, "fluid-dye-a", v4);
        self.dye_b = make_buf(device, "fluid-dye-b", v4);
        self.dye_c = make_buf(device, "fluid-dye-c", v4);
        self.dye_src = make_buf(device, "fluid-dye-src", v4);

        let e = ent;
        let u = || ent(0, &self.uniform);
        self.bg_inject = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fluid-bg-inject"),
            layout: &self.bgl_inject,
            entries: &[u(), e(1, &self.vel_a), e(3, &self.source), e(8, &self.dye_a)],
        }));
        self.bg_advect = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fluid-bg-advect"),
            layout: &self.bgl_advect,
            entries: &[u(), e(1, &self.vel_a), e(2, &self.vel_b), e(3, &self.source)],
        }));
        self.bg_curl = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fluid-bg-curl"),
            layout: &self.bgl_curlvort,
            entries: &[u(), e(1, &self.vel_a), e(3, &self.source), e(5, &self.curl)],
        }));
        self.bg_vort = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fluid-bg-vort"),
            layout: &self.bgl_curlvort,
            entries: &[u(), e(1, &self.vel_a), e(3, &self.source), e(5, &self.curl)],
        }));
        self.bg_div = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fluid-bg-div"),
            layout: &self.bgl_div,
            entries: &[u(), e(1, &self.vel_a), e(3, &self.source), e(4, &self.div)],
        }));
        // Jacobi ping-pong: a→b reads p_a writes p_b; b→a reads p_b writes p_a.
        self.bg_jac_ab = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fluid-bg-jac-ab"),
            layout: &self.bgl_jacobi,
            entries: &[u(), e(3, &self.source), e(4, &self.div), e(6, &self.p_a), e(7, &self.p_b)],
        }));
        self.bg_jac_ba = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fluid-bg-jac-ba"),
            layout: &self.bgl_jacobi,
            entries: &[u(), e(3, &self.source), e(4, &self.div), e(6, &self.p_b), e(7, &self.p_a)],
        }));
        self.bg_project = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fluid-bg-project"),
            layout: &self.bgl_project,
            entries: &[u(), e(1, &self.vel_a), e(3, &self.source), e(6, &self.p_a)],
        }));
        self.bg_dye_inject = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fluid-bg-dye-inject"),
            layout: &self.bgl_dye_inject,
            entries: &[u(), e(8, &self.dye_a), e(10, &self.dye_src)],
        }));
        self.bg_dye_advect = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fluid-bg-dye-advect"),
            layout: &self.bgl_dye_advect,
            entries: &[u(), e(1, &self.vel_a), e(8, &self.dye_a), e(9, &self.dye_b)],
        }));
        self.bg_dye_mac = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fluid-bg-dye-mac"),
            layout: &self.bgl_dye_mac,
            entries: &[
                u(),
                e(1, &self.vel_a),
                e(8, &self.dye_a),
                e(9, &self.dye_b),
                e(11, &self.dye_c),
            ],
        }));
    }

    /// One fluid step: upload the stir source, evolve the field. Call before
    /// `ParticleSystem::simulate` (which then samples `vel_buffer()`). `source` is a
    /// CUBE grid (`res³` cells) of world-space stir velocity; `gmin/gmax` is the cube
    /// AABB. A resolution change (or re-enable) resets the sim to rest.
    /// `dye` (#182 Tier 1): a CPU-splatted `res³` injection grid (rgb = node colour ×
    /// ball kernel) + its params — the dye is injected then advected by the projected
    /// velocity. `None` skips every dye pass.
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source: &[Vec4],
        res: [u32; 3],
        gmin: Vec3,
        gmax: Vec3,
        dt: f32,
        time: f32,
        p: FluidParams,
        dye: Option<(&[Vec4], DyeParams)>,
    ) {
        if source.is_empty() {
            self.active = false;
            return;
        }
        // Reset to rest when (re)activating after being off, so the wake starts clean.
        if !self.active {
            self.res = [0, 0, 0];
        }
        self.ensure(device, res);
        self.active = true;
        // A fresh ink session starts from clean water: clear the canonical dye
        // when the dye (re)activates while the solver kept running, so an old
        // session's ink (and its heat) doesn't ghost back in.
        let dye_on = dye.is_some();
        if dye_on && !self.dye_active {
            encoder.clear_buffer(&self.dye_a, 0, None);
        }
        self.dye_active = dye_on;

        let n = (res[0] * res[1] * res[2]) as usize;
        // The cube cell size (uniform): the AABB is built square in `bin/visual.rs`.
        let h = ((gmax.x - gmin.x) / res[0].max(1) as f32).max(1e-4);
        queue.write_buffer(&self.source, 0, bytemuck::cast_slice(&source[..n.min(source.len())]));
        let dp = dye.map(|(_, dp)| dp);
        // #182 Tier 2: substeps split the frame's dt so fast stirs stay stable
        // (each substep runs the FULL pass sequence — the honest cost).
        let substeps = p.substeps.clamp(1, 4);
        let dt_sub = dt / substeps as f32;
        let u = FluidU {
            res: [res[0], res[1], res[2], n as u32],
            gmin: [gmin.x, gmin.y, gmin.z, dt_sub],
            gmax: [gmax.x, gmax.y, gmax.z, time],
            p0: [p.force, p.vorticity, p.dissipation.max(0.0), p.inflow_decay.max(0.0)],
            p1: [
                h,
                dp.map(|d| d.rate.max(0.0)).unwrap_or(0.0),
                dp.map(|d| d.dissipation.max(0.0)).unwrap_or(0.0),
                p.boundaries as u32 as f32,
            ],
            p2: [
                // Buoyancy reads the heat riding dye_a.w — only meaningful while
                // the dye is being stepped; gate it off so stale heat from an
                // earlier ink session can't drive phantom convection.
                if dye.is_some() { p.buoyancy } else { 0.0 },
                p.heat_decay.max(0.0),
                p.splash.max(0.0),
                p.beat_env.max(0.0),
            ],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
        if let Some((dye_src, _)) = dye {
            queue.write_buffer(
                &self.dye_src,
                0,
                bytemuck::cast_slice(&dye_src[..n.min(dye_src.len())]),
            );
        }

        let groups = (n as u32).div_ceil(WG);
        let dispatch = |cp: &mut wgpu::ComputePass, pipe: &wgpu::ComputePipeline, bg: &wgpu::BindGroup| {
            cp.set_pipeline(pipe);
            cp.set_bind_group(0, bg, &[]);
            cp.dispatch_workgroups(groups, 1, 1);
        };
        // The curl pass also runs when the ink march wants |ω| for its
        // render-time micro-detail (#182 Tier 2), not just for confinement.
        let want_curl = p.vorticity > 0.0 || dp.map(|d| d.want_curl).unwrap_or(false);

        for si in 0..substeps {
            // 1) inject, 2) advect → vel_b, copy back.
            {
                let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fluid-inject-advect"),
                    timestamp_writes: None,
                });
                dispatch(&mut cp, &self.inject, self.bg_inject.as_ref().unwrap());
                dispatch(&mut cp, &self.advect, self.bg_advect.as_ref().unwrap());
            }
            encoder.copy_buffer_to_buffer(&self.vel_b, 0, &self.vel_a, 0, (n * 16) as u64);

            // 3) vorticity confinement (curl → confine); the curl half also runs
            //    for the ink's micro-detail. Confinement itself stays gated.
            if want_curl {
                let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fluid-vorticity"),
                    timestamp_writes: None,
                });
                dispatch(&mut cp, &self.curl_p, self.bg_curl.as_ref().unwrap());
                if p.vorticity > 0.0 {
                    dispatch(&mut cp, &self.vort, self.bg_vort.as_ref().unwrap());
                }
            }

            // 4) divergence, then warm-start the pressure at zero.
            {
                let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fluid-divergence"),
                    timestamp_writes: None,
                });
                dispatch(&mut cp, &self.diverge, self.bg_div.as_ref().unwrap());
            }
            encoder.clear_buffer(&self.p_a, 0, None);

            // 5) Jacobi pressure solve (even iters → result lands in p_a).
            let iters = p.iters.clamp(2, 200) & !1u32;
            {
                let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fluid-pressure"),
                    timestamp_writes: None,
                });
                for k in 0..iters {
                    let bg =
                        if k % 2 == 0 { self.bg_jac_ab.as_ref() } else { self.bg_jac_ba.as_ref() };
                    dispatch(&mut cp, &self.jacobi, bg.unwrap());
                }
            }

            // 6) Project to divergence-free.
            {
                let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fluid-project"),
                    timestamp_writes: None,
                });
                dispatch(&mut cp, &self.project, self.bg_project.as_ref().unwrap());
            }

            // 6b) Particle-Aura energization (#247): on the FINAL substep, bake the flow's
            //     energy density ½|u|² + ½|ω|² into vel_a.w for the riding motes. Curl is
            //     recomputed here on the projected field so |ω| matches the final velocity
            //     (projection subtracts a gradient, so it barely touches vorticity, but this
            //     keeps them consistent regardless of the confinement path). Only the last
            //     substep's `w` survives into what the motes sample.
            if p.energize && si + 1 == substeps {
                let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fluid-energy"),
                    timestamp_writes: None,
                });
                dispatch(&mut cp, &self.curl_p, self.bg_curl.as_ref().unwrap());
                dispatch(&mut cp, &self.energy_p, self.bg_curl.as_ref().unwrap());
            }

            // 7) Dye transport (#182 Tier 1), on the PROJECTED velocity: inject the
            //    CPU-splatted node colours, semi-Lagrangian advect (→ dye_b), then
            //    either the MacCormack correction (→ dye_c) or a straight copy back.
            if let Some((_, dp)) = dye {
                {
                    let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("fluid-dye"),
                        timestamp_writes: None,
                    });
                    dispatch(&mut cp, &self.dye_inject, self.bg_dye_inject.as_ref().unwrap());
                    dispatch(&mut cp, &self.dye_advect, self.bg_dye_advect.as_ref().unwrap());
                    if dp.maccormack {
                        dispatch(&mut cp, &self.dye_mac, self.bg_dye_mac.as_ref().unwrap());
                    }
                }
                let final_buf = if dp.maccormack { &self.dye_c } else { &self.dye_b };
                encoder.copy_buffer_to_buffer(final_buf, 0, &self.dye_a, 0, (n * 16) as u64);
            }
        }
    }
}

fn ent(binding: u32, buf: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource: buf.as_entire_binding() }
}

fn make_buf(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(16),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}
