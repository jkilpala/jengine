//! A frame or sprite that names an atlas region must be resolved everywhere it
//! is drawn - not only while the game is playing.
//!
//! # Why a source scan
//!
//! `region` is an *unresolved* reference: until something calls
//! `resolve_sprite_regions` (or `SpriteAnimator::resolve_frame_regions` behind
//! it) the frame keeps `AnimationFrame`'s default uv rect, which is the **whole
//! texture**. So the failure is not a blank or an error - it is the entire
//! sprite sheet drawn in place of one 32x32 creature, which looks like a
//! content mistake rather than a missing call.
//!
//! Both surfaces below shipped exactly that. The editor viewport resolved
//! regions inside `tick_play_world` only, so one game's scene and its six
//! region-named prefabs were correct in Play mode and showed the full sheet in
//! the Scene tab; and the animation editor's preview builds its `SpriteAnimator`
//! by hand, so every `.anim` converted to region names previewed as the sheet.
//!
//! Compiling proves nothing about a call that is simply absent, and neither
//! does a unit test of the resolver - it works. Only the call sites can be
//! checked.

use jengine_core::animation::{AnimationClip, SpriteAnimator};
use jengine_core::asset::AssetManager;
use jengine_core::renderer::atlas::AtlasData;

const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");

/// The editor draws the scene in two modes and both must resolve.
#[test]
fn the_editor_resolves_regions_outside_play_mode() {
    // `tick_play_world` has always called it; `render_editor` is the one that
    // runs in both modes, which is why the call belongs there.
    let render_editor = VIEWPORT
        .split_once("fn render_editor(")
        .expect("render_editor exists")
        .1;
    let body = &render_editor[..render_editor.len().min(6000)];
    assert!(
        body.contains("resolve_sprite_regions"),
        "`render_editor` does not resolve sprite regions, so a sprite or \
         animation frame that names a region draws the whole sheet in the \
         editor's Scene tab while looking correct in Play"
    );
}

/// And the animation editor's preview builds its own animator.
#[test]
fn the_animation_preview_resolves_its_frames() {
    let start = VIEWPORT
        .split_once("pub fn viewport_anim_preview_start(")
        .expect("preview entry point exists")
        .1;
    let body = &start[..start.len().min(3000)];
    assert!(
        body.contains("resolve_frame_regions"),
        "the animation preview never resolves region-named frames, so a `.anim` \
         that names regions previews as the entire sprite sheet"
    );
    assert!(
        VIEWPORT.contains("viewport_anim_preview_set_atlas"),
        "nothing lets the editor hand the preview the atlas its regions are \
         named in, so resolution there can never succeed"
    );
}

#[test]
fn the_native_runtime_resolves_regions() {
    assert!(
        NATIVE.contains("resolve_sprite_regions"),
        "the native runtime never resolves regions"
    );
}

/// The symptom, pinned: an unresolved region frame is the whole texture.
///
/// This is what makes the missing call look like bad content instead of a bug,
/// so it is worth stating outright.
#[test]
fn an_unresolved_region_frame_is_the_whole_texture() {
    // Authored the way `monster_brute.anim` is: a region name and nothing else.
    let clip: AnimationClip = serde_json::from_value(serde_json::json!({
        "name": "idle",
        "frame_duration": 0.25,
        "looping": true,
        "frames": [{ "region": "monster_brute_idle_0" }]
    }))
    .expect("clip fixture");
    let mut animator = SpriteAnimator::default();
    animator.clips.insert("idle".into(), clip);
    animator.play("idle");

    let unresolved = animator.current_frame_result().expect("a frame");
    assert_eq!(
        unresolved.uv_rect,
        [0.0, 0.0, 1.0, 1.0],
        "an unresolved region frame should be the full texture - if this ever \
         becomes a zero rect instead, the symptom changes from 'whole sheet' \
         to 'nothing drawn' and the tests above need rewording"
    );

    // With the atlas in hand it becomes the region's own rect.
    let mut assets = AssetManager::new();
    assets.register_atlas("sheet.png".into(), brute_atlas());
    assert!(animator.resolve_frame_regions(Some("sheet.png"), &assets));
    let resolved = animator.current_frame_result().expect("a frame");
    assert_ne!(resolved.uv_rect, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(resolved.uv_rect, [0.25, 0.0, 0.5, 1.0]);
}

/// A 4x1 sheet; `monster_brute_idle_0` is the second cell.
fn brute_atlas() -> AtlasData {
    serde_json::from_value(serde_json::json!({
        "width": 128, "height": 32,
        "regions": [
            { "name": "player_idle_0",        "x": 0,  "y": 0, "width": 32, "height": 32 },
            { "name": "monster_brute_idle_0", "x": 32, "y": 0, "width": 32, "height": 32 },
            { "name": "monster_brute_idle_1", "x": 64, "y": 0, "width": 32, "height": 32 }
        ]
    }))
    .expect("atlas fixture")
}
