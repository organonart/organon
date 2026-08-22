---
name: create-installer
description: Turn a Windows build into something a stranger can install, and know which of your claims about it you have actually tested. Use when asked to package, ship, hand out, or install an Organon build on a machine that is not the developer's; to write or repair an installer, a prerequisite check, an upgrade path or an uninstall; to decide between an installer and the existing deploy scripts; or when a build that runs perfectly here dies on someone else's machine before it prints anything. Covers what organon already has — `native\bundle.ps1`, `native\deploy.ps1`, `native\bundler.toml` — which is developer deploy, not an installer, and what the difference costs.
---

# Making something a stranger can install

## The governing idea

**An installer's real job is to close the gap between a machine configured by
building the product and one that never was.**

A machine that builds a thing is set up *by* building it. Every compiler,
runtime, SDK and driver the product needs is already there, put there by
something else, for reasons nobody wrote down. Your machine cannot tell you what
it is supplying, because from the inside a supplied dependency and an absent one
look identical: the program works.

So the first question, before any tooling decision, is:

> **What does this product take from the machine it was built on that a
> stranger's machine will not have?**

Every expensive bug in the worked example had that shape. None of them were
mistakes in the code. See `references/worked-example.md` — an installer that
installed perfectly, would not start, and said nothing, because the target had a
Visual C++ runtime from January 2022.

🚨 **You cannot answer that question from this machine.** It is the one question
your own box is structurally unable to answer. Getting an honest answer needs a
second machine, or an honest ledger saying which claims are reasoned rather than
run. Both are covered below.

---

## Before you propose anything: what organon already has

Read these files. They are real, they predate you, and an installer sits
**downstream** of them rather than replacing them.

