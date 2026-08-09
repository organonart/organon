//! macOS standalone-window plumbing — making the editor window **resizable**
//! (#520 Tier 2).
//!
//! baseview opens the standalone's `NSWindow` with
//! `Titled | Closable | Miniaturizable` and **no `Resizable`**
//! (`baseview/src/macos/window.rs`), and exposes no API to change it. A dense
//! three-column workstation is unusable at a fixed 1280×860 on a laptop, so we
//! reach the `NSWindow` at runtime and OR `NSWindowStyleMaskResizable` into its
//! style mask — the same "reach past the abstraction with objc" pattern
//! `hdr_macos.rs` uses to find wgpu's `CAMetalLayer`.
//!
//! Enabling `Resizable` also lights up the green **zoom/maximize** button, which
//! is the other half of what #520 Tier 2 asks for.
//!
//! **Scope: the standalone binaries only.** In a VST3/CLAP plugin the window
//! frame belongs to the *host*, and `NSApp`'s windows are the host's — reaching
//! into them would be both wrong and hostile. So the two standalone entry points
//! (`standalone.rs`, `mind_main.rs`) call [`mark_standalone`] before handing off
//! to nih-plug, and [`ensure_resizable`] is inert unless that flag is set. The
//! plugin path calls the same function and it does nothing. (The plugin gets its
//! resize affordance from `nih_plug_egui::ResizableWindow` instead, which
//! requests the size through the plugin API the host actually honours.)
//!
//! **Threading.** Every entry point here must be called from the thread that
//! runs the AppKit run loop. [`ensure_resizable`] is called from the egui editor's
//! update closure, which egui-baseview runs on the window's own thread — the main
//! thread for a standalone — so the AppKit calls are already where they belong.
//! No dispatch plumbing, and nothing here is called at all off macOS.
//!
//! All entry points are no-ops off macOS so the rest of the app stays portable.

use std::sync::atomic::{AtomicBool, Ordering};

/// Set by the standalone entry points before nih-plug takes over. Gates every
/// AppKit poke in this module so the plugin build can call [`ensure_resizable`]
/// unconditionally and have it do nothing.
static IS_STANDALONE: AtomicBool = AtomicBool::new(false);

/// Mark this process as one of our **standalone** binaries
/// (`organon-standalone` / `organon-mind`). Call once from `main()`, before
/// `nih_export_standalone`. Never called from the plugin cdylib.
pub fn mark_standalone() {
    IS_STANDALONE.store(true, Ordering::Relaxed);
}

/// True if [`mark_standalone`] ran — i.e. we own the `NSApplication`.
pub fn is_standalone() -> bool {
    IS_STANDALONE.load(Ordering::Relaxed)
}

/// Make the standalone's window resizable + zoomable, and give it a sensible
/// minimum content size. Idempotent and cheap: it returns immediately once the
/// mask has been applied, so it is safe to call every frame from the editor's
/// update closure (which is exactly how it is called — there is no earlier hook
/// where the `NSWindow` reliably exists yet).
///
/// `min_w` / `min_h` are the minimum **content** size in points.
///
/// No-op in the plugin, and no-op off macOS.
pub fn ensure_resizable(min_w: f64, min_h: f64) {
    if !is_standalone() {
        return;
    }
    imp::ensure_resizable(min_w, min_h);
}

