//! A prefab's texture is loaded before something spawns it.
//!
//! Runtimes preload textures by walking the **world**. That is right for
//! everything a scene names and wrong for everything it does not: nothing
//! references a prefab's sheet until a script spawns from it, which happens
//! mid-frame, long after the last chance to read an image off disk. The
//! prefab then spawns, lives, moves - and draws nothing.
//!
//! It is silent, too. The missing-texture warning only fires for ids
//! something asked to load, and nothing ever asked.
//!
//! The web runtime had its own scan for this from the start and the editor
//! viewport asks for what the renderer finds missing, so both were fine;
//! **native had neither**, which is how the arcade's RTS came to have an
//! invisible army (BUGS #137). One collector now, in core, called by the
//! two runtimes that preload.

use jengine_core::scene::format::{EntityData, prefab_texture_ids};

fn prefab(json: &str) -> EntityData {
    serde_json::from_str(json).expect("a prefab")
}

#[test]
fn a_prefabs_sprite_texture_is_collected() {
    let p = prefab(
        r#"{ "name": "Bullet", "components": {
                "sprite": { "texture_id": "atlases/guns.png", "region": "shot",
                            "color": [1, 1, 1, 1], "z_order": 0 } } }"#,
    );
    let ids = prefab_texture_ids([&p]);
    assert!(ids.contains("atlases/guns.png"), "{ids:?}");
}

#[test]
fn an_empty_or_absent_texture_is_not_collected() {
    // An empty id is the built-in white texture, not a file to go looking
    // for - asking the filesystem for "" logs a warning about a texture
    // nobody wanted.
    let blank = prefab(
        r#"{ "name": "A", "components": {
                "sprite": { "texture_id": "", "color": [1, 1, 1, 1], "z_order": 0 } } }"#,
    );
    let none = prefab(
        r#"{ "name": "B", "components": {
                "transform": { "position": [0, 0], "rotation": 0, "scale": [1, 1] } } }"#,
    );
    assert!(prefab_texture_ids([&blank, &none]).is_empty());
}

#[test]
fn the_frames_of_an_animation_are_collected_too() {
    // A sprite animator can swap the texture per frame, so the sheet the
    // first frame uses is not necessarily the only one.
    let p = prefab(
        r#"{ "name": "Walker", "components": {
                "sprite": { "texture_id": "sheets/idle.png", "color": [1, 1, 1, 1], "z_order": 0 },
                "sprite_animator": { "clips": { "walk": { "name": "walk",
                    "frame_duration": 0.1, "looping": true, "frames": [
                        { "texture_id": "sheets/walk_a.png" },
                        { "texture_id": "sheets/walk_b.png" }
                ] } } } } }"#,
    );
    let ids = prefab_texture_ids([&p]);
    for want in ["sheets/idle.png", "sheets/walk_a.png", "sheets/walk_b.png"] {
        assert!(ids.contains(want), "{want} missing from {ids:?}");
    }
}

#[test]
fn a_prefabs_tilemap_and_particles_are_collected() {
    let p = prefab(
        r#"{ "name": "Chunk", "components": {
                "tilemap": { "tile_size": [32, 32], "layers": [], "tilesets": [
                    { "texture_id": "atlases/ground.png", "tile_width": 32, "tile_height": 32,
                      "columns": 4, "tile_count": 4, "first_gid": 1,
                      "image_width": 128, "image_height": 32 } ] },
                "particle_emitter": { "texture_id": "sprites/smoke.png" } } }"#,
    );
    let ids = prefab_texture_ids([&p]);
    assert!(ids.contains("atlases/ground.png"), "{ids:?}");
    assert!(ids.contains("sprites/smoke.png"), "{ids:?}");
}

#[test]
fn many_prefabs_sharing_a_sheet_ask_for_it_once() {
    let sprite = r#""sprite": { "texture_id": "atlases/x.png", "color": [1, 1, 1, 1], "z_order": 0 }"#;
    let a = prefab(&format!(r#"{{ "name": "A", "components": {{ {sprite} }} }}"#));
    let b = prefab(&format!(r#"{{ "name": "B", "components": {{ {sprite} }} }}"#));
    assert_eq!(prefab_texture_ids([&a, &b]).len(), 1);
}

/// The ratchet: the runtimes that preload must actually ask.
///
/// The editor viewport is deliberately not in this list - it has no
/// filesystem and pulls instead, asking the editor for whatever the
/// renderer found missing, which covers prefabs by construction.
#[test]
fn both_preloading_runtimes_collect_prefab_textures() {
    for (path, what) in [
        ("../jengine-runtime/src/wgpu_scene_runner.rs", "the native runtime"),
        ("../jengine-web/src/lib.rs", "the web runtime"),
    ] {
        let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(
            src.contains("prefab_texture_ids"),
            "{what} preloads textures without looking at prefabs, so anything spawned from one is \
             invisible ({path})"
        );
    }
}