| File | What it actually does |
|---|---|
| `native/bundler.toml` | Two lines. Read by `cargo xtask bundle` (nih_plug_xtask, a git dependency of `native/xtask`) to map the package `organic-math-native` to the bundle name `Organon`. **It is not an installer manifest** and has nothing to say about a machine that is not yours. |
| `native/bundle.ps1` | Builds `organic-math-visual.exe` and `cargo xtask bundle`, then **embeds the visual inside** `target\bundled\Organon.vst3` at `Contents\<arch>-win\`. `-Install` copies the bundle to `F:\vst3`. `-WithLlm` also builds and embeds `organic-math-mind-runtime.exe`. |
| `native/deploy.ps1` | All of the above plus the `organon` CLI, the asset galleries into `%APPDATA%\OrganicMath\`, shell completions, an optional USER PATH edit, and detection of the loaded-DLL lock. |
| `native/bundle.sh` · `native/deploy.sh` | The macOS/Linux arms. Different layout, not `if` branches — see the headers. |

**What they are: developer deploy.** They assume a checkout, `cargo`, `rustup`,
a Windows box, and a DAW already told about `F:\vst3`. Every one of those is a
thing the machine has *because it builds Organon*.

**What they are not: an installer.** Nothing they produce can be sent to
someone. There is no prerequisite check, no upgrade path over a running copy, no
uninstall, no versioned artifact, and no place for the product to complain on a
machine with no console.

⚠️ **Do not "add installer support" to `bundle.ps1` or `deploy.ps1`.** They are
the inner loop of every native change on Windows — the standing deploy rule in
`CLAUDE.md` points at them — and an installer's concerns (elevation,
prerequisites, uninstall, a wizard) have a different lifetime and a different
audience. An installer *consumes* `target\bundled\Organon.vst3` and the release
binaries. It is a new sibling, and it must not change what those two scripts do
for the person at the keyboard.

⚠️ **"Install Organon" is ambiguous, and choosing is the first real decision.**
This repo builds a VST3/CLAP plugin, `organon-standalone`, `organon-console`
(behind `console-edition`), `organic-math-visual`, the `organon` CLI, and
optionally `organic-math-mind-runtime`. They have different audiences, different
prerequisites and different destinations — a plugin has to land where hosts
scan, a standalone does not. Decide which product you are shipping and say so
out loud before writing anything; an installer that ships "everything" is one
that has not been designed.

---

## The method

Seven stages. They are in this order because each one's failure hides the next.

### 1. Enumerate what the build machine supplies — and check versions, not presence

The failure mode you are hunting is a dependency that is present here and stale
or absent there. **A presence check does not find it.** In the worked example
the offending DLL was on the target machine, every import resolved, and the
process died anyway.

Do all three:

- **Ask the artifact what it imports.** `dumpbin /dependents` on each `.exe` and
  `.dll` you intend to ship, transitively for anything you built yourself.
- **Ask what put each one there.** A DLL that arrived with Visual Studio, the
  Windows SDK, CMake, a GPU driver or CUDA is a dependency on your toolchain
  that the product never declared.
- **Check the version floor, and know why the floor is where it is.** Write the
  reason next to the number, or the next person will "simplify" it to a presence
  check.

⚠️ **Some failures happen before `main()` and can therefore log nothing.** A
loader failure (`0xC0000142`, `STATUS_DLL_INIT_FAILED`) kills the process before
any of your code exists. There is no message, no log line, no window. A
prerequisite check in the installer is the *only* layer that can cover this;
stage 2 covers everything after it, and **neither substitutes for the other**.

For organon specifically, the candidates worth checking are in **"What an
organon installer has to answer"** below. `references/inno-setup.md` has the
worked version-check function, including the two Inno constants that are easy to
get backwards.

### 2. Give the product somewhere to complain

A program that fails on a stranger's machine must be able to say so, and on
Windows this is not free.

- A **console-subsystem** binary gets a console window it never asked for,
  allocated at process start for anything launched from a shortcut or Explorer.
  `FreeConsole` does not help — it closes the window only after it has appeared.
- A **GUI-subsystem** binary has no standard handles at all, so every
  `println!` writes nowhere and every diagnostic subcommand is silent.

The resolution is all three of these, in order:

1. GUI subsystem, so nothing flashes a console at sign-in.
2. `AttachConsole(ATTACH_PARENT_PROCESS)` before anything is printed, so a run
   from a terminal still prints to that terminal.
3. ⚠️ **Reopen `CONOUT$` onto the standard handles.** Skip this and
   `GetStdHandle` keeps returning null after a successful `AttachConsole` — the
   program looks like it has nothing to say.

And three cases, distinguished in this order: **inherited handles** (a pipe or a
redirect — leave them exactly as given, this is how a build script inspects the
artifact), then **a parent console**, then **neither**, which is when a log file
is written.

⚠️ **Gate the subsystem attribute on `not(test)`.** In Rust:
`#![cfg_attr(not(test), windows_subsystem = "windows")]`. The test harness is
built from the same crate root, so an unconditional attribute makes `cargo test`
a GUI binary that passes and fails in identical silence.

⚠️ **The application writes its own log.** A log path populated only by a
launcher script — a `.vbs`, a `.cmd`, a systemd unit — is a log that exists on
exactly one machine, and a "Open log" menu item pointing at it opens nothing
everywhere else.

`references/worked-example.md` carries the full resolution, including why
redirecting the process's standard handles beats adding a logging framework.

### 3. Verify the artifact, never the command that produced it

`cargo build` writes the same path whatever features it was given. Build twice
with different flags and the second silently replaces the first; the installer
then packages whichever ran last, installs cleanly, and is the wrong product.

**This is not hypothetical in organon.** Cargo features unify across every
target of a package, and `EDITION` in `organon-core/src/edition.rs` is a
compile-time `const` from which the IPC namespace is derived (`Full` →
`organic-math`, `Console` → `organon-shell`, both frozen and asserted by test).
So `target\release\organon.exe` built under `--features console-edition` and the
same path built without it are **different products at one path**, and the
difference is invisible until the CLI addresses a namespace nothing is
listening on.

So: build in a fixed order in a script, and then **ask the binary what it is**.

⚠️ **Check both directions.** A test for "not the wrong build" passes happily on
a build with the feature you *wanted* compiled out — which installs, runs, and
does nothing at all on the surface that feature was for. An absent feature
reports itself in no other way, so ask for evidence of its presence too.

