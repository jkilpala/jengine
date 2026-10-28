<#
.SYNOPSIS
  Run one scene in the native runtime and save screenshots of what it draws.

.DESCRIPTION
  The way to *see* a scene without the editor. Starts the workspace's
  release runtime (build it once: cargo build --release -p jengine-runtime)
  with this project as its working directory and the runtime's JENGINE_SHOT
  hook set, which takes screenshots of the presented frame through the
  engine's own screenshot path and quits. Saves and screenshots go to a temp
  dir, never the repo. Prints any error lines from the runtime's log - read
  them: a script error is not visible in a picture of a title screen.

  Never inject clicks or keys: someone may be using this machine. To see a
  state past the title screen, set an environment variable the scene's
  script reads through a test hook (docs/making-games.md, "Seeing it").

.EXAMPLE
  ./tools/capture.ps1 -Scene hub -Out $env:TEMP\hub.png
  ./tools/capture.ps1 -Scene 01-lantern-square -At 1,2,3 -Out $env:TEMP\square.png   # square-0.png ...
#>
param(
    [Parameter(Mandatory)] [string] $Scene,
    [Parameter(Mandatory)] [string] $Out,
    [double[]] $At = @(2.0),
    [hashtable] $Env = @{},
    [int] $Timeout = 60
)

$ErrorActionPreference = 'Stop'
$project = Split-Path -Parent $PSScriptRoot
$repo = Resolve-Path (Join-Path $project '..\..')
# The workspace's release runtime: in CARGO_TARGET_DIR when one is set.
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $repo 'target' }
$exe = Join-Path $target 'release\jengine-runtime.exe'
if (-not (Test-Path $exe)) { throw "build it first: cargo build --release -p jengine-runtime" }

$outDir = Split-Path -Parent $Out
if ($outDir -and -not (Test-Path $outDir)) { New-Item -ItemType Directory -Force $outDir | Out-Null }

$saves = Join-Path $env:TEMP ((Split-Path -Leaf $project) + "-shot-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force $saves | Out-Null
$name = [System.IO.Path]::GetFileNameWithoutExtension($Out)
$psi = New-Object System.Diagnostics.ProcessStartInfo $exe
$psi.WorkingDirectory = $project
$psi.Arguments = "--scene assets/scenes/$Scene.scene"
$psi.UseShellExecute = $false
$psi.RedirectStandardError = $true
$psi.RedirectStandardOutput = $true
$psi.EnvironmentVariables['JENGINE_SAVE_DIR'] = $saves
$psi.EnvironmentVariables['JENGINE_SHOT'] = $name
$psi.EnvironmentVariables['JENGINE_SHOT_AFTER'] = ($At | ForEach-Object { $_.ToString([cultureinfo]::InvariantCulture) }) -join ','
foreach ($k in $Env.Keys) { $psi.EnvironmentVariables[$k] = [string]$Env[$k] }
$p = [System.Diagnostics.Process]::Start($psi)
$errTask = $p.StandardError.ReadToEndAsync()
$outTask = $p.StandardOutput.ReadToEndAsync()
if (-not $p.WaitForExit($Timeout * 1000)) { $p.Kill(); Write-Output "timed out after $Timeout s" }
$p.WaitForExit()
$log = $errTask.Result + $outTask.Result
Set-Content -Path "$Out.log" -Value $log -Encoding utf8

$dir = Split-Path -Parent $Out
if ($dir -eq '') { $dir = '.' }
Get-ChildItem (Join-Path $saves 'screenshots') -Filter '*.png' -ErrorAction SilentlyContinue | ForEach-Object {
    Copy-Item $_.FullName (Join-Path $dir $_.Name) -Force
    Write-Output ("saved " + (Join-Path $dir $_.Name))
}
Remove-Item -Recurse -Force $saves -ErrorAction SilentlyContinue

# Errors, and the engine's own warnings - a refused setter (`Line.set: 'points': ...`)
# is a WARN from jengine_core, and it is as much a bug as an error. The tick-rate
# notice is not.
$bad = ($log -split "`n") | Where-Object {
    ($_ -match ' ERROR |panicked|attempt to |\[Script\].*error|Lua error|runtime error' -or $_ -match 'WARN  jengine_core') -and
    $_ -notmatch 'tick-rate metadata'
} | Select-Object -First 25
if ($bad) { Write-Output "--- errors in the runtime log ($Out.log) ---"; $bad | ForEach-Object { Write-Output $_.TrimEnd() } }
