# PowerShell 5.1, Git Bash, and encodings

Every trap here produces a **confident wrong diagnosis**. That is what they have
in common and why they are collected: none of them looks like an encoding or a
quoting problem from the outside.

---

## Git Bash eats `/SWITCH` arguments, and the symptom is a hang

🚨 **MSYS path conversion rewrites any argument beginning with `/` into a
Windows path.** So:

```bash
./organon-voice-setup.exe /VERYSILENT /SUPPRESSMSGBOXES
```

arrives as two nonexistent paths. The installer ignores them, **opens its
wizard**, and sits waiting for a click nobody is going to give it. It reads as a
hung installer. It is a full-screen dialog behind whatever you are looking at.

**Fix:** `MSYS_NO_PATHCONV=1`, or launch it from PowerShell.

**The diagnostic that settles it in one step:**

```powershell
Get-Process | Where-Object MainWindowTitle -ne '' | Select-Object Name, MainWindowTitle
```

A "hang" with a window is a dialog.

---

## A BOM-less `.ps1` is read as CP1252 by the PowerShell that ships in the box

🚨 **Windows PowerShell 5.1 reads a `.ps1` without a BOM as ANSI/CP1252**, and
5.1 is the only PowerShell on a stock Windows 11 — `pwsh` (7) is a separate
install and defaults to UTF-8.

organon hit this exactly. From `changelog.d/2026-08-20-ps1-bom-windows-deploy.md`:

> `→` (U+2192) is UTF-8 `E2 86 92`; read as CP1252 the three bytes become `â†’`,
> and the last of them is **U+2019, a right single quotation mark** — which
> PowerShell honours as a string delimiter. Ten arrows inside a comment block are
> ten stray delimiters. The parser ran off the end of a 377-line file hunting a
> terminator and reported *"Missing closing `}`"* at `function Add-UserPathEntry`,
> roughly 200 lines from anything actually wrong.

Measured on a stock Windows 11 workstation: `deploy.ps1` produced **8 parse
errors**, `bundle.ps1` **2** — neither would run at all. The identical bytes with
a BOM prepended parse with zero errors. **The content was never wrong, only its
label.**

⚠️ **And the CI gate that existed for this was green the whole time**, because
it ran under `pwsh`, which reads the file correctly. A clean AST parse in CI and
an unrunnable file on the target are perfectly consistent — *they are two
different readers*.

**The rule is about bytes, not syntax: a `.ps1` must be pure ASCII, or carry a
UTF-8 BOM.** organon picks the BOM, which keeps the house typography. The gate
is in `.github/workflows/ci.yml`, step *"Validate the PowerShell deploy
scripts"*, and it iterates a **hardcoded list** — `@('bundle.ps1','deploy.ps1')`
with `working-directory: native`. ⚠️ **Add any new `.ps1` to that list**, or it
is unchecked.

📌 This is about **encoding**, not line endings. `.gitattributes` carries a
separate, still-correct note on why `*.ps1` is not pinned to CRLF. Two settled
questions, not one.

---

## `Set-Content -Encoding utf8` writes a BOM, and `Get-Content` reads ANSI

Two halves of the same defect, in opposite directions.

- **Writing.** PS 5.1's `utf8` means **UTF-8 with a BOM**. For anything a
  non-Windows parser reads — JSON, TOML, a config file with `#` comments — that
  BOM is a silent poison: `serde_json` rejects it outright, and a parser that
  skips `#` comments takes the BOM'd first comment line as a **value**, because
  it no longer starts with `#`. Use `[IO.File]::WriteAllText(path, text)` for a
  BOM-less write, or `Copy-Item` a byte-exact file.
- **Reading.** `Get-Content` defaults to ANSI and renders a UTF-8 em-dash as
  `â€"`. ⚠️ **The file is usually fine — check the bytes before believing in
  corruption.** Pass `-Encoding UTF8`, or read it with something else.

When a script *reads* a UTF-8 source to generate something, `-Encoding UTF8` on
the read is load-bearing: without it every non-ASCII character arrives mojibaked
and is written straight into the generated output.

---

## `$ErrorActionPreference` does not cover native exit codes

`$ErrorActionPreference = 'Stop'` governs **cmdlets**. A native program exiting
non-zero does not stop the script. Without an explicit check the script sails
past a failed `cargo build` and packages a stale artifact that looks fresh.

organon's `bundle.ps1` routes every external call through one helper for this
reason; copy that shape rather than remembering to check each time.

---

## Redirecting a native executable's stderr fabricates errors

In PS 5.1, `native.exe 2>&1` wraps each stderr line in an `ErrorRecord` and sets
`$?` to `$false` **even when the program returned exit code 0**. A check that
reads a program's own usage text will fail on the program printing it — an error
about an error.

**Capture to a file instead:**

```powershell
$tmp = [IO.Path]::GetTempFileName()
Start-Process -FilePath $exe -NoNewWindow -Wait `
              -RedirectStandardError $tmp -RedirectStandardOutput "$tmp.out"
$text = (Get-Content $tmp -Raw) + (Get-Content "$tmp.out" -Raw)
```

---

## PowerShell 5.1 is not PowerShell 7

Assume 5.1 for anything a user or a fresh machine runs.

- `&&` and `||` are **parse errors**. Use `cmd; if ($?) { next }`.
- No ternary, no `??`, no `?.`.
- `ConvertFrom-Json` returns a `PSCustomObject`; there is no `-AsHashtable`.
- `$IsWindows` does not exist (it is a Core automatic variable), and under
  `Set-StrictMode` reading it is a **hard error**. organon's `bundle.ps1`
  handles this precisely: `if (Test-Path Variable:\IsWindows) { $IsWindows }
  else { $true }` — 5.1 only ever runs on Windows, so its absence *is* the
  answer.

---

## Execution policy differs between an agent's shell and the user's

⚠️ An automation shell frequently runs with `Process: Bypass` injected while the
user's own PowerShell is at the Windows client default of `Restricted`, which
refuses to load **any** `.ps1`. So a script can work perfectly for whoever wrote
it and fail for the person it was written for, invisibly.

Consequences worth designing around:

- PowerShell resolves a command to a `.ps1` (ExternalScript) **before** a `.cmd`
  (Application), so a bare command name stays blocked under `Restricted` even
  when both files are present. A `.cmd` shim sidesteps policy entirely.
- A launcher shim that invokes
  `powershell.exe -ExecutionPolicy Bypass -File …` is immune. That is worth
  doing for anything you hand to someone.
- If you are asking whether something landed on the user's filesystem, **have
  them check it** — `Test-Path` from your own shell is not evidence about
  theirs.

---

## PATH changes and the stale-environment trap

⚠️ "New terminal windows see a PATH change, already-open ones do not" is
**wrong**, and it costs whole debugging sessions.

Every process inherits its environment from its parent, and on a desktop the
parent of anything launched from the taskbar or Start menu is **`explorer.exe`**
— which may be hours old and hands each child its own stale snapshot. Windows
Terminal compounds it: one process serves all its windows and tabs, so "open a
new window" does not even get you a new process while one is alive.

**The diagnostic that settles it in one step:** compare
`(Get-Process explorer).StartTime` against the timestamp of the file you just
installed. **The fix:** `Stop-Process -Name explorer -Force` (Windows relaunches
it), then open the terminal from the restarted Explorer — or sign out and back
in.

**The durable answer for a launcher: resolve the tool's absolute path rather
than trusting PATH at all**, and if PATH lookup fails, say the environment is
stale rather than claiming the thing is not installed. An error message that
names the wrong cause costs more than no message.
