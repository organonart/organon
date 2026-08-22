#Requires -Version 5.1
<#
.SYNOPSIS
    Windows counterpart of deploy.sh — build + bundle (visual embedded) and install
    the .vst3 to your VST3 folder, plus the `organon` CLI and the asset galleries.

.DESCRIPTION
    #658 Tier 3. Run this after every native change on the Windows box; it is the
    Windows arm of the "deploy-native-build" standing rule.

    Where it deliberately differs from deploy.sh:

      * **No codesign.** Ad-hoc signing is a macOS concept. Windows does not gate an
        unsigned VST3 the way Gatekeeper gates an unsigned .vst3 bundle, so there is
        no equivalent step and no equivalent workaround needed.
      * **A locked DLL is the normal failure here, and it has no macOS analogue.**
        Windows takes a mandatory exclusive lock on a loaded DLL: while a DAW has
        Organon loaded, the installed file physically cannot be replaced, and the
        error you get is a bare "Access to the path is denied" that reads like a
        permissions problem. This script checks for that first and says what is
        actually holding it.
      * **The galleries land in %APPDATA%\OrganicMath\**, not
        ~/Library/Application Support/OrganicMath/. That is not a choice made here —
        every gallery path in preset.rs goes through `dirs::data_dir()`, which
        resolves to %APPDATA% on Windows. Keep the two in step by following that
        function, not by copying paths between the two scripts.

.PARAMETER Dest
    VST3 install folder. Defaults to F:\vst3 — add this folder to your DAW's VST3
    search path (in Ableton: Settings → Plug-Ins → VST3 Plug-In Custom Folder).

.PARAMETER WithLlm
    Also build the llama.cpp inference runtime (#367 Tier 2c) and embed it INSIDE the
    bundle, which is where the plugin looks for it. No loose copy is installed beside
    the bundle: that folder is a VST3 search path, not an install prefix. OFF by default.

    Needs MORE than cmake, and every missing piece fails in a build script rather
    than in Rust -- so the error names a C++ tool, not this switch. Measured on
    organon-one 2026-08-21, first Windows build of this target:

      * cmake            -- on PATH.
      * MSVC cl.exe      -- run from a Developer Prompt, or call vcvars64.bat first.
                            It is NOT on a normal shell's PATH.
      * Ninja + CMAKE_GENERATOR=Ninja  -- see the warning below. Not optional.
      * libclang         -- bindgen builds the llama binding. Set LIBCLANG_PATH
                            (e.g. 'C:\Program Files\LLVM\bin').
      * CUDA Toolkit     -- Cargo.toml's Windows table pins llama-cpp-4 to
                            features = ['cuda'], so this is a CUDA build whether or
                            not you wanted one. There is no CPU-only path here.

    WARNING -- CMAKE_GENERATOR=Ninja is REQUIRED, and without it the build cannot
    succeed at all. The `cmake` crate appends cargo's NUM_JOBS as `-j<N>` to the
    build command whatever the generator is. MSBuild -- the default generator on
    Windows -- has no `-j` switch, so it aborts with MSB1001 ('Unknown switch').
    Lowering the number does not help; NUM_JOBS=1 fails identically, because the
    switch itself is what MSBuild rejects. Ninja accepts `-j` and simply takes the
    last one (the real command line ends up `ninja -j 32 -j32 install`).

    Expect a long first build: ggml-cuda is 184 .cu files compiled for SEVEN GPU
    architectures (sm_75 through sm_121a). Set CMAKE_CUDA_ARCHITECTURES to just
    your own card's (120a for a 5090) if you only need to run it locally.

.PARAMETER Force
    Stop any running plugin host that is holding the installed DLL, instead of
    refusing. ⚠️ This kills your DAW — you lose anything unsaved. Off by default on
    purpose; the default behaviour is to tell you what to close.

.PARAMETER AddToPath
    Append -Dest to your USER PATH so plain `organon` works in a new shell. Off by
    default because it is a persistent change to your Windows profile, not to this
    repo. Without it the script just prints the full path to the CLI.

.EXAMPLE
    .\deploy.ps1
.EXAMPLE
    .\deploy.ps1 -Dest 'D:\Plugins\VST3' -WithLlm -AddToPath
#>
[CmdletBinding()]
param(
    [string]$Dest = 'F:\vst3',
    [switch]$WithLlm,
    [switch]$Force,
    [switch]$AddToPath,
    # Where the `organon` CLI and its completions go. Deliberately NOT $Dest: that is a
    # folder the DAW scans for plugins, and loose executables in a VST3 search path are
    # noise at best.
    #
    # ⚠️ NOT %LOCALAPPDATA%\Organon\bin. That exact directory has a recorded anomaly on
    # organon-one — files written there read as present from one shell and absent from
    # another, never diagnosed. ~/.local/bin is the prefix that demonstrably works here
    # (it already carries claude.exe and uv) and is already on the user PATH.
    [string]$CliDest = (Join-Path $env:USERPROFILE '.local\bin')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ── Helpers ─────────────────────────────────────────────────────────────────

function Invoke-Checked {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Exe,
        [Parameter(Mandatory)][string[]]$Arguments
    )
    Write-Host "→ $Exe $($Arguments -join ' ')" -ForegroundColor DarkGray
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "'$Exe $($Arguments -join ' ')' failed with exit code $LASTEXITCODE"
    }
}

