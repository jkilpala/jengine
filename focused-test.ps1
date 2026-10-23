# The per-change test run: what a feature can break, in a few minutes instead
# of the full workspace + builder + GPU suites' forty.
#
#   ./focused-test.ps1 -Tests lines_are_drawn_with_style,riders_follow_drawn_curves
#
# It checks the native runtime and both browser hosts still compile, then runs
# - in ONE cargo invocation under `gpu-wgpu`, so jengine-core is built once -
# core's unit tests, the cross-runtime guards (the tests that catch a function
# wired in one runtime and not another, a component the scene format or the
# editor lacks, a shipped scene or script that no longer loads; listed below
# and discovered by name), the pixel goldens, and the feature's own tests
# named with -Tests.
#
# It is not the full run. Packages, the builder, the editor's Rust and every
# other test binary are left out; run the full suites every few features and
# before a session ends (see CLAUDE.md, "Checks").
param(
    [string[]]$Tests = @(),
    [switch]$NoGpu
)

$ErrorActionPreference = "Continue"
# `-File` hands a comma list over as one string.
$Tests = @($Tests | ForEach-Object { $_ -split "," } | Where-Object { $_ })
$log = Join-Path $env:TEMP "jengine_focused_test.log"
$guards = @(
    "script_global_parity", "host_command_parity", "preamble_command_tags",
    "script_namespaces_reach_every_runtime", "play_state_reaches_browser_hosts",
    "shipped_scripts_use_namespaces", "spawned_prefabs_keep_their_components",
    "v1_components_are_registered", "every_component_is_editable", "authored_components_exist",
    "render_passes_reach_every_runtime", "motion_reaches_every_runtime",
    "projects_link_the_packages_their_scenes_use", "engine_test_lab_scenes",
    "shipped_shader_graphs_compile", "every_shipped_graph_compiles", "cpu_light_matches_the_shader",
    # A script-surface change that leaves docs/scripting-api.md's generated
    # index behind - it met only the full suite until Oct 2026.
    "script_api_index_is_current",
    # A new post effect the editor's panel knows and this table does not -
    # depth of field met only the full suite (Oct 2026).
    "editor_post_params_match_the_renderer"
)
# And every test whose name says it guards something across the runtimes or
# the shipped assets - discovered, so a new guard joins without editing this
# list (`cpu_light_matches_the_shader` was missed by hand once).
$pattern = "reach|parity|matches|^every_|_keep_|registered|link_|shipped_|authored|preamble"
$found = Get-ChildItem (Join-Path $PSScriptRoot "crates/jengine-core/tests") -Filter *.rs |
    ForEach-Object { $_.BaseName } | Where-Object { $_ -match $pattern }
# And every test that reads a runtime host's source: those are what a change
# to a host breaks, whatever they are called. Seven of them met only the full
# suite when the browser hosts' script mirror moved into core (Oct 2026).
$readsHosts = Get-ChildItem (Join-Path $PSScriptRoot "crates/jengine-core/tests") -Filter *.rs |
    Select-String -Pattern "jengine-editor-viewport/src/|jengine-web/src/|jengine-runtime/src/" -List |
    ForEach-Object { [System.IO.Path]::GetFileNameWithoutExtension($_.Path) }
$guards = @($guards + $found + $readsHosts | Sort-Object -Unique)
if (-not $NoGpu) { $guards += "render_golden" }

Write-Host "== compile: native runtime, editor viewport and web (wasm32)"
cargo check -p jengine-runtime 2>&1 | Out-File $log -Encoding utf8
$native = $LASTEXITCODE
cargo check -p jengine-editor-viewport -p jengine-web --target wasm32-unknown-unknown 2>&1 | Out-File $log -Append -Encoding utf8
$wasm = $LASTEXITCODE
if ($native -ne 0 -or $wasm -ne 0) {
    Select-String -Path $log -Pattern "^error" -Context 0, 6 | Select-Object -First 10
    Write-Host "FAILED to compile (native $native, wasm $wasm) - see $log"
    exit 1
}

$cargoArgs = @("test", "-p", "jengine-core", "-j", "4", "--no-fail-fast", "--lib")
if (-not $NoGpu) { $cargoArgs += @("--features", "gpu-wgpu") }
foreach ($t in ($guards + $Tests)) { $cargoArgs += @("--test", $t) }
Write-Host "== test: core lib, $($guards.Count) guards, $($Tests.Count) named"
& cargo @cargoArgs 2>&1 | Out-File $log -Append -Encoding utf8
$code = $LASTEXITCODE

$results = Select-String -Path $log -Pattern "^test result:"
$failed = Select-String -Path $log -Pattern "^test .* FAILED$|^---- "
$passed = ($results | ForEach-Object { if ($_.Line -match "(\d+) passed") { [int]$Matches[1] } } | Measure-Object -Sum).Sum
Write-Host "$($results.Count) test binaries, $passed tests passed"
if ($code -ne 0 -or $failed) {
    $failed | Select-Object -First 20 | ForEach-Object { Write-Host $_.Line }
    Write-Host "FAILED - see $log"
    exit 1
}
Write-Host "OK"
