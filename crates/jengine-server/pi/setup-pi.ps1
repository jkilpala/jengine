# Install - or upgrade - the game server host on the Raspberry Pi.
#
#   ./crates/jengine-server/pi/setup-pi.ps1
#   ./crates/jengine-server/pi/setup-pi.ps1 -Address 203.0.113.10     # first run, or the Pi moved
#   ./crates/jengine-server/pi/setup-pi.ps1 -TrustProxy 198.51.100.7  # a TLS proxy in front of the gateway
#
# The fleet file (fleet.json beside this script, copied from
# fleet.example.json - or JENGINE_FLEET_FILE) names the Pi. -Address writes a
# new address into it, the example's <pi-address> placeholder included.
#
# -TrustProxy names the TLS proxy (nginx, say, serving wss:// for a web
# build) whose X-Forwarded-For the host believes, so servers tell players
# apart by their own addresses rather than all being the proxy's. Without it:
# JENGINE_TRUST_PROXY, else the `pi` machine's "trust_proxy" in the fleet
# file, else whatever the host was installed with last time; `none` clears it.
#
# Before the first run, once, on the Pi (it needs your sudo password):
#   scp crates/jengine-server/pi/prepare-pi.sh <you>@<pi>:   then on the Pi:
#   sudo bash prepare-pi.sh        (makes the `jengine` account the servers run as)
#
# What it does, as `jengine` over SSH - no sudo:
#   1. builds jengine-fleet for the Pi (64-bit ARM)
#   2. makes the tokens, first time only (secrets.json, gitignored)
#   3. copies the tool, the tokens and install-host.sh over and runs it:
#      a systemd user service `jengine-host` - the agent on 9900 (HTTPS),
#      the gateway on 9000, the console at /manage - (re)started
#   4. fetches the agent's certificate into pi-cert.pem, which the fleet
#      file pins: nothing else is trusted as the Pi
#   5. checks the agent answers from here
# Run it again to upgrade the host tool; the game servers stop cleanly and
# come back.
param(
    [string]$Address,
    [string]$TrustProxy,
    [string]$User = "jengine"
)
# Native tools (ssh, scp, cargo) write to stderr, which Windows PowerShell
# turns into a terminating error under "Stop": go by exit codes instead.
$ErrorActionPreference = "Continue"
. "$PSScriptRoot/common.ps1"
$script:PiUser = $User

if ($Address -and $Address -ne (Get-PiUrlHost)) {
    # The Pi has a new address: the fleet file follows it (the certificate
    # is remade for it on the Pi, step 3).
    $text = Get-Content -Raw -Encoding UTF8 $FleetFile
    $text = $text.Replace((Get-PiUrlHost), $Address)
    [IO.File]::WriteAllText($FleetFile, $text)
    Write-Host "$FleetFile now names the Pi at $Address"
}
$address = Get-PiAddress
$trust = Get-TrustProxy $TrustProxy
if ($trust -and $trust -ne "none" -and $trust -notmatch '^[0-9A-Fa-f:.,/]+$') {
    throw "-TrustProxy: '$trust' is not a list of addresses (198.51.100.7,10.0.0.2)"
}
Write-Host "== the Pi at $address, as $User"

# 0. Can we get in as `jengine`?
ssh -o BatchMode=yes -o ConnectTimeout=10 -o StrictHostKeyChecking=accept-new "$User@$address" "true"
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Cannot sign in to $User@$address with an SSH key. Once, from here:"
    Write-Host "  scp crates/jengine-server/pi/prepare-pi.sh <you>@${address}:"
    Write-Host "  ssh <you>@$address"
    Write-Host "  sudo bash prepare-pi.sh"
    exit 1
}

# 1. The host tool, for the Pi.
Write-Host "== building jengine-fleet for the Pi"
& "$RepoRoot/crates/jengine-server/tools/build-pi.ps1" -Servers @() | Out-Host
$tool = Join-Path $RepoRoot "target/aarch64-unknown-linux-gnu/release/jengine-fleet"
if (-not (Test-Path $tool)) { throw "no $tool" }

# 2. The tokens.
$secrets = Get-PiSecrets

# 3. Over to the Pi, and installed. host.env with Unix line ends: systemd
#    would keep a carriage return as part of the token.
$envFile = Join-Path ([IO.Path]::GetTempPath()) "jengine-host-$PID.env"
[IO.File]::WriteAllText($envFile, "JENGINE_FLEET_TOKEN=$($secrets.fleet_token)`nJENGINE_CONSOLE_TOKEN=$($secrets.console_token)`nRUST_LOG=info`n")
try {
    Write-Host "== copying to the Pi"
    Invoke-Pi "mkdir -p ~/jengine/incoming"
    scp -q -o BatchMode=yes $tool $envFile "$User@${address}:jengine/incoming/"
    if ($LASTEXITCODE -ne 0) { throw "copying to the Pi failed" }
    Invoke-Pi "mv ~/jengine/incoming/$(Split-Path -Leaf $envFile) ~/jengine/incoming/host.env"
    scp -q -o BatchMode=yes "$PiDir/install-host.sh" "$User@${address}:jengine/install-host.sh"
} finally {
    Remove-Item $envFile -ErrorAction SilentlyContinue
}
Write-Host "== installing on the Pi"
Invoke-Pi "bash ~/jengine/install-host.sh '$address' '$trust'" | Out-Host
if ($LASTEXITCODE -ne 0) { throw "install-host.sh failed on the Pi" }

# 4. The certificate the fleet file pins.
scp -q -o BatchMode=yes "$User@${address}:jengine/tls/cert.pem" (Join-Path $PiDir "pi-cert.pem")
if ($LASTEXITCODE -ne 0) { throw "could not fetch the Pi's certificate" }

# 5. Does it answer from here?
Use-PiTokens | Out-Null
$ok = $false
for ($i = 0; $i -lt 20 -and -not $ok; $i++) {
    Invoke-Fleet status *> $null
    # status exits 2 when a service is not well, 1 when the machine is unreached.
    if ($LASTEXITCODE -ne 1) { $ok = $true } else { Start-Sleep -Milliseconds 500 }
}
Invoke-Fleet status | Out-Host
if (-not $ok) { throw "the Pi's agent does not answer at https://${address}:9900" }

Write-Host ""
Write-Host "The Pi is a host."
Write-Host "  game servers:  ws://${address}:9000/<service>     (deploy them: ./crates/jengine-server/pi/deploy-pi.ps1)"
Write-Host "  directory:     http://${address}:9000/"
Write-Host "  console:       ./crates/jengine-server/pi/pi.ps1 console   (signs you in)"
