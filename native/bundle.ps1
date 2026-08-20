#Requires -Version 5.1
<#
.SYNOPSIS
    Windows counterpart of bundle.sh — build the plugin bundles with the visual
    binary embedded inside them, so the editor's "Open Visual Window" button works
    from any host.

.DESCRIPTION
    #658 Tier 3. This is the same job bundle.sh does on the Mac, but the layout it
    has to produce is NOT the same, and the differences are the whole reason this
    is a separate script rather than a few `if` branches over there:

      * The plugin binary lives at Contents/<arch>-win/Organon.vst3 — note that the
        DLL is itself named `.vst3`, inside a folder also named `.vst3`. That is the
        VST3 bundle spec, not a typo.
      * The visual is `organic-math-visual.exe`. The `.exe` matters: the plugin
        probes for it through `Path::exists()` in places (see `mind_runtime_path`
        in lib.rs), which does not append EXE_SUFFIX for you.
      * There is NO codesign step. Ad-hoc signing is a macOS concept; Windows either
        has a real Authenticode certificate or it has nothing, and a self-built
        plugin has nothing. Nothing here needs signing to load.
      * ⚠️ **CLAP cannot carry the visual on Windows.** nih-plug emits the CLAP as a
        BARE DLL (`target/bundled/Organon.clap` is a file, not a directory) — see
        nih_plug_xtask's `clap_bundle_library_name`, where Windows and Linux return
        `{package}.clap` while macOS returns `{package}.clap/Contents/MacOS/…`. With
        no Contents/ there is nowhere to put a sibling binary, so only the VST3 gets
        an embedded visual. This is stated rather than silently skipped because
        "Open Visual Window" failing ONLY under a CLAP host is otherwise a baffling
        bug report. Set $env:ORGANIC_MATH_VISUAL to cover the CLAP case.

.PARAMETER Install
    Also copy the finished .vst3 to -InstallDest. Mirrors `bundle.sh --install`.
    This is the minimal copy only — for the full deploy (CLI, galleries, killing a
    running visual first) use deploy.ps1.

