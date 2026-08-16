### Organon Console compiles for macOS — the first macOS leg CI has ever had

Until now **nothing in CI proved macOS, for any edition**. Five jobs covered two editions on
Linux, a Windows cross-check and a real Windows build — and the platform the plugin is
actually developed and deployed on (`deploy.sh` is macOS-only) had no compile coverage at
all. `build (macos)` closes that for the Console: `macos-latest`, console edition, real
build plus `cargo test --workspace`.

It is **green on its first run**: the binary builds in 9 m 30 s and the suite is **2138
passed, 0 failed, 5 ignored** across 30 test binaries, whole job 17 m 46 s cold. So the
answer to "does the Console compile on macOS" is yes — with the caveat at the bottom, which
is the half that matters.

🚨 **The Windows cross-check trick does not transfer to Apple targets, and the reason is not
the one `ci.yml`'s header leads you to expect.** That header establishes — correctly — that
`check (windows cross)` needs no system packages because `native/` contains no `build.rs`
anywhere and `cargo check` does not link. Both facts are equally true of Apple targets, and
the check still fails, because the blocker is a build script in a **dependency** and
`cargo check` runs those:

```
error: failed to run custom build command for `coreaudio-sys v0.2.18`
coreaudio.h:1:10: fatal error: 'AudioUnit/AudioUnit.h' file not found
```

`nih_plug` (`features = ["standalone"]`) → `cpal` → `coreaudio-rs` → `coreaudio-sys`, whose
build script runs bindgen against the macOS SDK headers. Windows has no counterpart: its
audio and windowing reach WASAPI and Win32 through pure-Rust binding crates with nothing to
generate. So the cheapest-target ranking the header records still holds — Windows is proved
from Linux for free, macOS is not proved from Linux at all.

⚠️ **`-sys` in a crate name predicts nothing here, which matters before somebody "fixes" the
wrong one.** `jack-sys` is in the same Unix dependency graph, is equally a `-sys` crate, and
cross-checks **clean** for `aarch64-apple-darwin` from a Linux container — it is
`dlopen`-based (`libloading`) and needs no headers whatsoever. `coreaudio-sys` and
`coremidi-sys` are the bindgen ones, and they are the wall.

What *does* cross-check from Linux is nine of the workspace's eleven members, all targets,
lib and tests, in well under a minute — the compositor itself compiler-verified for Apple
silicon with no Mac in the room. That is now the contributor note's inner loop for macOS
work. It is deliberately **not** a CI job: its coverage is a strict subset of the real leg's,
and the real leg is free here. The one thing it cannot reach is the root crate, so
`console_main.rs` — the Console's entry point and window — is invisible to it.

⚠️ **The exclusion is written as a deny-list, and the allow-list version failed exactly as
predicted while this was being written.** The recipe is

```
cargo check --target aarch64-apple-darwin --all-targets \
  --workspace --exclude organic-math-native --exclude organon-visual
```

and its first draft was a `-p` list typed from CLAUDE.md's repository map, which silently
missed five real members: `organon-agent`, `organon-visual`, `organon-world`, `xtask` and
the vendored `egui-wgpu`. That is the same failure the `--workspace` rule further down
`ci.yml` exists to prevent, reproduced by hand within an hour of quoting it. `cargo metadata
--no-deps` is the authority on who the members are; a prose list is not. Only two members
reach nih_plug — the root crate, and `organon-visual`, which is excluded solely because it
*depends on* the root crate rather than being a nih_plug crate itself.

📌 **Nothing in the Console's own source needed changing**, which is worth recording as a
result rather than passing over as a non-event: `platform.rs` already takes the platform as
a **value**, macOS folds into `Platform::Unix`, and its `/bin/zsh` fallback was written for
a Mac. The rule that module's header lays down — a platform-dependent decision goes there
with a test per variant, never as a `#[cfg]` at the point of use — is why a first macOS
build found no missing arm to add.

⚠️ **The green leg means "green and ready to deploy", never "verified working."** The Console
has never been *run* on macOS — no window, no glyph, no PTY. `CONSOLE_ARCHITECTURE.md` §3
carries the list of what stays unknown until somebody opens it on a real Mac, and it is the
interesting half: Metal surface configuration, the backdrop's gamma pair on a backend where
it has never been measured, Retina scale factors, ⌘ chords arriving through winit on the
platform where ⌘ is native, a login `/bin/zsh -l` tab's inherited PATH — and the window's
icon, which is *nothing*, because `with_window_icon` is a no-op on macOS and the Console has
no `.app` bundle to carry one. No bundle, `Info.plist`, signing, notarization or install path
was attempted; a macOS build is not a macOS product.

⚠️ **Organon and Organon Mind still have no macOS coverage.** The Console leg compiles the
root crate's lib, so most shared macOS ground rides along; the
`cfg(not(feature = "console-edition"))` arms — the plugin's export macros, `standalone.rs`,
`mind_main.rs` — do not. Unlike the Windows/Mind asymmetry, which `ci.yml` argues for on
cost, this one is a gap and is labelled as one.
