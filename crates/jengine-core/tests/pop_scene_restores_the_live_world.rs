//! `pop_scene` must bring back the world as it was when `push_scene` left
//! it, in every runtime.
//!
//! The editor viewport pushed `play_snapshot` - the copy it takes when a
//! scene is *loaded*, kept so Stop can put the edited scene back - so every
//! pop in the editor rewound the scene underneath to its start. Monster
//! Town's hero came back from each battle on the start tile rather than in
//! the grass where the fight began. Native and web always snapshotted the
//! live world, so it was the editor alone, and nothing failed: the pop
//! restored *a* world, just the wrong one.

const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/lib.rs");

/// The text of `fn name` up to the next top-level item.
fn body<'a>(src: &'a str, name: &str) -> &'a str {
    let start = src.find(&format!("fn {name}(")).unwrap_or_else(|| panic!("no fn {name}"));
    let rest = &src[start..];
    let end = rest[1..].find("\n}\n").map(|i| i + 1).unwrap_or(rest.len());
    &rest[..end]
}

/// `snapshot` has to be taken of the live world before the entry is pushed.
fn snapshots_live_world_before_push(body: &str, snapshot: &str) -> bool {
    match (body.find(snapshot), body.find("scene_stack.push(")) {
        (Some(s), Some(p)) => s < p,
        _ => false,
    }
}

#[test]
fn viewport_pushes_the_live_world() {
    let push = body(VIEWPORT, "viewport_apply_scene_transition");
    assert!(
        snapshots_live_world_before_push(push, "snapshot_scene_v2(&app.world"),
        "the viewport's push_scene does not snapshot the live world, so pop_scene rewinds the scene to its start"
    );
    let pop = body(VIEWPORT, "viewport_pop_play_scene");
    assert!(
        pop.contains("from_str(&live)"),
        "the viewport's pop_scene does not restore the live snapshot it pushed"
    );
    assert!(
        pop.contains("play_snapshot = Some(loaded)"),
        "after a pop, Stop must still fall back to the scene as loaded, not one carrying play-mode state"
    );
}

#[test]
fn native_and_web_push_the_live_world() {
    assert!(
        snapshots_live_world_before_push(NATIVE, "snapshot_world(&state.world"),
        "native's push_scene does not snapshot the live world"
    );
    assert!(
        snapshots_live_world_before_push(WEB, "snapshot_scene_v2(&app.world"),
        "web's push_scene does not snapshot the live world"
    );
}
