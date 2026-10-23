//! The data-driven combat layer and the systems around it: hitboxes, obstacles,
//! health bars, camera follow, and follow/seek targets.
//!
//! None of these had a test. Each case here was a real misbehaviour:
//! a projectile damaged every enemy it overlapped, a destructible obstacle
//! pushed itself away, health bars named a texture only one project ships,
//! the camera followed whichever `CameraFollow` the query yielded first, and
//! `EntityRef::Player` resolved to nothing.

use glam::Vec2;
use hecs::World;
use jengine_core::asset::WHITE_TEXTURE_ID;
use jengine_core::ecs::behaviors::update_follow_targets;
use jengine_core::ecs::components::{
    Camera2DComponent, CameraFollow, EntityRef, FollowTarget, Health, HealthBar, HealthBarView,
    Hitbox, Obstacle, SpriteComponent, Tags, Transform, TriggerOverlaps,
};
use jengine_core::ecs::systems::{
    apply_hitbox_damage, resolve_obstacle_collisions, sync_health_bars, update_camera,
};
use jengine_core::renderer::camera::Camera2D;

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

fn health(hp: f32) -> Health {
    Health { current: hp, max: hp, ..Default::default() }
}

fn overlapping(victims: &[hecs::Entity]) -> TriggerOverlaps {
    TriggerOverlaps {
        staying: victims.iter().map(|e| e.to_bits().get()).collect(),
        ..Default::default()
    }
}

fn hp(world: &World, e: hecs::Entity) -> f32 {
    world.get::<&Health>(e).unwrap().current
}

#[test]
fn a_projectile_damages_only_the_nearest_target_and_is_consumed() {
    let mut world = World::new();
    let near = world.spawn((at(5.0, 0.0), health(10.0)));
    let far = world.spawn((at(9.0, 0.0), health(10.0)));
    let bullet = world.spawn((
        at(0.0, 0.0),
        Hitbox { damage: 3.0, despawn_on_hit: true, ..Default::default() },
        overlapping(&[far, near]),
    ));

    apply_hitbox_damage(&mut world, 0.016);

    assert_eq!(hp(&world, near), 7.0, "the nearest target takes the hit");
    assert_eq!(hp(&world, far), 10.0, "a bullet into a crowd hit the whole crowd");
    assert!(!world.contains(bullet), "the projectile was not consumed");
}

#[test]
fn a_projectile_skips_a_target_it_cannot_hurt() {
    let mut world = World::new();
    let shielded = world.spawn((
        at(1.0, 0.0),
        Health { current: 10.0, max: 10.0, invuln_timer: 1.0, ..Default::default() },
    ));
    let open = world.spawn((at(6.0, 0.0), health(10.0)));
    let bullet = world.spawn((
        at(0.0, 0.0),
        Hitbox { damage: 4.0, despawn_on_hit: true, ..Default::default() },
        overlapping(&[shielded, open]),
    ));

    apply_hitbox_damage(&mut world, 0.016);

    assert_eq!(hp(&world, shielded), 10.0);
    assert_eq!(hp(&world, open), 6.0);
    assert!(!world.contains(bullet));
}

#[test]
fn a_projectile_with_nothing_to_hurt_flies_on() {
    let mut world = World::new();
    let shielded = world.spawn((
        at(1.0, 0.0),
        Health { current: 10.0, max: 10.0, invuln_timer: 1.0, ..Default::default() },
    ));
    let bullet = world.spawn((
        at(0.0, 0.0),
        Hitbox { damage: 4.0, despawn_on_hit: true, ..Default::default() },
        overlapping(&[shielded]),
    ));
    apply_hitbox_damage(&mut world, 0.016);
    assert!(world.contains(bullet));
}

#[test]
fn a_lingering_hitbox_still_hits_everything_it_overlaps() {
    let mut world = World::new();
    let a = world.spawn((at(1.0, 0.0), health(10.0)));
    let b = world.spawn((at(2.0, 0.0), health(10.0)));
    let aura = world.spawn((
        at(0.0, 0.0),
        Hitbox { damage: 1.0, retrigger: 0.5, ..Default::default() },
        overlapping(&[a, b]),
    ));
    apply_hitbox_damage(&mut world, 0.016);
    assert_eq!((hp(&world, a), hp(&world, b)), (9.0, 9.0));
    assert!(world.contains(aura));

    // And the retrigger cooldown holds for both.
    apply_hitbox_damage(&mut world, 0.016);
    assert_eq!((hp(&world, a), hp(&world, b)), (9.0, 9.0));
}

