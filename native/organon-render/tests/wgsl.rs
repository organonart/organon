//! Offline WGSL validation for `organon-render`'s shaders.
//!
//! naga (the same crate wgpu uses internally) parses + validates each WGSL file the way
//! the GPU driver will at `create_shader_module`, but with no device. This catches
//! binding/type/uniformity errors in CI, since the GPU itself isn't available there —
//! the visual is verified on the Mac.
//!
//! #626 Tier 4 split this file out of `native/tests/wgsl.rs`. **The shaders live in this
//! crate, so their validation does too** — the same rule Tier 3 used in reverse, when a
//! test asserting *two* crates agree about a wire format was moved OUT of both into
//! `native/tests/`. One crate's invariant belongs to that crate; a shared one doesn't.
//! Four shaders (`capture`, `nca`, `overlay`, `rt_debug`) stayed with `world.rs` upstream
//! and are still validated by `native/tests/wgsl.rs`. The two files together cover all 54.

use naga::valid::{Capabilities, ValidationFlags, Validator};

fn validate(name: &str, src: &str) {
    let module = naga::front::wgsl::parse_str(src)
        .unwrap_or_else(|e| panic!("{name}: WGSL parse error:\n{}", e.emit_to_string(src)));
    Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{name}: WGSL validation error: {e:?}"));
}

#[test]
fn cube_shader_is_valid() {
    validate("cube.wgsl", include_str!("../src/cube.wgsl"));
}

#[test]
fn ibl_shader_is_valid() {
    validate("ibl.wgsl", include_str!("../src/ibl.wgsl"));
}

#[test]
fn skybox_shader_is_valid() {
    validate("skybox.wgsl", include_str!("../src/skybox.wgsl"));
}

#[test]
fn post_bloom_shader_is_valid() {
    validate("post.wgsl", include_str!("../src/post.wgsl"));
}

#[test]
fn composite_shader_is_valid() {
    validate("composite.wgsl", include_str!("../src/composite.wgsl"));
}

#[test]
fn fx_shader_is_valid() {
    // Post-composite creative FX (#152): NPR / DoF / lens FX / grade / feedback.
    validate("fx.wgsl", include_str!("../src/fx.wgsl"));
}

#[test]
fn ssao_shader_is_valid() {
    validate("ssao.wgsl", include_str!("../src/ssao.wgsl"));
}

#[test]
fn ssr_shader_is_valid() {
    validate("ssr.wgsl", include_str!("../src/ssr.wgsl"));
}

#[test]
fn vxgi_shader_is_valid() {
    // Voxel GI (#152 Tier 3, #10): voxelize compute + world-space gather.
    validate("vxgi.wgsl", include_str!("../src/vxgi.wgsl"));
}

#[test]
fn ssgi_shader_is_valid() {
    // Screen-space GI (#152 Tier 2).
    validate("ssgi.wgsl", include_str!("../src/ssgi.wgsl"));
}

#[test]
fn temporal_shader_is_valid() {
    // Temporal pass (#152 Tier 2): TAA + motion blur.
    validate("temporal.wgsl", include_str!("../src/temporal.wgsl"));
}

#[test]
fn metaball_field_shader_is_valid() {
    validate("field.wgsl", include_str!("../src/field.wgsl"));
}

#[test]
fn metaball_raymarch_shader_is_valid() {
    validate("metaball.wgsl", include_str!("../src/metaball.wgsl"));
}

#[test]
fn mandelbulb_raymarch_shader_is_valid() {
    validate("mandelbulb.wgsl", include_str!("../src/mandelbulb.wgsl"));
}

#[test]
fn creature_raymarch_shader_is_valid() {
    validate("creature.wgsl", include_str!("../src/creature.wgsl"));
}

#[test]
fn creature_overlay_shader_is_valid() {
    validate("creature_overlay.wgsl", include_str!("../src/creature_overlay.wgsl"));
}

#[test]
fn minimal_surface_raymarch_shader_is_valid() {
    validate("minimal.wgsl", include_str!("../src/minimal.wgsl"));
}

#[test]
fn lens_raymarch_shader_is_valid() {
    validate("lens.wgsl", include_str!("../src/lens.wgsl"));
}

#[test]
fn voxel_splat_shader_is_valid() {
    validate("voxelize.wgsl", include_str!("../src/voxelize.wgsl"));
}

#[test]
fn voxel_raymarch_shader_is_valid() {
    validate("voxel.wgsl", include_str!("../src/voxel.wgsl"));
}

