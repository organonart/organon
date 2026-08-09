//! Host-less standalone entry point. Opens the same editor (all sliders) as the
//! plugin; the visual window attaches in the next milestone.

use nih_plug::prelude::*;
use organic_math_native::OrganicMath;

fn main() {
    // #520 Tier 2 — we own this `NSApplication`, so the editor is allowed to
    // reach its own `NSWindow` and make it resizable. The plugin never marks
    // itself, and `ensure_resizable` is inert without the mark, so the same
    // editor code is safe in a host (whose windows are emphatically not ours).
    organic_math_native::window_macos::mark_standalone();

    // Two gap-fillers, chained. #667 supplies `--dpi-scale` (the editor came up at 1× on a
    // HiDPI Windows display; still Windows-only, because macOS answers it itself). #658 Tier 1
    // supplies `--period-size`, and that one is **every platform** — nih-plug's 512 default is
    // a ceiling the device can overshoot on WASAPI *and* on CoreAudio, and overshooting aborts
    // the process before the IPC writer exists. The value differs per platform and
    // [`DEFAULT_PERIOD_SIZE`] explains why. They compose cleanly because they obey the same
    // rule — **fill a gap, never override an explicit choice** — and name different flags, so
    // neither can mask the other.
    //
    // ⚠️ This was Windows-only until 2026-08-09, and the first cold trial of the public repo
    // aborted on the very first launch of a stock Mac because of it. "Only Windows has this
    // problem" was an assumption nobody had tested off Windows.
    let args = args_with_dpi_scale();
    let args = with_default_period_size(args, DEFAULT_PERIOD_SIZE);

    nih_export_standalone_with_args::<OrganicMath, _>(args);
}

/// The command line nih-plug's standalone wrapper will parse, with `--dpi-scale` filled in
/// on Windows when the caller did not supply one.
///
/// ## Why this exists
///
/// The standalone editor rendered at 1× on a HiDPI Windows display — unreadably small at
/// 300%. Nothing was broken in *our* code; the 1× is chosen deliberately two layers down
/// and is simply not a defensible default for this app:
///
/// 1. `vendor/nih_plug_egui/src/lib.rs` seeds `scaling_factor` to `Some(1.0)` on every
///    platform except macOS, with a TODO admitting "that may make the GUI tiny but it also
///    prevents it from getting cut off".
/// 2. That becomes `baseview::WindowScalePolicy::ScaleFactor(1.0)`, and baseview only calls
///    `GetDpiForWindow` in its *other* branch (`SystemScaleFactor`) — so the display's real
///    DPI is never consulted.
/// 3. nih-plug's standalone wrapper overrides both with `config.dpi_scale`, a `--dpi-scale`
///    CLI flag whose default is `1.0`, noting "baseview has no way to report this to us, so
///    we'll expose it as a command line option instead".
///
/// Every layer is deferring the question to the one above it, and the top layer is a flag
/// nobody passes. So the app answers it: ask Windows for the scale factor and pass the flag
/// ourselves. **An explicit `--dpi-scale` on the command line always wins** — this only
/// fills a gap, it never overrides a choice.
///
/// ⚠️ **The clamp is not decoration.** Removing the 1× default without addressing *why* it
/// was chosen would trade a tiny window for a cut-off one, which is the failure the upstream
/// TODO was avoiding. See [`windows_dpi_scale`].
///
/// Non-Windows is untouched: macOS handles this itself (`scaling_factor: None` → baseview's
/// `SystemScaleFactor`), which is why the flag is documented as ignored there.
fn args_with_dpi_scale() -> Vec<String> {
    let mut args: Vec<String> = std::env::args().collect();

    #[cfg(windows)]
    {
        let explicit = args
            .iter()
            .any(|a| a == "--dpi-scale" || a.starts_with("--dpi-scale="));
        if !explicit {
            if let Some(scale) = windows_dpi_scale() {
                // Reported rather than silent: this is the first thing to check if the UI
                // comes up the wrong size, and a number you cannot see is a number you
                // cannot debug.
                nih_log!("HiDPI: using --dpi-scale {scale}");
                args.push("--dpi-scale".into());
                args.push(scale.to_string());
            }
        }
    }

    args
}

/// The most we will scale up when Windows cannot tell us how big the display is.
///
/// Chosen so the editor's default still fits a 1440p panel — the smallest display anyone is
/// plausibly running HiDPI on — rather than so it looks best: this value only ever applies
/// when the fit check itself is unavailable, and being wrong here costs a window hanging off
/// the bottom of the screen.
#[cfg(windows)]
const UNKNOWN_DISPLAY_MAX_SCALE: f32 = 1.5;

