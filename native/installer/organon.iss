; Organon - Windows installer (Inno Setup 6)
;
; WHAT THIS IS: the thing a stranger downloads, runs, and ends up with a working
; Organon Console on a machine that has never built one. It CONSUMES the output of a
; release build; it does not produce one. `build.ps1` beside it is what produces and
; gates the artifact, and it is the only supported way to compile this script.
;
; WHAT THIS IS NOT: a replacement for `..\deploy.ps1`. That is developer deploy - it
; assumes a checkout, cargo, and a box configured by having built Organon. Every one
; of those is a thing a target machine does not have.
;
; THE PREREQUISITE FLOOR IS MEASURED, NOT GUESSED. See `doc\shipping-windows.md`:
; organon-console.exe imports nine symbols from VCRUNTIME140.dll, all present since
; the Visual C++ 2015 redistributable (14.0), and does NOT import __CxxFrameHandler4,
; which is what would have pushed the floor to 14.20. Do not raise this number
; without re-running dumpbin, and do not copy one from another product.
;
; ENCODING: pure ASCII on purpose. Inno 6 is Unicode and would accept UTF-8 with a
; BOM, but `build.ps1` next door must stay ASCII for Windows PowerShell 5.1 (see the
; CI encoding gate), and holding both files to one rule means nobody has to remember
; which is which.

#ifndef AppVersion
  #error AppVersion must be passed by build.ps1. A version this script invents would be a second source of truth.
#endif
#ifndef SourceExe
  #error SourceExe must be passed by build.ps1. Compiling this script by hand is not supported.
#endif
#ifndef RepoRoot
  #error RepoRoot must be passed by build.ps1.
#endif

