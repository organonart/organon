#Requires -Version 5.1
<#
.SYNOPSIS
    Build Organon Console and package it into a Windows installer.

.DESCRIPTION
    This is the ONLY supported way to compile organon.iss. It exists because of one
    property of cargo that quietly produces a wrong installer:

        cargo build writes the same path whatever features it was given.

    Build twice with different flags and the second silently replaces the first. The
    installer then packages whichever ran last, installs perfectly, and is the wrong
    product. So this script does not trust the command it just ran - it deletes the
    artifact first, rebuilds it, and then interrogates the file on disk.

    Five refusals, each with the failure it prevents:

      1. no artifact            - the build did not produce the binary it claimed to
      2. --version non-zero     - the binary exists but cannot run at all
      3. --version not Organon  - the file at that path is not our product
      4. version mismatch       - the binary and Cargo.toml disagree, so the
                                  installer would restate a version nothing else holds
      5. no ISCC                - Inno Setup is not installed

    Break any of them on purpose once and check that it fires. A gate that has never
    been seen to fail is an assertion, not a check.

    ENCODING: pure ASCII, deliberately. Windows PowerShell 5.1 reads a BOM-less .ps1
    as CP1252, so a non-ASCII byte here parses fine under pwsh in CI and is
    unparseable on the machine this is written for. CI checks this file by name -
    see the "Validate the PowerShell deploy scripts" step in ci.yml.

.PARAMETER SkipBuild
    Package the artifact already on disk instead of rebuilding it. For iterating on
    organon.iss only. Every artifact gate still runs, but the freshness guarantee is
    gone: what gets packaged is whatever happens to be sitting there.

.PARAMETER KeepVersionFile
    Leave target\installer\VERSION.txt behind after packaging. Normally it is written
    for the compile and removed, so it cannot go stale and be picked up by a later
    run that failed before regenerating it.

.EXAMPLE
    .\build.ps1
#>
[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$KeepVersionFile
)

$ErrorActionPreference = 'Stop'

function Fail {
    param([string]$Message, [string]$Why)
    Write-Host ''
    Write-Host "REFUSED: $Message" -ForegroundColor Red
    if ($Why) { Write-Host "  $Why" -ForegroundColor DarkGray }
    exit 1
}

# ---------------------------------------------------------------------------
# Where things are
# ---------------------------------------------------------------------------
$InstallerDir = $PSScriptRoot
$NativeDir    = Split-Path -Parent $InstallerDir
$RepoRoot     = Split-Path -Parent $NativeDir
$CargoToml    = Join-Path $NativeDir 'Cargo.toml'
$OutDir       = Join-Path $NativeDir 'target\installer'
$Exe          = Join-Path $NativeDir 'target\release\organon-console.exe'
$Iss          = Join-Path $InstallerDir 'organon.iss'

if (-not (Test-Path $CargoToml)) {
    Fail "Cargo.toml not found at $CargoToml" 'This script must live in native\installer\.'
}

# ---------------------------------------------------------------------------
# The version, from the one place that owns it
# ---------------------------------------------------------------------------
# Read the [package] version, not the first version= line in the file - the
# dependency table is full of them and the workspace block comes first.
$inPackage = $false
$Version = $null
foreach ($line in (Get-Content -LiteralPath $CargoToml)) {
    if ($line -match '^\s*\[package\]\s*$') { $inPackage = $true; continue }
    if ($inPackage -and $line -match '^\s*\[') { break }
    if ($inPackage -and $line -match '^\s*version\s*=\s*"([^"]+)"') {
        $Version = $matches[1]
        break
    }
}
if (-not $Version) {
    Fail 'could not read [package] version from Cargo.toml' 'The installer restates it, so it cannot be guessed.'
}
Write-Host "Cargo.toml [package] version: $Version" -ForegroundColor Cyan

# ---------------------------------------------------------------------------
# Build - after removing the artifact, so a stale one cannot be packaged
# ---------------------------------------------------------------------------
if ($SkipBuild) {
    Write-Host 'WARNING: -SkipBuild - packaging whatever artifact is already on disk.' -ForegroundColor Yellow
    Write-Host '         The gates below still run; the freshness guarantee does not.' -ForegroundColor Yellow
} else {
    if (Test-Path $Exe) {
        # Removing it first is what makes gate 1 meaningful. Without this, a failed
        # build leaves yesterday's binary in place and every later gate passes on it.
        Remove-Item -LiteralPath $Exe -Force
    }
    Write-Host 'Building Organon Console (console-edition)...' -ForegroundColor Cyan
    Push-Location $NativeDir
    try {
        cargo build --release --features console-edition --bin organon-console
        $code = $LASTEXITCODE
    } finally {
        Pop-Location
    }
    if ($code -ne 0) { Fail "cargo build exited $code" }
}

# --- gate 1: the artifact exists -------------------------------------------
if (-not (Test-Path $Exe)) {
    Fail "no artifact at $Exe" 'The build reported success without producing the binary.'
}
$ExeInfo = Get-Item $Exe
Write-Host ("artifact: {0} ({1:N0} bytes, {2})" -f $Exe, $ExeInfo.Length, $ExeInfo.LastWriteTime) -ForegroundColor Green

