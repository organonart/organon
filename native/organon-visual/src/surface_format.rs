//! Which swapchain format to configure — decided from what the surface offers **now**
//! (organon#237).
//!
//! # Why this is a separate, pure decision
//!
//! `organic-math-visual` died twice on the workstation, mid-run, in `Surface::configure`:
//!
//! ```text
//! Requested format Rgba16Float is not in list of supported formats:
//!   [Bgra8Unorm, Bgra8UnormSrgb, Rgba8Unorm, Rgba8UnormSrgb, Rgb10a2Unorm]
//! ```
//!
//! The format list had been read **once**, in `resumed`, and `Rgba16Float` was in it then.
//! On the Vulkan backend that list is `vkGetPhysicalDeviceSurfaceFormatsKHR`, which is a
//! *live* answer: NVIDIA advertises `R16G16B16A16_SFLOAT` + `EXTENDED_SRGB_LINEAR` only
//! while the output the window sits on is in HDR mode. A monitor waking in SDR, the HDR
//! toggle, or the window landing on the other display makes the swapchain `Outdated`, the
//! reconfigure re-issues the startup format, and the validation error is a panic — a ghost
//! "not responding" window, which `doc/pbr_text_engine.md` §13 names as the worst failure
//! the screensaver case can have. (DX12 is immune by construction: its list is a fixed six
//! entries with `Rgba16Float` always present — `wgpu-hal/src/dx12/adapter.rs` — which is why
//! the panic file's list, in the driver's own order and without fp16, identifies Vulkan.)
//!
//! So the choice is made **per configure**, from the capabilities read at that moment, and
//! it is made here so it can be tested without a GPU.
//!
//! # The order, and why it is not the obvious one
//!
//! 1. `Rgba16Float`, when HDR is wanted and the surface offers it — the EDR path.
//! 2. Else the first **sRGB** format the surface offers (`Bgra8UnormSrgb` on every backend
//!    we run) — exactly what the SDR path has always used, so HDR-off behaviour is
//!    byte-identical.
//! 3. Else the first format offered at all.
//!
//! ⚠️ **`Rgb10a2Unorm` is deliberately NOT in that ladder**, although it is the format one
//! reaches for when fp16 is gone and it was the brief's suggested fallback. `composite.wgsl`'s
//! SDR arm (`hdr_max <= 1`) writes **linear** [0,1] and relies on the surface being an sRGB
//! format so the hardware applies the OETF — its header says so in as many words. There is no
//! `Rgb10a2UnormSrgb`: a 10-bit swapchain is *interpreted* as sRGB-encoded but nothing encodes
//! into it, so the composite's linear 0.2 would display as sRGB 0.2 ≈ linear 0.03 — a crushed,
//! wrong picture rather than a slightly banded right one. Reaching 10-bit properly means an
//! sRGB encode inside the composite for non-sRGB targets, which is a change to the shared
//! shader and is out of scope here (and out of this crate). Until that exists, the honest
//! fallback is the format the SDR path already renders correctly into.
//!
//! The function never panics: an empty list — which wgpu says cannot happen — yields
//! `Bgra8UnormSrgb`, and `configure` then fails inside an error scope and is logged rather
//! than aborting the process.

/// Choose the swapchain format from `offered`, the surface's current format list.
///
/// Returns `(format, hdr)`: the format to configure, and whether it is the fp16 HDR
/// surface — `true` **only** for `Rgba16Float`, so `hdr` is the grant the caller reports
/// downstream (`hdr_max`, the composite's mode, the `HDR output:` lines), never the wish.
///
/// `want_hdr` is the caller's intent *and* its knowledge that fp16 can be presented
/// extended-linear here — on Windows `hdr_output_color_space` has to agree before fp16 is
/// worth asking for (`Auto` on an fp16 surface can quietly resolve to plain sRGB, which would
/// clamp the picture while every HDR control reported on).
pub fn pick_surface_format(
    want_hdr: bool,
    offered: &[wgpu::TextureFormat],
) -> (wgpu::TextureFormat, bool) {
    use wgpu::TextureFormat as F;
    if want_hdr && offered.contains(&F::Rgba16Float) {
        return (F::Rgba16Float, true);
    }
    if let Some(f) = offered.iter().copied().find(|f| f.is_srgb()) {
        return (f, false);
    }
    (offered.first().copied().unwrap_or(F::Bgra8UnormSrgb), false)
}

