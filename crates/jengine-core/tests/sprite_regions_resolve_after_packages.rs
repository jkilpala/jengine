//! A sprite a package spawns, or switches to another atlas region, draws
//! that region on its first frame, in every runtime that hosts packages.
//!
//! The native runner resolved regions once a frame, in `tick_world`, which
//! runs before the package phases - so everything a package system spawned
//! drew its whole atlas squashed into one sprite for a frame. Found as a
//! rainbow smear over the bullet hell's ship: its shots live a few frames,
//! so the smear was always there (BUGS #110). The editor viewport resolved
//! after `ScriptExec`, but not after the later phases.

const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/lib.rs");

fn resolves_after_last_phase(src: &str, phase_call: &str) -> bool {
    // The needle spans a line break, and a checkout with `core.autocrlf`
    // writes some of these files with CRLF.
    let src = src.replace("\r\n", "\n");
    let Some(at) = src.find(phase_call) else { return false };
    src[at..].find("resolve_sprite_regions(").is_some_and(|d| d < 600)
}

#[test]
fn native_resolves_regions_after_the_package_phases() {
    assert!(resolves_after_last_phase(NATIVE, "run_phase_with(
        SystemPhase::PreRender,"));
}

#[test]
fn the_viewport_resolves_regions_after_the_package_phases() {
    assert!(resolves_after_last_phase(VIEWPORT, ".run_phase_with(
        SystemPhase::PreRender,"));
}

#[test]
fn the_web_runtime_resolves_regions_after_the_package_phases() {
    assert!(resolves_after_last_phase(WEB, ".run_phase_with(
            SystemPhase::PreRender,"));
}