<#
.SYNOPSIS
    Is this file held open by another process?
.DESCRIPTION
    Asked by trying to open it for writing with no sharing — which is exactly the
    access the upcoming Copy-Item needs, so a pass here means the copy will work and
    a fail means it would not have. Inferring from a process list instead would be
    guesswork; this asks the filesystem the same question the copy asks.

    A missing file is not locked (nothing to hold), which makes first-run install
    fall through cleanly.
#>
function Test-FileLocked {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $false }
    $stream = $null
    try {
        $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open,
                                         [System.IO.FileAccess]::ReadWrite,
                                         [System.IO.FileShare]::None)
        return $false
    } catch [System.IO.IOException] {
        return $true
    } catch [System.UnauthorizedAccessException] {
        # Read-only file or an ACL problem — not a lock, and a different fix. Report
        # it as unlocked so the copy proceeds and fails with its own clearer error.
        return $false
    } finally {
        if ($null -ne $stream) { $stream.Dispose() }
    }
}

<#
.SYNOPSIS
    Running processes that plausibly have a VST3 loaded.
.DESCRIPTION
    Best-effort and deliberately so. Naming the exact holder of a file handle needs
    the Restart Manager API (or handle.exe); this list covers the hosts anyone
    building Organon is realistically running and is only ever used to make an error
    message actionable — never to decide whether to proceed. That decision comes
    from Test-FileLocked, which asks the filesystem directly.
#>
function Get-LikelyPluginHosts {
    [CmdletBinding()]
    param()
    $patterns = @(
        'Ableton Live*', 'Reaper*', 'FL64', 'FL', 'Cubase*', 'Nuendo*', 'Bitwig*',
        'Studio One*', 'Reason*', 'Renoise*', 'Waveform*', 'Cakewalk*', 'Mixcraft*',
        'ProTools*', 'Samplitude*', 'Ardour*', 'Tracktion*', 'organon-standalone',
        'organon-mind'
    )
    $running = @(Get-Process -ErrorAction SilentlyContinue)
    return @($running | Where-Object {
        $name = $_.ProcessName
        $patterns | Where-Object { $name -like $_ }
    } | Sort-Object -Property ProcessName -Unique)
}

<#
.SYNOPSIS
    Append a directory to the USER PATH, safely.
.DESCRIPTION
    ⚠️ Reads the RAW registry value rather than [Environment]::GetEnvironmentVariable.
    That API EXPANDS %USERPROFILE%-style references before handing them back; writing
    the result straight back would permanently bake today's expansion into the user's
    PATH, silently destroying entries that were meant to stay dynamic. Reading with
    DoNotExpandEnvironmentNames and writing back the original value kind is what keeps
    this reversible and non-destructive.

    Idempotent: an entry already present (case-insensitively, trailing slash ignored)
    is left alone.