#[test]
fn voxel_gi_downsample_shader_is_valid() {
    validate("voxgi.wgsl", include_str!("../src/voxgi.wgsl"));
}

#[test]
fn reaction_diffusion_shader_is_valid() {
    validate("rd.wgsl", include_str!("../src/rd.wgsl"));
}

#[test]
fn material_bake_shader_is_valid() {
    // #472 Tier 2: the procedural noise library + compute baker. naga confirms the
    // ~16-entry dispatcher, the periodic-lattice loops, and the storage-texture
    // write entry point compile offline (the actual bake needs a GPU).
    validate("material_bake.wgsl", include_str!("../src/material_bake.wgsl"));
}

#[test]
fn terrain_shader_is_valid() {
    validate("terrain.wgsl", include_str!("../src/terrain.wgsl"));
}

#[test]
fn stars_shader_is_valid() {
    validate("stars.wgsl", include_str!("../src/stars.wgsl"));
}

#[test]
fn particles_shader_is_valid() {
    validate("particles.wgsl", include_str!("../src/particles.wgsl"));
}

#[test]
fn splat_shader_is_valid() {
    // Gaussian Splatting surface (SurfaceMode::Splat): additive (Tier 1) + IBL-lit
    // 2DGS (Tier 2) anisotropic Gaussians. Both entry points parse + validate.
    validate("splat.wgsl", include_str!("../src/splat.wgsl"));
}

#[test]
fn fluid_shader_is_valid() {
    validate("fluid.wgsl", include_str!("../src/fluid.wgsl"));
}

#[test]
fn fluidvis_shader_is_valid() {
    // Fluid Ink (#182 Tier 1): dye blit + volumetric march + bilateral upsample.
    validate("fluidvis.wgsl", include_str!("../src/fluidvis.wgsl"));
}

#[test]
fn liquid_shader_is_valid() {
    // MLS-MPM liquid (#182 Tier 3a): P2G/grid/G2P + density splat/resolve.
    validate("liquid.wgsl", include_str!("../src/liquid.wgsl"));
}

#[test]
fn fluidlight_shader_is_valid() {
    // Fluid light coupling (#182 Tier 4): light-space dye transmittance +
    // liquid caustic splat/resolve.
    validate("fluidlight.wgsl", include_str!("../src/fluidlight.wgsl"));
}

#[test]
fn sway_shader_is_valid() {
    // Two-way coupling (#182 Tier 4): fluid velocity → per-node sway springs.
    validate("sway.wgsl", include_str!("../src/sway.wgsl"));
}

#[test]
fn liquidsurf_shader_is_valid() {
    // Refractive liquid surface (#182 T3b route B, first slice).
    validate("liquidsurf.wgsl", include_str!("../src/liquidsurf.wgsl"));
}

#[test]
fn refractsurf_shader_is_valid() {
    // Screen-space refraction for the Refractive material (#214 Tier 5 pt 2).
    validate("refractsurf.wgsl", include_str!("../src/refractsurf.wgsl"));
}

#[test]
fn kifs_shader_is_valid() {
    validate("kifs.wgsl", include_str!("../src/kifs.wgsl"));
}

#[test]
fn kaleido_shader_is_valid() {
    validate("kaleido.wgsl", include_str!("../src/kaleido.wgsl"));
}

#[test]
fn axes_shader_is_valid() {
    validate("axes.wgsl", include_str!("../src/axes.wgsl"));
}

#[test]
fn chamber_shader_is_valid() {
    validate("chamber.wgsl", include_str!("../src/chamber.wgsl"));
}

#[test]
fn rt_shadow_shader_is_valid() {
    // Hardware-RT shadow mask (#195 Tier 1): per-pixel any-hit rays toward the
    // key/fill lights off the depth prepass.
    validate("rt_shadow.wgsl", include_str!("../src/rt_shadow.wgsl"));
}

#[test]
fn rt_reflect_shader_is_valid() {
    // Hardware-RT reflections (#195 Tier 2): closest-hit reflection rays off
    // the depth prepass, hits shaded from the instance transform + tint.
    validate("rt_reflect.wgsl", include_str!("../src/rt_reflect.wgsl"));
}

#[test]
fn rt_ao_shader_is_valid() {
    // Hardware-RT ambient occlusion (#195 Tier 3): short cosine-weighted
    // hemisphere rays into GTAO's raw-AO target.
    validate("rt_ao.wgsl", include_str!("../src/rt_ao.wgsl"));
}

