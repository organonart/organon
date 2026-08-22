# The worked example: Organon Voice

Everything in `SKILL.md` came from one installer, in a sibling repository
(`workshop-machines`, `installer/` and `services/voice-tray/`). You do not need
that repository — this file carries what it taught, and quotes its own comments
where they are the best available writing on why a line exists.

**The product.** A Windows tray application for push-to-talk dictation. Hold a
chord, speak, release, and the text is typed at the cursor. Two speech models
run at once: a fast one that shows words as you say them, and a slower one that
re-reads the whole utterance and corrects it.

**The installer.** 22 MB, per-user, `PrivilegesRequired=lowest`. Nothing in
Program Files, nothing in HKLM, no service. It carries the program — about 25 MB
of executable and three DLLs — and **no model weights at all**: those are chosen
on a wizard page and fetched from their origins at install time, which keeps the
installer small enough to send someone and means no third party's weights are
redistributed.

---

## The failure that produced all of this

The first machine other than the build machine to run it was an Alienware box
that had been a games machine. It had never built anything.

- The installer **installed perfectly**.
- The program **would not start**.
- **Nothing said anything.**

The cause: ONNX Runtime 1.23 is built with MSVC 14.4x. Since Visual Studio 2022
17.10 the standard library changed how `std::mutex` is represented, and code
built after that change, run against an `msvcp140.dll` older than 14.40, **does
not fail to link** — every import resolves, and it then access-violates inside
the runtime during static initialisation. The loader reports
`STATUS_DLL_INIT_FAILED`, the process is gone at `0xC0000142` before `main()`,
and there is nothing to print to and nothing to log with.

That machine had 14.30, from January 2022. The build machine had had a current
runtime for years, because CMake and the build tools put one there.

> **That is the shape of every bug this machine will find: not a mistake in the
> code, but an unstated dependency on how the build machine happens to be set
> up.**
> — `hosts/organon-two/README.md`

And the second half of the failure, which is why stage 2 of the method exists:

- `--tray` printed everything to a console the shortcut throws away.
- The tray's "Open log" menu item pointed at a path that **only the build
  machine's `.vbs` launcher ever wrote**, so on any other machine it opened
  nothing.

**Note the order.** A log cannot record a failure in the loader, so a log would
not have caught this one. It catches everything after it. The prerequisite check
and the log cover different halves of the problem and neither substitutes for
the other.

---

## The prerequisite check, as shipped

Three decisions in it, each of which is wrong in an interesting way if reversed.

**It is a version test, not a presence test.** From the script's own comment:

> So the test has to be a version, not a presence: the file being there proves
> nothing, and a missing-export check would pass on the exact machine that
> crashes.

**It reads `{sysnative}`, not `{sys}`.** Setup is deliberately kept 32-bit
(`ArchitecturesInstallIn64BitMode` is *not* set, so the registry view under
which the uninstall entry was written does not move out from under an upgrade).
A 32-bit process sees `{sys}` redirected to SysWOW64:

> A machine with a current x86 runtime and a stale x64 one is ordinary, and
> reading the wrong one would report health on precisely the configuration that
> fails.

**It installs with `ShellExec`, not `Exec`.** `vc_redist.x64.exe` is manifested
`requireAdministrator`, and `Exec` uses `CreateProcess`, which does not elevate —
it fails with "elevation required" and looks like a broken download. Exit codes
**0, 1638 and 3010 are all success**; 3010 means "installed, wants a reboot",
the DLLs are already on disk, and reporting it as a failure is a bug.

**It is announced on the Ready page**, because a per-user installer that raises
UAC without warning reads as a betrayal of its own promise:

> Announcing it here means the prompt arrives as something that was agreed to
> rather than as a surprise from a per-user installer that promised to touch
> nothing outside the profile.

And when elevation is declined, the message names the **consequence**, not the
error — "will install, but will not start … exits immediately and silently" —
because the error is not the thing the reader needs.

The code is in `references/inno-setup.md`.

---

## Giving the program somewhere to complain

The binary is GUI-subsystem so the tray never flashes a console, and takes its
console *back* when there is one:

```rust
// ⚠️ `not(test)`, and it is load-bearing. The test harness is built from this
// same crate root, so an unconditional attribute would make the *test* binary a
// GUI application too — and `cargo test` would report nothing at all, passing
// and failing in identical silence.
#![cfg_attr(not(test), windows_subsystem = "windows")]
```

`attach_parent_console()` distinguishes three cases **in this order**, and its
doc comment is the clearest statement of why:

> 1. **Handles already valid.** The parent passed them in — a redirect, a pipe,
>    `Start-Process -RedirectStandardOutput`. Inherited handles are used exactly
>    as given; touching them would break the caller's redirection. This is the
>    case `installer/build.ps1` relies on when it reads the artifact's own usage
>    text.
> 2. **A parent console exists.** Started from a shell. `AttachConsole` joins
>    it, and `CONOUT$` is opened onto the standard handles — without that second
>    step `GetStdHandle` keeps returning null and `println!` writes nowhere,
>    which looks exactly like the program having nothing to say.
> 3. **Neither.** A shortcut, the Startup folder, a scheduled task. Nobody is
>    watching, so the caller should write a log file instead.

