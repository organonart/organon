//! Metal interop island (#200 Tier 3) — the ONE Mac-gated module where a native
//! Metal pass can splice into the wgpu frame, sharing wgpu-owned resources. This
//! is the *plumbing* rung ("Tier 0 of the island itself: plumbing, dark"): a
//! startup **probe** that reports whether the island can reach Metal + (later) a
//! measured tensor-op GFLOPs premium, surfaced via `Feedback` like `tlas_ms`.
//! Nothing in the render loop depends on it yet — Tiers 5–6 (neural denoiser,
//! hybrid path tracer, radiance cache) ride it.
//!
//! **Why this is a scaffold, not live Metal, in this drop.** Every `objc2-metal`
//! / `as_hal` call is Mac-only and CANNOT be compiled or verified in the
//! GPU-less CI/dev environment (the `hdr_macos.rs` reality, one level deeper).
//! Shipping unverified `objc2-metal` here would risk breaking the *entire* native
//! build on the Mac. So `probe()` returns `unavailable` on every platform for now;
//! the module carries the complete, compile-safe groundwork — the result type,
//! the CPU-mirror verify contract (`math::island_matmul_ref`), the MSL tensor
//! kernel, and the bindings + synchronization decisions — so filling `imp` on the
//! Mac is a focused, well-scoped step, not a blank page.
//!
//! ── Bindings decision ────────────────────────────────────────────────────────
//! Use **`objc2-metal` 0.3.2** (the exact version wgpu-hal 30's Metal backend
//! already resolves — no new crate, no ABI skew), NOT a Swift/Obj-C shim compiled
//! by `bundle.sh`. Rationale: (a) it's already in the dependency graph, (b) typed
//! `Retained<ProtocolObject<dyn MTL…>>` bindings are far less error-prone than raw
//! `msg_send!` (which `hdr_macos.rs` uses for its 3 calls), and (c) sharing a
//! wgpu-owned resource via `as_hal` hands back objc2 objects, so staying in objc2
//! avoids a bridging layer. Revisit ONLY if `MTLTensor` (Metal 4 tensor ops)
//! coverage lands in objc2-metal later than a hand-rolled shim would give it.
//!
//! ── The `as_hal` handshake (Tier 5, documented not built) ───────────────────
//! To share a wgpu BUFFER with a native Metal pass (the per-frame path a live
//! neural denoiser needs), reach the underlying `MTLBuffer`:
//! ```ignore
//! // SAFETY: the closure must not outlive the buffer; the island encodes and
//! // submits within it, on wgpu's OWN queue (below), so ordering is preserved.
//! unsafe {
//!     buffer.as_hal::<wgpu::hal::api::Metal, _, _>(|raw| {
//!         let mtl_buffer = raw.expect("metal backend").raw_buffer(); // objc2 MTLBuffer
//!         // …bind into a native MTLComputeCommandEncoder…
//!     });
//! }
//! ```
//! The probe below deliberately AVOIDS `as_hal` — it runs a self-contained matmul
//! on its own device/buffers, so "does Metal work at all + how fast are the tensor
//! ops" is measured before the harder resource-sharing plumbing is built.
//!
//! ── Synchronization contract ─────────────────────────────────────────────────
//! A live per-frame island pass MUST encode on wgpu's **own** `MTLCommandQueue`
//! (obtainable via `queue.as_hal::<Metal>()` → `raw_queue()`), so its command
//! buffers order correctly against wgpu's submissions on the shared GPU timeline;
//! cross-queue work needs an `MTLSharedEvent` fence. The one-shot PROBE instead
//! uses its own queue + `waitUntilCompleted` (a full CPU/GPU barrier) — fine for a
//! startup measurement, never acceptable per-frame. This is asserted by the probe
//! being called exactly once, off the render loop.
//!
//! ── MetalFX feasibility (Tier 5c, noted) ─────────────────────────────────────
//! MetalFX temporal upscaling (`MTLFXTemporalScaler`) would instantiate against
//! island-owned textures; `hdr_macos.rs` already proves a `CAMetalLayer` is in
//! reach. The probe is the place to also time a MetalFX upscale vs the composite's
//! bilinear DRS upscale — deferred until the matmul path is live.

