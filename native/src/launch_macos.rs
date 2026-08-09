//! macOS launch watchdog — make the visual's window open even when AppKit never
//! delivers `applicationDidFinishLaunching:` (organon#588).
//!
//! ## The failure this exists for
//!
//! Intermittently — several times in a session, on the plugin's "Open Visual Window"
//! path as well as on manual launches — the visual came up as an **invisible process
//! burning a whole core**: no window, no wgpu, completely empty stderr, and
//! `organon snap` timing out with *"no response from the visual within 10s"*. A
//! `sample` of a stuck process put the main thread inside `-[NSApplication run]` →
//! `_DPSNextEvent`, churning run-loop timers, with no window creation, no wgpu and no
//! render anywhere on the stack.
//!
//! It is **one missing callback**. winit dispatches `Resumed` — the event
//! `VisualApp::resumed` builds the window and the GPU device in — from its
//! `applicationDidFinishLaunching:` delegate method, and that method is also the only
//! thing that sets winit's `is_running` flag. Until the notification lands, `about_to_wait`
//! and `window_event` are gated off too, so the process has no way back: it is not waiting
//! on us, it is waiting on AppKit, forever.
//!
//! AppKit does not always post it. We are a **bare executable** — no `.app`, spawned by the
//! plugin (or a shell), not launched through LaunchServices — and for such a process the
//! notification can be deferred until something activates the app. That is exactly what the
//! bug report measured: `open`ing the *already-running* instance (LaunchServices activation,
//! no second process) produced the window within seconds, every time.
//!
//! **Waiting for that activation is not an option.** The visual is spawned while Ableton is
//! frontmost, and `main()` deliberately passes `with_activate_ignoring_other_apps(false)` so
//! it does *not* steal focus — that is what stops the host's floating plugin editor
//! disappearing out from under the user. So the fix cannot be "activate the app"; it has to
//! be "finish launching **without** activating".
//!
//! ## What it does
//!
//! [`arm`] schedules a repeating run-loop timer before `run_app`. Each tick asks [`decide`]
//! what to do:
//!
//! - **the healthy path** — `resumed` already ran, so the timer takes itself down and
//!   **AppKit is never touched at all**. This is what happens on every launch that works,
//!   because a normal `-[NSApplication run]` posts the notification synchronously, before
//!   the run loop pumps a single timer.
//! - **the stuck path** — [`GRACE`] seconds in and still nothing: call winit's
//!   `applicationDidFinishLaunching:` **directly on its delegate**, which is precisely the
//!   call the process is missing and nothing else.
//! - **give up** — after [`MAX_NUDGES`] fruitless attempts, stop and leave a line on stderr
//!   rather than poking AppKit on a timer forever.
//!
//! Calling the delegate method beats re-posting `NSApplicationDidFinishLaunchingNotification`
//! through the notification centre: the notification would also reach every *AppKit-internal*
//! observer of a launch that has genuinely not happened, whereas the delegate call reaches
//! exactly the one object whose missing callback is the bug. The real notification, if it
//! turns up later, still goes everywhere it normally would — and winit tolerates the double,
//! which is why `resumed` is written to be idempotent.
//!
//! Everything here is a no-op off macOS.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// How long AppKit gets to launch on its own before we conclude it is not going to.
/// Generous next to the ~0 ms a healthy launch takes, small next to the "did it crash?"
/// threshold of someone who just clicked *Open Visual Window*.
pub const GRACE: f64 = 0.75;

/// How many times to try before giving up. One nudge is either enough or it is the wrong
/// diagnosis; the retries exist because a *deferred* notification could in principle land
/// mid-nudge, not because repetition helps.
pub const MAX_NUDGES: u32 = 4;

/// Set by `VisualApp::resumed`. The one fact the watchdog needs: did winit dispatch
/// `Resumed`? Not "is there a window" — a launch that got as far as `Resumed` is off the
/// stuck path by definition, whatever happens next.
static RESUMED: AtomicBool = AtomicBool::new(false);

/// Nudges issued so far, so [`decide`] can stop.
static NUDGES: AtomicU32 = AtomicU32::new(0);

/// Record that winit dispatched `Resumed`. Called from the **top** of
/// `VisualApp::resumed`, before its own idempotence guard: a second `Resumed` still means
/// the app launched, and that is all this flag claims.
pub fn mark_resumed() {
    RESUMED.store(true, Ordering::Relaxed);
}

