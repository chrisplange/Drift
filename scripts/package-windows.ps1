# Build Drift's Windows downloads into dist\ :
#
#   Drift-<version>-windows-x64.zip         the program alone, to unpack and run
#   Drift-<version>-windows-x64-setup.exe   the installer (needs Inno Setup 6 or 7)
#
# Both hold drift.exe and drift-mcp.exe, the MCP server AI assistants use
# (docs/guide/ai/assistants.md).
#
#   powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1            a test copy
#   powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1 -Release   a release
#
# A test copy is labelled with the commit it was built from, e.g. 0.1.0-beta.1.5+test.9490d05
# (".modified" is added when there are uncommitted changes). Drift shows that version in
# Help > About, and its updater offers only releases after the one it was built from. The
# Package workflow passes -Release, which keeps the plain version from Cargo.toml.
#
# Without Inno Setup the zip is still made and the script says how to get the installer.
# Neither file is code-signed, so Windows SmartScreen warns the first time each is run.

param([switch]$Release)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# The version, from the workspace manifest.
$manifest = Get-Content (Join-Path $root 'Cargo.toml') -Raw
if ($manifest -notmatch '(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"') {
    throw 'no version found in Cargo.toml'
}
$version = $Matches[1]
if (-not $Release) {
    $commit = (git rev-parse --short HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or -not $commit) { throw 'no git commit to label the test copy with' }
    $version = "$version+test.$commit"
    if (git status --porcelain --untracked-files=no) { $version = "$version.modified" }
}
Write-Host "Packaging Drift $version"

# The app reads DRIFT_VERSION when it is built (drift_platform::VERSION). It is set only for
# this build, so later plain builds go back to the crate version.
$was = $env:DRIFT_VERSION
try {
    if ($Release) { Remove-Item Env:DRIFT_VERSION -ErrorAction SilentlyContinue } else { $env:DRIFT_VERSION = $version }
    cargo build --release --bin drift
    if ($LASTEXITCODE -ne 0) { throw 'the release build failed' }
} finally {
    if ($null -eq $was) { Remove-Item Env:DRIFT_VERSION -ErrorAction SilentlyContinue } else { $env:DRIFT_VERSION = $was }
}
# A separate build, so drift-mcp's dependencies can't change drift.exe's features.
cargo build --release --bin drift-mcp
if ($LASTEXITCODE -ne 0) { throw 'the drift-mcp release build failed' }
$exe = Join-Path $root 'target\release\drift.exe'
$mcp = Join-Path $root 'target\release\drift-mcp.exe'
& $mcp --version
if ($LASTEXITCODE -ne 0) { throw 'drift-mcp.exe does not run' }

$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null

# The portable zip.
$zip = Join-Path $dist "Drift-$version-windows-x64.zip"
$stage = Join-Path $dist 'stage\Drift'
if (Test-Path (Join-Path $dist 'stage')) { Remove-Item -Recurse -Force (Join-Path $dist 'stage') }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item $exe, $mcp, (Join-Path $root 'LICENSE.md'), (Join-Path $root 'THIRD-PARTY-NOTICES.txt') $stage
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
