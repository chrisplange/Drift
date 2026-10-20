# Build Drift's Windows downloads into dist\ :
#
#   Drift-<version>-windows-x64.zip         the program alone, to unpack and run
#   Drift-<version>-windows-x64-setup.exe   the installer (needs Inno Setup 6 or 7)
#
#   powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1
#
# Without Inno Setup the zip is still made and the script says how to get the installer.
# Neither file is code-signed, so Windows SmartScreen warns the first time each is run.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# The version, from the workspace manifest.
$manifest = Get-Content (Join-Path $root 'Cargo.toml') -Raw
if ($manifest -notmatch '(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"') {
    throw 'no version found in Cargo.toml'
}
$version = $Matches[1]
Write-Host "Packaging Drift $version"

cargo build --release --bin drift
if ($LASTEXITCODE -ne 0) { throw 'the release build failed' }
$exe = Join-Path $root 'target\release\drift.exe'

$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null

# The portable zip.
$zip = Join-Path $dist "Drift-$version-windows-x64.zip"
$stage = Join-Path $dist 'stage\Drift'
if (Test-Path (Join-Path $dist 'stage')) { Remove-Item -Recurse -Force (Join-Path $dist 'stage') }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item $exe, (Join-Path $root 'LICENSE.md'), (Join-Path $root 'THIRD-PARTY-NOTICES.txt') $stage
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $stage -DestinationPath $zip
Remove-Item -Recurse -Force (Join-Path $dist 'stage')
Write-Host "wrote $zip"

# The installer.
$iscc = @(
    (Get-Command iscc -ErrorAction SilentlyContinue | ForEach-Object Source)
    foreach ($major in 7, 6) {
        "$env:ProgramFiles\Inno Setup $major\ISCC.exe"
        "${env:ProgramFiles(x86)}\Inno Setup $major\ISCC.exe"
        "$env:LOCALAPPDATA\Programs\Inno Setup $major\ISCC.exe"
    }
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) {
    Write-Warning 'Inno Setup (6 or 7) was not found, so no installer was built.'
    Write-Warning 'Install it (winget install JRSoftware.InnoSetup) and run this script again.'
    exit 0
}
& $iscc "/DAppVersion=$version" /Qp (Join-Path $root 'installer\windows\drift.iss')
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup failed' }
Write-Host "wrote $(Join-Path $dist "Drift-$version-windows-x64-setup.exe")"