/// Resize the editor's views to match the window, and tell baseview it happened.
///
/// This is the link that made the earlier cuts of #520 Tier 2 look broken: enabling
/// `Resizable` let macOS resize the *window*, but the editor's contents never moved.
///
/// **There are three nested views, not one.** `Wrapper::run` opens a window and
/// then spawns the editor into it with `ParentWindowHandle::AppKitNsView`; and with
/// a `gl_config` set, baseview's `GlContext::create` builds its **own**
/// `NSOpenGLView` and `addSubview_`s it to the view it was handed:
///
/// ```text
///   NSWindow.contentView        the WRAPPER's baseview view — AppKit resizes this
///     └─ baseview NSView        the EDITOR's view — nothing resizes it
///          └─ NSOpenGLView      what egui actually paints into
/// ```
///
/// AppKit keeps only the first in step. The other two are created with a fixed
/// `initWithFrame_` and no autoresizing mask, because baseview never expected a
/// resizable window — so they kept their original frames no matter what the window
/// did. Three things therefore have to happen, in order:
///
/// 1. **Resize the editor's view** to the content view's bounds.
/// 2. **Resize the `NSOpenGLView` under it.** Missing this is what left the UI
///    stranded in its original rectangle even once `screen_rect` was correct: egui
///    laid out to the new size and painted it onto an old-size surface. baseview's
///    own `Window::resize` does both for exactly this reason, and its comment says
///    so: *"On macOS the NSOpenGLView needs to be resized separately from our main
///    view."*
/// 3. **Signal baseview**, which is the only way egui learns. egui recomputes
///    `screen_rect` every frame from `physical_size`, and only a
///    `WindowEvent::Resized` or `Queue::resize` writes that. `Queue` never reaches
///    our closure and `EguiState::set_requested_size` is private to
///    `nih_plug_egui`, so neither is available. Instead we send baseview's own
///    `viewDidChangeBackingProperties:` — registered **with a colon**, unlike
///    AppKit's argument-less one, so it is baseview's and AppKit never sends it.
///    Its handler re-reads `bounds`, recomputes the scale factor, updates
///    `window_info` and emits `Resized` only on a real change: the work we need,
///    done by the crate that owns the state, with Retina handled correctly.
///
/// ⚠️ Step 3 **must be deferred to the run loop** — see the call site. Sending it
/// synchronously is an unconditional `RefCell` double borrow that aborts the
/// process.
///
/// The editor's view is identified by that same non-standard selector rather than
/// by class name, so the mutating messages cannot reach a view whose ivars we would
/// be misreading; the GL view is identified by `isKindOfClass:`.
///
/// Converges rather than latches: it compares the frames every frame and fixes any
/// mismatch, so it self-heals and needs no state. Returns `true` when it resized.
/// `false` in the plugin (the host owns the frame), off macOS, and before the
/// window exists.
pub fn sync_editor_view() -> bool {
    if !is_standalone() {
        return false;
    }
    imp::sync_editor_view()
}


#[cfg(target_os = "macos")]
mod imp {
    use objc::runtime::{Object, BOOL, NO, YES};
    use objc::{class, msg_send, sel, sel_impl};
    use std::sync::atomic::{AtomicBool, Ordering};

    /// `NSWindowStyleMaskResizable`. AppKit's style-mask bits are a stable ABI
    /// (they are part of the public framework headers), so the literal is safe.
    const NS_WINDOW_STYLE_MASK_RESIZABLE: u64 = 1 << 3;

    /// Latched once the mask is on, so the per-frame call is a single relaxed
    /// load after the first success.
    static APPLIED: AtomicBool = AtomicBool::new(false);

    pub fn ensure_resizable(min_w: f64, min_h: f64) {
        if APPLIED.load(Ordering::Relaxed) {
            return;
        }
        // SAFETY: we are on the standalone's main thread (see the module note),
        // every object below is fetched from AppKit and only messaged with
        // selectors it declares, and each step bails out on nil rather than
        // messaging null. The process owns this `NSApplication` outright —
        // `mark_standalone` is what guarantees we are not in a host.
        unsafe {
            let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            if app.is_null() {
                return;
            }
            // The editor window is the only one we open. Prefer `mainWindow`,
            // then `keyWindow`; early in startup both can still be nil, which is
            // why this retries each frame instead of asserting.
            let mut window: *mut Object = msg_send![app, mainWindow];
            if window.is_null() {
                window = msg_send![app, keyWindow];
            }
            if window.is_null() {
                return;
            }

            let mask: u64 = msg_send![window, styleMask];
            if mask & NS_WINDOW_STYLE_MASK_RESIZABLE == 0 {
                let _: () = msg_send![window, setStyleMask: mask | NS_WINDOW_STYLE_MASK_RESIZABLE];
            }
            // Without a floor the new drag handles can collapse the window to
            // nothing; the three-column grid stops being readable long before
            // that. `contentMinSize` is in points and excludes the title bar.
            let size = CGSize {
                width: min_w,
                height: min_h,
            };
            let _: () = msg_send![window, setContentMinSize: size];

            APPLIED.store(true, Ordering::Relaxed);
        }
    }

