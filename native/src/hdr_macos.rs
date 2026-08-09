//! macOS Extended Dynamic Range (EDR) plumbing — the path to *true* HDR output.
//!
//! wgpu has no first-class HDR-surface API, so we reach the `CAMetalLayer` that
//! wgpu created on our `NSView` and flip the two properties that turn on EDR:
//!
//!   - `wantsExtendedDynamicRangeContent = YES`
//!   - an **extended-linear** colorspace (`extendedLinearSRGB`), in which values
//!     may exceed 1.0 — where `1.0` = SDR reference white and brighter values are
//!     genuine HDR highlights, up to the display's headroom.
//!
//! We render into an `Rgba16Float` surface and write *linear* radiance; the
//! window-server then encodes to whatever HDR signal the display wants (e.g.
//! HDR10/PQ over HDMI to an HDR projector). We never touch PQ ourselves — on
//! macOS you hand the system extended-range linear float and it does the rest.
//!
//! All entry points are no-ops off macOS so the rest of the app stays portable.

/// Enable or disable EDR on the window's metal layer, and (when enabling) return
/// the display's EDR **headroom**: the maximum component value above 1.0 the
/// screen can currently show. `1.0` means "no headroom / EDR unavailable" — the
/// HDR tonemap then behaves like the SDR one. Re-call this after every
/// `surface.configure()` while HDR is on, in case wgpu reset the layer.
/// `wide_gamut` picks `extendedLinearITUR_2020` (Rec.2020 primaries) over the
/// default `extendedLinearSRGB` (Rec.709) — the wide container for a triple-laser /
/// wide-gamut projector. The composite expands Rec.709 content into it (`hdr_vivid`).
#[cfg(target_os = "macos")]
pub fn set_edr(window: &winit::window::Window, enable: bool, wide_gamut: bool) -> f32 {
    imp::set_edr(window, enable, wide_gamut)
}

#[cfg(not(target_os = "macos"))]
pub fn set_edr(_window: &winit::window::Window, _enable: bool, _wide_gamut: bool) -> f32 {
    1.0
}

#[cfg(target_os = "macos")]
mod imp {
    use objc::runtime::{Class, Object, NO, YES};
    use objc::{class, msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::ffi::c_void;
    use winit::window::Window;

    // CoreGraphics: the extended-linear sRGB colorspace name + the create/release
    // pair. `kCGColorSpaceExtendedLinearSRGB` is a CFStringRef global.
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        static kCGColorSpaceExtendedLinearSRGB: *const c_void;
        static kCGColorSpaceExtendedLinearITUR_2020: *const c_void;
        fn CGColorSpaceCreateWithName(name: *const c_void) -> *mut c_void;
        fn CGColorSpaceRelease(space: *mut c_void);
    }

    fn ns_view(window: &Window) -> *mut Object {
        let Ok(handle) = window.window_handle() else {
            return std::ptr::null_mut();
        };
        match handle.as_raw() {
            RawWindowHandle::AppKit(a) => a.ns_view.as_ptr().cast(),
            _ => std::ptr::null_mut(),
        }
    }

    /// Find the `CAMetalLayer` wgpu draws into. wgpu (via `raw-window-metal`)
    /// adds it as a **sublayer** of the view's root layer rather than replacing
    /// the root, so `[view layer]` is winit's plain `CALayer` — we must hunt the
    /// sublayers (or use the root itself in the rare case it's already metal).
    /// Returns null if none is found (then EDR is simply not applied).
    unsafe fn metal_layer(view: *mut Object) -> *mut Object {
        let root: *mut Object = msg_send![view, layer];
        if root.is_null() {
            return std::ptr::null_mut();
        }
        let metal_cls: &Class = class!(CAMetalLayer);
        let root_is_metal: bool = msg_send![root, isKindOfClass: metal_cls];
        if root_is_metal {
            return root;
        }
        let sublayers: *mut Object = msg_send![root, sublayers];
        if sublayers.is_null() {
            return std::ptr::null_mut();
        }
        let count: usize = msg_send![sublayers, count];
        for i in 0..count {
            let l: *mut Object = msg_send![sublayers, objectAtIndex: i];
            let is_metal: bool = msg_send![l, isKindOfClass: metal_cls];
            if is_metal {
                return l;
            }
        }
        std::ptr::null_mut()
    }

    pub fn set_edr(window: &Window, enable: bool, wide_gamut: bool) -> f32 {
        // SAFETY: standard AppKit/QuartzCore message sends on a live NSView/layer.
        // Every pointer is null-checked; selectors exist on the documented classes.
        unsafe {
            let view = ns_view(window);
            if view.is_null() {
                return 1.0;
            }
            // The CAMetalLayer wgpu presents is a sublayer of the view's root
            // layer, not the root itself — sending EDR selectors to the root
            // (a plain CALayer) would raise an unrecognized-selector exception.
            let layer = metal_layer(view);
            if layer.is_null() {
                eprintln!("HDR output: no CAMetalLayer found on the window; EDR not applied.");
                return 1.0;
            }

            let want = if enable { YES } else { NO };
            let _: () = msg_send![layer, setWantsExtendedDynamicRangeContent: want];

            if enable {
                // Wide-gamut → Rec.2020 (extendedLinearITUR_2020). Triple-laser
                // projectors / wide displays are ~Rec.2020, so this is the container
                // that actually exposes their saturated primaries; the composite then
                // expands our Rec.709 content into it by `hdr_vivid` (#119).
                let cs_name = if wide_gamut {
                    kCGColorSpaceExtendedLinearITUR_2020
                } else {
                    kCGColorSpaceExtendedLinearSRGB
                };
                let cs = CGColorSpaceCreateWithName(cs_name);
                if !cs.is_null() {
                    let _: () = msg_send![layer, setColorspace: cs];
                    CGColorSpaceRelease(cs);
                }
                headroom(view)
            } else {
                // Back to the layer's default (SDR) colorspace.
                let null_cs: *mut c_void = std::ptr::null_mut();
                let _: () = msg_send![layer, setColorspace: null_cs];
                1.0
            }
        }
    }

    /// The EDR headroom of the screen the window is on. Prefers the live max;
    /// falls back to the *potential* max (what fullscreen could reach) when the
    /// live value reports no headroom yet.
    unsafe fn headroom(view: *mut Object) -> f32 {
        let window: *mut Object = msg_send![view, window];
        if window.is_null() {
            return 1.0;
        }
        let screen: *mut Object = msg_send![window, screen];
        if screen.is_null() {
            return 1.0;
        }
        let live: f64 = msg_send![screen, maximumExtendedDynamicRangeColorComponentValue];
        let max = if live > 1.0 {
            live
        } else {
            msg_send![screen, maximumPotentialExtendedDynamicRangeColorComponentValue]
        };
        (max as f32).max(1.0)
    }
}