.PARAMETER WithLlm
    Also build the embedded llama.cpp inference runtime
    (organic-math-mind-runtime, #367) and embed it beside the visual, so the Mind
    tab can launch it as a child with no separate terminal. Needs cmake. OFF by
    default so the normal fast path stays llama.cpp/C++-free.

.PARAMETER InstallDest
    Where -Install copies to. Defaults to F:\vst3.

.EXAMPLE
    .\bundle.ps1
.EXAMPLE
    .\bundle.ps1 -Install -WithLlm
#>
[CmdletBinding()]
param(
    [switch]$Install,
    [switch]$WithLlm,
    [string]$InstallDest = 'F:\vst3'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ── Helpers ─────────────────────────────────────────────────────────────────
# Defined above the main body so this script can be dot-sourced for testing
# without running a build (see the invocation gate at the bottom).

# PowerShell does NOT fail a script when a native command exits non-zero —
# $ErrorActionPreference governs cmdlets, not process exit codes. Without this the
# script would sail past a failed `cargo build` and cheerfully embed a STALE visual
# from a previous run, producing a bundle that looks fresh and isn't. Every external
# call goes through here.
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
    Locate the directory inside a .vst3 bundle that holds the plugin DLL.
.DESCRIPTION
    Discovered rather than hardcoded, deliberately. nih-plug names this directory
    after the compilation target — `x86_64-win`, `arm_64-win`, `x86-win` (note the
    inconsistent underscore in the ARM one, which is nih-plug's spelling, not ours).
    Hardcoding `x86_64-win` would silently produce a bundle with the visual in the
    wrong place the first time anyone builds on an ARM64 Windows machine: the plugin
    would load fine and only "Open Visual Window" would fail, which is a miserable
    thing to debug. Asking the tree what the bundler actually emitted cannot drift.
#>
function Find-PluginBinaryDir {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$BundleRoot)

    $contents = Join-Path $BundleRoot 'Contents'
    if (-not (Test-Path -LiteralPath $contents -PathType Container)) { return $null }

    $archDirs = @(Get-ChildItem -LiteralPath $contents -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like '*-win' })

    if ($archDirs.Count -eq 0) { return $null }
    if ($archDirs.Count -gt 1) {
        $names = ($archDirs | ForEach-Object { $_.Name }) -join ', '
        throw "Found $($archDirs.Count) architecture directories in ${contents} ($names). " +
              'Expected exactly one. Delete target/bundled and rebuild.'
    }
    return $archDirs[0].FullName
}

function Main {
    Set-Location -LiteralPath $PSScriptRoot

    if (-not $IsWindowsHost) {
        throw "bundle.ps1 is for Windows. On macOS/Linux use ./bundle.sh instead."
    }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw "cargo not found on PATH. Install Rust from https://rustup.rs and reopen this shell."
    }

    # organon#49 T4c-ii — `-p organon-visual`: the visual moved to its own package. The
    # built path (target\release\organic-math-visual.exe) is unchanged.
    Invoke-Checked cargo @('build', '--release', '-p', 'organon-visual', '--bin', 'organic-math-visual')
    Invoke-Checked cargo @('xtask', 'bundle', 'organic-math-native', '--release')

    # The embedded llama.cpp runtime (#367 Tier 2c). Guarded on cmake exactly as
    # bundle.sh does, because llama.cpp's build needs it and the failure without it
    # is a wall of C++ tooling noise rather than a clear "install cmake".
    if ($WithLlm) {
        if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
            throw "bundle.ps1 -WithLlm: cmake not found — it is needed to build llama.cpp. " +
                  "Install it with:  winget install Kitware.CMake   (then reopen this shell)"
        }
        Write-Host '→ building embedded-llm runtime (organic-math-mind-runtime; first build is slow)…'
        Invoke-Checked cargo @('build', '--release', '--features', 'embedded-llm',
                               '--bin', 'organic-math-mind-runtime')
    }

    $visual  = Join-Path $PSScriptRoot 'target\release\organic-math-visual.exe'
    $runtime = Join-Path $PSScriptRoot 'target\release\organic-math-mind-runtime.exe'
    if (-not (Test-Path -LiteralPath $visual)) {
        throw "Built the visual but cannot find it at ${visual}."
    }

    $vst3 = Join-Path $PSScriptRoot 'target\bundled\Organon.vst3'
    if (-not (Test-Path -LiteralPath $vst3 -PathType Container)) {
        throw "cargo xtask bundle did not produce ${vst3}."
    }

    $binDir = Find-PluginBinaryDir -BundleRoot $vst3
    if (-not $binDir) {
        throw "No Contents\<arch>-win directory inside ${vst3} — the bundler layout changed. " +
              'Check nih_plug_xtask vst3_bundle_library_name before editing this script.'
    }

    Copy-Item -LiteralPath $visual -Destination (Join-Path $binDir 'organic-math-visual.exe') -Force
    Write-Host "embedded visual: $binDir\organic-math-visual.exe" -ForegroundColor Green

    if ($WithLlm) {
        if (-not (Test-Path -LiteralPath $runtime)) {
            throw "-WithLlm was set but ${runtime} is missing."
        }
        Copy-Item -LiteralPath $runtime -Destination (Join-Path $binDir 'organic-math-mind-runtime.exe') -Force
        Write-Host "embedded mind runtime: $binDir\organic-math-mind-runtime.exe" -ForegroundColor Green
    }

    # The CLAP asymmetry, reported every run rather than buried in a comment. It is a
    # real functional difference between the two formats on this platform.
    $clap = Join-Path $PSScriptRoot 'target\bundled\Organon.clap'
    if (Test-Path -LiteralPath $clap -PathType Leaf) {
        Write-Host "note: Organon.clap is a bare DLL on Windows — no Contents\ to embed the visual in." -ForegroundColor Yellow
        Write-Host "      Under a CLAP host, set ORGANIC_MATH_VISUAL to the full path of organic-math-visual.exe." -ForegroundColor Yellow
    }

    if ($Install) {
        New-Item -ItemType Directory -Force -Path $InstallDest | Out-Null
        # The old name must go too — it carries the SAME VST3 class ID, so leaving it
        # behind means the host sees the plugin twice and may bind saved sets to the
        # stale copy. Same reasoning as bundle.sh's `rm -rf "Organic Math.vst3"`.
        foreach ($old in @('Organon.vst3', 'Organic Math.vst3')) {
            $p = Join-Path $InstallDest $old
            if (Test-Path -LiteralPath $p) { Remove-Item -LiteralPath $p -Recurse -Force }
        }
        Copy-Item -LiteralPath $vst3 -Destination $InstallDest -Recurse -Force
        Write-Host "installed: $InstallDest\Organon.vst3" -ForegroundColor Green
    }
}

# `$IsWindows` is a PowerShell *Core* automatic variable; it does not exist in
# Windows PowerShell 5.1, where StrictMode would make reading it a hard error. 5.1
# only ever runs on Windows, so its absence IS the answer.
$IsWindowsHost = if (Test-Path Variable:\IsWindows) { $IsWindows } else { $true }

# Run unless dot-sourced. Dot-sourcing (`. .\bundle.ps1`) loads the helpers above
# without building anything, which is how the test harness exercises
# Find-PluginBinaryDir on a machine that is not Windows and has no cargo.
if ($MyInvocation.InvocationName -ne '.') { Main }
