# Inno Setup: the parts that are decisions

Inno Setup 6 is the tool the worked example used: one `.iss` script, one
`ISCC.exe` compile, one self-contained `setup.exe`, no runtime dependency on the
target. It is a reasonable default for a Windows desktop product that is not
going through the Store.

What follows is not a tutorial — the official help covers syntax. These are the
places where the obvious choice is wrong, with the fragment that gets it right.
Every fragment is adapted from a shipped installer; **read the reason, do not
copy the constants.**

---

## The trap that makes a comment into code

🚨 **A Pascal `{ }` comment containing a brace-delimited constant is closed
early by that constant, and the rest of the sentence compiles as code.**

```pascal
{ This reads the DLL from {sys}, not {sysnative}, on purpose. }
                        { ^ the comment ENDS here. The rest is code. }
```

The failure is a compile error somewhere else, or — worse — a fragment of prose
that happens to parse. **Use `//` line comments** for anything that mentions a
constant. Both files in the worked example say so at the point of use, twice,
because it was met twice.

---

## Per-user by default, and the one exception

```ini
[Setup]
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Organon\voice
DisableDirPage=yes
DisableProgramGroupPage=yes
```

Nothing in Program Files, nothing in HKLM, no service. The whole install is a
directory or two under the user's own profile and one shortcut — which is also
what makes the uninstall **complete rather than approximate**.

⚠️ **A per-user installer that raises UAC without warning reads as a betrayal.**
If one step genuinely needs machine-wide privilege (a Microsoft
redistributable), say so on the Ready page *before* it happens — see
`UpdateReadyMemo` below.

### The 32-bit / 64-bit decision, which is not the obvious one

```ini
ArchitecturesAllowed=x64compatible
; ⚠️ Note what is deliberately NOT set: ArchitecturesInstallIn64BitMode. Setup
; itself stays 32-bit, so its registry view — and therefore the uninstall entry
; it already wrote on machines that have this — does not move out from under an
; upgrade. The consequence is that {sys} means SysWOW64 here.
```

Refusing a 32-bit machine **here** is better than discovering it later: an x64
product will install happily and never run.

---

## A prerequisite that is a version, not a presence

The whole point of stage 1. Note the constant, and note that the comment is
`//`-style precisely because it names two constants.

```pascal
// The constant used is {sysnative}, not {sys}. Setup is 32-bit, so {sys} is
// redirected to SysWOW64 and would measure the 32-bit runtime -- a different
// file, commonly a different version, and never the one this x64 program loads.
// A machine with a current x86 runtime and a stale x64 one is ordinary, and
// reading the wrong one would report health on precisely the configuration
// that fails.
function VCRuntimeIsOldEnoughToCrash(): Boolean;
var
  Major, Minor, MS, LS: Cardinal;
begin
  if not GetVersionNumbers(ExpandConstant('{sysnative}\msvcp140.dll'), MS, LS) then
  begin
    // Absent, or unreadable. Either way it cannot be shown to be new enough.
    Result := True;
    Exit;
  end;
  Major := MS shr 16;
  Minor := MS and $FFFF;
  Result := (Major < 14) or ((Major = 14) and (Minor < 40));
end;
```

⚠️ **`14.40` is ONNX Runtime 1.23's floor, not a universal one.** Measure the
floor for what *your* product links, and write the reason beside the number.

### Installing it: `ShellExec`, and three success codes

```pascal
procedure InstallVCRedist();
var
  ResultCode: Integer;
begin
  // ShellExec rather than Exec. The package is manifested requireAdministrator,
  // and Exec uses CreateProcess, which does not elevate -- it fails with
  // "elevation required" and would look like a broken download.
  if not ShellExec('', ExpandConstant('{tmp}\' + VCRedistFile),
                   '/install /passive /norestart', '',
                   SW_SHOW, ewWaitUntilTerminated, ResultCode) then
  begin
    // Declining the elevation prompt lands here. Naming the CONSEQUENCE matters
    // more than naming the error: without this the program installs and then
    // will not start, with no message anywhere.
    MsgBox('The Visual C++ runtime was not installed.' + #13#10 + #13#10 +
           'It will install, but will not start without it - it exits' + #13#10 +
           'immediately and silently. Install it from:' + #13#10 + #13#10 +
           VCRedistUrl, mbError, MB_OK);
    Exit;
  end;
  // 0 installed, 1638 a newer one is already present, 3010 installed and wants
  // a reboot. All three are success. 3010 in particular is NOT a failure and
  // must not be reported as one -- the DLLs land on disk immediately.
  if (ResultCode <> 0) and (ResultCode <> 1638) and (ResultCode <> 3010) then
    MsgBox('The runtime installer reported error ' + IntToStr(ResultCode) + '.',
           mbError, MB_OK);
end;
```

