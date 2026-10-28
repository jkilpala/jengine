param(
    # Pass -Clean to force `cargo clean -p jengine-core` before building. This
    # avoids stale-rlib LNK2019/LNK2001 link errors but forces a full rebuild of
    # jengine-core, which is slow. Omit it for fast incremental builds.
    [switch]$Clean,

    # Pass -DevWasm to build the viewport with `--dev` instead of `--release`.
    # Faster to compile, but the result is UNOPTIMIZED and frame rates are not
    # representative of the real game -- this script used to do it always, which
    # made the editor look far slower than the engine actually is. Use it only
    # when iterating on viewport Rust, and never to judge performance.
    [switch]$DevWasm,

    # Build the viewport with a project's own Rust packages linked, instead of
    # the stock one. Defaults to whatever the existing stamp says it was last
    # built for, so a project viewport SURVIVES a relaunch. Without that this
    # script rebuilt stock every launch and silently threw away the packages,
    # and the editor's "Rebuild viewport" banner came back on every start no
    # matter how many times it was answered.
    [string]$Project,

    # Force the stock viewport, discarding any project packages currently
    # linked in. This is what the script always used to do.
    [switch]$Stock
)

$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
Set-Location $PSScriptRoot

# A pull that adds a package leaves editor/node_modules behind it, and Vite
# then fails in the editor window ("Failed to resolve import ..."). npm keeps
# what it installed in node_modules/.package-lock.json, so reinstall when the
# project's lockfile is newer than that.
$lock = Join-Path $PSScriptRoot "editor/package-lock.json"
$installed = Join-Path $PSScriptRoot "editor/node_modules/.package-lock.json"
if ((Test-Path $lock) -and (-not (Test-Path $installed) -or
    (Get-Item $lock).LastWriteTime -gt (Get-Item $installed).LastWriteTime)) {
    Write-Host "editor/package-lock.json changed since the last install - running npm ci..." -ForegroundColor Cyan
    Push-Location (Join-Path $PSScriptRoot "editor")
    try { npm ci } finally { Pop-Location }
}

# --- Kill any stale Vite / Tauri dev processes from a previous session --------
# On Windows, Tauri does NOT kill the beforeDevCommand (Vite) when the window
# closes. The stale Vite keeps holding port 5173. When cargo tauri dev runs
# again it falls back to 5174, but tauri.conf.json devUrl still points to
# 5173 (stale content) -> white screen. Kill it unconditionally up-front.
# Also kill any lingering jengine-editor.exe so Cargo can overwrite it.
Stop-Process -Name "jengine-editor" -Force -ErrorAction SilentlyContinue
$vitePort = 5173
$stalePids = Get-NetTCPConnection -LocalPort $vitePort -ErrorAction SilentlyContinue |
             Select-Object -ExpandProperty OwningProcess | Sort-Object -Unique
foreach ($p in $stalePids) {
    Write-Host "Killing stale process on port $vitePort (PID $p)..." -ForegroundColor Yellow
    Stop-Process -Id $p -Force -ErrorAction SilentlyContinue
}
# Also kill any lingering node processes running vite (belt-and-suspenders)
Get-Process -Name "node" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -like "*vite*" } |
    ForEach-Object { Write-Host "Killing stale vite node PID $($_.Id)..."; Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue }

# Clear WebView2 cache so stale JS modules never cause a white/broken screen.
$wv2Cache = "$env:LOCALAPPDATA\com.jengine.editor\EBWebView"
if (Test-Path $wv2Cache) {
    Write-Host "Clearing WebView2 cache..." -ForegroundColor Cyan
    Remove-Item -Recurse -Force $wv2Cache
}
# -------------------------------------------------------------------------------

# Workaround: editor builds frequently fail with LNK2019 / LNK2001 unresolved
# external symbol errors against stale `jengine-core` rlibs in target/debug/deps
# (mlua + image_webp symbols, etc.) when `jengine-core` was last built by a
# different consumer (runtime, tests). Force a clean of jengine-core so the
# editor rebuilds it against the current crate-graph fingerprint.
# Only runs when -Clean is passed, since it forces a slow full rebuild.
if ($Clean) {
    Write-Host "Cleaning jengine-core to avoid stale rlib link errors..." -ForegroundColor Cyan
    cargo clean -p jengine-core
} else {
    Write-Host "Skipping jengine-core clean (pass -Clean to force). Using incremental build." -ForegroundColor DarkGray
}

