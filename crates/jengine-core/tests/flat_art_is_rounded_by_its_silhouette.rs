//! **Flat art is rounded by its silhouette** (Oct 2026; `ecs::auto_normal`):
//! a sprite with an `auto_normal` and no normal map is given one, worked
//! out from its alpha, through the per-frame step every host makes with its
//! assets (`carve::apply_carves`) - its material pointed at it, so the
//! normal pass draws it like a painted one. The map is remade when the
//! texture changes, and kept when nothing has.
#![cfg(feature = "auto_normal")]

use hecs::World;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::auto_normal::{AutoNormal, set_auto_normal_field};
use jengine_core::ecs::carve::apply_carves;
use jengine_core::ecs::components::{MaterialComponent, SpriteComponent};

/// A disc of radius `r` in a `w` x `w` texture.
fn disc(w: u32, r: f32) -> RawTextureData {
    let mut rgba = vec![0u8; (w * w * 4) as usize];
    let c = w as f32 / 2.0;
    for y in 0..w {
        for x in 0..w {
            if ((x as f32 + 0.5 - c).powi(2) + (y as f32 + 0.5 - c).powi(2)).sqrt() < r {
                let i = ((y * w + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&[200, 120, 90, 255]);
            }
        }
    }
    RawTextureData { width: w, height: w, rgba }
}

fn texel(t: &RawTextureData, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * t.width + x) * 4) as usize;
    [t.rgba[i], t.rgba[i + 1], t.rgba[i + 2], t.rgba[i + 3]]
}

#[test]
fn a_flat_cut_out_is_given_rounded_edges() {
    let mut world = World::new();
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("moon.png".into(), disc(64, 28.0));
    let moon = world.spawn((
        SpriteComponent { texture_id: "moon.png".into(), ..Default::default() },
        AutoNormal { bevel: 8.0, ..Default::default() },
    ));
    apply_carves(&mut world, &mut assets);
    let id = world.get::<&MaterialComponent>(moon).unwrap().normal_map_id.clone().expect("a normal map");
    let map = assets.get_texture_bytes(&id).expect("the map").clone();
    // The middle faces the viewer; the rims face out.
    let mid = texel(&map, 32, 32);
    assert!(mid[2] > 250 && (mid[0] as i32 - 128).abs() < 3, "the top is flat: {mid:?}");
    assert!(texel(&map, 6, 32)[0] < 90, "the left rim faces left: {:?}", texel(&map, 6, 32));
    assert!(texel(&map, 57, 32)[0] > 166, "the right rim faces right: {:?}", texel(&map, 57, 32));
    assert_eq!(texel(&map, 0, 0)[3], 0, "outside the silhouette, nothing");

    // A new picture under the same name: a new map.
    assets.insert_texture_bytes("moon.png".into(), disc(64, 12.0));
    apply_carves(&mut world, &mut assets);
    let again = assets.get_texture_bytes(&id).expect("the map").clone();
    assert_eq!(texel(&again, 6, 32)[3], 0, "remade for the smaller disc");

    // Strength 0 lays it flat: a map facing the viewer everywhere.
    set_auto_normal_field(&world, moon, "strength", &serde_json::json!(0.0)).unwrap();
    apply_carves(&mut world, &mut assets);
    let flat_id = world.get::<&MaterialComponent>(moon).unwrap().normal_map_id.clone().unwrap();
    let flat = assets.get_texture_bytes(&flat_id).unwrap();
    assert!((texel(flat, 25, 32)[0] as i32 - 128).abs() <= 1, "flat again: {:?}", texel(flat, 25, 32));
}

#[test]
fn a_painted_normal_map_is_left_alone() {
    let mut world = World::new();
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("rock.png".into(), disc(32, 14.0));
    let rock = world.spawn((
        SpriteComponent { texture_id: "rock.png".into(), ..Default::default() },
        MaterialComponent { material_id: "lit_sprite".into(), normal_map_id: Some("rock_n.png".into()), ..Default::default() },
        AutoNormal::default(),
    ));
    apply_carves(&mut world, &mut assets);
    assert_eq!(world.get::<&MaterialComponent>(rock).unwrap().normal_map_id.as_deref(), Some("rock_n.png"));
}
