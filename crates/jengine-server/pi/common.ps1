# Shared by setup-pi.ps1, deploy-pi.ps1 and pi.ps1 (dot-sourced).
#
# The fleet file is the one place that names the Pi: its address is read from
# the `pi` machine's url there. It is `fleet.json` beside these scripts (copy
# fleet.example.json to start one; it is gitignored, being yours), or the file
# JENGINE_FLEET_FILE names - e.g. one kept outside the repository, or one whose
# machine and console URLs are SSH tunnels, so these scripts work from outside
# the LAN. That file's paths are absolute or relative to it.
#
# secrets.json (beside these scripts, gitignored) holds the tokens setup-pi.ps1
# made: the agent's, the console's and the servers' admin token.

$PiDir = $PSScriptRoot
$RepoRoot = (Resolve-Path "$PiDir/../../..").Path
$FleetFile = if ($env:JENGINE_FLEET_FILE) { $env:JENGINE_FLEET_FILE } else { Join-Path $PiDir "fleet.json" }
$SecretsFile = Join-Path $PiDir "secrets.json"
$PiMachine = "pi"

function Read-Fleet {
    if (-not (Test-Path $FleetFile)) {
        throw "no fleet file at $FleetFile - copy fleet.example.json to fleet.json beside these scripts and put your Pi's address in it (or set JENGINE_FLEET_FILE)"
    }
    Get-Content -Raw -Encoding UTF8 $FleetFile | ConvertFrom-Json
}

# The host part of the Pi's url in the fleet file, as written - which may
# still be the example's <pi-address>.
function Get-PiUrlHost {
    $url = (Read-Fleet).machines.$PiMachine.url
    if (-not $url) { throw "$FleetFile has no machine '$PiMachine'" }
    if ($url -notmatch '^[a-z]+://(\[[^\]]+\]|[^:/]+)') { throw "$FleetFile`: '$url' is not a url" }
    $Matches[1]
}

# The Pi's address, from the machine's url in the fleet file.
function Get-PiAddress {
    $address = Get-PiUrlHost
    if ($address -match '[<>]') {
        throw "$FleetFile still names the Pi as $address - put its address there, or run setup-pi.ps1 -Address <the Pi's address>"
    }
    $address
}

# The TLS proxy (or proxies, comma-separated) in front of the Pi's gateway
# whose X-Forwarded-For the host believes: -TrustProxy, else
# JENGINE_TRUST_PROXY, else the `pi` machine's "trust_proxy" in the fleet
# file (read by these scripts only). Empty when none is named.
function Get-TrustProxy([string]$given) {
    if ($given) { return $given }
    if ($env:JENGINE_TRUST_PROXY) { return $env:JENGINE_TRUST_PROXY }
    $fromFleet = (Read-Fleet).machines.$PiMachine.trust_proxy
    if ($fromFleet) { return [string]$fromFleet }
    ""
}

function New-Token {
    $bytes = New-Object byte[] 24
    [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
    -join ($bytes | ForEach-Object { $_.ToString("x2") })
}

# The tokens, made the first time they are asked for.
function Get-PiSecrets {
    if (Test-Path $SecretsFile) {
        return Get-Content -Raw $SecretsFile | ConvertFrom-Json
    }
    $secrets = [ordered]@{
        fleet_token   = New-Token   # the agent: uploads and runs programs on the Pi
        console_token = New-Token   # the console at http://<pi>:9000/manage
        admin_token   = New-Token   # each game server's dashboard (the console adds it for you)
    }
    [IO.File]::WriteAllText($SecretsFile, ($secrets | ConvertTo-Json))
    Write-Host "made new tokens in $SecretsFile (gitignored - keep it)"
    return [pscustomobject]$secrets
}

# What the fleet file names its tokens by.
function Use-PiTokens {
    $s = Get-PiSecrets
    $env:PI_FLEET_TOKEN = $s.fleet_token
    $env:PI_ADMIN_TOKEN = $s.admin_token
    $s
}

# jengine-fleet for this machine, built (or found up to date) in release.
function Get-FleetTool {
    Push-Location $RepoRoot
    $ErrorActionPreference = "Continue"
    try {
        cargo build --release -q -p jengine-server --bin jengine-fleet 2>&1 | Where-Object { $_ -match "error" } | ForEach-Object { Write-Host $_ }
        if ($LASTEXITCODE -ne 0) { throw "jengine-fleet did not build" }
    } finally { Pop-Location }
    Join-Path $RepoRoot "target/release/jengine-fleet.exe"
}

function Invoke-Fleet {
    $tool = Get-FleetTool
    & $tool @args --fleet $FleetFile
}

# A command on the Pi, as the account the servers run as.
function Invoke-Pi([string]$command) {
    ssh -o BatchMode=yes -o ConnectTimeout=10 "$script:PiUser@$(Get-PiAddress)" $command
}
$script:PiUser = "jengine"

# Tell the Pi's console about the services the fleet file deploys, so it can
# restart, stop, roll back and show their dashboards too. It keeps any build
# uploaded through it.
function Sync-Console([string[]]$names) {
    $fleet = Read-Fleet
    $secrets = Get-PiSecrets
    $console = $fleet.machines.$PiMachine.console
    if (-not $console) { return }
    foreach ($name in $names) {
        $def = $fleet.services.$name
        $body = [ordered]@{
            name        = $name
            machines    = @("host")
            artifacts   = @{}
            args        = $def.args
            port        = $def.port
            drain_secs  = $def.drain_secs
            admin_token = $secrets.admin_token
        } | ConvertTo-Json -Depth 5
        try {
            Invoke-RestMethod -Method Put -Uri "$console/api/fleet/service" -ContentType "application/json" -Body $body `
                -Headers @{ Authorization = "Bearer $($secrets.console_token)" } | Out-Null
        } catch {
            Write-Warning "the console did not take '$name': $($_.Exception.Message)"
        }
    }
}
