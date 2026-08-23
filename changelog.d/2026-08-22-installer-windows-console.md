### A Windows installer for Organon Console

`native/installer/` — `build.ps1` produces and gates the artifact, `organon.iss` packages
it. Output is `native/target/installer/organon-console-<version>-x64-setup.exe`, a
**per-user install needing no admin**, which is only possible because it ships the
Console alone: no plugin, no visual, no CLI, no LLM runtime. A plugin would have to land
where hosts scan, and that is the thing that would force elevation.

It is a **sibling** of `bundle.ps1` / `deploy.ps1`, never an extension of them. Those are
developer deploy — they assume a checkout, `cargo`, and a machine configured by the act
of having built Organon, which is precisely what a stranger's machine is not.

**The prerequisite check is derived, not copied.** `doc/shipping-windows.md` measured the
floor at Visual C++ 14.0, so that is what the installer checks, with the reason recorded
beside the number. A missing C runtime kills the process *before* `main()` — no window,
no log line, nothing to read — which is why it is checked at install time rather than
left to the first launch.

⚠️ **`AppMutex` is deliberately absent.** It is the standard answer to Windows refusing
to overwrite a running executable, and it works only if the product creates that mutex.
`organon-console` creates none, so an `AppMutex` line would do nothing while looking
exactly like it worked. `CloseApplications` uses Restart Manager, which needs no
cooperation from the application.

⚠️ **The galleries install `onlyifdoesntexist` and survive uninstall, which deliberately
disagrees with `deploy.ps1`** — that copies them with `-Force` and so silently replaces a
preset edited under a shipped filename. Overwriting is right for a developer who wants
the repo's copy back and wrong for someone whose own work is in that file. Verified:
installing over an existing store left all 21 files untouched.

📌 **Four of the five build gates were broken on purpose and seen to fire**, because a
gate that has never failed is an assertion rather than a check — and breaking one found a
real defect instead of confirming one. `whoami --version` writes to stderr, and Windows
PowerShell 5.1 with `$ErrorActionPreference = 'Stop'` turns native stderr into a
*terminating* error, so the script died at the call site before any gate could speak; it
read as a bug in the build script rather than as a bad artifact, which is the exact
inversion the gates exist to prevent. The fifth refusal — Inno Setup not installed —
has never been observed, and the README says so.

🚨 **None of this has happened on a machine that did not build Organon.** The install,
the upgrade path and the uninstall were exercised here, where the prerequisite the
installer checks for is present *because* Visual Studio is. There is also no code-signing
certificate, so the SmartScreen warning an unsigned download raises is reasoned and
unobserved.
