# The build script that refuses to package a wrong artifact

An installer build has one job beyond compiling: **make it impossible to ship
the wrong thing quietly.** Every gate below exists because the failure it
prevents is silent — the installer compiles, installs, and is wrong.

The worked example's `build.ps1` refuses on five conditions. Its header states
the reason it exists at all:

> `cargo build` writes to the same path whatever features it was given, so
> building the full tray after the dictation build silently replaces one with
> the other — and the installer then packages whichever ran last. That happened
> here: the packaged binary was the agent build, it installed cleanly, and the
> fault only appeared because a diagnostic flag that exists in one configuration
> and not the other was missing.
>
> **So the build order is not left to whoever is at the keyboard.**

---

## Gate 1 — two names for one mutex

The installer's `AppMutex` must be the product's actual single-instance mutex.
Nothing in Pascal can see Rust and nothing in Rust can see Pascal, so the build
compares them.

```powershell
$rustSrc = Get-Content (Join-Path $crate 'src\main.rs') -Raw
$issSrc  = Get-Content (Join-Path $here 'organon-voice.iss') -Raw
if ($rustSrc -notmatch 'SINGLE_INSTANCE_MUTEX:\s*&str\s*=\s*"([^"]+)"') {
    throw "could not find SINGLE_INSTANCE_MUTEX in main.rs -- did it get renamed?"
}
$rustMutex = $Matches[1]
if ($issSrc -notmatch '(?m)^AppMutex=(.+)$') {
    throw "organon-voice.iss has no AppMutex. Setup will not notice a running tray."
}
if ($rustMutex -ne $Matches[1].Trim()) {
    throw "mutex mismatch: main.rs says '$rustMutex', the .iss says '$($Matches[1].Trim())'."
}
```

⚠️ **Note the two "not found" throws.** A regex that silently matches nothing is
how a comparison gate becomes a no-op — it must fail on *absence* as loudly as
on *disagreement*.

The Rust side carries the other half of the note, so whoever renames the
constant is told where else to look:

```rust
/// ⚠️ **Also written out in `installer/organon-voice.iss` as `AppMutex`** …
/// Two copies of one fact in two languages, and nothing in either language can
/// see the other — so `installer/build.ps1` compares them and refuses to
/// package a mismatch. Change this and the build will tell you what else to
/// change.
```

## Gate 2 — two versions

Same class. The product stamps its log with its own crate version and the
uninstall entry shows the installer's `AppVersion`; drift puts **one number in
Add/Remove Programs and a different one in the file you read to diagnose it.**

```powershell
if ($cargoSrc -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw "no version in Cargo.toml" }
$crateVersion = $Matches[1]
if ($issSrc -notmatch '#define\s+AppVersion\s+"([^"]+)"') { throw "no AppVersion in the .iss" }
if ($crateVersion -ne $Matches[1]) { throw "version mismatch: $crateVersion vs $($Matches[1])." }
```

**In organon** the source of truth is `version = "0.1.0"` on `organic-math-native`
in `native/Cargo.toml`.

## Gates 3 and 4 — ask the artifact, in both directions

```powershell
# Verify the artifact rather than trusting the command that just ran.
$exe = Join-Path $release 'voice-tray.exe'
if (-not (Test-Path $exe)) { throw "no binary at $exe" }

# ⚠️ Captured to a FILE rather than piped. Redirecting a native executable's
# stderr in PowerShell 5.1 wraps each line in an ErrorRecord and trips
# $ErrorActionPreference, so the check would fail on the program printing its
# own usage -- an error about an error.
$tmp = [IO.Path]::GetTempFileName()
Start-Process -FilePath $exe -NoNewWindow -Wait `
              -RedirectStandardError $tmp -RedirectStandardOutput "$tmp.out"
$help = (Get-Content $tmp -Raw) + (Get-Content "$tmp.out" -Raw)
Remove-Item $tmp, "$tmp.out" -ErrorAction SilentlyContinue

# 3. Not the wrong build.
if ($help -match 'Vera answers out loud') {
    throw "this is the FULL build, not the dictation build. Something rebuilt it with default features."
}

# 4. And the other direction. The check above can only prove what the binary is
# NOT, so on its own it passes happily on a build with the feature you wanted
# compiled out -- which installs, runs, and silently does nothing on the surface
# that feature was for. An absent feature reports itself in no other way.
if ($help -notmatch 'the question goes to the web') {
    throw "this binary has no search. Expected --features native-corrector,search."
}
```

**This only works if the artifact can be asked.** That is what stage 2 of the
method buys: a GUI-subsystem binary with **inherited handles left untouched** is
exactly the case that lets a build script read its usage text. Get that wrong
and the gate reads an empty string and passes.

**In organon**, the fact worth asking about is the edition. `EDITION` in
`organon-core/src/edition.rs` is a compile-time `const` and the IPC namespace
derives from it (`Full` → `organic-math`, `Console` → `organon-shell`); cargo
features unify across a package's targets, so `target\release\organon.exe` built
under `--features console-edition` is a **different product at the same path**.
If a binary you ship does not already print something that distinguishes its
edition, adding that is part of making it shippable.

## Gate 5 — the siblings are present

```powershell
foreach ($dll in 'moonshine.dll','onnxruntime.dll','DirectML.dll') {
    if (-not (Test-Path (Join-Path $release $dll))) {
        throw "$dll is missing from $release. It must sit beside the executable."
    }
}
```

A missing sibling DLL does not fail the compile and does not fail the install.
It fails on the target machine, at load, on a machine you are not standing at.

---

## Two things the script does that are not gates

**Regenerate the generated table first**, so a stale one cannot be packaged:

```powershell
& powershell -ExecutionPolicy Bypass -File (Join-Path $here 'gen-models-iss.ps1')
if ($LASTEXITCODE -ne 0) { throw "model table generation failed" }
```

The output is committed so a build machine needs no extra step — and **CI should
fail if the committed copy differs from a fresh run**, which is the check that
keeps "committed generated file" from meaning "hand-edited file".

**Build in a fixed order**, with the features written down in the script rather
than in someone's shell history.

---

## Testing the gates

🚨 **A gate that has never been seen to fail is an assertion, not a check.**
Break each one on purpose once — change a character in the mutex name, bump one
version, build with the wrong features, move a DLL aside — and confirm the build
refuses. In the same repository, a model-conflict check was written, was
silently a no-op because the generator dropped the field it read, and looked
exactly like a working one.

---

## PowerShell 5.1 notes that apply to this script

- `$ErrorActionPreference = 'Stop'` governs **cmdlets, not native exit codes**.
  Check `$LASTEXITCODE` after every external call, or the script sails past a
  failed `cargo build` and packages a stale artifact. (organon's `bundle.ps1`
  routes every external call through an `Invoke-Checked` helper for exactly
  this.)
- ⚠️ **Never pipe a native executable's stderr with `2>&1`** — see gate 3.
- ⚠️ **`.ps1` files must be pure ASCII or carry a UTF-8 BOM.** Windows
  PowerShell 5.1 reads a BOM-less `.ps1` as CP1252, and one em-dash or arrow in
  a comment becomes a stray string delimiter. Full mechanism in
  `references/windows-shell.md`; organon has a CI gate for it that a new script
  must be **added to by name**.
