# Build the game servers for the Pi and deploy them to it.
#
#   ./crates/jengine-server/pi/deploy-pi.ps1                         # every service in the fleet file
#   ./crates/jengine-server/pi/deploy-pi.ps1 -Services my-game       # just these
#   ./crates/jengine-server/pi/deploy-pi.ps1 -NoBuild                # what is already built
#
# Each deploy uploads only a build the Pi does not have yet, drains the
# running server, stops it (its players saved), starts the new one and waits
# for its health check - putting the old one back if it never answers.
# Afterwards the Pi's console is told about each service, so its buttons and
# dashboards work for them too.
param(
    [string[]]$Services,
    [switch]$NoBuild
)
# Native tools (ssh, scp, cargo) write to stderr, which Windows PowerShell
# turns into a terminating error under "Stop": go by exit codes instead.
$ErrorActionPreference = "Continue"
. "$PSScriptRoot/common.ps1"

$fleet = Read-Fleet
$all = @($fleet.services.PSObject.Properties.Name)
if (-not $Services) { $Services = $all }
foreach ($s in $Services) {
    if ($all -notcontains $s) { throw "$FleetFile has no service '$s' (it has: $($all -join ', '))" }
}

if (-not $NoBuild) {
    # The package to build is the artifact's file name: .../release/my-game-server.
    $packages = $Services | ForEach-Object { Split-Path -Leaf $fleet.services.$_.artifacts.'linux-aarch64' } | Sort-Object -Unique
    Write-Host "== building for the Pi: $($packages -join ', ')"
    & "$RepoRoot/crates/jengine-server/tools/build-pi.ps1" -Servers $packages | Out-Host
}

Use-PiTokens | Out-Null
$failed = @()
foreach ($s in $Services) {
    Write-Host "== deploying $s"
    Invoke-Fleet deploy $s | Out-Host
    if ($LASTEXITCODE -ne 0) { $failed += $s }
}
Sync-Console $Services

Write-Host ""
Invoke-Fleet status | Out-Host
$address = Get-PiAddress
Write-Host ""
foreach ($s in $Services) { Write-Host ("  {0,-14} ws://{1}:9000/{0}" -f $s, $address) }
if ($failed) { throw "these did not deploy: $($failed -join ', ')" }
