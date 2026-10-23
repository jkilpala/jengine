//! A nine-slice sprite learns its texture's pixel size once the texture is
//! loaded - the half of BUGS #96 that runs in every runtime.
//!
//! The slicing itself is unit-tested in `wgpu_systems`; it is only correct if
//! `source_px` is filled, and nothing else fills it. `resolve_sprite_regions`
//! is the per-frame call every runtime already makes for atlas regions.

use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::{SpriteComponent, SpriteFill};
use jengine_core::ecs::systems::resolve_sprite_regions;

fn source_px(world: &hecs::World, e: hecs::Entity) -> Option<[f32; 2]> {
    match world.get::<&SpriteComponent>(e).unwrap().fill.clone() {
        Some(SpriteFill::NineSlice { source_px, .. }) => source_px,
        _ => panic!("not a nine-slice"),
    }
}

#[test]
fn a_nine_slice_sprite_resolves_its_texture_size() {
    let fill: SpriteFill =
        serde_json::from_str(r#"{ "mode": "nine_slice", "left": 16, "right": 16, "top": 16, "bottom": 16 }"#)
            .unwrap();
    let mut world = hecs::World::new();
    let whole = world.spawn((SpriteComponent {
        texture_id: "frame.png".into(),
        size: Some([240.0, 96.0]),
        fill: Some(fill.clone()),
        ..Default::default()
    },));
    let half = world.spawn((SpriteComponent {
        texture_id: "frame.png".into(),
        uv_rect: Some([0.0, 0.0, 0.5, 1.0]),
        fill: Some(fill),
        ..Default::default()
    },));

    let mut assets = AssetManager::new();
    resolve_sprite_regions(&mut world, &assets);
    assert_eq!(source_px(&world, whole), None, "nothing to resolve before the texture loads");

    assets.insert_texture_bytes(
        "frame.png".into(),
        RawTextureData { width: 48, height: 48, rgba: vec![255; 48 * 48 * 4] },
    );
    resolve_sprite_regions(&mut world, &assets);
    assert_eq!(source_px(&world, whole), Some([48.0, 48.0]));
    assert_eq!(source_px(&world, half), Some([24.0, 48.0]), "a uv_rect samples part of the texture");
}
