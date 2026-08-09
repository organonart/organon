//! Two-way coupling (#182 Tier 4) — host for `sway.wgsl`.
//!
//! Runs one compute dispatch right after the instance upload (before the
//! depth/shadow/scene passes), displacing every instance's translation by a
//! per-node damped spring driven by the local fluid velocity — the structure
//! finally moves *because of* the water it stirs (the deferred #99
//! back-reaction), with no GPU→CPU readback. The spring state persists in a
//! GPU buffer keyed by instance index; the host zeroes it when the instance
//! count changes (procedural node order is only stable at a fixed count).
//! Included by `render.rs` via `#[path]` (visual binary only).

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

/// Matches `SwayU` in sway.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SwayU {
    box0: [f32; 4],
    box1: [f32; 4],
    grid: [f32; 4],
    ctl: [f32; 4],
}

/// Fixed spring feel (the dial scales the drive; these set the character —
/// ~1.1 s period, over-damped enough not to ring). Mirrored in math tests.
pub const SWAY_SPRING: f32 = 30.0;
pub const SWAY_DAMP: f32 = 7.0;
/// Drive per unit dial: fluid velocity (world/s) → offset acceleration.
pub const SWAY_DRIVE: f32 = 12.0;
/// Offset cap in world units at dial 1 (scaled by the dial).
pub const SWAY_MAX: f32 = 2.0;

pub struct SwayFrame<'a> {
    /// The sway dial (`Shared.fluidgi[3]`, 0 = off).
    pub amount: f32,
    /// Velocity source grid: buffer + AABB + per-axis res + grid→world scale.
    pub vel_buf: &'a wgpu::Buffer,
    /// Bumped when the source buffer is recreated (res change).
    pub vel_gen: u64,
    /// Which solver the buffer belongs to (0 = Navier–Stokes, 1 = MLS-MPM) —
    /// part of the bind cache key, since the two generation counters are
    /// independent and can coincide across a source switch.
    pub vel_src: u32,
    pub vel_min: Vec3,
    pub vel_max: Vec3,
    pub vel_res: [u32; 3],
    pub vel_scale: f32,
    pub dt: f32,
}

pub struct Sway {
    ubo: wgpu::Buffer,
    state: wgpu::Buffer,
    state_cap: usize,
    last_count: usize,
    pipeline: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
    // Keyed by (instance-buffer gen, vel-buffer gen, vel source) — the two
    // buffer generations are independent counters, so the source tag keeps a
    // NS↔liquid switch from ever reusing a bind on the wrong buffer.
    bind: Option<((u64, u64, u32), wgpu::BindGroup)>,
}

impl Sway {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sway"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sway.wgsl").into()),
        });
        let ubo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sway-ubo"),
            size: std::mem::size_of::<SwayU>() as u64,
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
            label: Some("sway-layout"),
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
                sto(1, false), // instances (mutated in place)
                sto(2, false), // spring state
                sto(3, true),  // fluid velocity grid
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sway-pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("sway"),
            layout: Some(&pl),
            module: &shader,
            entry_point: Some("cs_sway"),
            compilation_options: Default::default(),
            cache: None,
        });
        Sway {
            ubo,
            state: make_state(device, 64),
            state_cap: 64,
            last_count: 0,
            pipeline,
            bgl,
            bind: None,
        }
    }

    /// Dispatch the sway update over `count` instances in `inst_buf`
    /// (`inst_gen` bumps when that buffer is recreated). Call right after the
    /// instance upload; skipped upstream when `amount == 0`.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        inst_buf: &wgpu::Buffer,
        inst_gen: u64,
        count: usize,
        f: &SwayFrame,
    ) {
        if count == 0 {
            return;
        }
        // (Re)size + zero the spring state when the node count changes — the
        // index↔node mapping is only meaningful at a stable count.
        let mut rebind = false;
        if count > self.state_cap {
            self.state_cap = count.next_power_of_two();
            self.state = make_state(device, self.state_cap);
            self.last_count = count;
            rebind = true;
            // wgpu zero-initializes fresh buffers, but write zeros explicitly
            // so the spring state never depends on that guarantee.
            queue.write_buffer(&self.state, 0, &vec![0u8; self.state_cap * 32]);
        } else if count != self.last_count {
            self.last_count = count;
            queue.write_buffer(&self.state, 0, &vec![0u8; count * 32]);
        }

        let key = (inst_gen, f.vel_gen, f.vel_src);
        if rebind || self.bind.as_ref().map(|(k, _)| *k) != Some(key) {
            self.bind = Some((
                key,
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("sway-bind"),
                    layout: &self.bgl,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: self.ubo.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: inst_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: self.state.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 3, resource: f.vel_buf.as_entire_binding() },
                    ],
                }),
            ));
        }

        let a = f.amount.clamp(0.0, 1.0);
        let u = SwayU {
            box0: [f.vel_min.x, f.vel_min.y, f.vel_min.z, count as f32],
            box1: [f.vel_max.x, f.vel_max.y, f.vel_max.z, f.dt.clamp(0.0, 0.1)],
            grid: [
                f.vel_res[0] as f32,
                f.vel_res[1] as f32,
                f.vel_res[2] as f32,
                f.vel_scale,
            ],
            ctl: [SWAY_DRIVE * a, SWAY_SPRING, SWAY_DAMP, SWAY_MAX * a],
        };
        queue.write_buffer(&self.ubo, 0, bytemuck::bytes_of(&u));

        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("sway-pass"),
            timestamp_writes: None,
        });
        cpass.set_pipeline(&self.pipeline);
        cpass.set_bind_group(0, &self.bind.as_ref().unwrap().1, &[]);
        cpass.dispatch_workgroups((count as u32).div_ceil(64), 1, 1);
    }
}

fn make_state(device: &wgpu::Device, cap: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sway-state"),
        size: (cap * 32) as u64, // 2 × vec4 per instance
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
