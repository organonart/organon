# installer — Organon Console for a machine that never built it

Two files. `build.ps1` produces and **gates** the artifact; `organon.iss` packages it.

```powershell
.\build.ps1                 # build, gate, package
.\build.ps1 -SkipBuild      # package what is already on disk (iterating on the .iss)
```

Output lands in `native\target\installer\organon-console-<version>-x64-setup.exe`.

## What this is not

**It is not `..\deploy.ps1`, and it must not grow into it.** That script is the inner
loop of every native change on Windows: it assumes a checkout, `cargo`, and a box
configured by the act of having built Organon. An installer's audience has none of
those, and its concerns — prerequisites, elevation, upgrade over a running copy,
uninstall — have a different lifetime. This consumes `deploy.ps1`'s world; it does not
extend it.

## What ships

| | |
|---|---|
| `organon-console.exe` | the product — `--features console-edition` |
| `LICENSE-GPL`, `NOTICE` | GPL-3.0-or-later travels **with** the binary, not as a link |
| `VERSION.txt` | generated — version, commit, build time |
| the five asset galleries | to `%APPDATA%\OrganicMath\`, **theirs** (see below) |
| an empty `models\` folder | where you put a `.gguf` |

No plugin, no visual, no CLI, no LLM runtime. Per-user install, no admin — which is
only possible *because* no plugin ships, since a plugin has to land where hosts scan.

## The three things most likely to be "tidied" wrongly

**1. `AppMutex` is absent on purpose.** It is the standard answer to Windows refusing
to overwrite a running executable, and it only works if the product actually creates
that mutex. `organon-console` creates none — so an `AppMutex` line would do nothing
while looking exactly like it worked. `CloseApplications` uses Restart Manager, which
needs no cooperation from the application.

**2. The galleries are `onlyifdoesntexist uninsneveruninstall`, and that deliberately
disagrees with `deploy.ps1`,** which copies them with `-Force`. Overwriting is right
for a developer who wants the repo's copy back and wrong for someone whose own work is
in a file with a shipped name. Verified: installing over an existing
`%APPDATA%\OrganicMath` left all 21 files untouched.

**3. The version is not restated anywhere.** `build.ps1` reads `[package] version`
from `Cargo.toml`, asks the built binary what version it thinks it is, refuses on a
mismatch, and passes the agreed value to `ISCC` as `-DAppVersion`. The `.iss` `#error`s
if that define is missing rather than defaulting, because a version this script invents
would be a second source of truth.

## The gates, and the one that has never been seen to fire

`build.ps1` refuses on five conditions. Four were broken on purpose and observed to
fire (2026-08-22):

| Gate | Provoked with | Fires |
|---|---|---|
| no artifact | deleted the exe | ✅ |
| `--version` exits non-zero | swapped in `whoami.exe` | ✅ |
| `--version` runs but is not ours | swapped in `tar.exe` | ✅ |
| version mismatch | set `Cargo.toml` to `0.9.9` | ✅ |
| Inno Setup not installed | — | ⚠️ **never observed** |

⚠️ Breaking gate 2 found a real defect rather than confirming one: `whoami --version`
writes to stderr, and Windows PowerShell 5.1 with `$ErrorActionPreference = 'Stop'`
turns native stderr into a **terminating** error — so the script died at the call site
with a `NativeCommandError` before any gate could speak. It read like a bug in the
build script rather than a bad artifact, which is the exact inversion the gates exist
to prevent. Fixed by dropping to `Continue` around that one call.

## Encoding

Both files are **pure ASCII**. `.ps1` must be, or carry a UTF-8 BOM: Windows
PowerShell 5.1 reads a BOM-less script as CP1252, and one stray `→` becomes a string
delimiter that makes the parser report a missing brace 200 lines away. CI checks this
file **by name** — `build.ps1` is in the hardcoded list in the *"Validate the
PowerShell deploy scripts"* step. **A new `.ps1` here is unchecked until it is added
to that list.**

## What has been verified, and what has not

**Verified on organon-one, 2026-08-22** — the machine that built it:

- compiles; installs silently to a scratch directory; exit 0
- the install tree holds exactly the five expected files plus Inno's uninstaller
- `%APPDATA%\OrganicMath\models` is created
- an existing gallery is not overwritten
- uninstall exits 0, the install directory is **removed** (so `dirifempty` is not
  defeated — the application logs to `%LOCALAPPDATA%\organon\console\console.log`,
  outside the tree), and galleries, models and `OrganonShell` are left behind
- no uninstall entry remains

🚨 **Not verified, and the gap that matters most:** none of this has happened on a
machine that did not build Organon. The prerequisite check exists precisely for the
case this machine cannot exercise — it has the Visual C++ runtime because it has
Visual Studio. The GPU path is likewise unexercised: a machine with no usable adapter
fails *after* `main()`, which is the product's job to report, not the installer's.

⚠️ **There is no code-signing certificate.** An unsigned installer downloaded from the
internet is expected to raise a SmartScreen warning that a stranger reads as "this is
malware". Nobody has observed it. Reasoned, not run.

⚠️ **`organon-console.exe` is a console-subsystem binary**, so a Start-menu shortcut
will also open a console window it never asked for. `[Icons]` ships one anyway because
people need a way to launch it; `doc\shipping-windows.md` carries the resolution, which
is a three-part change to the application and not a switch here.

⚠️ **Inno Setup 6.7.3 prints "Non-commercial use only"** when compiling. That is the
compiler's own banner, recorded here as observed output rather than as a reading of its
licence — worth settling before Organon is distributed commercially.