# --- gates 2-4: ask the artifact what it is --------------------------------
# Not "did the build command succeed" - the file itself.
#
# The two lines below look like ceremony and are not. Windows PowerShell 5.1 turns a
# native program's stderr into ErrorRecord objects, and with $ErrorActionPreference =
# 'Stop' at the top of this script that is TERMINATING - so a binary that writes one
# byte to stderr kills this script at the call site with a NativeCommandError, before
# any gate below can run. The failure then reads as a bug in this script rather than
# as the artifact being wrong, which is the exact inversion these gates exist to
# prevent. Found by pointing this at whoami.exe, which does precisely that.
#
# So: drop to 'Continue' for the call, merge stderr into the output so a real error
# message still survives to be printed, and let the gates below judge it.
$prevEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
try {
    $Reported = & $Exe --version 2>&1 | ForEach-Object { $_.ToString() }
    $code = $LASTEXITCODE
} finally {
    $ErrorActionPreference = $prevEap
}
$first = $Reported | Where-Object { $_ -and $_.Trim() -ne '' } | Select-Object -First 1
if ($first) { $Reported = $first.Trim() } else { $Reported = '' }

if ($code -ne 0) {
    Fail "--version exited $code (it said: '$Reported')" `
         'The binary exists but cannot run. A prerequisite may be missing on THIS machine.'
}
if ($Reported -eq '') {
    Fail '--version exited 0 but printed nothing' `
         'A binary that cannot introduce itself cannot be gated on. Check the subsystem: a GUI-subsystem build has no standard handles, so every println goes nowhere.'
}
Write-Host "artifact reports: $Reported" -ForegroundColor Green

if ($Reported -notmatch '^Organon') {
    Fail "--version said '$Reported'" 'That does not name an Organon product, so this is not our binary.'
}

# The stage-4 pair: two languages restating one fact. The match is deliberately on
# the VERSION only and not on the product name - the Console's display name is stated
# in two places that cannot see each other (console_main.rs PRODUCT_NAME and
# edition.rs EDITION.name()) and a rename is expected, so pinning the exact spelling
# here would go red on a rename that is not a defect.
if ($Reported -notmatch [regex]::Escape($Version)) {
    Fail "version mismatch: Cargo.toml says $Version, the binary says '$Reported'" `
         'The installer restates the version. Shipping a disagreement makes both wrong.'
}
Write-Host "version agrees: $Version" -ForegroundColor Green

# ---------------------------------------------------------------------------
# VERSION.txt - what makes the Corresponding Source identifiable
# ---------------------------------------------------------------------------
# GPLv3 section 6 obliges us to offer the source for THIS binary. "The repo" is not an
# answer; a commit is. A dirty tree is recorded as such, because a build from
# uncommitted changes has no corresponding source anyone else can obtain.
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir | Out-Null }

Push-Location $RepoRoot
try {
    $Sha = (git rev-parse HEAD).Trim()
    $dirty = git status --porcelain
} finally {
    Pop-Location
}
if ($dirty) {
    $Sha = "$Sha (BUILT FROM A DIRTY TREE - this commit is not what was compiled)"
    Write-Host 'WARNING: working tree is dirty. VERSION.txt records that.' -ForegroundColor Yellow
}

$VersionFile = Join-Path $OutDir 'VERSION.txt'
$stamp = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
$lines = @(
    "Organon Console $Version",
    "commit:   $Sha",
    "built:    $stamp",
    "",
    "Organon is free software under the GNU General Public License, version 3 or",
    "later. The complete corresponding source for this binary is the commit named",
    "above, at https://github.com/organonart/organon - see LICENSE-GPL beside this",
    "file for the licence text, and NOTICE for third-party material."
)
# Set-Content -Encoding utf8 would add a BOM in 5.1. This file is read by people, but
# writing it BOM-less costs nothing and keeps one rule for the whole directory.
[IO.File]::WriteAllText($VersionFile, ($lines -join "`r`n") + "`r`n")
Write-Host "wrote $VersionFile" -ForegroundColor Green

# ---------------------------------------------------------------------------
# gate 5: Inno Setup, then compile
# ---------------------------------------------------------------------------
$IsccCandidates = @(
    (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
    (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
    (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe')
)
$Iscc = $IsccCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $Iscc) {
    $cmd = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($cmd) { $Iscc = $cmd.Source }
}
if (-not $Iscc) {
    Fail 'Inno Setup 6 (ISCC.exe) not found' 'Install it from https://jrsoftware.org/isdl.php, or add ISCC.exe to PATH.'
}
Write-Host "ISCC: $Iscc" -ForegroundColor Cyan

& $Iscc "/DAppVersion=$Version" "/DSourceExe=$Exe" "/DRepoRoot=$RepoRoot" $Iss
$code = $LASTEXITCODE
if ($code -ne 0) { Fail "ISCC exited $code" }

if (-not $KeepVersionFile) {
    # Removed so a later run that fails before regenerating it cannot package a stale
    # commit id alongside a fresh binary - the one mismatch nothing downstream checks.
    Remove-Item -LiteralPath $VersionFile -Force
}

$Setup = Join-Path $OutDir "organon-console-$Version-x64-setup.exe"
if (-not (Test-Path $Setup)) {
    Fail "ISCC reported success but $Setup does not exist"
}
$SetupInfo = Get-Item $Setup
Write-Host ''
Write-Host ("installer: {0} ({1:N0} bytes)" -f $Setup, $SetupInfo.Length) -ForegroundColor Green
Write-Host ''
Write-Host 'NOT VERIFIED BY THIS SCRIPT: that it installs, runs, upgrades or uninstalls' -ForegroundColor Yellow
Write-Host 'on a machine that has never built Organon. See doc\shipping-windows.md.' -ForegroundColor Yellow