Microsoft's evergreen link is `https://aka.ms/vs/17/release/vc_redist.x64.exe`.

### Announcing it on the Ready page

```pascal
function UpdateReadyMemo(Space, NewLine, MemoUserInfoInfo, MemoDirInfo,
  MemoTypeInfo, MemoComponentsInfo, MemoGroupInfo, MemoTasksInfo: String): String;
begin
  Result := MemoDirInfo + NewLine + NewLine + MemoTasksInfo;
  if NeedVCRedist then
    Result := Result + NewLine + NewLine +
      'Prerequisite:' + NewLine +
      Space + 'Microsoft Visual C++ runtime (download, then install)' + NewLine +
      Space + 'The installed one is too old to run this program. Windows will' + NewLine +
      Space + 'ask for permission, because that part is machine-wide.';
end;
```

⚠️ **Decide once, at `InitializeWizard`, and store it** (`NeedVCRedist`). Asking
the question again at each place that needs the answer is how two parts of one
installer come to disagree.

---

## Downloading, and moving out of `{tmp}`

`CreateDownloadPage` + `DownloadPage.Add(url, filename, '')` per file, `Show`,
`Download` inside a `try…except`, `Hide` in the `finally`.

**Order matters.** Put the prerequisite **first in the queue**: it is small, and
it is the one download whose absence stops the program from starting at all.

⚠️ **Name the right casualty in the failure message.** The original said "the
models" while the runtime rode in the same queue:

> a message naming the wrong casualty is worse than a vague one: someone told
> their models failed will go looking at model paths, when what actually did not
> arrive is the thing without which nothing starts.

⚠️ **"We asked for it" and "it arrived" are different questions.** The download
block catches its own exception and carries on, so guard the install step with
`FileExists` as well as the flag — otherwise a missing file produces a reported
failure of a step that never ran.

```pascal
if NeedVCRedist and FileExists(ExpandConstant('{tmp}\' + VCRedistFile)) then
  InstallVCRedist();
```

**Move, do not copy, out of `{tmp}`:**

```pascal
// Same volume as the destination, so a rename is instant and costs no extra
// disk -- where a copy of a 2.4 GB asset would need headroom a nearly-full
// drive may not have.
DeleteFile(Dst);
if not RenameFile(Src, Dst) then
  // Falls back to a copy: TEMP is not guaranteed to be on the same volume.
  CopyFile(Src, Dst, False);
```

---

## Upgrading over a running copy

```ini
[Setup]
AppId={{8C4E1F2A-9D3B-4A6E-B1C7-5F2E8A0D4B93}
AppMutex=organon-voice-tray-single-instance
CloseApplications=yes
RestartApplications=no
```

- The **`AppId`** is what makes a re-run an upgrade rather than a second
  install. It never changes.
- **`AppMutex`** is the product's own single-instance mutex name. Without it,
  installing while the program runs fails to replace the executable — Windows
  will not overwrite a running image — and ⚠️ **the failure is a
  permissions-shaped error naming a file, not a running program**, which sends
  you to ACLs instead of to the tray icon.
- 🚨 **The name lives in two languages and nothing in either can see the
  other.** Compare them in the build (see `references/build-gates.md`), or
  changing one silently stops the installer noticing a running instance.
- **`RestartApplications=no`** because the `[Run]` entry at the end already
  offers to start it, and two of them race.

⚠️ **`AppMutex` only works if the product actually creates that mutex.** Adding
the line to an installer whose product does not is a no-op that looks like a
feature.

---

## What belongs to the user

```ini
[Files]
Source: "{#SrcDir}\app.exe";  DestDir: "{app}"; Flags: ignoreversion
; Both DLLs must sit BESIDE the executable rather than anywhere on PATH.
; Windows ships its own onnxruntime.dll in System32 and the executable's own
; directory is the only one searched ahead of it -- put ours on PATH instead
; and the OS copy wins silently, warns about an unsupported API version, and
; then fails later at model load.
Source: "{#SrcDir}\onnxruntime.dll"; DestDir: "{app}"; Flags: ignoreversion

; The starter vocabulary. `onlyifdoesntexist` because this is the USER's file
; once installed -- an upgrade that overwrote it would silently discard whatever
; they had added, which is the one edit this program invites them to make.
Source: "bias-terms.txt"; DestDir: "{app}"; Flags: onlyifdoesntexist
```

⚠️ **A sibling DLL is a placement decision, not a convenience.** The executable's
own directory is searched ahead of System32; PATH is not.