#>
function Add-UserPathEntry {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Directory)

    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    if ($null -eq $key) { throw 'Could not open HKCU\Environment to update PATH.' }
    try {
        $raw = $key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $kind = if ($null -eq $key.GetValue('Path')) {
            [Microsoft.Win32.RegistryValueKind]::ExpandString
        } else {
            $key.GetValueKind('Path')
        }

        $normalized = $Directory.TrimEnd('\')
        $existing = @($raw -split ';' | Where-Object { $_ -ne '' })
        foreach ($entry in $existing) {
            if ($entry.TrimEnd('\') -ieq $normalized) {
                Write-Host "PATH already contains $Directory — left unchanged." -ForegroundColor DarkGray
                return $false
            }
        }

        $updated = (@($existing) + $normalized) -join ';'
        $key.SetValue('Path', $updated, $kind)
        Write-Host "added to USER PATH: $Directory (open a new shell to pick it up)" -ForegroundColor Green
        return $true
    } finally {
        $key.Dispose()
    }
}

function Copy-Gallery {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$From,
        [Parameter(Mandatory)][string]$To,
        [Parameter(Mandatory)][string]$Filter,
        [Parameter(Mandatory)][string]$Label
    )
    if (-not (Test-Path -LiteralPath $From -PathType Container)) {
        Write-Host "note: $Label source $From is missing — skipped." -ForegroundColor Yellow
        return
    }
    New-Item -ItemType Directory -Force -Path $To | Out-Null
    $files = @(Get-ChildItem -LiteralPath $From -Filter $Filter -File -ErrorAction SilentlyContinue)
    foreach ($f in $files) { Copy-Item -LiteralPath $f.FullName -Destination $To -Force }
    Write-Host "installed ${Label}: $To ($($files.Count) files)" -ForegroundColor Green
}