/// True once `Resumed` has been dispatched at least once.
pub fn launched() -> bool {
    RESUMED.load(Ordering::Relaxed)
}

/// What one tick of the watchdog should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    /// The app launched. Take the timer down; touch nothing.
    Stop,
    /// Still not launched — deliver the missing `applicationDidFinishLaunching:`.
    Nudge,
    /// Nudged [`MAX_NUDGES`] times without effect. Stop trying and say so.
    GiveUp,
}

/// The watchdog's entire policy, as a pure decision over the two facts it has — separated
/// from the AppKit call so the policy is testable where there is no AppKit (and no GPU, and
/// no run loop), which is everywhere `cargo test` runs.
pub fn decide(launched: bool, nudges_so_far: u32) -> Tick {
    if launched {
        Tick::Stop
    } else if nudges_so_far >= MAX_NUDGES {
        Tick::GiveUp
    } else {
        Tick::Nudge
    }
}

/// Arm the watchdog. Call **once**, on the main thread, after the `EventLoop` is built and
/// before `run_app` — the timer is added to the main run loop, which `-[NSApplication run]`
/// is about to start pumping.
///
/// No-op off macOS: no other platform gates window creation behind an activation the process
/// may never receive.
pub fn arm() {
    imp::arm();
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{decide, launched, Tick, GRACE, MAX_NUDGES, NUDGES};
    use objc::runtime::{Object, BOOL, NO};
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::c_void;
    use std::sync::atomic::Ordering;

    type CFRunLoopRef = *mut c_void;
    type CFRunLoopTimerRef = *mut c_void;
    type CFAllocatorRef = *const c_void;
    type CFStringRef = *const c_void;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFRunLoopCommonModes: CFStringRef;
        fn CFAbsoluteTimeGetCurrent() -> f64;
        fn CFRunLoopGetMain() -> CFRunLoopRef;
        fn CFRunLoopAddTimer(rl: CFRunLoopRef, timer: CFRunLoopTimerRef, mode: CFStringRef);
        fn CFRunLoopTimerCreate(
            allocator: CFAllocatorRef,
            fire_date: f64,
            interval: f64,
            flags: usize,
            order: isize,
            callout: extern "C" fn(CFRunLoopTimerRef, *mut c_void),
            context: *mut c_void,
        ) -> CFRunLoopTimerRef;
        fn CFRunLoopTimerInvalidate(timer: CFRunLoopTimerRef);
        fn CFRelease(cf: *const c_void);
    }

    // `NSApplicationDidFinishLaunchingNotification` is an exported `NSString * const` in
    // AppKit. winit's delegate ignores the notification body, but a well-formed one costs
    // two message sends and keeps the call honest.
    #[link(name = "AppKit", kind = "framework")]
    extern "C" {
        static NSApplicationDidFinishLaunchingNotification: *const Object;
    }

    pub fn arm() {
        // SAFETY: plain CoreFoundation calls on the main run loop, which exists from
        // process start. The timer is a C callback with no captured state — everything it
        // needs is in the two statics above it — so there is nothing to keep alive but the
        // timer itself, and the run loop retains that.
        unsafe {
            let timer = CFRunLoopTimerCreate(
                std::ptr::null(),
                CFAbsoluteTimeGetCurrent() + GRACE,
                GRACE,
                0,
                0,
                tick,
                std::ptr::null_mut(),
            );
            if timer.is_null() {
                return;
            }
            // Common modes, not the default mode: a modal panel or a live window resize
            // pushes the run loop into another mode, and a watchdog that stops watching
            // during exactly the states where launch is most likely to be disturbed is not
            // a watchdog. `CFRunLoopAddTimer` retains, so our +1 goes back here.
            CFRunLoopAddTimer(CFRunLoopGetMain(), timer, kCFRunLoopCommonModes);
            CFRelease(timer);
        }
    }

    extern "C" fn tick(timer: CFRunLoopTimerRef, _info: *mut c_void) {
        match decide(launched(), NUDGES.load(Ordering::Relaxed)) {
            Tick::Stop => unsafe { CFRunLoopTimerInvalidate(timer) },
            Tick::GiveUp => {
                eprintln!(
                    "organon: the visual's window never opened — AppKit did not deliver \
                     applicationDidFinishLaunching: and {MAX_NUDGES} attempts to deliver it \
                     had no effect (organon#588). Activating the process may still recover it."
                );
                unsafe { CFRunLoopTimerInvalidate(timer) };
            }
            Tick::Nudge => {
                NUDGES.fetch_add(1, Ordering::Relaxed);
                eprintln!(
                    "organon: applicationDidFinishLaunching: has not arrived {GRACE:.2}s after \
                     launch — delivering it so the visual's window can open (organon#588)."
                );
                // The nudge synchronously runs winit's `did_finish_launching`, and therefore
                // ALL of `VisualApp::resumed`: window, adapter, device, renderer. That code
                // is full of `.expect(…)`, and a panic unwinding out of an `extern "C"` fn
                // aborts. Catch it here and exit the way a panic on the normal path would
                // (winit stashes it and resumes the unwind after `run`, so the process dies
                // with 101) — the panic hook has already written the sidecar by then, which
                // is the artifact anyone debugging this will actually read.
                let nudged = std::panic::catch_unwind(std::panic::AssertUnwindSafe(deliver));
                if nudged.is_err() {
                    std::process::exit(101);
                }
                if launched() {
                    unsafe { CFRunLoopTimerInvalidate(timer) };
                }
            }
        }
    }

    /// Call `applicationDidFinishLaunching:` on whatever delegate `NSApp` has — winit's,
    /// in this process. Every step bails on nil rather than messaging null, and the
    /// selector is checked before it is sent, so a winit that stops implementing it (or a
    /// process with no delegate at all) degrades to doing nothing.
    fn deliver() {
        // SAFETY: main-thread AppKit message sends. `sharedApplication`, `delegate` and
        // `notificationWithName:object:` are documented; the delegate call is guarded by
        // `respondsToSelector:`.
        unsafe {
            let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            if app.is_null() {
                return;
            }
            let delegate: *mut Object = msg_send![app, delegate];
            if delegate.is_null() {
                return;
            }
            let responds: BOOL =
                msg_send![delegate, respondsToSelector: sel!(applicationDidFinishLaunching:)];
            if responds == NO {
                return;
            }
            let name = NSApplicationDidFinishLaunchingNotification;
            if name.is_null() {
                return;
            }
            let note: *mut Object =
                msg_send![class!(NSNotification), notificationWithName: name object: app];
            if note.is_null() {
                return;
            }
            let _: () = msg_send![delegate, applicationDidFinishLaunching: note];
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn arm() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The policy, exhaustively — it is three lines of code and the whole of what can be
    /// checked without a Mac run loop, so it is checked completely.
    #[test]
    fn the_watchdog_only_acts_on_a_launch_that_did_not_happen() {
        // A launched app is never touched, however many nudges are on the clock.
        for n in [0, 1, MAX_NUDGES, MAX_NUDGES + 10] {
            assert_eq!(decide(true, n), Tick::Stop, "a launched app must be left alone");
        }
        // Not launched: nudge, up to the cap, then stop rather than poke AppKit forever.
        for n in 0..MAX_NUDGES {
            assert_eq!(decide(false, n), Tick::Nudge, "nudge {n} should still be attempted");
        }
        assert_eq!(decide(false, MAX_NUDGES), Tick::GiveUp);
        assert_eq!(decide(false, MAX_NUDGES + 1), Tick::GiveUp);
    }

    /// One test for the process-global, deliberately: `cargo test` runs the whole suite in
    /// one process across threads in no defined order, so a second test that also wrote
    /// `RESUMED` would race this one's "defaults false" assertion and fail it for reasons
    /// unrelated to the code under test (the same trap `window_macos.rs` documents).
    ///
    /// The default matters on its own: it is what makes the watchdog *arm* — a flag that
    /// started true would silently disable the fix.
    #[test]
    fn the_resumed_flag_defaults_false_and_latches() {
        assert!(!launched(), "the watchdog must assume the app has NOT launched");
        assert_eq!(decide(launched(), 0), Tick::Nudge);

        // Arming is safe with no run loop pumping (here, and off macOS where it is a
        // no-op): the timer is created and never fires.
        arm();

        mark_resumed();
        assert!(launched());
        assert_eq!(decide(launched(), 0), Tick::Stop);
        mark_resumed(); // idempotent — `resumed` can fire more than once
        assert!(launched());
    }
}
