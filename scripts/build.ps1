<#
.SYNOPSIS
    Builds a DisplayGroups release: the NSIS installer, the portable ZIP and
    SHA256SUMS.txt, in dist-release\.

.DESCRIPTION
    Stages, run in order by -Stage All (the default):

      Binaries   Settings UI (npm), tests (unless -SkipTests), and the release builds
                 of displaygroups.exe, displaygroupsw.exe and displaygroups-settings.exe.
      Package    The installer (Tauri's NSIS bundler with tauri.release.conf.json),
                 the portable ZIP, then Checksums.
      Checksums  SHA256SUMS.txt for the .exe, .zip and .tar.gz files in the output
                 folder (the release workflow adds the Linux package before this stage).
                 Run it again after anything changes those files (installer signing).

    Signing is off unless DG_SIGN_MODE is set; scripts/sign.ps1 describes the modes
    and docs/releasing.md the release flow. The installer is never run by this script.

.EXAMPLE
    .\scripts\build.ps1
.EXAMPLE
    .\scripts\build.ps1 -Version v1.0.0 -SkipTests
#>
param(
    [ValidateSet('All', 'Binaries', 'Package', 'Checksums')]
    [string]$Stage = 'All',

    # Expected version, with or without a leading "v" (a release tag). The build stops
    # unless Cargo.toml, tauri.conf.json and package.json all say the same.
    [string]$Version,

    [switch]$SkipTests,

    [string]$OutDir
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = Split-Path -Parent $PSScriptRoot
$settingsDir = Join-Path $root 'apps\settings'
$tauriDir = Join-Path $settingsDir 'src-tauri'
$releaseDir = Join-Path $root 'target\release'
if (-not $OutDir) { $OutDir = Join-Path $root 'dist-release' }
$OutDir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutDir)
$triple = 'x86_64-pc-windows-msvc'
$utf8 = New-Object System.Text.UTF8Encoding($false)

function Invoke-Step([string]$What, [scriptblock]$Command) {
    Write-Host "==> $What" -ForegroundColor Cyan
    & $Command
    if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE." }
}

function Get-ReleaseVersion {
    $cargo = Get-Content -Raw (Join-Path $root 'Cargo.toml')
    $found = [ordered]@{
        'Cargo.toml'      = [regex]::Match($cargo, '(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"').Groups[1].Value
        'tauri.conf.json' = (Get-Content -Raw (Join-Path $tauriDir 'tauri.conf.json') | ConvertFrom-Json).version
        'package.json'    = (Get-Content -Raw (Join-Path $settingsDir 'package.json') | ConvertFrom-Json).version
    }
    $distinct = @($found.Values | Sort-Object -Unique)
    if ($distinct.Count -ne 1 -or -not $distinct[0]) {
        $list = ($found.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ', '
        throw "Version mismatch: $list"
    }
    $v = $distinct[0]
    if ($Version -and $Version.TrimStart('v') -ne $v) {
        throw "The tag says $Version but the manifests say $v."
    }
    $v
}

function Invoke-Sign([string]$File) {
    if ($env:DG_SIGN_MODE) {
        & (Join-Path $PSScriptRoot 'sign.ps1') $File
    }
}

function Build-Binaries {
    Push-Location $settingsDir
    try {
        if ($env:CI -or -not (Test-Path 'node_modules')) {
            Invoke-Step 'npm ci' { npm ci }
        }
        Invoke-Step 'Settings UI' { npm run build }
    }
    finally { Pop-Location }

    Push-Location $root
    try {
        if (-not $SkipTests) {
            Invoke-Step 'Tests' { cargo test --workspace --locked }
        }
        Invoke-Step 'Release build' {
            cargo build --release --locked -p displaygroups -p displaygroups-settings --features displaygroups-settings/custom-protocol
        }
    }
    finally { Pop-Location }
}

function Build-Installer([string]$ver) {
    # Tauri takes sidecars from src-tauri\binaries\<name>-<target triple>.exe.
    $bin = Join-Path $tauriDir 'binaries'
    New-Item -ItemType Directory -Force -Path $bin | Out-Null
    foreach ($name in 'displaygroups', 'displaygroupsw') {
        Copy-Item -LiteralPath (Join-Path $releaseDir "$name.exe") -Destination (Join-Path $bin "$name-$triple.exe") -Force
    }

    $bundleArgs = @('tauri', 'bundle', '--ci', '--bundles', 'nsis', '--no-binary-patching',
        '--features', 'custom-protocol', '--config', 'src-tauri/tauri.release.conf.json')
    if ($env:DG_SIGN_MODE) {
        # A config file rather than inline JSON: Windows PowerShell mangles quotes
        # passed to native programs.
        $signConfig = @{ bundle = @{ windows = @{ signCommand = @{
                        cmd  = 'powershell.exe'
                        args = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $PSScriptRoot 'sign.ps1'), '%1')
                    } } } }
        $signFile = Join-Path $releaseDir 'dg-sign.conf.json'
        [IO.File]::WriteAllText($signFile, ($signConfig | ConvertTo-Json -Depth 8), $utf8)
        $bundleArgs += @('--config', $signFile)
    }
    else {
        $bundleArgs += '--no-sign'
    }

    Push-Location $settingsDir
    try { Invoke-Step 'Installer' { npx @bundleArgs } }
    finally { Pop-Location }

    $built = Join-Path $releaseDir "bundle\nsis\DisplayGroups_${ver}_x64-setup.exe"
    if (-not (Test-Path -LiteralPath $built)) { throw "The bundler did not produce $built." }
    Copy-Item -LiteralPath $built -Destination (Join-Path $OutDir "DisplayGroups-$ver-setup-x64.exe") -Force
}

