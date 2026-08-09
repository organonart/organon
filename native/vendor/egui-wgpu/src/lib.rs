//! **`egui-wgpu`, minimally vendored and ported to wgpu 30** (#554 Tier 4).
//!
//! # Why this crate exists in our tree
//!
//! Four places in this repository recorded the same conclusion, and it was wrong in an
//! important way:
//!
//! > *No released `egui-wgpu` pairs with wgpu 30, therefore egui and the renderer can never
//! > share a device, therefore the viewport has to go through CPU memory.*
//!
//! (`native/Cargo.toml`, [#541], `native/src/frame_ring.rs`'s module docs, and `STATUS.md`.)
//! The **version constraint** is entirely real — `egui-wgpu` 0.33 pins
//! `wgpu = "27.0.1"`, 0.34/0.35 moved to `^29`, and wgpu 30 is the newest wgpu, so cargo can
//! never resolve the pair. What nobody measured is the **API gap behind that constraint**, and
//! in the file we keep it is **eight mechanical fixes**:
//!
//! | change in wgpu 28–30 | fix | sites |
//! |---|---|---|
//! | pipeline-layout bind groups became optional | `bind_group_layouts: &[Some(&…)]` | 1 |
//! | `push_constant_ranges` → `immediate_size` | field rename, `&[]` → `0` | 1 |
//! | `depth_write_enabled` became `Option<bool>` | wrap in `Some` | 1 |
//! | `depth_compare` became `Option<CompareFunction>` | wrap in `Some` | 1 |
//! | vertex buffer layouts became optional | `buffers: &[Some(…)]` | 1 |
//! | `multiview` → `multiview_mask` | field rename, `None` unchanged | 1 |
//! | `QueueWriteBufferView` dropped `DerefMut<[u8]>` | `.slice(range)` → `WriteOnly` | 2 |
//!
//! Two further changes are **ours, not port fixes**: [`is_linear_target`] below, and
//! `Renderer::set_target_format` in `renderer.rs` — see "What we added" at the end.
//!
//! The full upstream crate produced 21 errors against wgpu 30; the other 13 were all in files
//! this vendor drops, which is one of the reasons for dropping them (see below).
//!
//! None of it is structural. That matters because the alternative reading — "egui is behind our
//! renderer, so they cannot share a device" — is what motivated the CPU frame mirror
//! (`frame_ring.rs`), and a CPU mirror can never be *fully accelerated* and can never carry
//! **HDR**: `egui::ColorImage` is 8-bit sRGB, and the readback costs a round trip through
//! system memory every frame. Sharing the device removes both limits at once.
//!
//! # Why vendored rather than patched
//!
//! Cargo cannot `[patch]` a crates.io crate with a different version of itself, and the change
//! we need *is* a version change. Vendoring is the same move already made for
//! `vendor/nih_plug_egui` (#541 Tier 2), for the same class of reason.
//!
//! # What was trimmed, and why that is the safe direction
//!
//! Upstream is 3,112 lines across five modules. We vendor **`renderer.rs` only** (~1,180 lines),
//! which is the part that turns egui's tessellated output into wgpu draw calls. Dropped:
//!
//! - `lib.rs` — instance/adapter/device/surface creation and `RenderState`. Organon's visual
//!   **already owns all of that** (`Gfx` in `bin/visual.rs`); egui-wgpu creating a *second*
//!   device is precisely what this tier exists to avoid.
//! - `setup.rs` — adapter selection helpers, same reason.
//! - `winit.rs` — a whole winit event/painter loop. The visual owns its winit loop.
//! - `capture.rs` — screenshot readback. `recorder.rs` and `snap.rs` already do this, better.
//!
//! Two thirds of the upstream port errors lived in exactly those dropped files, so trimming is
//! not just smaller — it removes the parts that would rot against wgpu, since they are the ones
//! that touch surface/adapter APIs. What remains is mesh upload and draw calls, which are the
//! most stable corner of the wgpu API.
//!
//! **`renderer.rs` is upstream verbatim except the wgpu-30 fixes and one addition**, each
//! marked `ORGANON PATCH (#554)` or `ORGANON ADDITION (#554)`. Keeping it otherwise
//! byte-identical is what makes it cheap to rebase
//! onto a future upstream that has done the port itself — at which point this directory is
//! deleted and the dependency goes back to crates.io.
//!
//!
//! # What we added
//!
//! **`Renderer::set_target_format`** (plus the `pipeline_layout` / `shader_module` fields it
//! needs, and `build_pipeline` lifted verbatim out of `Renderer::new`). Upstream has no way
//! to change a renderer's target format, because upstream never needs one: a surface format
//! is chosen at startup and kept. Organon's is not fixed — toggling HDR output swaps the
//! swapchain between an SDR `*Unorm` format and `Rgba16Float`, which changes both the colour
//! target and, through [`is_linear_target`], the fragment entry point.
//!
//! The obvious alternative — throw the `Renderer` away and build a new one — **crashes**, and
//! did on the Mac. A new `Renderer` starts with an empty texture map while the `egui::Context`
//! still believes its font atlas is resident, so the next atlas change arrives as an
//! *incremental* delta and `update_texture` panics on a texture that was never allocated. It
//! needs HDR toggling *and* text on screen to reproduce, which is why it survived a green
//! cloud build: with the panel hidden nothing lays out text and no delta is ever emitted.
//!
//! When upstream ships a wgpu-30 `egui-wgpu`, this is the one piece that does not come back
//! for free — reapply it, or replace it with whatever upstream provides for format changes.
//!
//! [#541]: https://github.com/organonart/organon/issues/541

