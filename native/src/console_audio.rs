//! **Turning a hosted producer down in the operating system's mixer.**
//!
//! 🚨 **The console does not ask the module to be quiet; it turns the module down.** That is the
//! whole design, and it is forced by `organon_module::input`'s refusal table, which has no audio
//! in either direction and says out loud that the absence is *promised, not enforced* — a
//! separate process can open WASAPI itself, and Ascent does. A `Mute` verb on that protocol
//! would be the console **asking**, so a producer that ignored it could not be silenced: a
//! control that works only while nobody minds. Naming the process to Windows' own mixer needs no
//! grant, cannot be declined, and adds no verb to a contract whose whole point is that it grants
//! narrowly.
//!
//! ⚠️ **The pid is the only thing that crosses, and it goes OUTWARD.** Nothing here opens the
//! process, reads it, or reaches inside it — `OpenProcess` appears nowhere in this file. The
//! only use is naming a process to a facility that already governs it from outside, which is
//! what [`organon_console::module_work::ModuleProcess::pid`]'s doc restricts it to.
//!
//! ⚠️ **`windows`, not `windows-sys`, and the manifest's own argument is what decides it.**
//! `Cargo.toml` chose `windows-sys` for the DPI and module-path work because *"two functions do
//! not justify the larger crate"*. This is not two functions — it is a COM subsystem, five
//! interfaces deep, and hand-rolling vtable calls over raw bindings is where thrift stops being
//! thrift. 📌 **And it costs nothing to fetch**: `windows 0.62` is already in `Cargo.lock` via
//! wgpu's own Windows tail, so on a Windows build this resolves to a crate that was going to be
//! compiled anyway — the same test `windows-sys` was admitted under.
//!
//! ⚠️ **Every failure is swallowed and reported as `false`.** A console that refused to draw
//! because it could not enumerate an audio session would be worse than one whose mute button
//! does nothing, and there is no useful recovery: the sessions are the OS's, they appear and
//! disappear on their own, and a producer that has not yet made a sound **has no session at
//! all** — see [`set_process_muted`]'s note, because that case is normal rather than an error.

/// **Mute or unmute every audio session belonging to `pid`.** Answers whether one was found.
///
/// ⚠️ **`false` is not necessarily a failure, and this is the note worth keeping.** A process
/// that has not yet played anything has **no session to mute** — Windows creates one lazily, on
/// first render — so a mute issued before the first sound legitimately finds nothing. The caller
/// must therefore treat this as *state to re-apply* rather than as a command that either worked
/// or did not: `Console` re-asserts the muted set periodically for exactly this reason, which is
/// the same "anything ambient and long-lived needs re-assertion, not change detection" shape the
/// lighting renderer on this workstation already learned the hard way.
///
/// **Every** session for the pid is set, not the first: a process may hold more than one (a game
/// with separate music and effects streams is the ordinary case), and muting one of two is a
/// control that half works.
#[cfg(windows)]
pub fn set_process_muted(pid: u32, muted: bool) -> bool {
    use windows::core::Interface;
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
        ISimpleAudioVolume, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };

    // SAFETY: every call below is a documented COM entry point, each pointer comes from the call
    // immediately above it, and every `?`/`else` arm returns without using a null. The whole body
    // is `unsafe` rather than a dozen blocks because the sequence is one transaction: an
    // interface obtained here is meaningless outside it.
    unsafe {
        // ⚠️ **Already-initialised is a SUCCESS, not a failure.** winit/wgpu initialise COM on
        // this thread first, and `CoInitializeEx` then answers `RPC_E_CHANGED_MODE` or
        // `S_FALSE` — treating either as an error would make the mixer unreachable on precisely
        // the thread the console runs on. The return is deliberately ignored; nothing here
        // uninitialises, because this thread's COM is not ours to end.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let Ok(enumerator) =
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        else {
            return false;
        };
        // The default **render** endpoint for the **console** role: the device a person's
        // ordinary program plays out of. A machine with no output device at all answers `Err`,
        // which is a real state (a headless session) and not an error to shout about.
        let Ok(device) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) else {
            return false;
        };
        let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
            return false;
        };
        let Ok(sessions) = manager.GetSessionEnumerator() else { return false };
        let Ok(count) = sessions.GetCount() else { return false };

        let mut found = false;
        for i in 0..count {
            let Ok(control) = sessions.GetSession(i) else { continue };
            // `IAudioSessionControl2` is what carries the owning pid; the base interface does
            // not, which is the whole reason for the cast.
            let Ok(control2) = control.cast::<IAudioSessionControl2>() else { continue };
            let Ok(session_pid) = control2.GetProcessId() else { continue };
            if session_pid != pid {
                continue;
            }
            let Ok(volume) = control2.cast::<ISimpleAudioVolume>() else { continue };
            // The GUID is the *event context*: it says which caller made the change so a
            // notification can be attributed. Nothing here listens for those, so it is null —
            // and passing null is the documented way to say "no context", not a shortcut.
            //
            // ⚠️ A raw `*const GUID`, not an `Option`: this parameter is not nullable in the
            // generated signature, so the null has to be spelled out.
            if volume.SetMute(muted, std::ptr::null()).is_ok() {
                found = true;
            }
        }
        found
    }
}

/// Off Windows there is no per-process mixer to reach, so this reports that it did nothing.
///
/// ⚠️ **`false` rather than a compile error or a panic**, because the console builds and its
/// tests run on Linux and macOS in CI: the control simply has no effect there, which is the
/// honest state, and the caller already treats `false` as "nothing to re-assert yet".
#[cfg(not(windows))]
pub fn set_process_muted(_pid: u32, _muted: bool) -> bool {
    false
}