```ini
[UninstallDelete]
; Downloaded rather than installed, so Inno does not know about them.
Type: filesandordirs; Name: "{localappdata}\Organon\models"
; ⚠️ The program writes its own log INSIDE this tree -- so without this line it
; outlives the uninstall and keeps the parent non-empty, which silently defeats
; the dirifempty below. Found by uninstalling and looking: nothing errors, the
; folder is simply still there.
Type: filesandordirs; Name: "{localappdata}\Organon\voice-tray"
; Last, and it must stay last: dirifempty only fires if everything above has
; already gone.
Type: dirifempty;     Name: "{localappdata}\Organon"
```

To keep something deliberately — a key the user fetched — install it with
`uninsneveruninstall` and **say so in the README**, or it reads as a leak.

---

## An optional secret on its own page

```pascal
// Placed AFTER the required pages, so the wizard runs required-then-optional.
//
// ⚠️ The field is NOT a password field. The key's destination is a plain text
// file whose path is printed in the README and named by the program itself, so
// masking would protect nothing at rest -- while costing the one thing the
// field is for: seeing that a long pasted key arrived whole. A truncated paste
// behind asterisks produces a 401 later, at the point furthest from the cause.
KeyPage := CreateInputQueryPage(ModelPage.ID,
  'Web search', 'Optional - you can skip this and add it later.',
  'Leave this blank and everything else still works.');
KeyPage.Add('API key:', False);   // False = not masked
```

**Sanitise on the way in, warn on shape:**

```pascal
function TidyKey(S: String): String;
begin
  Result := Trim(S);
  // Copied from a curl example or the API docs.
  if Copy(Result, 1, 7) = 'Bearer ' then Result := Trim(Copy(Result, 8, Length(Result)));
  // Copied from a JSON or shell snippet, either quote style.
  if (Length(Result) >= 2) and (Copy(Result, 1, 1) = '"')
     and (Copy(Result, Length(Result), 1) = '"') then
    Result := Copy(Result, 2, Length(Result) - 2);
  Result := Trim(Result);
end;
```

> anything decorative here becomes part of the Authorization header and comes
> back as a 401 — an error that names the key as the problem and gives no hint
> that the problem is a pair of quotes. Cheaper to remove them than to explain
> them.

A key containing a space gets a **confirmation, not a refusal**: the shape
belongs to the vendor and may change, and *"an installer that refuses a valid
key because it stopped matching a guess is worse than one that accepts a bad
one"* — the bad one produces a message naming the file to fix.

**Writing it:**

```pascal
// Keeps every comment line, drops every existing key line, appends the new one.
// So it is the same operation whether it is the first key or a replacement, and
// it cannot leave two keys in the file where the older would silently win by
// being first.
//
// ⚠️ SaveStringsToFile writes ANSI, and that is deliberate. The UTF-8 variant
// writes a byte-order mark, and a BOM lands *before* the first '#' -- so the
// header line stops being a comment as far as the reader is concerned, and gets
// returned as the key. An API key is ASCII, so ANSI loses nothing.
```

The reader strips a BOM defensively as well. **Neither guard makes the other
unnecessary**, because the file belongs to whoever edits it next and PowerShell
5.1 adds a BOM by default.

**Say which state the feature ends up in, on the Ready page:**

> "Not configured" is a perfectly good outcome and is worth naming as one, so
> that finding the feature inert later reads as a choice already made rather
> than a fault.

⚠️ **Never accept a key as a command-line parameter.**

---

## Silent installs

```ini
[Run]
Filename: "{app}\app.exe"; Description: "Start it now"; \
    Flags: nowait postinstall skipifsilent
```

🚨 **A silent install skips wizard pages, so `NextButtonClick` never fires for
it.** Any validation, sanitising or decision that lives on a page does not
happen. Decide deliberately what a silent install is allowed to do, and put
anything that must always run in `CurStepChanged(ssPostInstall)` instead.

`skipifsilent` on every `[Run]` entry, or an unattended install starts a window.

---

## Compiling

`ISCC.exe your.iss`. Inno Setup 6 installs per-user to
`%LOCALAPPDATA%\Programs\Inno Setup 6\` or machine-wide to
`%ProgramFiles(x86)%\Inno Setup 6\`; check both.

⚠️ **Inno treats a `.iss` as UTF-8 only when it carries a BOM.** Without one,
every non-ASCII byte is read as ANSI and reaches the wizard as mojibake. Either
add a BOM or keep the file ASCII — and if the file is **generated**, make the
generator *refuse* a character it has no fold for rather than emitting it.

⚠️ `#include` splits a generated table sensibly: Pascal Script needs a constant
**before** the array declaration that uses it and the initialiser **after** it,
which is two generated files, not one.
