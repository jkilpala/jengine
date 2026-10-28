//! **Sprite stacking** (Oct 2026; `ecs::sprite_stack`): a model drawn as
//! slices raised up the screen. Its strip is preloaded like a sprite's, its
//! `spin` turns it through `tick_motion`, and its slices rise along the
//! screen's up whichever way the camera has turned. Found by the showcase's
//! Toy Town.
#![cfg(feature = "sprite_stack")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{Transform, WorldTransform};
use jengine_core::ecs::sprite_stack::{SpriteStack, quads, screen_up};

#[test]
fn a_stack_preloads_spins_and_stands_up_under_a_turned_camera() {
    let mut world = World::new();
    let house = world.spawn((
        Transform { position: Vec2::new(40.0, 10.0), ..Default::default() },
        SpriteStack { texture_id: "sprites/toytown/house.png".into(), slices: 20, spin: 1.0, ..Default::default() },
    ));
    assert!(
        jengine_core::scene::format::world_texture_ids(&world).contains("sprites/toytown/house.png"),
        "the strip is preloaded"
    );
    for _ in 0..30 {
        jengine_core::ecs::behaviors::tick_motion(&mut world, 1.0 / 60.0, None, false);
    }
    let turned = world.get::<&Transform>(house).unwrap().rotation;
    assert!((turned - 0.5).abs() < 1e-3, "spin turned it half a radian: {turned}");

    // A camera turned a quarter anticlockwise: the screen's up is the
    // world's -x, and the top slice sits that way from the foot.
    let corners = [Vec2::new(-10.0, -10.0), Vec2::new(-10.0, 10.0), Vec2::new(10.0, 10.0), Vec2::new(10.0, -10.0)];
    let up = screen_up(Some(corners));
    assert!((up - Vec2::new(-1.0, 0.0)).length() < 1e-5, "{up}");
    let stack = world.get::<&SpriteStack>(house).unwrap().clone();
    let wt = WorldTransform::from(&*world.get::<&Transform>(house).unwrap());
    let (pos, ..) = quads(&stack, &wt, up);
    let n = pos.len();
    let top = (pos[n - 4] + pos[n - 2]) * 0.5;
    let rise = top - wt.position;
    assert!(rise.x < -30.0 && rise.y.abs() < 1e-3, "the top rises up the screen: {rise}");
}
