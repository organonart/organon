//! Headless probe for the #195 GPU freeze: does a BLAS+TLAS built in ONE
//! `build_acceleration_structures` call complete on Metal, or wedge the queue?
//! Run: cargo run --release --example rt_probe -- [same|split]
use std::time::{Duration, Instant};

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "same".into());
    // Watchdog: a wedged GPU queue means poll(Wait) never returns.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(12));
        eprintln!("WATCHDOG: still not done after 12s — GPU queue wedged");
        std::process::exit(2);
    });

    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    }))
    .expect("adapter");
    let feats = adapter.features();
    assert!(
        feats.contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY),
        "adapter lacks EXPERIMENTAL_RAY_QUERY"
    );
    let al = adapter.limits();
    let mut limits = wgpu::Limits::default();
    limits.max_blas_primitive_count = al.max_blas_primitive_count;
    limits.max_blas_geometry_count = al.max_blas_geometry_count;
    limits.max_tlas_instance_count = al.max_tlas_instance_count;
    limits.max_acceleration_structures_per_shader_stage =
        al.max_acceleration_structures_per_shader_stage;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("rt-probe"),
        required_features: wgpu::Features::EXPERIMENTAL_RAY_QUERY,
        required_limits: limits,
        experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
        ..Default::default()
    }))
    .expect("device");
    println!("device up (mode: {mode})");

    // A unit cube, tightly packed [f32; 3] + u32 triangle list (mirrors rt_mesh).
    let positions: Vec<[f32; 3]> = vec![
        [-0.5, -0.5, -0.5],
        [0.5, -0.5, -0.5],
        [0.5, 0.5, -0.5],
        [-0.5, 0.5, -0.5],
        [-0.5, -0.5, 0.5],
        [0.5, -0.5, 0.5],
        [0.5, 0.5, 0.5],
        [-0.5, 0.5, 0.5],
    ];
    #[rustfmt::skip]
    let indices: Vec<u32> = vec![
        0,1,2, 0,2,3,  4,6,5, 4,7,6,  0,4,5, 0,5,1,
        3,2,6, 3,6,7,  1,5,6, 1,6,2,  0,3,7, 0,7,4,
    ];
    use wgpu::util::DeviceExt;
    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("verts"),
        contents: bytemuck::cast_slice(&positions),
        usage: wgpu::BufferUsages::BLAS_INPUT,
    });
    let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("indices"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::BLAS_INPUT,
    });
    let size = wgpu::BlasTriangleGeometrySizeDescriptor {
        vertex_format: wgpu::VertexFormat::Float32x3,
        vertex_count: positions.len() as u32,
        index_format: Some(wgpu::IndexFormat::Uint32),
        index_count: Some(indices.len() as u32),
        flags: wgpu::AccelerationStructureGeometryFlags::OPAQUE,
    };
    let blas = device.create_blas(
        &wgpu::CreateBlasDescriptor {
            label: Some("blas"),
            flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
            update_mode: wgpu::AccelerationStructureUpdateMode::Build,
        },
        wgpu::BlasGeometrySizeDescriptors::Triangles { descriptors: vec![size.clone()] },
    );
    let mut tlas = device.create_tlas(&wgpu::CreateTlasDescriptor {
        label: Some("tlas"),
        max_instances: 4096,
        flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
        update_mode: wgpu::AccelerationStructureUpdateMode::Build,
    });
    for i in 0..1000usize {
        let x = (i % 10) as f32;
        let y = ((i / 10) % 10) as f32;
        let z = (i / 100) as f32;
        #[rustfmt::skip]
        let transform = [
            1.0, 0.0, 0.0, x * 2.0,
            0.0, 1.0, 0.0, y * 2.0,
            0.0, 0.0, 1.0, z * 2.0,
        ];
        tlas[i] = Some(wgpu::TlasInstance::new(&blas, transform, i as u32, 0xff));
    }

    macro_rules! blas_entry {
        () => {
            wgpu::BlasBuildEntry {
                blas: &blas,
                geometry: wgpu::BlasGeometries::TriangleGeometries(vec![
                    wgpu::BlasTriangleGeometry {
                        size: &size,
                        vertex_buffer: &vbuf,
                        first_vertex: 0,
                        vertex_stride: 12,
                        index_buffer: Some(&ibuf),
                        first_index: Some(0),
                        transform_buffer: None,
                        transform_buffer_offset: None,
                    },
                ]),
            }
        };
    }

    let t0 = Instant::now();
    if mode == "same" {
        // The rt.rs path: BLAS + the TLAS that references it, ONE call.
        let mut enc = device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("same") });
        let entries = [blas_entry!()];
        enc.build_acceleration_structures(entries.iter(), std::iter::once(&tlas));
        queue.submit(std::iter::once(enc.finish()));
    } else {
        // Split: BLAS submission first, then the TLAS build.
        let mut enc = device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("blas") });
        let entries = [blas_entry!()];
        enc.build_acceleration_structures(entries.iter(), std::iter::empty());
        queue.submit(std::iter::once(enc.finish()));
        let mut enc = device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("tlas") });
        enc.build_acceleration_structures(std::iter::empty(), std::iter::once(&tlas));
        queue.submit(std::iter::once(enc.finish()));
    }
    println!("submitted, polling…");
    device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).expect("poll");
    println!("DONE in {:.1} ms — queue healthy", t0.elapsed().as_secs_f64() * 1000.0);

    // Prove the queue still works: 60 more TLAS-only builds (the per-frame path).
    let t1 = Instant::now();
    for _ in 0..60 {
        let mut enc =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        enc.build_acceleration_structures(std::iter::empty(), std::iter::once(&tlas));
        queue.submit(std::iter::once(enc.finish()));
    }
    device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).expect("poll2");
    println!(
        "60 per-frame TLAS rebuilds: {:.2} ms total ({:.3} ms each)",
        t1.elapsed().as_secs_f64() * 1000.0,
        t1.elapsed().as_secs_f64() * 1000.0 / 60.0
    );
}
