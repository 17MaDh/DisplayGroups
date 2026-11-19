<#
.SYNOPSIS
    Signs one file of a DisplayGroups release.

.DESCRIPTION
    Tauri's bundler calls this for every file it signs (bundle.windows.signCommand,
    set by scripts/build.ps1): the three executables, the NSIS plugin DLLs that the
    installer unpacks and loads, the uninstaller while the installer is compiled, and
    the installer. build.ps1 also calls it for the executables of the portable ZIP.
    A file that already has a valid signature is left alone.

    The mode comes from DG_SIGN_MODE:

      (unset)   Nothing is signed. The file is listed as unsigned and the build goes on.

      signtool  signtool.exe with the certificate whose SHA-1 thumbprint is in
                SIGN_CERT_THUMBPRINT (CurrentUser\My or LocalMachine\My).
                SIGN_TIMESTAMP_URL overrides the RFC 3161 timestamp server.

      signpath  Signing in rounds through SignPath (docs/releasing.md), because a
                signing request is a CI step, not a command. If a signed copy of the
                identical file is anywhere under $DG_SIGN_DIR\signed (named by the
                SHA-256 of the unsigned file), the file is replaced by it. Otherwise
                it is copied to $DG_SIGN_DIR\unsigned for the next signing request.
                The installer is never batched: it is signed last, on its own.
#>
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Path
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Test-Signed([string]$File) {
    (Get-AuthenticodeSignature -LiteralPath $File).Status -eq 'Valid'
}

# All of them are PE files. The uninstaller arrives under a temporary .tmp name,
# so the key uses .dll or .exe, which signing services recognise.
function Get-Key([string]$File) {
    $hash = (Get-FileHash -LiteralPath $File -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($File -like '*.dll') { "$hash.dll" } else { "$hash.exe" }
}

function Find-SignTool {
    $onPath = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $found = Get-ChildItem -Path $kits -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.Directory.Name -eq 'x64' } |
        Sort-Object { $_.Directory.Parent.Name } -Descending |
        Select-Object -First 1
    if (-not $found) { throw 'signtool.exe was not found. Install the Windows SDK signing tools.' }
    $found.FullName
}

$Path = (Resolve-Path -LiteralPath $Path).ProviderPath
$name = Split-Path -Leaf $Path

if (Test-Signed $Path) {
    Write-Host "sign: already signed: $name"
    exit 0
}

$mode = $env:DG_SIGN_MODE
switch ($mode) {
    { [string]::IsNullOrEmpty($_) } {
        Write-Host "sign: not configured, left unsigned: $name"
        exit 0
    }

    'signtool' {
        if (-not $env:SIGN_CERT_THUMBPRINT) { throw 'DG_SIGN_MODE=signtool needs SIGN_CERT_THUMBPRINT.' }
        $timestamp = if ($env:SIGN_TIMESTAMP_URL) { $env:SIGN_TIMESTAMP_URL } else { 'http://timestamp.digicert.com' }
        $signtool = Find-SignTool
        & $signtool sign /sha1 $env:SIGN_CERT_THUMBPRINT /fd sha256 /tr $timestamp /td sha256 /d DisplayGroups $Path
        if ($LASTEXITCODE -ne 0) { throw "signtool failed ($LASTEXITCODE) on $name" }
        Write-Host "sign: signed with signtool: $name"
    }

    'signpath' {
        if (-not $env:DG_SIGN_DIR) { throw 'DG_SIGN_MODE=signpath needs DG_SIGN_DIR.' }
        if ($name -like '*-setup.exe') {
            Write-Host "sign: installer, signed last on its own: $name"
            exit 0
        }
        $key = Get-Key $Path
        $signedDir = Join-Path $env:DG_SIGN_DIR 'signed'
        $signed = if (Test-Path -LiteralPath $signedDir) {
            Get-ChildItem -LiteralPath $signedDir -Recurse -File -Filter $key | Select-Object -First 1
        }
        if ($signed) {
            Copy-Item -LiteralPath $signed.FullName -Destination $Path -Force
            if (-not (Test-Signed $Path)) { throw "The signed copy of $name ($key) does not have a valid signature." }
            Write-Host "sign: replaced with the signed copy: $name"
        }
        else {
            $out = Join-Path $env:DG_SIGN_DIR 'unsigned'
            New-Item -ItemType Directory -Force -Path $out | Out-Null
            Copy-Item -LiteralPath $Path -Destination (Join-Path $out $key) -Force
            Add-Content -LiteralPath (Join-Path $env:DG_SIGN_DIR 'unsigned.txt') -Value "$key`t$name" -Encoding UTF8
            Write-Host "sign: queued for the next signing request: $name ($key)"
        }
    }

    default { throw "Unknown DG_SIGN_MODE '$mode' (expected signtool or signpath)." }
}
