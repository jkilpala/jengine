# Day to day with the Pi.
#
#   ./crates/jengine-server/pi/pi.ps1 status                  every service, its version, players, uptime
#   ./crates/jengine-server/pi/pi.ps1 logs my-game [200]      a server's output
#   ./crates/jengine-server/pi/pi.ps1 restart|stop|start|rollback my-game
#   ./crates/jengine-server/pi/pi.ps1 console                 the Pi's console in your browser, signed in
#   ./crates/jengine-server/pi/pi.ps1 open                    the list players see (http://<pi>:9000/)
#   ./crates/jengine-server/pi/pi.ps1 host-log [100]          the host tool's own log (systemd journal)
#   ./crates/jengine-server/pi/pi.ps1 ssh                     a shell on the Pi as jengine
#   ./crates/jengine-server/pi/pi.ps1 tokens                  where the tokens are, and the console's
#   ./crates/jengine-server/pi/pi.ps1 liveops                 what a game is built with to use the Pi's live ops
#   ./crates/jengine-server/pi/pi.ps1 publish <build> [-Channel dev]
#                                                             a game build's asset packs, as a live ops release
#   ./crates/jengine-server/pi/pi.ps1 backup [folder]         the live ops signing key and the tokens, copied
#                                                             to a folder (default ~/jengine-pi-backup)
param(
    [Parameter(Position = 0)][string]$Command = "status",
    [Parameter(Position = 1)][string]$Name,
    [Parameter(Position = 2)][int]$Lines = 100,
    [string]$Channel = "dev"
)
# Native tools (ssh, scp, cargo) write to stderr, which Windows PowerShell
# turns into a terminating error under "Stop": go by exit codes instead.
$ErrorActionPreference = "Continue"
. "$PSScriptRoot/common.ps1"
$address = Get-PiAddress

switch ($Command) {
    "status" {
        Use-PiTokens | Out-Null
        Invoke-Fleet status
    }
    "logs" {
        if (-not $Name) { throw "logs <service>" }
        Use-PiTokens | Out-Null
        Invoke-Fleet logs $Name --machine $PiMachine --lines $Lines
    }
    { $_ -in "restart", "stop", "start", "rollback" } {
        if (-not $Name) { throw "$Command <service>" }
        Use-PiTokens | Out-Null
        Invoke-Fleet $Command $Name
    }
    "console" {
        # The token rides in the fragment, which a browser never sends; the
        # page keeps it for the tab and takes it out of the address.
        $s = Get-PiSecrets
        Start-Process ("http://{0}:9000/manage#token={1}" -f $address, [Uri]::EscapeDataString($s.console_token))
    }
    "open" { Start-Process "http://${address}:9000/" }
    "host-log" {
        Invoke-Pi "XDG_RUNTIME_DIR=/run/user/`$(id -u) journalctl --user -u jengine-host --no-pager -n $Lines"
    }
    "ssh" { ssh "$script:PiUser@$address" }
    "liveops" {
        # The service's own answer through the gateway, as a game will ask.
        $url = "http://${address}:9000/liveops"
        try { $key = (Invoke-WebRequest -UseBasicParsing "$url/liveops/v1/key").Content.Trim() }
        catch { throw "the Pi's live ops service does not answer at $url - deploy it: deploy-pi.ps1 -Services liveops" }
        Write-Host "A game uses the Pi's live ops with, in project.jengine's settings:"
        Write-Host ""
        Write-Host ('  "liveops": { "url": "' + $url + '", "public_key": ' + $(if ($key.StartsWith('"') -or $key.StartsWith("{")) { $key } else { '"' + $key + '"' }) + ' }')
        Write-Host ""
        Write-Host "Edit config, experiments, events and releases: pi.ps1 console -> Server dashboards -> host / liveops -> Live ops"
    }
    "publish" {
        if (-not $Name) { throw "publish <game build folder> [-Channel dev|staging|live]" }
        $s = Get-PiSecrets
        $build = (Resolve-Path $Name).Path
        Push-Location $RepoRoot
        try {
            cargo run --release -q -p jengine-builder --bin jengine -- liveops publish $build --server "http://${address}:9000/liveops" --channel $Channel --token $s.admin_token
        } finally { Pop-Location }
    }
    "backup" {
        # What cannot be made again: the key every game is built to trust
        # (a new one means rebuilding every game), and the tokens. Both are
        # secrets - keep the folder somewhere private.
        $dir = if ($Name) { $Name } else { Join-Path $HOME "jengine-pi-backup" }
        New-Item -ItemType Directory -Force $dir | Out-Null
        $stamp = Get-Date -Format "yyyy-MM-dd-HHmm"
        $key = Join-Path $dir "liveops-signing-$stamp.key"
        scp -q -o BatchMode=yes "$script:PiUser@${address}:jengine/data/services/liveops/liveops/signing.key" $key
        if ($LASTEXITCODE -ne 0) { throw "could not copy the live ops key (is the liveops service deployed?)" }
        Copy-Item $SecretsFile (Join-Path $dir "secrets-$stamp.json")
        Copy-Item (Join-Path $PiDir "pi-cert.pem") (Join-Path $dir "pi-cert-$stamp.pem")
        Write-Host "backed up to $dir :"
        Get-ChildItem $dir -Filter "*$stamp*" | ForEach-Object { Write-Host ("  {0}  ({1} bytes)" -f $_.Name, $_.Length) }
        Write-Host "To put the key back on a new Pi: deploy liveops once, then"
        Write-Host "  scp <key> jengine@${address}:jengine/data/services/liveops/liveops/signing.key"
        Write-Host "  ./crates/jengine-server/pi/pi.ps1 restart liveops"
    }
    "tokens" {
        $s = Get-PiSecrets
        Write-Host "tokens: $SecretsFile"
        Write-Host "console token (to sign in at http://${address}:9000/manage): $($s.console_token)"
    }
    default { throw "unknown command '$Command' - see the top of pi.ps1" }
}