The worked example's build script refuses to package on five conditions: mutex
mismatch, version mismatch, the wrong feature present, the wanted feature
absent, and a missing sibling DLL. See `references/build-gates.md`.

### 4. Name every fact that lives in two languages

An installer restates things the product already knows: its version, its
single-instance mutex, where it puts its data, which files a downloaded asset
consists of. Nothing in either language can see the other, so the copies drift —
and the drift is silent in whichever direction is worse.

Two acceptable answers, and no third:

- **Compare them in the build**, and refuse to package a mismatch.
- **Generate one from the other**, and never hand-edit the generated file. The
  worked example generates its Inno model table from a `models.toml` catalogue,
  writes `DO NOT EDIT` at the top of the output, and its generator *throws*
  rather than silently emitting a lossy table.

⚠️ **A generator that drops a key is worse than no generator**, because the
catalogue then looks authoritative while a field evaporates on the way through.
That happened: single-line TOML arrays had no case in the parser, every
`conflicts_with` came out empty, and the installer would have let someone tick
two models the catalogue says cannot coexist.

⚠️ **Divergence is asymmetric, and the harmless-looking direction is the
dangerous one.** In a related case on the same fleet a vocabulary list split
into two copies: the copy used for *spelling* was hand-immunised against
staleness and stayed correct, while the copy used for *hearing* could not be —
so the system went on writing a word correctly while having lost the ability to
recognise it, and no transcript ever showed the difference. Ask which half of a
duplicated fact **cannot** be given a fallback; that is the half that fails
quietly.

### 5. Decide what belongs to the user, and prove the uninstall

Go through everything the install puts on disk and mark each as **yours** or
**theirs**.

- **Theirs** — a vocabulary file, a key, a preset gallery, anything the product
  invites them to edit. Install it `onlyifdoesntexist`, or an upgrade silently
  discards their work. Consider leaving it behind on uninstall and saying so.
- **Yours** — the binaries, the generated tables, the README.
- ⚠️ **Anything downloaded rather than installed is invisible to the
  uninstaller.** It has no `[Files]` entry, so it must be named explicitly in
  `[UninstallDelete]` or gigabytes stay behind that nobody can account for.
- ⚠️ **A log the application writes into its own install tree defeats
  `dirifempty`.** `dirifempty` must come last and fires only if everything above
  it has already gone. Nothing errors; the folder is simply still there.

**The only way this shows up is to uninstall and look.** Do it, and record that
you did.

### 6. Secrets, if the product has any

- Prompt on a wizard page, **optional**, and say on the Ready page which of the
  two states the feature will end up in. "Not configured" is a real answer worth
  naming, so that finding the feature inert later reads as a choice already made
  rather than a fault.
- ⚠️ **Do not mask a field whose destination is a plaintext file the program
  names anyway.** It protects nothing at rest and costs the one thing the field
  is for: seeing that a long pasted key arrived whole. A truncated paste behind
  asterisks becomes a 401 at the point furthest from the cause.
- **Sanitise on the way in** — surrounding quotes, a `Bearer ` prefix. **Warn
  rather than block** on shape: the format belongs to the vendor and may change,
  and an installer that refuses a valid key is worse than one that accepts a bad
  one.
- **Never accept a key as a command-line parameter.** It lands in shell history
  and in process listings.
- ⚠️ **Byte-order marks.** A config file whose parser skips `#` comments will
  take a BOM'd first comment line **as the value**, because it no longer *starts
  with* `#`. Write ANSI from the installer, strip a BOM in the reader, and keep
  any shipped text ASCII if the installer read-modify-writes it. All three —
  none of them makes the others unnecessary, because the file belongs to
  whoever edits it next, and PowerShell 5.1's `Set-Content -Encoding utf8` adds
  a BOM by default.

### 7. Keep a machine that is not the developer's — and write down which is which

This matters more than any single trap in this file.

Get a second Windows machine that has never built the product, and hand the
installer to it. The worked example's first such run failed in forty seconds and
produced every lesson above.

⚠️ **Making it a build host destroys its value for the check it was bought
for.** Installing Visual Studio there put a current Visual C++ runtime in
System32, so the prerequisite check that machine's own failure motivated can no
longer be exercised on it. That is not a mistake to avoid — it is what happens —
but it means **the ledger is the deliverable**, not the machine.