# Which project, if any, should the rebuilt viewport carry the packages of?
#
# The viewport directory is shared across projects and a stamp beside the
# module records who it was last built for. This script rebuilds the viewport
# on every launch so the JS glue matches the current Rust - but it used to
# always rebuild it STOCK, which threw the project packages away every time.
# The visible symptom was the editor's "Rebuild viewport" banner returning on
# every start: you answered it, the build was correct, and the next launch
# quietly undid it.
$stampPath = Join-Path $PSScriptRoot "editor/src/lib/viewport-wasm/.jengine-viewport.json"
$viewportProject = $null
if ($Stock) {
    Write-Host "-Stock: building the stock viewport, dropping any project packages." -ForegroundColor Yellow
} elseif ($Project) {
    $viewportProject = (Resolve-Path $Project).Path
} elseif (Test-Path $stampPath) {
    # A malformed or hand-edited stamp must not stop the editor launching, so
    # anything unreadable here just falls through to a stock build.
    try {
        $stampRaw = [System.IO.File]::ReadAllText($stampPath).TrimStart([char]0xFEFF)
        $stamp = $stampRaw | ConvertFrom-Json
        if ((-not $stamp.stock) -and $stamp.project) {
            if (Test-Path $stamp.project) {
                $viewportProject = $stamp.project
            } else {
                Write-Host "Viewport was built for '$($stamp.project)', which no longer exists - building stock." -ForegroundColor Yellow
            }
        }
    } catch {
        Write-Host "Could not read the viewport stamp ($_) - building stock." -ForegroundColor Yellow
    }
}

if ($viewportProject) {
    # `viewport-build` is release-only, and deliberately: the wrapper crate
    # re-exports the viewport, so a dev build of it carries the same
    # unoptimized cost the -DevWasm note below describes.
    if ($DevWasm) {
        Write-Host "-DevWasm ignored: a project viewport is release-only." -ForegroundColor Yellow
        Write-Host "  Pass -Stock as well if you want a dev stock viewport instead." -ForegroundColor Yellow
    }
    # Loads the project's own cached build when nothing it was compiled from
    # changed, so a relaunch with no Rust edits costs no compile at all.
    Write-Host "Loading editor viewport WASM with the packages of '$viewportProject'..." -ForegroundColor Cyan
    Write-Host "  (pass -Stock for the stock viewport, or -Project <path> for another project)" -ForegroundColor DarkGray
    & (Join-Path $PSScriptRoot "build-project-viewport.ps1") -Project $viewportProject
    if ($LASTEXITCODE -ne 0) {
        Write-Host "project viewport build failed - aborting." -ForegroundColor Red
        exit 1
    }
    # `jengine_builder::viewport::build` writes the stamp itself, so there is
    # nothing to stamp here - and nothing may overwrite it with the stock one.
} else {
    # Rebuild the stock viewport so the JS glue matches the current Rust code.
    # Release by default: a --dev build is roughly 10.5 MB against 4.6 MB
    # release, unoptimized, and every frame of engine work in the viewport
    # (ECS, trigger overlap, the per-frame custom-component hydration) runs
    # through it. Building dev here silently replaced an optimized viewport
    # with a slow one on every launch, which reads as "the game got slower"
    # with no code change to blame.
    if ($DevWasm) {
        Write-Host "Building editor viewport WASM (DEV - unoptimized)..." -ForegroundColor Yellow
        Write-Host "  Frame rates from this build are NOT representative." -ForegroundColor Yellow
        wasm-pack build crates/jengine-editor-viewport --target web --dev --out-dir ../../editor/src/lib/viewport-wasm
    } else {
        # The builder's stock build: wasm-pack, then wasm-opt at the editor's
        # preferred level (Project Settings > Graphics > Viewport build), the
        # cleanup and the stock stamp - one implementation with the editor's
        # own Rebuild viewport.
        Write-Host "Building editor viewport WASM (release)..." -ForegroundColor Cyan
        cargo run --quiet -p jengine-builder --bin jengine -- viewport-build --stock
    }
    if ($LASTEXITCODE -ne 0) {
        Write-Host "viewport WASM build failed - aborting." -ForegroundColor Red
        exit 1
    }
    if ($DevWasm) {
        Remove-Item "editor/src/lib/viewport-wasm/package.json" -ErrorAction SilentlyContinue
        Remove-Item "editor/src/lib/viewport-wasm/.gitignore" -ErrorAction SilentlyContinue
        # Stamp the module as the stock build. The editor decides whether a
        # project's Rust packages are linked by reading this stamp. Rebuilding
        # stock without rewriting it left a project build's stamp beside a module
        # that no longer carried those packages, so the editor trusted a package
        # that silently did nothing. UTF-8 without a BOM: `JSON.parse` rejects one.
        [System.IO.File]::WriteAllText($stampPath, '{ "project": "", "packages": [], "stock": true }', (New-Object System.Text.UTF8Encoding $false))
    }
}

cargo tauri dev
