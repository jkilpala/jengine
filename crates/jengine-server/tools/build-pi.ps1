# Build the server side for a Raspberry Pi (64-bit Raspberry Pi OS / Ubuntu):
# jengine-fleet (the host tool), the stock jengine-server, and the game
# servers, cross-compiled from this machine with cargo-zigbuild - zig is the
# C compiler SQLite and ring need, so no Linux toolchain or WSL is involved.
#
#   ./crates/jengine-server/tools/build-pi.ps1                         # the fleet file's servers
#   ./crates/jengine-server/tools/build-pi.ps1 -Servers my-game-server # just these packages
#   ./crates/jengine-server/tools/build-pi.ps1 -Servers @()            # only jengine-fleet and jengine-server
#
# With no -Servers, the game servers are the workspace packages named by the
# `linux-aarch64` artifacts in the Pi's fleet file (pi/fleet.json, or the file
# JENGINE_FLEET_FILE names) - an artifact's file name is its package.
#
# Output: target/aarch64-unknown-linux-gnu/release/{jengine-fleet, jengine-server, <servers>}.
# glibc 2.31 is asked for, so the programs run on Raspberry Pi OS 11 and later.
param(
    [string[]]$Servers,
    [string]$Glibc = "2.31"
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot/../../.."
$target = "aarch64-unknown-linux-gnu"

# Built anyway, below; not built twice.
$always = @("jengine-fleet", "jengine-server")
if (-not $PSBoundParameters.ContainsKey("Servers")) {
    $fleetFile = if ($env:JENGINE_FLEET_FILE) { $env:JENGINE_FLEET_FILE } else { Join-Path $PSScriptRoot "../pi/fleet.json" }
    $Servers = @()
    if (Test-Path $fleetFile) {
        $fleet = Get-Content -Raw -Encoding UTF8 $fleetFile | ConvertFrom-Json
        foreach ($s in $fleet.services.PSObject.Properties) {
            $artifact = $s.Value.artifacts.'linux-aarch64'
            if ($artifact) { $Servers += Split-Path -Leaf $artifact }
        }
        Write-Host "servers named by $fleetFile : $(if ($Servers) { ($Servers | Sort-Object -Unique) -join ', ' } else { 'none' })"
    } else {
        Write-Host "no fleet file at $fleetFile - building jengine-fleet and jengine-server only"
    }
}
$Servers = @($Servers | Where-Object { $_ -and ($always -notcontains $_) } | Sort-Object -Unique)

if (-not ((rustup target list --installed) -join " ").Contains($target)) {
    rustup target add $target
}
if (-not (Get-Command cargo-zigbuild -ErrorAction SilentlyContinue)) {
    Write-Host "installing cargo-zigbuild"
    cargo install --locked cargo-zigbuild
}
# cargo-zigbuild finds zig on PATH, or as the `ziglang` Python package.
if (-not (Get-Command zig -ErrorAction SilentlyContinue)) {
    python -c "import ziglang" 2>$null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "installing zig (pip install ziglang)"
        python -m pip install --user ziglang
    }
    # cargo-zigbuild asks `python3 -m ziglang`, and on Windows `python3` is
    # usually the Store's stub: point it at the package's zig.exe instead.
    $zig = python -c "import ziglang, os; print(os.path.join(os.path.dirname(ziglang.__file__), 'zig.exe'))"
    $env:CARGO_ZIGBUILD_ZIG_PATH = $zig
}

Push-Location $root
# Cargo writes its progress to stderr, which Windows PowerShell turns into a
# terminating error under "Stop" once output is redirected: go by exit codes.
$ErrorActionPreference = "Continue"
try {
    cargo zigbuild --release --target "$target.$Glibc" -p jengine-server --bins
    if ($LASTEXITCODE -ne 0) { throw "jengine-server did not build" }
    if ($Servers.Count -gt 0) {
        $packages = $Servers | ForEach-Object { "-p"; $_ }
        cargo zigbuild --release --target "$target.$Glibc" @packages
        if ($LASTEXITCODE -ne 0) { throw "the game servers did not build" }
    }
} finally {
    Pop-Location
}

$out = Join-Path $root "target/$target/release"
# CARGO_TARGET_DIR moves cargo's output (as build-editor.ps1 allows), but fleet.json's
# artifacts and setup-pi.ps1 name <repo>/target/...: the Pi builds are copied there.
$built = Join-Path $(if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root "target" }) "$target/release"
if ([IO.Path]::GetFullPath($built).TrimEnd('\', '/') -ne [IO.Path]::GetFullPath($out).TrimEnd('\', '/')) {
    New-Item -ItemType Directory -Force $out | Out-Null
    foreach ($exe in $always + $Servers) {
        $file = Join-Path $built $exe
        if (Test-Path $file) { Copy-Item $file $out -Force }
    }
}
Write-Host ""
Write-Host "Built for the Pi, in $out :"
foreach ($exe in $always + $Servers) {
    $file = Join-Path $out $exe
    if (Test-Path $file) {
        "{0,-22} {1,6:N1} MB" -f $exe, ((Get-Item $file).Length / 1MB) | Write-Host
    }
}
Write-Host ""
Write-Host "Install or upgrade the host: crates/jengine-server/pi/setup-pi.ps1"
Write-Host "Deploy the servers:         crates/jengine-server/pi/deploy-pi.ps1 -NoBuild"