So, in the PR and in whatever doc owns the installer, separate:

- **Verified** — "I ran this, on that machine, and saw this."
- **Reasoned** — "This follows from how Windows works, and has not been run."

Both are legitimate. Presenting the second as the first is not. In organon this
is house practice already: `CLAUDE.md`'s "What can and can't be verified where"
draws the same line, and `MIND_ARCHITECTURE.md` / `CONSOLE_ARCHITECTURE.md` both
carry an honesty ledger for exactly this.

---

## What an organon installer has to answer

Organon-specific, and every one of these is a stage-1 question with a name.

**Prerequisites the build machine supplies for free**

- **The Visual C++ runtime.** Rust's MSVC target links the CRT dynamically by
  default, and anything that links C++ pulls `msvcp140.dll` as well —
  `--features embedded-llm` statically links llama.cpp and is the obvious case.
  Check the **version**, not presence. ⚠️ *Reasoned, not measured: no organon
  binary was inspected when this was written, and the 14.40 floor in
  `references/worked-example.md` is ONNX Runtime's — organon does not use ONNX
  Runtime. Run `dumpbin /dependents` over each shipped binary and derive the
  floor from what organon actually links. Do not copy that number.*
- **A GPU adapter wgpu can use** (`wgpu = "30"`; DX12 or Vulkan on Windows) and
  a driver new enough for it. A machine with no usable adapter fails differently
  from one with a stale driver, and neither is a code bug. Decide what the
  product does when adapter selection fails, and make sure it can say so
  (stage 2).
- **`cmake` is a build dependency only** — `-WithLlm` needs it, an installed
  copy does not.

**Destinations, and the fact that `F:\vst3` is a developer's choice**

- A plugin must land where hosts scan. `deploy.ps1` defaults to `F:\vst3` and
  tells you to add it to the DAW's search path by hand — fine for the person who
  set the box up, useless as an install. The installer has to decide the real
  destination (the common VST3 location, or an asked-for one) and this is a
  design decision, not a constant to copy.