/// The scale factor to hand the editor on Windows, clamped so the default window still fits
/// the primary monitor's work area.
///
/// Returns `None` when the OS cannot answer or the answer is 1×, so the caller adds no flag
/// and nih-plug's own default applies unchanged.
///
/// ## The clamp
///
/// The editor's default is [`EDITOR_DEFAULT_W`]×[`EDITOR_DEFAULT_H`] **logical** points, and
/// baseview sizes the window as logical × scale. At 300% that is 3840×2580 physical — taller
/// than a 4K display, so the window would open with its lower edge off-screen. That is
/// exactly the "cut off" the upstream TODO chose 1× to avoid, so honouring the display's
/// scale *requires* answering it rather than inheriting it.
///
/// So: take the largest scale that still fits, never exceeding what the user actually asked
/// Windows for. On a display with room for the full factor this clamp does nothing. On one
/// without, the UI still comes up far larger than 1× instead of overflowing. The window
/// carries `WS_SIZEBOX` on Windows, so the user can resize from wherever this lands.
///
/// [`EDITOR_DEFAULT_W`]: organic_math_native::params::EDITOR_DEFAULT_W
/// [`EDITOR_DEFAULT_H`]: organic_math_native::params::EDITOR_DEFAULT_H
#[cfg(windows)]
fn windows_dpi_scale() -> Option<f32> {
    use organic_math_native::params::{EDITOR_DEFAULT_H, EDITOR_DEFAULT_W};
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::HiDpi::{
        GetDpiForSystem, SetProcessDpiAwarenessContext,
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA};

    // ⚠️ ORDER IS LOAD-BEARING, and getting it wrong fails *quietly with a plausible
    // number*. `GetDpiForSystem` answers a DPI-unaware process with a flat 96 — scale 1.0,
    // indistinguishable from a genuine 100% display — so awareness must be established
    // before the query, not merely before the window. Doing it here also satisfies what
    // Windows actually asks for: awareness set before any window exists.
    //
    // baseview sets `PER_MONITOR_AWARE` (V1) itself during window creation and ignores the
    // result. Awareness can only be set once per process, so that later call fails harmlessly
    // and V2 — which we prefer, for automatic non-client scaling and saner `WM_DPICHANGED` —
    // is what stands. This is additive: we are not editing baseview, only arriving first.
    let dpi = unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        GetDpiForSystem()
    };
    if dpi == 0 {
        return None;
    }

    let scale = dpi as f32 / 96.0;
    if !scale.is_finite() || scale <= 1.0 {
        return None; // 100% or nonsense — nih-plug's own 1.0 default is already right
    }

    // SPI_GETWORKAREA is the primary monitor minus the taskbar, in physical pixels for a
    // per-monitor-aware process — which we just became.
    //
    // ⚠️ On failure we must NOT fall through to the raw `scale`. Returning it unclamped
    // would take the one path in this function that skips the fit check and hand it the
    // largest possible factor — reintroducing precisely the cut-off window the clamp exists
    // to prevent, in the one case where we have no idea how big the display is. Cap it at a
    // factor the editor's default fits on any display plausibly running HiDPI at all
    // (1280×860 × 1.5 = 1920×1290, inside a 1440p panel), which is still a large, readable
    // improvement over 1×. Rare enough to be untested in practice; cheap enough to get right.
    let mut work = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let got_work_area = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut work as *mut RECT as *mut core::ffi::c_void,
            0,
        ) != 0
    };
    if !got_work_area {
        return Some(scale.min(UNKNOWN_DISPLAY_MAX_SCALE));
    }

    let (work_w, work_h) = (
        (work.right - work.left) as f32,
        (work.bottom - work.top) as f32,
    );
    // A degenerate rect is the same "we don't know the display" case as an outright failure,
    // so it takes the same conservative cap rather than the unclamped factor.
    if work_w <= 0.0 || work_h <= 0.0 {
        return Some(scale.min(UNKNOWN_DISPLAY_MAX_SCALE));
    }

    let fits = (work_w / EDITOR_DEFAULT_W as f32).min(work_h / EDITOR_DEFAULT_H as f32);
    // Two decimals: enough to matter visually, few enough that the logged value and the
    // `--dpi-scale` you would type by hand are the same string.
    let clamped = ((scale.min(fits)) * 100.0).floor() / 100.0;

    // Below 1.0 the window cannot fit even unscaled — a display smaller than the editor's
    // own minimum. Scaling *down* is not this function's job, so leave it to the default.
    if clamped <= 1.0 {
        None
    } else {
        Some(clamped)
    }
}