#[test]
fn a_destructible_obstacle_does_not_push_itself_away() {
    let mut world = World::new();
    let barrel = world.spawn((at(50.0, 50.0), health(5.0), Obstacle { radius: 20.0 }));
    for _ in 0..10 {
        resolve_obstacle_collisions(&mut world);
    }
    assert_eq!(world.get::<&Transform>(barrel).unwrap().position, Vec2::new(50.0, 50.0));

    // Other bodies are still pushed out of it.
    let walker = world.spawn((at(55.0, 50.0), health(5.0)));
    resolve_obstacle_collisions(&mut world);
    let d = world.get::<&Transform>(walker).unwrap().position.distance(Vec2::new(50.0, 50.0));
    assert!((d - 20.0).abs() < 1e-3, "walker should sit on the obstacle's rim, got {d}");
}

/// A mover with no `Health` - a seeker, a flock agent - is kept out too.
///
/// Only `Health` used to count, so test-lab scene 42's flock walked through
/// the rocks its own hint said kept it out (BUGS #155). A projectile that is
/// consumed by cover must still be consumed rather than slid round the rim,
/// and a rapier body is left to rapier.
#[test]
fn an_obstacle_keeps_out_any_mover_but_still_eats_projectiles() {
    use jengine_core::ecs::components::{RigidBody2D, Velocity2D};
    let mut world = World::new();
    world.spawn((at(0.0, 0.0), Obstacle { radius: 20.0 }));
    let agent = world.spawn((at(5.0, 0.0), Velocity2D::default()));
    let bullet = world.spawn((
        at(-5.0, 0.0),
        Velocity2D::default(),
        Hitbox { despawn_on_hit: true, ..Default::default() },
    ));
    let body = world.spawn((at(0.0, 5.0), Velocity2D::default(), RigidBody2D::default()));
    let prop = world.spawn((at(0.0, -5.0),));
    resolve_obstacle_collisions(&mut world);

    let d = world.get::<&Transform>(agent).unwrap().position.length();
    assert!((d - 20.0).abs() < 1e-3, "a velocity-driven agent should sit on the rim, got {d}");
    assert!(!world.contains(bullet), "a despawn_on_hit projectile must still be consumed");
    assert_eq!(world.get::<&Transform>(body).unwrap().position, Vec2::new(0.0, 5.0), "rapier owns bodies");
    assert_eq!(world.get::<&Transform>(prop).unwrap().position, Vec2::new(0.0, -5.0), "a static prop is not a mover");
}

#[test]
fn health_bars_use_the_built_in_white_texture() {
    let mut world = World::new();
    world.spawn((at(0.0, 0.0), health(10.0), HealthBar::default()));
    sync_health_bars(&mut world);
    let textures: Vec<String> = world
        .query::<(&SpriteComponent, &HealthBarView)>()
        .iter()
        .map(|(_, (s, _))| s.texture_id.to_string())
        .collect();
    assert_eq!(textures.len(), 2, "background and fill");
    assert!(textures.iter().all(|t| t == WHITE_TEXTURE_ID), "{textures:?}");
}

#[test]
fn removing_the_health_bar_component_removes_the_bar() {
    let mut world = World::new();
    let owner = world.spawn((at(0.0, 0.0), health(10.0), HealthBar::default()));
    sync_health_bars(&mut world);
    assert_eq!(world.query::<&HealthBarView>().iter().count(), 2);

    world.remove_one::<HealthBar>(owner).unwrap();
    sync_health_bars(&mut world);
    assert_eq!(world.query::<&HealthBarView>().iter().count(), 0, "a frozen bar was left behind");
}

#[test]
fn the_camera_follows_the_active_cameras_own_follow() {
    let mut world = World::new();
    // Spawned first, so it is what the query yields first.
    world.spawn((at(-500.0, 0.0), CameraFollow { smoothing: 1.0, ..Default::default() }));
    world.spawn((
        at(300.0, 0.0),
        Camera2DComponent { is_active: true, ..Default::default() },
        CameraFollow { smoothing: 1.0, ..Default::default() },
    ));
    let mut camera = Camera2D::new(800.0, 600.0);
    update_camera(&world, &mut camera, 1.0);
    assert!(
        (camera.position.x - 300.0).abs() < 1e-3,
        "followed the other CameraFollow: x = {}",
        camera.position.x
    );
}

#[test]
fn a_follow_target_of_player_finds_the_entity_tagged_player() {
    let mut world = World::new();
    let player = world.spawn((at(100.0, 0.0), Tags(["player".to_string()].into_iter().collect())));
    let pet = world.spawn((
        at(0.0, 0.0),
        FollowTarget { target: EntityRef::Player, max_speed: 1000.0, stop_distance: 0.0, ..Default::default() },
    ));
    update_follow_targets(&mut world, 0.05);
    let x = world.get::<&Transform>(pet).unwrap().position.x;
    assert!((x - 50.0).abs() < 1e-3, "EntityRef::Player resolved to nothing: x = {x}");
    let _ = player;
}