- ⚠️ **The galleries go to `%APPDATA%\OrganicMath\` because `preset.rs` calls
  `dirs::data_dir()`.** Follow that function, never a copied path — and treat a
  gallery a user has edited as **theirs** (stage 5).
- ⚠️ **The visual lives *inside* the VST3 bundle**, at
  `Contents\<arch>-win\organic-math-visual.exe`, and that directory is
  discovered rather than hardcoded because nih-plug names it after the target
  (`x86_64-win`, `arm_64-win`). An installer that flattens the bundle breaks
  "Open Visual Window" while the plugin still loads — a miserable thing to
  debug.
- ⚠️ **CLAP cannot carry the visual on Windows**: nih-plug emits it as a bare
  DLL, so there is no `Contents\` to embed into. If the installer ships the
  CLAP, it has to place the visual somewhere and set `ORGANIC_MATH_VISUAL`, or
  say plainly that the CLAP does not get one.

**Upgrading over a running copy**

- ⚠️ **Windows will not overwrite a loaded DLL or a running executable**, and
  reports it as `Access to the path is denied` — which reads as a permissions
  problem and sends people to ACLs. `deploy.ps1` already detects this for the
  plugin and names the likely host; an installer needs the same answer in its
  own idiom (Inno's `AppMutex` plus `CloseApplications`, and see
  `references/inno-setup.md`).
- ⚠️ **`AppMutex` only works if the product actually creates that mutex**, and
  the name then exists in two languages — stage 4 applies. If no organon binary
  takes a named single-instance mutex today, adding `AppMutex` to an installer
  changes nothing and looks like it works.

**Version, licence, and signing**

- ⚠️ **The version is `version = "0.1.0"` on `organic-math-native` in
  `native/Cargo.toml`.** An installer restates it. Compare or generate — stage 4.
- 🚨 **An installer that ships the plugin, the standalone, the visual or the
  `organon` CLI is distributing GPL-3.0-or-later software.** Those are all
  binaries of the root crate, whose licence is forced by `vst3-sys` through
  `nih_export_vst3!`. Source-availability and same-licence obligations follow
  the binary to whoever you hand it to, and `NOTICE` covers third-party
  material. Read `LICENSING.md`, and **ask the manifests
  (`grep -E '^name|^license' native/*/Cargo.toml`) rather than that file's
  table** — it has fallen behind before. The engine crates are MIT/Apache and
  are not the constraint; the VST3 arm is.
- **There is no codesign step and nothing replaces it.** Ad-hoc signing is a
  macOS concept; Windows either has a real Authenticode certificate or nothing.
  ⚠️ *(Reasoned, not verified here: an unsigned installer downloaded from the
  internet is expected to raise a SmartScreen warning on first run, which a
  stranger reads as "this is malware". If you ship one, either test that on the
  second machine and record what you saw, or say plainly that you have not.)*

---

## The verification bar

**On this machine, before anything else:** the installer must *compile* and the
build script must *refuse* correctly. Break each gate on purpose once and check
that it fires — a gate that has never been seen to fail is an assertion, not a
check. (The worked example's own conflict-detection gate was written, was
silently a no-op, and looked exactly like a working one.)

**On a machine that has never built organon:** install, run, upgrade over the
running copy, uninstall, and look at the directory afterwards. Report what you
*saw*.

⚠️ **Do not pass `/SWITCH` arguments through Git Bash.** MSYS rewrites any
argument beginning with `/` into a Windows path, so
`setup.exe /VERYSILENT /SUPPRESSMSGBOXES` arrives as nonexistent paths, the
installer ignores them and opens its wizard, and it reads as a **hang**. It is a
dialog behind whatever you are looking at. Use `MSYS_NO_PATHCONV=1` or
PowerShell; the diagnostic is `Get-Process | Where MainWindowTitle -ne ''`,
which names it immediately.

⚠️ **A silent install skips wizard pages**, so `NextButtonClick` never fires for
it and any `[Run]` entry needs `skipifsilent`. If you have logic on a wizard
page, a silent install does not run it — decide deliberately what a silent
install is allowed to do.

More PowerShell 5.1 and encoding traps, all of which produce confident wrong
diagnoses, are in `references/windows-shell.md`.

---

## Recording it in this repo

- **`CLAUDE.md` owns the build/install workflow** and currently documents
  `bundle.ps1` / `deploy.ps1` as the Windows arm. An installer changes what that
  section is true about, so it moves in the same change.
- **A `changelog.d/` fragment**, `YYYY-MM-DD-<branch-slug>.md`, in the same
  commit. Never write into `CHANGELOG.md`.
- ⚠️ **Add any new `.ps1` to the CI encoding gate.** `.github/workflows/ci.yml`,
  step *"Validate the PowerShell deploy scripts"*, iterates a **hardcoded list**
  — `@('bundle.ps1', 'deploy.ps1')` with `working-directory: native`. A new
  script outside that list is unchecked, and the failure it would have caught is
  a file that parses fine in CI's `pwsh` and cannot be parsed at all by the
  PowerShell 5.1 that ships in the box. That gate exists because it was green
  for four months while `deploy.ps1` could not run.
- ⚠️ **Skills here are ordinary tracked files, never git symlinks.** A symlink
  under `.claude/skills/` materialises as a 24-byte text file on a Windows
  checkout and the skill silently does not load (organon #19, #27).

---

## References

| File | What is in it |
|---|---|
| `references/worked-example.md` | Organon Voice — the installer this method came from. The `0xC0000142` failure in full, the console/logging resolution, and the ledger of what could and could not be tested. Self-contained; you do not need the other repository. |
| `references/inno-setup.md` | Inno Setup mechanics with worked fragments: the version-gated prerequisite, elevation, the download page, upgrade and uninstall ordering, an optional-secret wizard page, and the Pascal comment trap that compiles half a sentence as code. |
| `references/build-gates.md` | The build script that refuses to package a wrong artifact — five refusals, each with the failure it prevents, in PowerShell 5.1 that runs in the box. |
| `references/windows-shell.md` | PowerShell 5.1, Git Bash and encoding: the traps that make a correct diagnosis look wrong. |