/// The period size the standalone asks for when the user did not name one (#658 Tier 1;
/// generalized off Windows after the first public-repo trial, 2026-08-09).
///
/// **Why this exists at all.** nih-plug's cpal backend hard-asserts that a callback never
/// hands it more frames than the configured period:
///
/// ```text
/// thread 'cpal_wasapi_out' panicked at 'Received 1056 samples, while the configured
/// buffer size is 512': .../wrapper/standalone/backend/cpal.rs:832
/// ```
///
/// and nih-plug's default is **512**. That is not a device quirk to shrug at: cpal's WASAPI
/// backend cannot honour `BufferSize::Fixed` in shared mode, so the period it *actually*
/// delivers is the endpoint's, and **any** device whose period exceeds the configured size
/// panics. 512 is smaller than a great many of them — the default output on the workstation
/// this was measured on delivers **1056**.
///
/// **Why it is worse than "no audio".** The panic lands on the audio thread *before*
/// [`OrganicMath::initialize`] runs, and `initialize` is where the IPC `Writer` is created.
/// So the failure is not a silent speaker — the standalone comes up with **no `Shared`
/// snapshot at all**, "Open Visual Window" spawns a visual that reads nothing, and `organon
/// status` reports no live session. A plain double-click was dead on arrival on Windows
/// until this line.
///
/// **Why 8192 costs nothing.** The value only sizes nih-plug's scratch buffers and sets the
/// assert's ceiling; the frames actually processed each callback are whatever WASAPI delivers.
/// `BufferConfig::max_buffer_size` is not read anywhere in this crate — `initialize` uses only
/// `sample_rate` — so nothing downstream changes shape. 8192 frames is 170 ms at 48 kHz and
/// 43 ms at 192 kHz, comfortably past any shared-mode period, for 64 KB of stereo `f32`.
///
/// This is a **default**, not an override: `-p` / `--period-size` on the command line still
/// wins, which is what [`has_period_size`] is for.
#[cfg(windows)]
const DEFAULT_PERIOD_SIZE: &str = "8192";

/// 🍎 **The same assert fires on macOS, for a different reason, and the value must differ.**
/// The first public-repo trial (2026-08-09, M5 Max, macOS 26.4) aborted eight seconds into
/// the very first launch:
///
/// ```text
/// thread 'unnamed' panicked at 'Received 558 samples, while the configured buffer size
/// is 512': .../wrapper/standalone/backend/cpal.rs:832
/// thread caused non-unwinding panic. aborting.
/// ```
///
/// **558 is not arbitrary: 512 × 48000/44100 = 557.4.** nih-plug's CLI defaults are
/// `--sample-rate 48000 --period-size 512`, and the machine's output device — like every
/// stock Mac using built-in speakers — runs at **44 100 Hz**. The rate mismatch is the root
/// cause; the oversized callback is the symptom that trips the ceiling. `organon-mind` aborts
/// identically, same file, same line, same 558.
///
/// **Why the Windows value is wrong here.** `cpal.rs:432` requests
/// `BufferSize::Fixed(period_size)` from the device *and* filters candidate configs to those
/// whose supported range contains it. CoreAudio's range tops out far below 8192, so reusing
/// the Windows number would trade an abort for "no matching output config" — a different
/// failure, not a fix. 1024 sits inside CoreAudio's range with ~1.8× headroom over the
/// measured 558.
///
/// ⚠️ **What this fixes and what it does not.** It stops the abort: the assert is a *ceiling*
/// (`actual <= configured`), so a generous one survives whatever the device delivers. It does
/// **not** correct the rate mismatch — nih-plug still believes 48 kHz on a 44.1 kHz device, so
/// tempo- and pitch-derived motion is off by 8.8% until someone passes `--sample-rate 44100`.
/// Fixing that properly means asking the device its rate before nih-plug starts, which needs
/// `cpal` as a direct dependency; that is deliberately not done here.
///
/// 📌 **Measured on Windows, reasoned on macOS.** The 8192 above was verified against a real
/// WASAPI endpoint. This 1024 is derived from the trial's numbers and from cpal's source, not
/// yet observed working — no CI leg has an audio device, so nothing here can confirm it.
/// Treat it as unverified until a Mac reports a clean launch.
#[cfg(not(windows))]
const DEFAULT_PERIOD_SIZE: &str = "1024";