function Build-Portable([string]$ver) {
    Write-Host '==> Portable ZIP' -ForegroundColor Cyan
    $name = "DisplayGroups-$ver-portable-x64"
    $stageDir = Join-Path $OutDir $name
    if (Test-Path -LiteralPath $stageDir) { Remove-Item -LiteralPath $stageDir -Recurse -Force }
    New-Item -ItemType Directory -Force -Path (Join-Path $stageDir 'locales') | Out-Null

    foreach ($exe in 'displaygroups.exe', 'displaygroupsw.exe', 'displaygroups-settings.exe') {
        $target = Join-Path $stageDir $exe
        Copy-Item -LiteralPath (Join-Path $releaseDir $exe) -Destination $target
        Invoke-Sign $target
    }
    Copy-Item -Path (Join-Path $root 'locales\*.json') -Destination (Join-Path $stageDir 'locales')
    Copy-Item -LiteralPath (Join-Path $root 'README.md') -Destination $stageDir
    Copy-Item -LiteralPath (Join-Path $root 'LICENSE') -Destination (Join-Path $stageDir 'LICENSE.txt')
    # Its presence keeps groups, settings and logs in .\data next to the executables.
    [IO.File]::WriteAllText((Join-Path $stageDir 'portable.flag'),
        "DisplayGroups keeps its data in the data folder next to this file while this file exists.`r`n", $utf8)

    # Entries are written one by one: Windows PowerShell's ZipFile.CreateFromDirectory
    # stores "\" in entry names, which other unzip tools reject.
    $zip = Join-Path $OutDir "$name.zip"
    if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
    Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::Open($zip, [IO.Compression.ZipArchiveMode]::Create)
    try {
        Get-ChildItem -LiteralPath $stageDir -Recurse -File | Sort-Object FullName | ForEach-Object {
            $entry = "$name/" + $_.FullName.Substring($stageDir.Length + 1).Replace('\', '/')
            [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $_.FullName, $entry,
                [IO.Compression.CompressionLevel]::Optimal)
        }
    }
    finally { $archive.Dispose() }
    Remove-Item -LiteralPath $stageDir -Recurse -Force
}

function Write-Checksums {
    Write-Host '==> SHA256SUMS.txt' -ForegroundColor Cyan
    $lines = Get-ChildItem -LiteralPath $OutDir -File |
        Where-Object { $_.Extension -in '.exe', '.zip' -or $_.Name -like '*.tar.gz' } |
        Sort-Object Name |
        ForEach-Object { '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $_.Name }
    if (-not $lines) { throw "Nothing to checksum in $OutDir." }
    [IO.File]::WriteAllText((Join-Path $OutDir 'SHA256SUMS.txt'), (($lines -join "`n") + "`n"), $utf8)
    $lines | ForEach-Object { Write-Host "    $_" }
}

$ver = Get-ReleaseVersion
Write-Host "DisplayGroups $ver ($Stage)" -ForegroundColor Cyan

function Reset-SigningQueue {
    if ($env:DG_SIGN_MODE -ne 'signpath') { return }
    if (-not $env:DG_SIGN_DIR) { throw 'DG_SIGN_MODE=signpath needs DG_SIGN_DIR.' }
    foreach ($item in 'unsigned', 'unsigned.txt') {
        $p = Join-Path $env:DG_SIGN_DIR $item
        if (Test-Path -LiteralPath $p) { Remove-Item -LiteralPath $p -Recurse -Force }
    }
}

# How many files are waiting for the next SignPath request; 0 means only the
# installer is left. Also written to the step's outputs in GitHub Actions.
function Write-SigningQueue {
    if ($env:DG_SIGN_MODE -ne 'signpath') { return }
    $queue = Join-Path $env:DG_SIGN_DIR 'unsigned'
    $count = if (Test-Path -LiteralPath $queue) { @(Get-ChildItem -LiteralPath $queue -File).Count } else { 0 }
    Write-Host "==> Files waiting for signing: $count" -ForegroundColor Cyan
    $list = Join-Path $env:DG_SIGN_DIR 'unsigned.txt'
    if (Test-Path -LiteralPath $list) { Get-Content -LiteralPath $list | Sort-Object -Unique | ForEach-Object { Write-Host "    $_" } }
    if ($env:GITHUB_OUTPUT) { Add-Content -LiteralPath $env:GITHUB_OUTPUT -Value "pending=$count" -Encoding UTF8 }
}

if ($Stage -in 'All', 'Binaries') { Build-Binaries }
if ($Stage -in 'All', 'Package') {
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
    Get-ChildItem -LiteralPath $OutDir -Filter 'DisplayGroups-*' | Remove-Item -Recurse -Force
    Reset-SigningQueue
    Build-Installer $ver
    Build-Portable $ver
    Write-SigningQueue
}
if ($Stage -in 'All', 'Package', 'Checksums') { Write-Checksums }
