//! Compiles the RT ray-query shaders into real pipelines — the point where
//! naga generates MSL and Metal's compiler judges it (offline naga validation
//! can't catch MSL-level errors like the deleted intersection_query
//! assignment that crashed Ray-Traced AO at first enable, #195 T3).
//! Run: cargo run --release --example rt_msl_probe
//!
//! ⚠️ #626 Tier 4 — this probe spans BOTH crates, which is why it stayed here.
//! `rt_ao`/`rt_shadow`/`rt_reflect`/`rt_gi` moved into `organon-render`; `rt_debug`
//! belongs to `rt.rs`, which stayed with `world.rs` upstream. A check that asserts two
//! crates' shaders compile together belongs to neither of them — the same rule that put
//! Tier 3's cross-crate IPC test in `native/tests/` rather than inside either crate.
fn main() {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    }))
    .expect("adapter");
    assert!(adapter.features().contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY));
    let al = adapter.limits();
    let mut limits = wgpu::Limits::default();
    limits.max_blas_primitive_count = al.max_blas_primitive_count;
    limits.max_blas_geometry_count = al.max_blas_geometry_count;
    limits.max_tlas_instance_count = al.max_tlas_instance_count;
    limits.max_acceleration_structures_per_shader_stage =
        al.max_acceleration_structures_per_shader_stage;
    let (device, _queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("rt-msl-probe"),
        required_features: wgpu::Features::EXPERIMENTAL_RAY_QUERY,
        required_limits: limits,
        experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
        ..Default::default()
    }))
    .expect("device");
    device.on_uncaptured_error(std::sync::Arc::new(|e| {
        eprintln!("MSL COMPILE FAILED:\n{e}");
        std::process::exit(1);
    }));

    // (name, wgsl, fragment target format)
    let shaders: &[(&str, &str, wgpu::TextureFormat)] = &[
        ("rt_ao", include_str!("../organon-render/src/rt_ao.wgsl"), wgpu::TextureFormat::R8Unorm),
        ("rt_shadow", include_str!("../organon-render/src/rt_shadow.wgsl"), wgpu::TextureFormat::Rg8Unorm),
        (
            "rt_reflect",
            include_str!("../organon-render/src/rt_reflect.wgsl"),
            wgpu::TextureFormat::Rgba16Float,
        ),
        ("rt_debug", include_str!("../src/rt_debug.wgsl"), wgpu::TextureFormat::Rgba16Float),
        ("rt_gi", include_str!("../organon-render/src/rt_gi.wgsl"), wgpu::TextureFormat::Rgba16Float),
    ];
    for (name, src, format) in shaders {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::ShaderSource::Wgsl((*src).into()),
        });
        // layout: None = auto — enough to force per-entry-point MSL generation.
        let _ = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&format!("{name}-msl-probe")),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: None, // single @vertex per file
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: None, // single @fragment per file
                compilation_options: Default::default(),
                targets: &[Some((*format).into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        device
            .poll(wgpu::PollType::Wait { submission_index: None, timeout: None })
            .expect("poll");
        println!("{name}: MSL compiles OK");
    }
    println!("all RT shaders pass Metal compilation");
}