/// Did the user already name a period size on the command line?
///
/// Every spelling clap accepts for `-p` / `--period-size` has to count, or we would append a
/// second value and silently overrule an explicit one: the long flag, the long flag with an
/// `=`, the short flag with a detached value, and the short flag with the value **attached**
/// (`-p1024`), which is the easy one to miss.
///
/// `--` ends option parsing, so anything past it is a positional and must not be inspected.
fn has_period_size(args: &[String]) -> bool {
    for a in args {
        if a == "--" {
            return false;
        }
        if a == "--period-size" || a == "-p" || a.starts_with("--period-size=") {
            return true;
        }
        // `-p1024`, and clustered shorts ending in `p` such as `-vp1024`. Only shorts:
        // a lone `-` or anything starting `--` was handled above.
        if a.len() > 1 && a.starts_with('-') && !a.starts_with("--") && a.contains('p') {
            return true;
        }
    }
    false
}

/// `args` with `--period-size <size>` appended when the user did not supply one.
///
/// Kept pure and platform-independent so the spelling matrix above is unit-tested on every
/// host, not only on the one where the flag is actually injected.
fn with_default_period_size(mut args: Vec<String>, size: &str) -> Vec<String> {
    if !has_period_size(&args) {
        args.push("--period-size".to_string());
        args.push(size.to_string());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn appends_a_period_size_when_none_was_given() {
        // argv[0] is deliberately a placeholder: the standalone's file name is being
        // renamed in a separate change (#666), and this test is about the flag, not the name.
        let out = with_default_period_size(v(&["prog"]), "8192");
        assert_eq!(out, v(&["prog", "--period-size", "8192"]));
    }

    /// The platform default must clear the callback sizes that were actually observed to
    /// abort the process, with headroom — a ceiling equal to the overshoot is not a fix.
    ///
    /// These two numbers are the whole reason the constant is platform-split, so pin them:
    /// **1056** is the WASAPI endpoint from #658 Tier 1, **558** is the CoreAudio callback
    /// from the first public-repo trial (512 × 48000/44100, a stock 44.1 kHz Mac).
    #[test]
    fn the_platform_default_clears_the_callbacks_that_actually_aborted() {
        let configured: u32 = DEFAULT_PERIOD_SIZE.parse().expect("period size must be numeric");
        let observed_overshoot = if cfg!(windows) { 1056 } else { 558 };
        assert!(
            configured > observed_overshoot,
            "DEFAULT_PERIOD_SIZE {configured} does not clear the observed {observed_overshoot} \
             frame callback — nih-plug's assert is `actual <= configured`, so this aborts"
        );
    }

    /// The default is injected on **every** platform now. It was Windows-only, and that is
    /// exactly how a stock Mac came to abort on its first launch — so guard the wiring, not
    /// just the helper it calls.
    #[test]
    fn the_default_is_injected_on_this_platform_whatever_it_is() {
        let out = with_default_period_size(v(&["prog"]), DEFAULT_PERIOD_SIZE);
        assert_eq!(out, v(&["prog", "--period-size", DEFAULT_PERIOD_SIZE]));
    }

    #[test]
    fn every_spelling_of_the_flag_suppresses_the_default() {
        // If any of these regressed we would append a second value and quietly overrule a
        // period the user chose on purpose.
        for spelling in [
            v(&["x", "--period-size", "1024"]),
            v(&["x", "--period-size=1024"]),
            v(&["x", "-p", "1024"]),
            v(&["x", "-p1024"]),
        ] {
            assert!(has_period_size(&spelling), "not detected: {spelling:?}");
            let out = with_default_period_size(spelling.clone(), "8192");
            assert_eq!(out, spelling, "default was appended over an explicit one");
        }
    }

    #[test]
    fn an_unrelated_flag_does_not_look_like_a_period_size() {
        assert!(!has_period_size(&v(&["x", "--sample-rate", "44100"])));
        assert!(!has_period_size(&v(&["x", "--backend", "wasapi"])));
        // `--midi-input` contains a 'p', and is exactly the kind of long flag a naive
        // `contains('p')` test would swallow.
        assert!(!has_period_size(&v(&["x", "--midi-input", "port"])));
    }

    #[test]
    fn positionals_after_a_double_dash_are_not_flags() {
        assert!(!has_period_size(&v(&["x", "--", "-p", "1024"])));
    }

    /// The two Windows gap-fillers must not shadow each other. #667's `--dpi-scale` is a long
    /// flag containing no `p`-initial short, but this pins the composition rather than trusting
    /// it: injecting the DPI flag must never look like a period size, and vice versa.
    #[test]
    fn the_dpi_flag_does_not_suppress_the_period_default() {
        let dpi_only = v(&["x", "--dpi-scale", "1.5"]);
        assert!(!has_period_size(&dpi_only));
        assert_eq!(
            with_default_period_size(dpi_only, "8192"),
            v(&["x", "--dpi-scale", "1.5", "--period-size", "8192"]),
        );
    }
}