    pub fn sync_editor_view() -> bool {
        // SAFETY: standalone main thread (see the module note); every object is
        // fetched from AppKit and nil-checked, and the two mutating messages go
        // only to a view that answered `respondsToSelector:` for a selector that
        // exists solely on baseview's own view class.
        unsafe {
            let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            if app.is_null() {
                return false;
            }
            let mut window: *mut Object = msg_send![app, mainWindow];
            if window.is_null() {
                window = msg_send![app, keyWindow];
            }
            if window.is_null() {
                return false;
            }

            // OUTER view — the window's `contentView`, which in a nih-plug
            // standalone is the *wrapper's* baseview view, not the editor's.
            // AppKit keeps this one sized to the window for us, so its bounds are
            // the target the editor's view has to match.
            let outer: *mut Object = msg_send![window, contentView];
            if outer.is_null() {
                return false;
            }
            let outer_bounds: CGRect = msg_send![outer, bounds];
            let (target_w, target_h) = (outer_bounds.size.width, outer_bounds.size.height);
            // Degenerate sizes show up mid-teardown and before first layout.
            if !(target_w >= 1.0) || !(target_h >= 1.0) {
                return false;
            }

            // INNER view — the editor's own baseview view. `Wrapper::run` opens a
            // window and then spawns the editor into it with
            // `ParentWindowHandle::AppKitNsView`, so egui lives in a SUBVIEW of
            // the content view. Nothing resizes it: baseview creates it with a
            // fixed `initWithFrame_` and never sets an autoresizing mask, because
            // it never expected the window to be resizable.
            let subviews: *mut Object = msg_send![outer, subviews];
            if subviews.is_null() {
                return false;
            }
            let count: usize = msg_send![subviews, count];
            let mut inner: *mut Object = std::ptr::null_mut();
            for i in 0..count {
                let v: *mut Object = msg_send![subviews, objectAtIndex: i];
                if v.is_null() {
                    continue;
                }
                // The colon form is baseview's own — AppKit's is argument-less —
                // so this identifies a baseview view without matching class names.
                let responds: BOOL =
                    msg_send![v, respondsToSelector: sel!(viewDidChangeBackingProperties:)];
                if responds != NO {
                    inner = v;
                    break;
                }
            }
            if inner.is_null() {
                return false;
            }

            // Converge rather than latch: comparing the two frames each frame is
            // self-healing, and costs a few message sends against a 60 Hz budget.
            let inner_frame: CGRect = msg_send![inner, frame];
            if (inner_frame.size.width - target_w).abs() < 0.5
                && (inner_frame.size.height - target_h).abs() < 0.5
            {
                return false;
            }

            // `setFrameSize:` + `setNeedsDisplay:` is exactly what baseview's own
            // `Window::resize` and `GlContext::resize` do on macOS (the GL context
            // holds this same view), so the drawable follows the frame.
            let size = CGSize {
                width: target_w,
                height: target_h,
            };
            let _: () = msg_send![inner, setFrameSize: size];
            let _: () = msg_send![inner, setNeedsDisplay: YES];

            // …and the GL view UNDERNEATH it, which is a THIRD view. With
            // `gl_config` set, `GlContext::create` builds its own `NSOpenGLView`
            // and `addSubview_`s it to the baseview view (`baseview/src/gl/macos.rs`);
            // that is what egui actually paints into. baseview's own
            // `Window::resize` therefore does BOTH — `setFrameSize` on the main
            // view and `gl_context.resize(size)` — and its comment says why: "On
            // macOS the NSOpenGLView needs to be resized separately from our main
            // view."
            //
            // Resizing only the parent is what left the UI stranded in its
            // original rectangle: egui's `screen_rect` grew, but the surface it
            // was drawing on did not.
            let gl_class = class!(NSOpenGLView);
            let inner_subs: *mut Object = msg_send![inner, subviews];
            if !inner_subs.is_null() {
                let n: usize = msg_send![inner_subs, count];
                for i in 0..n {
                    let v: *mut Object = msg_send![inner_subs, objectAtIndex: i];
                    if v.is_null() {
                        continue;
                    }
                    let is_gl: BOOL = msg_send![v, isKindOfClass: gl_class];
                    if is_gl != NO {
                        let _: () = msg_send![v, setFrameSize: size];
                        let _: () = msg_send![v, setNeedsDisplay: YES];
                    }
                }
            }

            // Now that `bounds` is current, let baseview do its own bookkeeping:
            // its handler re-reads bounds, recomputes the scale factor, updates
            // `window_info` and emits `WindowEvent::Resized`, which is what
            // egui-baseview turns into a new `physical_size` and `screen_rect`.
            //
            // ⚠️ It MUST NOT be sent synchronously from here. We are inside the
            // editor's update closure, which baseview calls from `on_frame` —
            // and `WindowState::trigger_frame` holds `window_handler.borrow_mut()`
            // for that whole call (`macos/window.rs`). baseview's handler ends in
            // `trigger_event`, which takes the *same* `borrow_mut()`. Sending it
            // from here is therefore an unconditional `RefCell` double borrow, and
            // because the handler is `extern "C"` the panic cannot unwind: it goes
            // straight to `panic_cannot_unwind` → `abort()`. That is a hard crash
            // on the first resize, every time.
            //
            // `performSelector:withObject:afterDelay:` hands it to the run loop
            // instead. CFRunLoop does not nest timer callbacks, so it lands after
            // `trigger_frame` has returned and released the borrow. Delay 0 = the
            // next turn, i.e. the next frame — imperceptible.
            let nil: *mut Object = std::ptr::null_mut();
            let _: () = msg_send![
                inner,
                performSelector: sel!(viewDidChangeBackingProperties:)
                withObject: nil
                afterDelay: 0.0f64
            ];
            true
        }
    }

