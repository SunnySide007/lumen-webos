<#
.SYNOPSIS
  Runs the Lumen shell on this Windows PC, in a window, as a simulator of the TV.

.DESCRIPTION
  The shell loads SDL2 at run time. The first run downloads the official SDL2
  release for Windows (pinned version and checksum) into third_party/, which
  git ignores. The UI is laid out for 1920x1080 and letterboxed into the window.

.EXAMPLE
  pwsh tools/sim.ps1
  pwsh tools/sim.ps1 -ExitAfter 5      # smoke test: run five seconds, then quit
  pwsh tools/sim.ps1 -ExitAfter 3 -Screenshot frame.png
#>
param(
    [double]$ExitAfter = 0,
    [string]$Screenshot = '',
    [switch]$Release
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

$sdlVersion = '2.32.10'
$sdlSha256 = '6cf9706eefd0a4a06dc764007934d428afaf029fabdd408a9e646048c91e18fb'
$sdlDir = Join-Path $root 'third_party/sdl2-windows'
$sdlDll = Join-Path $sdlDir 'SDL2.dll'

if (-not (Test-Path $sdlDll)) {
    $zipName = "SDL2-$sdlVersion-win32-x64.zip"
    $url = "https://github.com/libsdl-org/SDL/releases/download/release-$sdlVersion/$zipName"
    New-Item -ItemType Directory -Force $sdlDir | Out-Null
    $zip = Join-Path $sdlDir $zipName
    Write-Host "Downloading $zipName from libsdl-org..."
    Invoke-WebRequest -Uri $url -OutFile $zip
    $actual = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
    if ($actual -ne $sdlSha256) {
        Remove-Item $zip
        throw "SDL2 checksum mismatch: expected $sdlSha256, got $actual"
    }
    Expand-Archive -Path $zip -DestinationPath $sdlDir -Force
    Remove-Item $zip
}

$env:LUMEN_SDL2 = $sdlDll
$env:LUMEN_LOG = Join-Path $root 'shell/target-host/lumen-shell.log'
$env:CARGO_TARGET_DIR = Join-Path $root 'shell/target-host'

$cargoArgs = @('run', '--manifest-path', (Join-Path $root 'shell/Cargo.toml'))
if ($Release) { $cargoArgs += '--release' }
$cargoArgs += @('--', '--windowed')
if ($ExitAfter -gt 0) { $cargoArgs += @('--exit-after', "$ExitAfter") }
if ($Screenshot) { $cargoArgs += @('--screenshot', [System.IO.Path]::GetFullPath($Screenshot)) }

& cargo @cargoArgs
exit $LASTEXITCODE
