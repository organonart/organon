//! Windows HDR output — the analog of `hdr_macos.rs`, and deliberately **not** its
//! mirror image (organon#658 Tier 4).
//!
//! macOS EDR has to be negotiated behind wgpu's back: `hdr_macos.rs` hunts the
//! `CAMetalLayer` wgpu put on the `NSView` and flips two Objective-C properties,
//! because at the time it was written wgpu had no HDR-surface API at all. Windows
//! needs none of that. **wgpu 30 exposes the whole path natively**, so this file is
//! ~100 lines of pure decision plus two one-line wgpu calls, and there is no
//! `windows-sys` dependency and no second raw-API island.
//!
//! # Why there is no DXGI in this file (the investigation #658 T4 asked for)
//!
//! #658 specified the raw route: an `Rgba16Float` scRGB swapchain
//! (`DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709`) plus headroom read off
//! `IDXGIOutput6::GetDesc1`'s `MaxLuminance`. wgpu 30 already does exactly that,
//! and does it better:
//!
//! - [`wgpu::SurfaceColorSpace::ExtendedSrgbLinear`] **is** scRGB. `wgpu-hal`'s DX12
//!   backend maps it straight to `DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709` and applies
//!   it with `IDXGISwapChain3::SetColorSpace1` on every `configure` — so the colour
//!   space rides in [`wgpu::SurfaceConfiguration`] and needs no re-assert of its own.
//!   Vulkan-on-Windows maps the same variant to `VK_COLOR_SPACE_EXTENDED_SRGB_LINEAR_EXT`,
//!   so the backend question (#658's "which backend gets RT") does not fork the HDR path.
//! - [`wgpu::Surface::display_hdr_info`] reads `IDXGIOutput6::GetDesc1` for the window's
//!   monitor — the very call #658 named — **and** the `DISPLAYCONFIG_SDR_WHITE_LEVEL`
//!   query. That second one is the part a hand-rolled `MaxLuminance` read would have got
//!   wrong: `MaxLuminance` is absolute nits, and the composite wants a *multiple of SDR
//!   white*, which moves with the Windows HDR brightness slider. `MaxLuminance` alone is
//!   not headroom. [`wgpu::DisplayHdrInfo::tone_map_headroom`] does the division.
//!
//! What that buys, beyond less code: `wgpu-hal` shares this DXGI query between the DX12
//! and Vulkan backends, so the same monitor reports the same numbers either way.
//!
//! # What this file does **not** do
//!
//! - **It does not touch the Mac.** The unification `hdr_macos.rs`'s TODO imagines —
//!   Metal EDR through the same wgpu colour-space API — is explicitly out of scope for
//!   #658 (its "Out of scope" section says so): it would need its own Mac verification,
//!   which no cloud session can give. `bin/visual.rs` selects between the two at compile
//!   time and macOS keeps the `CAMetalLayer` path byte-for-byte.
//! - **It does not deliver Rec.2020 wide gamut.** See [`WIDE_GAMUT_GRANTED`].
//!
//! # The seam with the composite
//!
//! Nothing downstream changes. `composite.wgsl`'s `hdr_max` / `hdr_knee` / `hdr_reexpand`
//! / `hdr_vivid` take a headroom *number*, which the world receives as
//! `FrameTarget::hdr_max` and never measures itself (`world.rs`'s `frame_hdr_max`). This
//! file's whole job is to make that number true on Windows instead of the flat `1.0` the
//! off-macOS stub returns today — at which point every HDR control the #24–#27 render
//! work built starts operating, unmodified.

/// Whether a Rec.2020 wide-gamut request (`hdr_wide`, #119) can be honoured on Windows.
///
/// **`false`, and not as an oversight.** macOS grants it by tagging the layer
/// `extendedLinearITUR_2020` — an *extended-linear* container with Rec.2020 primaries.
/// wgpu 30's colour-space enum has no such variant, and neither does DXGI: the only
/// Rec.2020 swapchain colour space Windows offers is [`wgpu::SurfaceColorSpace::Bt2100Pq`]
/// (HDR10) on `Rgb10a2Unorm`, which is **PQ-encoded**. Our composite writes linear
/// extended-range radiance; feeding that to a PQ surface would not be a wide-gamut
/// picture, it would be a wrong one.
///
/// So reaching Rec.2020 here means adding a PQ encode to `composite.wgsl` and a
/// third surface format to the swap — a change to the shared composite, which is
/// exactly what #658 Tier 4 promised *not* to do ("so `hdr_max` / knee / `hdr_vivid`
/// in `composite.wgsl` work **unchanged**"). It is a follow-up, and the constant is
/// here so the call site reads as a decision rather than a missing feature.
///
/// The consequence at the call site matters: with the surface tagged Rec.709 scRGB, the
/// composite must **not** arm `hdr_vivid`'s Rec.709 → Rec.2020 expansion, or it would
/// stretch colour into a container the display never agreed to. `bin/visual.rs` therefore
/// reports the *granted* gamut to the frame, not the requested one.
pub const WIDE_GAMUT_GRANTED: bool = false;

/// The colour space an HDR (`Rgba16Float`) swapchain should be configured with, given
/// the set the surface reports for that format — or `None` when the surface cannot
/// present extended-range linear at all, in which case HDR is unavailable and the
/// caller stays on the SDR surface.
///
/// Being explicit rather than leaning on [`wgpu::SurfaceColorSpace::Auto`] is the point.
/// `Auto` on an `Rgba16Float` surface resolves to `ExtendedSrgbLinear` *when supported*
/// and **silently falls back to plain `Srgb` when it is not** — which would clamp our
/// extended-range radiance to SDR while every HDR control still reported "on". Asking for
/// the colour space by name turns that silent degradation into an answerable question,
/// and `None` is the answer.
pub fn hdr_color_space(supported: wgpu::SurfaceColorSpaces) -> Option<wgpu::SurfaceColorSpace> {
    supported
        .contains(wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR)
        .then_some(wgpu::SurfaceColorSpace::ExtendedSrgbLinear)
}