#[test]
fn rt_gi_shader_is_valid() {
    // Hardware-RT diffuse GI (#195 Tier 4): cosine-hemisphere one-bounce gather
    // into the SSGI buffer, hits shaded from the instance transform + tint.
    validate("rt_gi.wgsl", include_str!("../src/rt_gi.wgsl"));
}

#[test]
fn rt_pathtrace_shader_is_valid() {
    // Progressive path tracer (#200 Tier 4): camera rays + N diffuse bounces vs
    // the TLAS + NEE + emissive + sky, MRT into the accumulation + HDR scene.
    // #256 Tier 0 concatenates the radiance-cache query library (`nrc.wgsl`) ahead
    // of it (as rt_pathtrace.rs builds the module), so the early-termination's
    // `nrc_query` / `nrc_encode` / `NRC_WEIGHTS` resolve.
    let src = concat!(
        "enable wgpu_ray_query;\n",
        include_str!("../src/nrc.wgsl"),
        "\n",
        include_str!("../src/rt_pathtrace.wgsl")
    );
    validate("rt_pathtrace.wgsl", src);
}

#[test]
fn nrc_shader_is_valid() {
    // Neural radiance cache query (#256 Tier 0): the SIREN inference library has no
    // binding / entry point of its own (consumers concatenate it in — the path
    // tracer, the cube-shader ambient), so wrap it in a shell that declares a weight
    // storage buffer and calls `nrc_query` / `nrc_encode`. Validates the storage-
    // pointer function param + the SIREN arithmetic offline via naga.
    let shell = concat!(
        include_str!("../src/nrc.wgsl"),
        "\n@group(0) @binding(0) var<storage, read> nrc_w: array<f32, NRC_WEIGHTS>;\n",
        "@fragment fn fs_probe(@builtin(position) fp: vec4<f32>) -> @location(0) vec4<f32> {\n",
        "    let x = nrc_encode(fp.xyz, vec3<f32>(-1.0), vec3<f32>(1.0), normalize(fp.xyz + 0.1));\n",
        "    return vec4<f32>(nrc_query(x, 4.0), 1.0);\n",
        "}\n",
    );
    validate("nrc.wgsl", shell);
}

#[test]
fn rt_caustic_shader_is_valid() {
    // Photon-mapped caustics (#258 Tier 5): light-traced photons from the key
    // light through the specular chain (dielectric/lens/spectral), fixed-point
    // screen splats, and the KDE resolve blur into the caustic map.
    validate("rt_caustic.wgsl", include_str!("../src/rt_caustic.wgsl"));
}

#[test]
fn rt_temporal_shader_is_valid() {
    // Beat-aware temporal accumulator (#200 Tier 4½ part 3): reproject the RT
    // reflection/GI history by camera motion, neighborhood-clamp, beat-relax,
    // MRT into the composite buffer + the new history.
    validate("rt_temporal.wgsl", include_str!("../src/rt_temporal.wgsl"));
}

#[test]
fn rt_denoise_shader_is_valid() {
    // Edge-aware à-trous denoiser (#200 Tier 4½ part 2) over the RT
    // reflection / GI buffers.
    validate("rt_denoise.wgsl", include_str!("../src/rt_denoise.wgsl"));
}

#[test]
fn rt_ndenoise_shader_is_valid() {
    // Neural denoiser (#200 Tier 5a): kernel-predicting filter (classical
    // bilateral base × seeded-MLP modulation) over the RT reflection / GI buffers.
    validate("rt_ndenoise.wgsl", include_str!("../src/rt_ndenoise.wgsl"));
}

#[test]
fn neural_shader_is_valid() {
    // Neural field generator (#200 Tier 1): the MLP library concatenated with the
    // raymarch (same as neural.rs loads it), so naga validates the field eval +
    // isosurface march + PBR shading together.
    let src = concat!(include_str!("../src/mlp.wgsl"), "\n", include_str!("../src/neural.wgsl"));
    validate("neural.wgsl", src);
}

#[test]
fn mlp_shader_is_valid() {
    // Neural shading foundation (#200 Tier 0): the MLP micro-library has no
    // entry point of its own (later tiers paste it in), so wrap it in a trivial
    // fragment shell that calls `mlp_eval` — this validates the arithmetic + the
    // dynamic array indexing / integer-hash loops offline via naga.
    let shell = concat!(
        include_str!("../src/mlp.wgsl"),
        "\n@fragment fn fs_probe(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {\n",
        "    return mlp_eval(1234u, 5678u, 0.5, p, 4.0);\n",
        "}\n",
    );
    validate("mlp.wgsl", shell);
}