function Main {
    Set-Location -LiteralPath $PSScriptRoot

    if (-not $IsWindowsHost) {
        throw 'deploy.ps1 is for Windows. On macOS use ./deploy.sh instead.'
    }

    # Checked HERE, before the build, and that ordering is the entire value of the
    # check. The default -Dest is on a secondary drive, which — unlike C: — can
    # genuinely be absent: external disk unplugged, drive letter reassigned, a volume
    # that mounts late after boot. `New-Item -Force` reports that perfectly well on its
    # own, but it does so at the INSTALL step, which is a full release build later. A
    # one-line test up front turns a ten-minute round trip into an instant one.
    if ($Dest -match '^([A-Za-z]):') {
        $driveRoot = "$($Matches[1]):\"
        if (-not (Test-Path -LiteralPath $driveRoot)) {
            throw "Destination drive $driveRoot is not available (-Dest '$Dest'). " +
                  'Mount it, or pass -Dest with a path on a drive that exists.'
        }
    }

    # ── Build + bundle (the visual gets embedded in there) ──────────────────
    $bundleArgs = @{ }
    if ($WithLlm) { $bundleArgs['WithLlm'] = $true }
    & (Join-Path $PSScriptRoot 'bundle.ps1') @bundleArgs

    $src = Join-Path $PSScriptRoot 'target\bundled\Organon.vst3'
    if (-not (Test-Path -LiteralPath $src -PathType Container)) {
        throw "bundle.ps1 did not produce ${src}."
    }

    # ── Clear the way ───────────────────────────────────────────────────────
    # The visual is OUR child process and holds no user state — a running one keeps
    # organic-math-visual.exe open inside the installed bundle, which blocks the
    # replace. Stopping it is safe and required; stopping a DAW is neither, which is
    # why only this one is automatic.
    $visuals = @(Get-Process -Name 'organic-math-visual' -ErrorAction SilentlyContinue)
    if ($visuals.Count -gt 0) {
        Write-Host "stopping $($visuals.Count) running visual process(es)…" -ForegroundColor DarkGray
        $visuals | Stop-Process -Force
        Start-Sleep -Milliseconds 400
    }

    New-Item -ItemType Directory -Force -Path $Dest | Out-Null

    # The installed DLL is the thing a host actually locks — probe the FILE, not the
    # folder, since the folder is removable while the DLL inside it is not. Found by
    # recursive search for the same reason bundle.ps1 discovers its arch directory:
    # hardcoding `Contents\x86_64-win` here would silently stop detecting the lock on an
    # ARM64 box, turning this clear refusal back into the bare "Access to the path is
    # denied" it exists to replace.
    $lockedPaths = @(Get-ChildItem -LiteralPath $Dest -Recurse -Filter 'Organon.vst3' -File -ErrorAction SilentlyContinue |
        Where-Object { Test-FileLocked -Path $_.FullName } |
        ForEach-Object { $_.FullName })

    if ($lockedPaths.Count -gt 0) {
        # ⚠️ The `@(…)` is load-bearing, and its absence was a real bug caught by a
        # runtime test rather than by reading. PowerShell UNROLLS an array on function
        # return, so a `return @()` from Get-LikelyPluginHosts arrives here as $null,
        # not as an empty array — and under `Set-StrictMode -Version Latest` reading
        # `.Count` on $null THROWS. The failure mode was as bad as it gets: it fired
        # only when no known DAW matched, i.e. exactly when this branch is trying to
        # tell you it could not identify the holder, replacing that message with a
        # PowerShell property error. Wrap at the CALL SITE; the callee cannot fix it.
        $hostProcs = @(Get-LikelyPluginHosts)
        $hostList = if ($hostProcs.Count -gt 0) {
            ($hostProcs | ForEach-Object { "$($_.ProcessName) (pid $($_.Id))" }) -join ', '
        } else { '(no known DAW process matched — check any host you have open)' }

        if (-not $Force) {
            throw @"
The installed plugin is loaded and cannot be replaced.

  locked: $($lockedPaths[0])
  likely holder(s): $hostList

Close the host (or remove Organon from the set) and run this again, or re-run with
-Force to have this script stop those processes for you. -Force kills them outright,
so save your work first.
"@
        }

        Write-Host "-Force: stopping $($hostProcs.Count) host process(es): $hostList" -ForegroundColor Yellow
        foreach ($h in $hostProcs) { Stop-Process -Id $h.Id -Force -ErrorAction SilentlyContinue }
        Start-Sleep -Seconds 2
        foreach ($p in $lockedPaths) {
            if (Test-FileLocked -Path $p) { throw "Still locked after -Force: $p" }
        }
    }

    # ── Install the plugin ──────────────────────────────────────────────────
    # The old name goes too: it carries the SAME VST3 class ID, so leaving it means
    # the host may bind saved sets to the stale copy.
    foreach ($old in @('Organon.vst3', 'Organic Math.vst3')) {
        $p = Join-Path $Dest $old
        if (Test-Path -LiteralPath $p) { Remove-Item -LiteralPath $p -Recurse -Force }
    }
    Copy-Item -LiteralPath $src -Destination $Dest -Recurse -Force
    Write-Host "installed: $Dest\Organon.vst3" -ForegroundColor Green

    # ── The `organon` CLI (#452 Tiers 1–2) ──────────────────────────────────
    # Built explicitly — bundle.ps1 only builds the plugin + visual.
    Invoke-Checked cargo @('build', '--release', '--bin', 'organon')
    $cliSrc = Join-Path $PSScriptRoot 'target\release\organon.exe'
    if (-not (Test-Path $CliDest)) { New-Item -ItemType Directory -Force -Path $CliDest | Out-Null }
    $cliDst = Join-Path $CliDest 'organon.exe'
    if (Test-FileLocked -Path $cliDst) {
        Write-Host "note: $cliDst is in use — close it and re-run to update the CLI." -ForegroundColor Yellow
    } else {
        Copy-Item -LiteralPath $cliSrc -Destination $cliDst -Force
        Write-Host "installed CLI: $cliDst" -ForegroundColor Green
    }

    if ($AddToPath) {
        Add-UserPathEntry -Directory $CliDest | Out-Null
    } else {
        Write-Host "note: run it as `"$cliDst`" — or re-run with -AddToPath to put $Dest on your USER PATH." -ForegroundColor DarkGray
    }

    # Tab completion. clap_complete emits a PowerShell completer; it is sourced from
    # $PROFILE rather than dropped in a directory (PowerShell has no site-functions
    # equivalent), so this writes the file and prints the one line to add.
    # Generated from the freshly BUILT binary, not the installed one: if the installed
    # copy was locked and skipped just above, $cliDst is stale or absent, and completions
    # should still describe the CLI this deploy actually produced.
    $completion = Join-Path $CliDest 'organon-completion.ps1'
    try {
        & $cliSrc completions powershell | Set-Content -LiteralPath $completion -Encoding UTF8
        if ($LASTEXITCODE -eq 0) {
            Write-Host "installed completions: $completion" -ForegroundColor Green
            Write-Host "  add to your `$PROFILE:  . `"$completion`"" -ForegroundColor DarkGray
        }
    } catch {
        Write-Host "note: could not generate completions ($($_.Exception.Message)) — skipped." -ForegroundColor Yellow
    }

    # ── The galleries ───────────────────────────────────────────────────────
    # %APPDATA%\OrganicMath — `dirs::data_dir()` in preset.rs. Idempotent: re-copied
    # every deploy so the installed set matches the repo.
    $store = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'OrganicMath'
    Copy-Gallery -From (Join-Path $PSScriptRoot 'assets\networks')        -To (Join-Path $store 'networks')  -Filter '*.json' -Label 'gallery'
    Copy-Gallery -From (Join-Path $PSScriptRoot 'assets\materials\graphs') -To (Join-Path $store 'materials') -Filter '*.json' -Label 'material graphs'
    Copy-Gallery -From (Join-Path $PSScriptRoot 'assets\creatures')        -To (Join-Path $store 'creatures') -Filter '*.json' -Label 'creatures'
    Copy-Gallery -From (Join-Path $PSScriptRoot 'assets\fields')           -To (Join-Path $store 'fields')    -Filter '*.bin'  -Label 'field clips'
    Copy-Gallery -From (Join-Path $PSScriptRoot 'assets\nca')              -To (Join-Path $store 'nca')       -Filter '*.json' -Label 'NCA gallery'

    # —— The runtime lives in the bundle, and only in the bundle (#367 Tier 2c) ——
    if ($WithLlm) {
        # 📌 **No loose copy beside the bundle any more.** It was never on the plugin's
        # search path: `mind_runtime_path()` probes an env override, then the directory of
        # the plugin DYLIB (i.e. inside Organon.vst3\Contents), then the directory of the
        # current exe. $Dest is the grandparent of the first and matches none of them, so
        # the copy installed here served only direct terminal use — at 169 MB, sitting in
        # a folder the DAW scans.
        #
        # ⚠️ For terminal use, point ORGANIC_MATH_MIND_RUNTIME at the copy inside the
        # bundle, or run it out of targetelease. The plugin needs neither.
        Write-Host 'runtime: embedded in Organon.vst3 (no separate copy installed)' -ForegroundColor Green
        Write-Host '  → The Mind tab launches it itself. Load a .gguf → prompt → Generate (the track'
        Write-Host '    must be processing audio).'
    }

    Write-Host ''
    Write-Host "→ Add $Dest to your DAW's VST3 search path if you have not already" -ForegroundColor Cyan
    Write-Host '  (Ableton: Settings → Plug-Ins → VST3 Plug-In Custom Folder), then Rescan.' -ForegroundColor Cyan
    Write-Host '  The visual is a separate process — close and reopen its window if it was open.' -ForegroundColor Cyan
}

# See bundle.ps1 for why this is not a bare `$IsWindows`.
$IsWindowsHost = if (Test-Path Variable:\IsWindows) { $IsWindows } else { $true }

if ($MyInvocation.InvocationName -ne '.') { Main }
