//! **`settings.frame_rate_cap` is applied by every runtime a build ships.**
//!
//! It replaced `settings.target_fps`, which Project Settings offered for
//! years while no runtime read it - the shape this guards against. The native
//! runner (desktop and Android) and the web runtime each read the cap for
//! their platform and pace their loop by `time::FrameCap`; the editor's Play
//! mode is deliberately uncapped, a development view.

const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const NATIVE_LIB: &str = include_str!("../../jengine-runtime/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/lib.rs");

#[test]
fn the_native_runner_reads_the_cap_and_paces_by_it() {
    assert!(NATIVE.contains("FrameRateCaps::from_project("), "native never reads the cap");
    assert!(NATIVE.contains("CapPlatform::native()"), "native reads the wrong platform's cap");
    assert!(NATIVE.contains("frame_cap.ready("), "native reads the cap and never paces by it");
    // The pacing happens between events; without the hook the loop, which no
    // longer requests its own next frame, would stop after one.
    assert!(NATIVE_LIB.contains("fn about_to_wait("), "the event loop never asks for the next capped frame");
}

#[test]
fn the_web_runtime_reads_the_cap_and_paces_by_it() {
    assert!(WEB.contains("frame_rate_cap"), "web never reads the cap");
    assert!(WEB.contains("CapPlatform::Web"), "web reads the wrong platform's cap");
    assert!(WEB.contains("frame_cap.ready("), "web reads the cap and never paces by it");
}