/// The stderr line for "HDR was wanted and the surface could not give it", naming the format
/// actually configured and what the surface offered — so the operator can see *which* of the
/// two refusals it was: fp16 absent from the list, or present without an extended-linear
/// colour space to present it in (the Windows `Auto`-would-have-clamped case).
///
/// Ends by saying EDR is off, because the `HDR output: ON — EDR headroom …` line that would
/// otherwise follow a toggle must not appear when the fallback is in force.
pub fn fallback_line(
    offered: &[wgpu::TextureFormat],
    chosen: wgpu::TextureFormat,
    fp16_offered: bool,
) -> String {
    let why = if fp16_offered {
        "surface offers Rgba16Float but no extended-linear colour space for it"
    } else {
        "surface offers no Rgba16Float"
    };
    format!(
        "HDR output: {why} — falling back to {chosen:?}; EDR is off (SDR / ACES). \
         Offered: {offered:?}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::TextureFormat as F;

    /// The exact list from the two panics on organon-one (Vulkan, display out of HDR).
    const PANIC_LIST: [F; 5] =
        [F::Bgra8Unorm, F::Bgra8UnormSrgb, F::Rgba8Unorm, F::Rgba8UnormSrgb, F::Rgb10a2Unorm];

    /// What `wgpu-hal`'s DX12 backend advertises unconditionally.
    const DX12_LIST: [F; 6] = [
        F::Bgra8UnormSrgb,
        F::Bgra8Unorm,
        F::Rgba8UnormSrgb,
        F::Rgba8Unorm,
        F::Rgb10a2Unorm,
        F::Rgba16Float,
    ];

    #[test]
    fn the_panic_list_falls_back_to_the_srgb_8bit_surface() {
        // The list the visual died on. Wanting HDR against it must not ask for fp16 — and
        // must not take the 10-bit surface either, because the composite's SDR arm writes
        // linear and depends on the surface's sRGB OETF (see the module docs). The one
        // format the SDR path already renders correctly into is the sRGB 8-bit one.
        assert_eq!(
            pick_surface_format(true, &PANIC_LIST),
            (F::Bgra8UnormSrgb, false),
            "the fallback must be the sRGB surface the SDR path uses, and hdr must be false"
        );
    }

    #[test]
    fn rgba16float_is_chosen_when_wanted_and_offered() {
        assert_eq!(pick_surface_format(true, &DX12_LIST), (F::Rgba16Float, true));
    }

    #[test]
    fn hdr_off_never_takes_the_fp16_surface() {
        // The SDR path is unchanged by #237: fp16 offered and not wanted is the sRGB pick,
        // exactly as `resumed` chose it before the format became a per-configure decision.
        assert_eq!(
            pick_surface_format(false, &DX12_LIST),
            (F::Bgra8UnormSrgb, false),
            "with HDR off the choice must be the first sRGB format, never fp16"
        );
    }

    #[test]
    fn no_preferred_entry_takes_the_first_offered() {
        // Neither fp16 nor any sRGB format: whatever the surface lists first, hdr false.
        assert_eq!(
            pick_surface_format(true, &[F::Rgba8Unorm, F::Rgb10a2Unorm]),
            (F::Rgba8Unorm, false)
        );
    }

    #[test]
    fn ten_bit_is_never_preferred_over_an_srgb_surface() {
        // The mutation this guards: "prefer Rgb10a2Unorm when fp16 is gone". Listed first
        // so that a naive "first non-8-bit" pick would take it.
        assert_eq!(
            pick_surface_format(true, &[F::Rgb10a2Unorm, F::Rgba8UnormSrgb]),
            (F::Rgba8UnormSrgb, false),
            "Rgb10a2Unorm has no hardware OETF; the composite would display linear as sRGB"
        );
    }

    #[test]
    fn an_empty_list_cannot_panic() {
        // wgpu promises at least one format; the promise is not what keeps a lock-screen
        // process alive, this is. `configure` then fails inside its error scope.
        assert_eq!(pick_surface_format(true, &[]), (F::Bgra8UnormSrgb, false));
    }

    #[test]
    fn the_fallback_line_says_which_refusal_and_that_edr_is_off() {
        let absent = fallback_line(&PANIC_LIST, F::Bgra8UnormSrgb, false);
        assert!(absent.contains("surface offers no Rgba16Float"), "{absent}");
        assert!(absent.contains("falling back to Bgra8UnormSrgb"), "{absent}");
        assert!(absent.contains("EDR is off"), "{absent}");
        assert!(absent.contains("Rgb10a2Unorm"), "must list what was offered: {absent}");

        let no_space = fallback_line(&DX12_LIST, F::Bgra8UnormSrgb, true);
        assert!(no_space.contains("no extended-linear colour space"), "{no_space}");
        assert!(no_space.contains("EDR is off"), "{no_space}");
    }
}
