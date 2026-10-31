<#
.SYNOPSIS
  Runs the Lumen shell on this Windows PC, in a window, as a simulator of the TV.

.DESCRIPTION
  The shell loads SDL2 at run time. The first run downloads the official SDL2
  release for Windows (pinned version and checksum) into third_party/, which
  git ignores. The UI is laid out for 1920x1080 and letterboxed into the window.

  For automated checks, -Keys plays remote keys (U, D, L, R, OK, BACK),
  -FixedDt makes the clock deterministic, and -ScreenshotWhenSettled saves
  the frame once the keys are done and nothing moves, then quits.
  -ScreenshotAt moves -Screenshot's frame from 1.5 s to the given time on
  the loop's clock: the first key goes down at 0.5 s, so with -FixedDt
  -Screenshot frame.png -ScreenshotAt 0.62 -Keys R catches a tile leaning.

  The Top Shelf heroes come from shell/assets/art, the owner's from
  shell/assets/art-owner. For screenshots of a given hero, -First moves the
  mock apps it names to the front, onto row 0, and -Colors gives mock apps a
  webOS icon colour, as "id=rrggbb,id=rrggbb".

.EXAMPLE
  pwsh tools/sim.ps1
  pwsh tools/sim.ps1 -ExitAfter 5      # smoke test: run five seconds, then quit
  pwsh tools/sim.ps1 -ExitAfter 3 -Screenshot frame.png
  pwsh tools/sim.ps1 -Keys "R,R,D" -FixedDt 0.016667 -ScreenshotWhenSettled row1.png
  pwsh tools/sim.ps1 -First com.webos.app.hdmi1 -Colors "spotify-beehive=1db954"
#>
param(
    [double]$ExitAfter = 0,
    [string]$Screenshot = '',
    [double]$ScreenshotAt = -1,
    [string]$Keys = '',
    [double]$FixedDt = 0,
    [string]$ScreenshotWhenSettled = '',
    [string]$First = '',
    [string]$Colors = '',
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
$env:LUMEN_MOCK_ICONS = Join-Path $root 'shell/assets/mock-icons'
$env:LUMEN_ART = Join-Path $root 'shell/assets/art'
$env:LUMEN_ART_OWNER = Join-Path $root 'shell/assets/art-owner'
# Empty clears them: the session keeps what an earlier run set.
$env:LUMEN_MOCK_FIRST = $First
$env:LUMEN_MOCK_COLORS = $Colors
$env:CARGO_TARGET_DIR = Join-Path $root 'shell/target-host'

$cargoArgs = @('run', '--manifest-path', (Join-Path $root 'shell/Cargo.toml'))
if ($Release) { $cargoArgs += '--release' }
$cargoArgs += @('--', '--windowed')
# An automated run that never settles must still end.
if ($ScreenshotWhenSettled -and $ExitAfter -le 0) { $ExitAfter = 30 }
if ($ExitAfter -gt 0) { $cargoArgs += @('--exit-after', "$ExitAfter") }
if ($Screenshot) { $cargoArgs += @('--screenshot', [System.IO.Path]::GetFullPath($Screenshot)) }
if ($ScreenshotAt -ge 0) { $cargoArgs += @('--screenshot-at', "$ScreenshotAt") }
if ($Keys) { $cargoArgs += @('--keys', $Keys) }
if ($FixedDt -gt 0) { $cargoArgs += @('--fixed-dt', "$FixedDt") }
if ($ScreenshotWhenSettled) {
    $cargoArgs += @('--screenshot-when-settled', [System.IO.Path]::GetFullPath($ScreenshotWhenSettled))
}

& cargo @cargoArgs
exit $LASTEXITCODE