mod renderer;

pub use renderer::*;

/// Re-exported so downstream code provably uses the same `wgpu` this renderer was built
/// against, rather than a second one resolved independently.
pub use wgpu;

/// Does this render target hold **linear** colour values?
///
/// This is Organon's, not upstream's — see the `ORGANON PATCH (#554)` note at the fragment
/// entry-point choice in `renderer.rs` for why the question had to change.
///
/// egui tessellates in gamma (sRGB) space and ships two fragment entry points: one that
/// converts to linear before writing, one that does not. Upstream picks between them with
/// `TextureFormat::is_srgb()`, which answers a *narrower* question than the one that matters.
/// Two different kinds of target want linear values:
///
/// - **`*Srgb` formats** — the buffer stores linear; the hardware applies the OETF on write.
/// - **float formats** — `Rgba16Float`/`Rgba32Float`, which the visual swaps to for macOS EDR
///   with an extended-linear colorspace. The buffer stores linear and there is no hardware
///   OETF at all. `is_srgb()` is `false` for these, which is what made upstream wrong here.
///
/// Everything else (`Rgba8Unorm`, `Bgra8Unorm` — egui's preferred SDR targets) wants the
/// gamma-encoded path, unchanged.
pub fn is_linear_target(format: wgpu::TextureFormat) -> bool {
    use wgpu::TextureFormat as F;
    format.is_srgb()
        || matches!(
            format,
            F::Rgba16Float | F::Rgba32Float | F::Rg11b10Ufloat | F::Rgb9e5Ufloat
        )
}

#[cfg(test)]
mod tests {
    use super::is_linear_target;
    use wgpu::TextureFormat as F;

    /// The SDR default. egui's own preferred target, and the branch upstream optimises for:
    /// the shader must gamma-encode because nothing else will.
    #[test]
    fn plain_unorm_targets_take_the_gamma_path() {
        assert!(!is_linear_target(F::Rgba8Unorm));
        assert!(!is_linear_target(F::Bgra8Unorm));
    }

    /// sRGB-tagged targets: hardware applies the OETF, so the shader must not.
    #[test]
    fn srgb_targets_take_the_linear_path() {
        assert!(is_linear_target(F::Rgba8UnormSrgb));
        assert!(is_linear_target(F::Bgra8UnormSrgb));
    }

    /// **The reason this function exists.** `Rgba16Float` is the swapchain format
    /// `bin/visual.rs` switches to for macOS EDR, and `is_srgb()` reports `false` for it — so
    /// upstream's test would send gamma-encoded values into an extended-*linear* buffer. The
    /// symptom is a washed-out, too-bright UI that appears only with HDR output on, only on a
    /// display with real headroom, and never in any test that can run without a GPU.
    #[test]
    fn float_targets_take_the_linear_path_even_though_they_are_not_srgb() {
        assert!(!F::Rgba16Float.is_srgb(), "premise: this is why upstream gets it wrong");
        assert!(is_linear_target(F::Rgba16Float));
        assert!(is_linear_target(F::Rgba32Float));
    }
}
