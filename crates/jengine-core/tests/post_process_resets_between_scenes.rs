//! A scene's post-process chain must not outlive the scene.
//!
//! # Why a source scan
//!
//! The effect chain is session state the world does not own - the same shape
//! as looping music, which stranded after Stop for the same reason. Replacing
//! the world does not touch it, so unless a runtime *replaces* the chain at
//! every place a scene ends, scene 14's bloom and vignette follow the player
//! into every scene after it, and there is nothing on screen to explain why.
//!
//! A scene declaring no `post_process` block means an **empty** chain, not
//! "leave what was there" - which is why every site below reads
//! `from_scene_json(..).unwrap_or_default()` rather than `if let Some(..)`.
//!
//! Four sites, and each one was wrong somewhere when this was written
//! (BUGS #84):
//!
//! - loading a scene: web read the block **nowhere at all**, so a scene that
//!   authored effects rendered without them there,
//! - popping a pushed scene: the chain cannot ride in the snapshot, because
//!   `SceneDataV2` has no `post_process` field - it has to be carried beside
//!   it, exactly as the native snapshot already carried its camera,
//! - the editor's Stop: restores the world from a snapshot and left whatever
//!   Play switched on running over the edited scene,
//! - and the parser itself, which lived as a private copy in two runtimes and
//!   was absent from the third.

use jengine_core::renderer::postprocess::PostProcessSettings;

const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/lib.rs");

fn runtimes() -> [(&'static str, &'static str); 3] {
    [("native", NATIVE), ("viewport", VIEWPORT), ("web", WEB)]
}

#[test]
fn every_runtime_replaces_the_chain_when_a_scene_loads() {
    for (name, src) in runtimes() {
        assert!(
            src.contains("from_scene_value"),
            "{name} never reads a scene's `post_process` block, so a scene that \
             authors effects renders without them there - and nothing clears \
             the previous scene's effects either"
        );
        assert!(
            src.contains("from_scene_value(&scene_header).unwrap_or_default()")
                || src.contains("from_scene_value(&header).unwrap_or_default()"),
            "{name} applies a scene's chain only when the scene declares one, so \
             a scene with no `post_process` block inherits the last scene's \
             effects instead of rendering clean"
        );
    }
}

/// Every runtime that can push a scene must carry the chain on its stack.
#[test]
fn a_popped_scene_gets_its_own_chain_back() {
    for (name, src) in runtimes() {
        assert!(
            src.contains("post_process: state.post_process.settings().clone()")
                || src.contains("post_process: app.post_process.settings().clone()")
                || src.contains("gpu.post_process.settings().clone()"),
            "{name} pushes a scene without remembering its effect chain, so \
             popping back to it loses the effects (or keeps the pushed \
             scene's). The snapshot JSON cannot carry them - `SceneDataV2` has \
             no `post_process` field."
        );
    }
}

/// And the editor has a fourth way to leave a scene that no game has: Stop.
#[test]
fn the_editor_restores_the_chain_it_had_before_play() {
    assert!(
        VIEWPORT.contains("pre_play_post_process"),
        "`viewport_stop` restores the world but not the effect chain, so bloom \
         switched on during Play keeps rendering over the edited scene with no \
         way to turn it off short of reopening the editor - exactly what \
         looping music used to do"
    );
}

/// The parser is one function, not one per runtime.
#[test]
fn the_scene_block_is_parsed_in_one_place() {
    for (name, src) in runtimes() {
        assert!(
            !src.contains("fn parse_scene_post_process"),
            "{name} carries its own copy of the scene-block parser again. Two \
             copies existed and the third runtime had none, which is how web \
             came to ignore the block entirely."
        );
    }
}

/// The contract the four sites above depend on.
#[test]
fn a_scene_without_a_block_means_an_empty_chain() {
    let with = r#"{"entities":[],"post_process":{"effects":[{"name":"bloom"}]}}"#;
    let without = r#"{"entities":[]}"#;

    assert_eq!(
        PostProcessSettings::from_scene_json(with)
            .expect("a chain")
            .effects
            .len(),
        1
    );
    assert!(
        PostProcessSettings::from_scene_json(without)
            .unwrap_or_default()
            .effects
            .is_empty(),
        "a scene with no block must clear the chain, not inherit one"
    );
}
