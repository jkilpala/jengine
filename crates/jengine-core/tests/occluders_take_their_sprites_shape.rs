//! **Shadows in the shape of the sprite** (Oct 2026): a `light_occluder`
//! with `from_alpha` casts the outline of its sprite's opaque pixels. Before
//! it a sprite's shadow was its box (`from_sprite`) or a polygon typed in by
//! hand, so a tree threw a rectangle. Found by the showcase's Lantern
//! Orchard.
//!
//! The tracing is unit-tested in `ecs::alpha_shape`; this runs it through the
//! step every host makes (`apply_carves`), `gather_lights` and the CPU light,
//! which the GPU is pinned to by `cpu_light_matches_the_shader`.

use glam::Vec2;
use hecs::World;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::carve::apply_carves;
use jengine_core::ecs::components::{AmbientLight, LightOccluder, PointLight2D, SpriteComponent, Transform};
use jengine_core::renderer::lighting::{gather_lights, light_at};

/// A 32 x 32 texture whose opaque pixels are an L: a post up the left
/// (x 0..8, all the way down) and a foot along the bottom (y 24..32).
fn l_shape() -> RawTextureData {
    let mut rgba = Vec::new();
    for y in 0..32u32 {
        for x in 0..32u32 {
            let solid = x < 8 || y >= 24;
            rgba.extend_from_slice(&[200, 200, 200, if solid { 255 } else { 0 }]);
        }
    }
    RawTextureData { width: 32, height: 32, rgba }
}

fn world_with(occ: LightOccluder) -> (World, AssetManager) {
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("l.png".into(), l_shape());
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0, 1.0, 1.0], intensity: 0.0 },));
    // Drawn 64 x 64 at the origin: the post is x -32..-16, the foot y -32..-16.
    world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "l.png".into(), size: Some([64.0, 64.0]), ..Default::default() },
        occ,
    ));
    // A light up and to the right, shining down-left across the L's notch.
    world.spawn((
        Transform { position: Vec2::new(200.0, 200.0), ..Default::default() },
        PointLight2D { intensity: 1.0, radius: 900.0, falloff: 0.0, ..Default::default() },
    ));
    (world, assets)
}

#[test]
fn the_shadow_is_the_sprites_shape() {
    let (mut world, mut assets) = world_with(LightOccluder { from_alpha: true, ..Default::default() });
    apply_carves(&mut world, &mut assets);
    let lights = gather_lights(&world);
    assert_eq!(lights.occluders.len(), 6, "an L has six edges: {:?}", lights.occluders);
    // In the notch of the L, on the light's side of both arms: lit. The
    // sprite's box would cover it and shadow it.
    let notch = light_at(Vec2::new(0.0, 0.0), &lights);
    assert!(notch.x > 0.2, "the notch is lit: {notch:?}");
    // Behind the post, in line with the light: dark.
    let behind = light_at(Vec2::new(-60.0, -60.0), &lights);
    assert!(behind.x < 0.05, "behind the L is in its shadow: {behind:?}");

    let (mut boxed, mut assets) = world_with(LightOccluder { from_sprite: true, ..Default::default() });
    apply_carves(&mut boxed, &mut assets);
    let lights = gather_lights(&boxed);
    assert_eq!(lights.occluders.len(), 4, "the box it used to be");
}

#[test]
fn it_is_traced_once_and_again_when_the_sprite_changes() {
    let (mut world, mut assets) = world_with(LightOccluder { from_alpha: true, ..Default::default() });
    apply_carves(&mut world, &mut assets);
    let key = |w: &World| {
        let mut q = w.query::<&LightOccluder>();
        q.iter().next().and_then(|(_, o)| o.traced.as_ref().map(|t| t.key)).expect("traced")
    };
    let first = key(&world);
    apply_carves(&mut world, &mut assets);
    assert_eq!(key(&world), first, "nothing changed, nothing retraced");
    for (_, s) in world.query_mut::<&mut SpriteComponent>() {
        s.size = Some([128.0, 64.0]);
    }
    apply_carves(&mut world, &mut assets);
    assert_ne!(key(&world), first, "a new size is a new outline");
    let lights = gather_lights(&world);
    let xs = lights.occluders.iter().flat_map(|s| [s[0], s[2]]);
    assert!(xs.fold(f32::MIN, f32::max) > 60.0, "stretched with the sprite");
}

#[test]
fn before_its_pixels_load_it_is_the_sprites_box() {
    let (world, _) = world_with(LightOccluder { from_alpha: true, ..Default::default() });
    // No trace step run, as on the frames before the texture arrives.
    assert_eq!(gather_lights(&world).occluders.len(), 4);
}

#[test]
fn the_scene_format_keeps_the_flag_and_not_the_outline() {
    let (mut world, mut assets) = world_with(LightOccluder { from_alpha: true, alpha_tolerance: 2.0, ..Default::default() });
    apply_carves(&mut world, &mut assets);
    let scene = jengine_core::scene::format::save_scene(&world);
    let occ = scene.entities.iter().find_map(|e| e.components.get::<jengine_core::ecs::components::LightOccluder>("light_occluder")).expect("saved");
    assert!(occ.from_alpha && occ.alpha_tolerance == 2.0);
    let json = serde_json::to_value(occ).unwrap();
    assert!(json.get("traced").is_none(), "the outline is worked out, not authored");
}
