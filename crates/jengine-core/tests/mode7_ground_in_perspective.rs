//! **Mode 7** (Oct 2026; `ecs::mode7`): a ground plane in perspective and
//! sprites standing on it. A kart in front of the camera is placed on the
//! screen and shrinks with distance, one behind is hidden, the camera
//! follows a kart from behind, the kart shows the view of itself the camera
//! sees, and scripts move things through the commands' own paths (`m7p`,
//! `m7s`). Found by the showcase's Kart Circuit.

use hecs::{Entity, World};
use jengine_core::ecs::components::{Name, SpriteComponent, Transform};
use jengine_core::ecs::mode7::{Mode7, Mode7Sprite};
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};

struct Host(World);
impl ScriptWorldHost for Host {
    fn world(&mut self) -> &mut World {
        &mut self.0
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
    }
}

fn tick(world: &mut World) {
    jengine_core::ecs::behaviors::tick_motion(world, 1.0 / 60.0, None, false);
}

fn kart(world: &mut World, name: &str, x: f32, y: f32, angle: f32) -> Entity {
    world.spawn((
        Name(name.into()),
        Transform::default(),
        SpriteComponent { texture_id: "kart.png".into(), size: Some([16.0, 16.0]), y_sort: true, ..Default::default() },
        Mode7Sprite { x, y, angle, frames: 8, ..Default::default() },
    ))
}

#[test]
fn karts_stand_on_the_ground_and_the_camera_follows() {
    let mut world = World::new();
    let plane = world.spawn((
        Name("Track".into()),
        Transform::default(),
        Mode7 { texture_id: "track.png".into(), height: 20.0, fov: 1.5, horizon: 0.5, far: 800.0, size: [800.0, 400.0], ..Default::default() },
    ));
    assert!(jengine_core::scene::format::world_texture_ids(&world).contains("track.png"), "the ground is preloaded");
    // The camera at the origin facing +x.
    let near = kart(&mut world, "Near", 60.0, 0.0, 0.0);
    let far = kart(&mut world, "Far", 300.0, 0.0, std::f32::consts::PI);
    let behind = kart(&mut world, "Behind", -50.0, 0.0, 0.0);
    tick(&mut world);
    let at = |w: &World, e: Entity| w.get::<&Transform>(e).map(|t| (t.position, t.scale.x)).unwrap();
    let ((np, ns), (fp, fs)) = (at(&world, near), at(&world, far));
    assert!(ns > fs && fs > 0.0, "nearer is bigger: {ns} {fs}");
    assert!(fp.y > np.y && np.y < 0.0, "nearer is lower, both under the horizon: {np} {fp}");
    assert!(np.x.abs() < 1e-3, "straight ahead is the middle");
    assert_eq!(at(&world, behind).1, 0.0, "behind the camera: hidden");
    // Views: the near kart is seen from behind (frame 0), the far one,
    // facing the camera, from the front (frame 4 of 8).
    let frame = |w: &World, e: Entity| w.get::<&SpriteComponent>(e).unwrap().uv_rect.unwrap()[0];
    assert!(frame(&world, near).abs() < 1e-6 && (frame(&world, far) - 0.5).abs() < 1e-6);

    // Following: the camera sits behind the near kart, facing its way.
    world.get::<&mut Mode7>(plane).unwrap().follow = "Near".into();
    world.get::<&mut Mode7>(plane).unwrap().follow_distance = 30.0;
    let mut host = Host(world);
    let v = serde_json::json!({ "e": near.id(), "x": 100.0, "y": 50.0, "a": std::f32::consts::FRAC_PI_2 });
    assert!(handle_world_command(&mut host, "m7p", &JsonArgs(v)), "the shared arm takes `m7p`");
    let v = serde_json::json!({ "e": plane.id(), "f": "height", "v": 35.0 });
    assert!(handle_world_command(&mut host, "m7s", &JsonArgs(v)), "the shared arm takes `m7s`");
    tick(&mut host.0);
    let m = host.0.get::<&Mode7>(plane).unwrap().clone();
    assert!((m.x - 100.0).abs() < 1e-3 && (m.y - 20.0).abs() < 1e-3, "30 behind it: ({}, {})", m.x, m.y);
    assert!((m.angle - std::f32::consts::FRAC_PI_2).abs() < 1e-6 && m.height == 35.0);
    let (np, ns) = at(&host.0, near);
    assert!(np.x.abs() < 1e-3 && ns > 0.0, "the followed kart is in the middle of the view");
}