`CONOUT$` must be opened **read/write** — a write-only handle is rejected by
some consoles.

**Why standard-handle redirection rather than a logging crate.** The program
already says everything it has to say through `println!` / `eprintln!` in a few
hundred places, and the ones most worth having are in the paths nobody edits
carefully. Replacing the process's standard handles captures all of it —
including messages written before anyone thought about logging, and including
**the panic message**, which is the single most valuable line and the one a
hand-rolled logger would miss. (Rust's Windows stdio calls `GetStdHandle` per
write rather than caching it, so this takes effect immediately.)

Two details that bite:

- ⚠️ **The file handle must outlive the function.** `SetStdHandle` stores it
  without taking ownership, so dropping the `File` closes the handle, and every
  later `println!` then fails — and Rust's `println!` **panics** when it cannot
  write. It is leaked on purpose, for the life of the process.
- ⚠️ **Do not redirect when a shell is watching.** If someone ran the program
  from a terminal to read what it says, swallowing that into a file breaks the
  exact workflow that finds problems. `GetConsoleProcessList` is the
  discriminator: a console Windows allocated for a shortcut has one process
  attached; a console inherited from PowerShell has at least two. The
  redirection function *asks the attach function* rather than re-deciding —
  "a second, independent test here is how a program ends up writing to a
  console it has already decided is absent."

The log rotates by rename at 2 MB rather than truncating, because truncation
destroys the run that produced the size — on a program that appends once per
launch, likely the interesting one.

**The visible cost, stated because it looks like a bug:** a GUI process does not
hold the shell, so a diagnostic run from a terminal returns the prompt
immediately and prints *under* it. Complete and correct, just after the prompt.
Redirected runs are unaffected, which is what keeps case 1 above working.

---

## The catalogue, and one fact in one place

The models are described once, in `installer/models.toml`, which is read by
**both** the installer and the application. Its header says why:

> ⚠️ **One file, two readers, on purpose.** The installer reads this to build
> its selection page and to download; the application reads it to find what was
> installed. They must not each carry their own list.

An Inno Setup script cannot read TOML, so `gen-models-iss.ps1` generates the
Pascal table from it, into files whose first line is `DO NOT EDIT`. The
generator **refuses** rather than degrading:

- A model with more than one `conflicts_with` entry throws, because the Pascal
  side holds a single id and would otherwise silently drop the rest.
- A non-ASCII character it does not have a fold for throws, because Inno reads a
  BOM-less `.iss` as ANSI and the character would reach the selection page as
  mojibake.

⚠️ **And it was written with a hole in it.** Single-line TOML arrays had no case
in the parser, so every `conflicts_with` came out empty and the installer would
have let someone tick two mutually exclusive models. The generator existed, the
catalogue was correct, and the fact evaporated in between —
*"A key that exists in the catalogue and evaporates in the generator is the
exact failure the catalogue was written to prevent."*

---

## What could not be tested, and why

This is the part to imitate.

**The prerequisite check can no longer be exercised on the machine that
motivated it.** Making that box a build host is what took it away: Visual Studio
ships its own redistributable and keeps `msvcp140.dll` current in System32
regardless, so uninstalling the standalone redistributable changes nothing.
Getting below 14.40 would mean removing the C++ workload, which is the same as
no longer being a build host. Measured, then reverted.

> The check therefore rests on reasoning rather than on a run: read the version
> of the **x64** `msvcp140.dll` and fetch Microsoft's package when it is below
> 14.40. A third machine, or this one before it was set up, is what would prove
> it.

**`{sysnative}` versus `{sys}` lost its evidence the same way.** The
redistributable installs x86 and x64 together, so both System32 and SysWOW64 now
hold 14.44 — the two paths agree, and a wrong constant would look right.

**What *was* verified on that machine**, and is worth separating out:

- The corrector reached DirectML on a GPU other than the developer's, on a 2021
  driver: ~6 s to load, then ~178 ms warm and essentially flat with clip length.
  ⚠️ Flat with length means what is being measured is **fixed overhead, not
  throughput** — do not quote it as a rate.
- The log file, in both directions: started from the shortcut it writes a
  header and captures everything after it; started from a terminal it leaves the
  file untouched and prints to the terminal, saying so and naming the path.
  Before that change **no log file existed on that machine at all**.
- A silent install `/VERYSILENT` launched through Git Bash "hung" — it was a
  wizard, waiting for a click, behind everything else. See
  `references/windows-shell.md`.

---

## Where the original lives

`workshop-machines`, on the branch that carried this work rather than `main`:
`installer/organon-voice.iss`, `installer/build.ps1`,
`installer/gen-models-iss.ps1`, `installer/models.toml`,
`services/voice-tray/src/logfile.rs`, and the subsystem attribute at the top of
`services/voice-tray/src/main.rs`. ⚠️ It is a different product with different
prerequisites — read it for the *shape* of a decision, never to copy a version
floor or a path.