[Setup]
; The AppId is the identity Windows uses to recognise an upgrade rather than a second
; parallel install. It must NEVER change, for the same reason the VST3 class ID must
; never change: changing it orphans every existing install, which then cannot be
; upgraded or uninstalled by the new one.
AppId={{7F3C1A94-6D2E-4B58-9E17-2A0C5D8B4E63}
AppName=Organon Console
AppVersion={#AppVersion}
AppVerName=Organon Console {#AppVersion}
AppPublisher=Organon
AppPublisherURL=https://organon.art
DefaultDirName={autopf}\Organon
DefaultGroupName=Organon
DisableProgramGroupPage=yes
OutputDir={#RepoRoot}\native\target\installer
OutputBaseFilename=organon-console-{#AppVersion}-x64-setup
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; No plugin ships here, so nothing needs a machine-wide location and nothing needs
; admin. `lowest` makes {autopf} resolve to {localappdata}\Programs, which is a real
; per-user install rather than a privileged one that merely asks nicely.
PrivilegesRequired=lowest
; GPL-3.0-or-later. The licence text travels WITH the binary; it is not a link.
LicenseFile={#RepoRoot}\LICENSE-GPL
; Windows will not overwrite a running executable and reports it as "Access to the
; path is denied", which reads as a permissions problem and sends people to ACLs.
; AppMutex is the usual answer and is DELIBERATELY ABSENT: it only works if the
; product creates that mutex, and organon-console creates none - so an AppMutex line
; here would do nothing while looking exactly like it worked. Restart Manager detects
; the file in use without needing the application's cooperation.
CloseApplications=yes
CloseApplicationsFilter=organon-console.exe
RestartApplications=no
UninstallDisplayName=Organon Console {#AppVersion}
UninstallDisplayIcon={app}\organon-console.exe

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
; --- ours: replaced on every upgrade ---
Source: "{#SourceExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\LICENSE-GPL"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\NOTICE"; DestDir: "{app}"; Flags: ignoreversion
; VERSION.txt names the exact commit this binary was built from. That is what makes
; the Corresponding Source identifiable under GPLv3 section 6 - "the source for THAT
; binary" is not a claim anyone can make without recording which commit it was.
Source: "{#RepoRoot}\native\target\installer\VERSION.txt"; DestDir: "{app}"; Flags: ignoreversion

; --- theirs: the asset galleries ---
; onlyifdoesntexist + uninsneveruninstall is the "theirs" combination, and it is a
; DELIBERATE DIVERGENCE FROM deploy.ps1, which copies these with -Force and so
; silently replaces a preset the user edited under a shipped filename. That is the
; right behaviour for a developer who wants the repo's copy back, and the wrong
; behaviour for someone whose work is in that file. The destination follows
; preset.rs's dirs::data_dir(), which is %APPDATA% on Windows - never a copied path.
Source: "{#RepoRoot}\native\assets\networks\*.json"; DestDir: "{userappdata}\OrganicMath\networks"; Flags: onlyifdoesntexist uninsneveruninstall
Source: "{#RepoRoot}\native\assets\materials\graphs\*.json"; DestDir: "{userappdata}\OrganicMath\materials"; Flags: onlyifdoesntexist uninsneveruninstall
Source: "{#RepoRoot}\native\assets\creatures\*.json"; DestDir: "{userappdata}\OrganicMath\creatures"; Flags: onlyifdoesntexist uninsneveruninstall
Source: "{#RepoRoot}\native\assets\fields\*.bin"; DestDir: "{userappdata}\OrganicMath\fields"; Flags: onlyifdoesntexist uninsneveruninstall
Source: "{#RepoRoot}\native\assets\nca\*.json"; DestDir: "{userappdata}\OrganicMath\nca"; Flags: onlyifdoesntexist uninsneveruninstall

[Dirs]
; An empty models folder, created and then left alone. Organon ships no model and
; downloads none: a .gguf is gigabytes and carries its own licence terms, which a
; background download cannot obtain consent for. This is where you put yours.
Name: "{userappdata}\OrganicMath\models"; Flags: uninsneveruninstall

[Icons]
Name: "{group}\Organon Console"; Filename: "{app}\organon-console.exe"
Name: "{group}\Uninstall Organon Console"; Filename: "{uninstallexe}"

[Run]
; skipifsilent is load-bearing: a silent install runs no wizard pages, so anything
; offered here has to be opt-in from a page a silent install never sees.
Filename: "{app}\organon-console.exe"; Description: "Launch Organon Console"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; The application writes its log to %LOCALAPPDATA%\organon\console\console.log,
; OUTSIDE the install tree - verified, not assumed - so dirifempty is not defeated by
; a log file appearing under the app directory after installation. If that ever
; changes, this is the line that stops working, and it stops working silently: the
; folder simply stays behind and nothing reports it.
Type: dirifempty; Name: "{app}"

[Code]
// Comments in this section are // and never brace-delimited, because a Pascal brace
// comment is CLOSED EARLY by the first Inno constant inside it - the rest of the
// sentence then compiles as code, and the error points somewhere else entirely.

const
  VCR_MIN_MAJOR = 14;
  VCR_MIN_MINOR = 0;
  VCR_URL = 'https://aka.ms/vs/17/release/vc_redist.x64.exe';

function VCRuntimeOk(var Found: String): Boolean;
var
  DllPath: String;
  MS, LS: Cardinal;
  Major, Minor: Cardinal;
begin
  Result := False;
  Found := '';
  DllPath := ExpandConstant('{sys}') + '\VCRUNTIME140.dll';
  if not FileExists(DllPath) then
    Exit;
  if not GetVersionNumbers(DllPath, MS, LS) then
  begin
    // Present but unreadable. Treated as satisfied rather than blocking: the floor
    // is the LOWEST version of this DLL that has ever shipped, so presence alone
    // very nearly implies it, and refusing a valid machine is worse than accepting
    // a doubtful one for a check whose whole purpose is to avoid a silent
    // 0xC0000142.
    Found := 'present, version unreadable';
    Result := True;
    Exit;
  end;
  Major := MS shr 16;
  Minor := MS and $FFFF;
  Found := Format('%d.%d', [Major, Minor]);
  Result := (Major > VCR_MIN_MAJOR) or ((Major = VCR_MIN_MAJOR) and (Minor >= VCR_MIN_MINOR));
end;

function InitializeSetup(): Boolean;
var
  Found: String;
  Msg: String;
  ErrorCode: Integer;
begin
  Result := True;
  if VCRuntimeOk(Found) then
    Exit;

  if Found = '' then
    Msg := 'Organon needs the Microsoft Visual C++ runtime, and VCRUNTIME140.dll was not found on this machine.'
  else
    Msg := 'Organon needs Microsoft Visual C++ runtime ' + Format('%d.%d', [VCR_MIN_MAJOR, VCR_MIN_MINOR]) +
           ' or newer. This machine has ' + Found + '.';

  Msg := Msg + #13#10#13#10 +
    'Without it Organon cannot start, and it fails BEFORE it can display anything - no ' +
    'window and no error message. That is why this is checked here rather than left to ' +
    'the first launch.' + #13#10#13#10 +
    'Install "Microsoft Visual C++ Redistributable (x64)" and run this again.' + #13#10#13#10 +
    'Open the download page now?';

  if MsgBox(Msg, mbCriticalError, MB_YESNO) = IDYES then
    ShellExecAsOriginalUser('open', VCR_URL, '', '', SW_SHOWNORMAL, ewNoWait, ErrorCode);

  Result := False;
end;

procedure CurUninstallStepChanged(CurStep: TUninstallStep);
begin
  if CurStep = usPostUninstall then
    MsgBox('Organon Console has been removed.' + #13#10#13#10 +
           'Your galleries, saved layouts and models were left in place, under ' +
           ExpandConstant('{userappdata}') + '\OrganicMath and ' +
           ExpandConstant('{userappdata}') + '\OrganonShell. Delete those folders by ' +
           'hand if you want them gone.',
           mbInformation, MB_OK);
end;