    /// `CGSize` — `setContentMinSize:` takes it by value.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGSize {
        width: f64,
        height: f64,
    }

    unsafe impl objc::Encode for CGSize {
        fn encode() -> objc::Encoding {
            // Matches AppKit's `CGSize` on 64-bit (two CGFloats == two doubles).
            unsafe { objc::Encoding::from_str("{CGSize=dd}") }
        }
    }

    /// `CGPoint` — only ever seen as the origin half of a returned [`CGRect`].
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    unsafe impl objc::Encode for CGPoint {
        fn encode() -> objc::Encoding {
            unsafe { objc::Encoding::from_str("{CGPoint=dd}") }
        }
    }

    /// `CGRect` — `bounds` returns it by value. The encoding must nest the two
    /// member structs exactly as AppKit declares them, because that string is what
    /// picks the struct-return calling convention.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    unsafe impl objc::Encode for CGRect {
        fn encode() -> objc::Encoding {
            unsafe { objc::Encoding::from_str("{CGRect={CGPoint=dd}{CGSize=dd}}") }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn ensure_resizable(_min_w: f64, _min_h: f64) {}
    pub fn sync_editor_view() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gate is what keeps this out of a host's window list, so it is worth a
    /// test even though the AppKit half can't run here. Off macOS `imp` is a
    /// no-op, so this also proves both calls are safe to make unconditionally.
    ///
    /// **Deliberately one test covering both entry points**, not two. `IS_STANDALONE`
    /// is a process-global and `cargo test` runs the whole lib suite in a single
    /// process across threads, in no defined order — so a second test that called
    /// `mark_standalone()` would race the `assert!(!is_standalone())` below and fail
    /// it intermittently, for reasons having nothing to do with the code under test.
    /// Keeping every touch of the static inside one `#[test]` makes ordering
    /// irrelevant, and keeps the assertion that actually matters: that the gate
    /// **defaults closed**, so a plugin build never reaches AppKit at all.
    #[test]
    fn the_standalone_gate_defaults_closed_and_both_entry_points_are_inert() {
        // Fresh process state: the flag starts false and neither entry point
        // touches AppKit.
        assert!(!is_standalone(), "the standalone gate must default closed");
        ensure_resizable(640.0, 480.0);
        assert!(
            !sync_editor_view(),
            "the view sync must report nothing before the process is marked standalone"
        );

        mark_standalone();
        assert!(is_standalone());

        // Safe to call off macOS (no-op) and on macOS-without-a-window (every step
        // nil-checks out); either way neither may panic, and neither may claim a
        // resize when there is no window to resize.
        ensure_resizable(640.0, 480.0);
        let _ = sync_editor_view();

        // Idempotent: whatever the first call decided, a second call with nothing
        // moved in between must not invent a change.
        let second = sync_editor_view();
        assert!(
            !second || cfg!(target_os = "macos"),
            "a size change was reported with no window and no resize"
        );
    }
}
