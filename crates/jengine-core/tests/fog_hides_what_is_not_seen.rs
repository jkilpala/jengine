//! **Fog of war** (Oct 2026; `ecs::fog_of_war`): a scout's `sight` clears
//! the fog round it, what it has seen stays explored, and a `fog_hidden`
//! enemy is hidden from every drawing pass (`in_view_layers`) until the scout
//! is near. The fog's picture is made beside the carved textures and its
//! sprite pointed at it. Found by the showcase's Fog of War.

use glam::Vec2;
use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::{CustomComponents, SpriteComponent, Transform};
use jengine_core::ecs::fog_of_war::{FogHidden, FogOfWar, Sight, hidden};

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

fn visible(world: &World, e: hecs::Entity) -> bool {
    world.get::<&CustomComponents>(e).unwrap().0["Fog"]["visible"].as_bool().unwrap()
}

/// Whether the drawing passes would draw `e` (`in_view_layers`, the question
/// every world collector asks).
fn drawn(world: &World, e: hecs::Entity, want: bool) {
    #[cfg(feature = "gpu-wgpu")]
    assert_eq!(jengine_core::ecs::wgpu_systems::in_view_layers(world, e), want, "drawn");
    #[cfg(not(feature = "gpu-wgpu"))]
    let _ = (world, e, want);
}

#[test]
fn an_enemy_shows_only_while_the_scout_sees_it() {
    let mut world = World::new();
    let fog = world.spawn((
        at(0.0, 0.0),
        FogOfWar { size: [800.0, 400.0], cell: 20.0, reveal_speed: 10.0, ..Default::default() },
        SpriteComponent { texture_id: "".into(), size: Some([800.0, 400.0]), ..Default::default() },
    ));
    let scout = world.spawn((at(-300.0, 0.0), Sight { radius: 120.0, soft: 30.0 }));
    let enemy = world.spawn((at(250.0, 0.0), FogHidden::default()));
    let tick = |world: &mut World, n: usize| {
        for _ in 0..n {
            jengine_core::ecs::behaviors::tick_motion(world, 1.0 / 60.0, None, false);
        }
    };
    tick(&mut world, 30);
    assert!(hidden(&world, enemy) && !visible(&world, enemy), "far from the scout: hidden");
    drawn(&world, enemy, false);
    drawn(&world, scout, true);
    // The scout walks up to it.
    world.get::<&mut Transform>(scout).unwrap().position.x = 200.0;
    tick(&mut world, 60);
    assert!(!hidden(&world, enemy) && visible(&world, enemy), "in sight: shown");
    drawn(&world, enemy, true);
    // And away again: hidden, though the land stays explored.
    world.get::<&mut Transform>(scout).unwrap().position.x = -300.0;
    tick(&mut world, 60);
    assert!(hidden(&world, enemy), "out of sight again: hidden");
    let explored = world.get::<&CustomComponents>(fog).unwrap().0["FogOfWar"]["explored"].as_f64().unwrap();
    assert!(explored > 0.1 && explored < 0.9, "part of the map explored: {explored}");

    // Its picture: made, and the sprite pointed at it.
    let mut assets = AssetManager::new();
    jengine_core::ecs::fog_of_war::sync(&mut world, &mut assets);
    let id = world.get::<&SpriteComponent>(fog).unwrap().texture_id.as_str().to_string();
    assert!(id.starts_with("__fog/"), "{id}");
    let tex = assets.get_texture_bytes(&id).expect("the picture");
    assert_eq!((tex.width, tex.height), (40, 20));
    let alpha = |x: u32, y: u32| tex.rgba[((y * tex.width + x) * 4 + 3) as usize];
    // Row 10 is the middle; column 5 is where the scout stands now (x -300).
    assert!(alpha(5, 10) < 40, "clear where the scout is: {}", alpha(5, 10));
    assert!(alpha(35, 2) > 200, "dark where nobody has been: {}", alpha(35, 2));
    // Column 30 is where the scout went (x 200) and left: dimmed, not dark.
    assert!((60..200).contains(&alpha(30, 10)), "dimmed where the scout has been: {}", alpha(30, 10));
}