/// Result of the startup island probe. `available` = the island reached a Metal
/// device and ran its self-check; `verified` = the MSL matmul matched
/// `math::island_matmul_ref`; `tensor_gflops` = measured throughput (0 until the
/// live kernel runs). Surfaced via `Feedback` for the editor readout.
#[derive(Clone, Debug, Default)]
pub struct IslandProbe {
    pub available: bool,
    // `verified` (MSL matmul == CPU mirror) is set + consumed by the Mac `imp`;
    // off macOS it stays false and unread — allow it so the stub build is clean.
    #[allow(dead_code)]
    pub verified: bool,
    pub tensor_gflops: f32,
    pub device_name: String,
}

impl IslandProbe {
    pub fn unavailable() -> Self {
        IslandProbe::default()
    }
}

/// Probe the Metal island once at startup. Compile-safe on every platform; the
/// live `objc2-metal` implementation is filled into `imp` on the Mac (see the
/// module docs). `_device` is the wgpu device the live path will `as_hal` into.
pub fn probe(_device: &wgpu::Device) -> IslandProbe {
    #[cfg(target_os = "macos")]
    {
        imp::probe(_device)
    }
    #[cfg(not(target_os = "macos"))]
    {
        IslandProbe::unavailable()
    }
}

/// The trivial MSL tensor-op the island runs to (a) prove the interop path and
/// (b) measure the neural-accelerator premium vs Tier 2's WGSL/simdgroup path.
///
/// This straightforward per-thread `N×N` matmul is the BASELINE; the tensor/
/// simdgroup version replaces the inner loop with `simdgroup_matrix<float, 8, 8>`
/// + `simdgroup_multiply_accumulate` (MSL 2.3+, every Apple-silicon GPU) — the
/// A/B comparison that answers "what is the tensor-op premium on the M5". Both
/// are verified against `math::island_matmul_ref` before their timing counts.
/// (Consumed by the Mac `imp`; unused in the non-macOS stub build.)
#[allow(dead_code)]
pub const ISLAND_MATMUL_MSL: &str = r#"
#include <metal_stdlib>
using namespace metal;

// c = a * b, row-major, square N (passed in dims[0]). One thread per output
// element. `iters` (dims[1]) repeats the accumulation so the kernel is
// compute-bound (a throughput microbenchmark, not memory-bound).
kernel void island_matmul(
    device const float* a      [[buffer(0)]],
    device const float* b      [[buffer(1)]],
    device float*       c      [[buffer(2)]],
    constant uint2&     dims   [[buffer(3)]],
    uint2               gid    [[thread_position_in_grid]])
{
    uint n = dims.x;
    uint iters = max(dims.y, 1u);
    if (gid.x >= n || gid.y >= n) { return; }
    float acc = 0.0;
    for (uint it = 0; it < iters; ++it) {
        float s = 0.0;
        for (uint k = 0; k < n; ++k) {
            s += a[gid.y * n + k] * b[k * n + gid.x];
        }
        acc += s;
    }
    c[gid.y * n + gid.x] = acc / float(iters);
}
"#;

// ── Mac-only implementation (objc2-metal) ───────────────────────────────────
// Filled on the Mac (the module docs are the recipe). Kept compiling on macOS by
// returning `unavailable` until the objc2-metal device-acquire + MSL matmul +
// `island_matmul_ref` verify + GFLOPs timing lands. Isolated here so a non-macOS
// build never sees a line of Metal, and the Mac build never breaks on unverified
// interop code merged from a GPU-less session.
#[cfg(target_os = "macos")]
mod imp {
    pub fn probe(_device: &wgpu::Device) -> super::IslandProbe {
        // TODO(on-Mac, #200 Tier 3): objc2-metal 0.3.2 —
        //   1. dev = MTLCreateSystemDefaultDevice(); report dev.name().
        //   2. lib = dev.newLibraryWithSource_options_error(ISLAND_MATMUL_MSL, …);
        //      pso = dev.newComputePipelineStateWithFunction_error(lib."island_matmul").
        //   3. upload a/b MTLBuffers, dispatch (N=256, iters tuned to ~ms), wait.
        //   4. verify vs math::island_matmul_ref(&a, &b, N); measure GFLOPs =
        //      2·N³·iters / seconds; return { available:true, verified, gflops }.
        //   5. (Tier 5) redo bound to a wgpu buffer via `as_hal`, on wgpu's queue.
        super::IslandProbe::unavailable()
    }
}
