//! Offline WGSL validation for the shaders that stayed with `world.rs`.
//!
//! #626 Tier 4 moved the renderer into `organon-render`, and its 50 shaders' tests went
//! with it (`organon-render/tests/wgsl.rs`) — the shaders live there, so the validation
//! does too. These four belong to `world`'s own submodules (`capture`, `nca`, `overlay`,
//! `rt_debug`) and stay here. **54 total across the two files**; if you add a shader,
//! add its test to whichever crate owns it.

use naga::valid::{Capabilities, ValidationFlags, Validator};

fn validate(name: &str, src: &str) {
    let module = naga::front::wgsl::parse_str(src)
        .unwrap_or_else(|e| panic!("{name}: WGSL parse error:\n{}", e.emit_to_string(src)));
    Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{name}: WGSL validation error: {e:?}"));
}

#[test]
fn nca_shader_is_valid() {
    // Neural Cellular Automaton step (learned surrogate, Tier B #407): perception +
    // two matmuls + tanh squash over storage buffers. Validated-but-not-yet-dispatched
    // — the CPU rollout in math.rs is authoritative; GPU dispatch is deferred (like
    // FieldSim). naga confirms the compute entry point + bounded loops here.
    validate("nca.wgsl", include_str!("../src/nca.wgsl"));
}

#[test]
fn capture_shader_is_valid() {
    validate("capture.wgsl", include_str!("../src/capture.wgsl"));
}

#[test]
fn overlay_shader_is_valid() {
    validate("overlay.wgsl", include_str!("../src/overlay.wgsl"));
}

#[test]
fn rt_debug_shader_is_valid() {
    // Hardware-RT debug view (#195 Tier 0): fullscreen ray query against the
    // TLAS. `Capabilities::all()` includes RAY_QUERY, so the `enable
    // wgpu_ray_query;` extension validates offline like every other shader.
    validate("rt_debug.wgsl", include_str!("../src/rt_debug.wgsl"));
}
