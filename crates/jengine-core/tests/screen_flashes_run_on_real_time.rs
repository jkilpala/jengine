//! A `Camera.flash` / fade runs on **real** time in every host: it is the
//! screen, not the world, so a hitstop (time scale 0) must not freeze it at
//! full brightness, a slow motion must not stretch it, and the editor must
//! not advance it by the frame's CPU work time. Each host advanced it by
//! the scaled dt (native, web) or by `frame_time_ms` (the editor) until
//! Oct 2026 - Shooter Classics' Void Armada found a 0.5 s flash lasting 2 s
//! under slow motion. The overlay's tick is host code a native test cannot
//! run, so this reads it.

use std::path::Path;

fn read(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)).unwrap()
}

#[test]
fn screen_flashes_run_on_real_time() {
    for (host, expect) in [
        ("crates/jengine-runtime/src/wgpu_scene_runner.rs", "t.elapsed += real_dt;"),
        ("crates/jengine-web/src/lib.rs", "t.elapsed += real_dt;"),
        ("crates/jengine-editor-viewport/src/lib.rs", "t.elapsed += app.real_dt;"),
    ] {
        let src = read(host);
        let ticks: Vec<&str> = src.lines().map(str::trim).filter(|l| l.starts_with("t.elapsed +=")).collect();
        assert_eq!(ticks, vec![expect], "{host}: the fade overlay must advance by the unscaled frame time");
    }
}