/// The composite's `hdr_max` for a display described by `info`: the linear multiple of
/// SDR reference white it can drive before clipping. `1.0` = no headroom, i.e. SDR, which
/// is also what an unknown display reports — the tone-map then behaves exactly as it does
/// today.
///
/// Split from the query so the *interpretation* is testable on every platform, GPU or no
/// GPU. On Windows [`wgpu::DisplayHdrInfo::tone_map_headroom`] resolves to
/// `MaxLuminance / SDR-white-nits`; the clamp and the finite check here are ours, because
/// a headroom below 1.0 (a panel dimmer than its own SDR white — EDID numbers run
/// optimistic and occasionally nonsensical) must not *darken* the picture.
pub fn headroom_of(info: &wgpu::DisplayHdrInfo) -> f32 {
    info.tone_map_headroom()
        .filter(|h| h.is_finite())
        .unwrap_or(1.0)
        .max(1.0)
}

/// Read the display's live headroom for a presented HDR surface. `1.0` when HDR is off,
/// when the display is SDR, or when Windows reports nothing usable.
///
/// The colour space is *not* set here — it travels in [`wgpu::SurfaceConfiguration`] and
/// is applied by `configure` (see the module docs), which is why this has nothing to
/// re-assert. What it does have is a number that goes stale: the headroom moves when the
/// window changes display, when the HDR brightness slider moves, or when the panel enters
/// or leaves HDR mode. Call it wherever `hdr_macos::set_edr` is called — after every
/// `surface.configure()` — and the two platforms refresh on exactly the same events.
pub fn set_hdr_output(
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
    enable: bool,
) -> f32 {
    if !enable {
        return 1.0;
    }
    headroom_of(&surface.display_hdr_info(adapter))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `DisplayHdrInfo` shaped the way Windows reports one: absolute nits, no
    /// Apple-style headroom multiplier.
    fn windows_info(max_nits: Option<f32>, sdr_white_nits: Option<f32>, hdr: bool) -> wgpu::DisplayHdrInfo {
        wgpu::DisplayHdrInfo {
            luminance: Some(wgpu::DisplayLuminance {
                max_nits,
                sdr_white_nits,
                ..Default::default()
            }),
            coarse: Some(wgpu::DisplayCoarseRange {
                high_dynamic_range: Some(hdr),
                gamut: None,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn an_hdr_display_reports_nits_over_sdr_white() {
        // The whole reason this path exists: 1000-nit panel, Windows mapping SDR white
        // to 200 nits → 5× headroom, and the composite's re-expansion has somewhere to go.
        assert_eq!(headroom_of(&windows_info(Some(1000.0), Some(200.0), true)), 5.0);
    }

    #[test]
    fn max_luminance_alone_is_not_headroom() {
        // #658 specified "headroom from IDXGIOutput6::GetDesc1 MaxLuminance". Taken
        // literally that is 1000, not 5 — a 200× overstatement of the highlight range.
        // Without the SDR white level there is no multiplier to be had, and the honest
        // answer is the SDR fallback rather than a guess.
        assert_eq!(headroom_of(&windows_info(Some(1000.0), None, true)), 1.0);
    }

    #[test]
    fn an_sdr_display_is_flat_even_when_it_claims_nits() {
        // An SDR panel still has a physical peak above its SDR white, and that ratio is
        // not headroom anyone can drive. `high_dynamic_range: Some(false)` must win over
        // the nit arithmetic, or every SDR monitor would silently get an HDR tone-map.
        assert_eq!(headroom_of(&windows_info(Some(400.0), Some(200.0), false)), 1.0);
    }

    #[test]
    fn unknown_and_nonsensical_displays_fall_back_to_sdr() {
        // Nothing known (every backend without a display query, including a Linux or
        // macOS build that compiles this file but never calls it).
        assert_eq!(headroom_of(&wgpu::DisplayHdrInfo::default()), 1.0);
        // A panel reporting itself dimmer than its own SDR white must not *darken* the
        // picture — clamp up, never down.
        assert_eq!(headroom_of(&windows_info(Some(100.0), Some(200.0), true)), 1.0);
        // A zero SDR-white level would divide to infinity; wgpu filters it, we re-check.
        assert_eq!(headroom_of(&windows_info(Some(1000.0), Some(0.0), true)), 1.0);
    }

    #[test]
    fn scrgb_is_chosen_when_offered_and_declined_when_not() {
        // What DX12 reports for Rgba16Float.
        assert_eq!(
            hdr_color_space(wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR),
            Some(wgpu::SurfaceColorSpace::ExtendedSrgbLinear)
        );
        // A surface offering only SDR sRGB: `Auto` would have taken it and quietly
        // clamped. We decline instead, and the caller keeps the SDR swapchain.
        assert_eq!(hdr_color_space(wgpu::SurfaceColorSpaces::SRGB), None);
        // HDR10/PQ is not a substitute — our composite writes linear, not PQ.
        assert_eq!(
            hdr_color_space(wgpu::SurfaceColorSpaces::SRGB | wgpu::SurfaceColorSpaces::BT2100_PQ),
            None
        );
        assert_eq!(hdr_color_space(wgpu::SurfaceColorSpaces::empty()), None);
    }
}
