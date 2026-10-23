//! `CameraFollow`'s dead zone and look-ahead: the camera holds while its
//! target moves inside a box round the middle of the view and is dragged by
//! the box's edge past it, and it leads a moving target by its velocity.
//! Every test runs the real `update_camera` every host calls.

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::ecs::components::{Camera2DComponent, CameraFollow, Transform};
use jengine_core::ecs::systems::update_camera;
use jengine_core::renderer::camera::Camera2D;

const DT: f32 = 1.0 / 60.0;

fn setup(follow: CameraFollow) -> (World, Entity, Camera2D) {
    let mut world = World::new();
    world.spawn((Transform::default(), Camera2DComponent { is_active: true, ..Default::default() }));
    let target = world.spawn((Transform::default(), follow));
    (world, target, Camera2D::new(1280.0, 720.0))
}

fn move_to(world: &mut World, e: Entity, x: f32, y: f32) {
    world.get::<&mut Transform>(e).unwrap().position = Vec2::new(x, y);
}

#[test]
fn the_camera_holds_inside_the_dead_zone_and_is_dragged_by_its_edge() {
    let follow = CameraFollow { smoothing: 1.0, dead_zone: [200.0, 100.0], ..Default::default() };
    let (mut world, target, mut cam) = setup(follow);
    update_camera(&world, &mut cam, DT);
    assert_eq!(cam.position, Vec2::ZERO);
    // Inside the box: the camera does not move.
    move_to(&mut world, target, 90.0, -40.0);
    update_camera(&world, &mut cam, DT);
    assert_eq!(cam.position, Vec2::ZERO, "held");
    // Past its right edge: dragged so the target sits on the edge.
    move_to(&mut world, target, 150.0, 0.0);
    update_camera(&world, &mut cam, DT);
    assert!((cam.position.x - 50.0).abs() < 1e-3, "{}", cam.position);
    // Back inside: held where it was dragged to.
    move_to(&mut world, target, 60.0, 0.0);
    update_camera(&world, &mut cam, DT);
    assert!((cam.position.x - 50.0).abs() < 1e-3);
    // Up past the top edge.
    move_to(&mut world, target, 60.0, 130.0);
    update_camera(&world, &mut cam, DT);
    assert!((cam.position.y - 80.0).abs() < 1e-3, "{}", cam.position);
}

#[test]
fn without_a_dead_zone_it_follows_as_before() {
    let (mut world, target, mut cam) = setup(CameraFollow { smoothing: 1.0, ..Default::default() });
    move_to(&mut world, target, 37.0, -12.0);
    update_camera(&world, &mut cam, DT);
    assert_eq!(cam.position, Vec2::new(37.0, -12.0));
}

#[test]
fn the_camera_leads_a_moving_target() {
    let follow = CameraFollow { smoothing: 1.0, look_ahead: 0.5, look_ahead_max: 160.0, ..Default::default() };
    let (mut world, target, mut cam) = setup(follow);
    // Run right at 200 units a second for three seconds.
    let mut x = 0.0;
    for _ in 0..180 {
        x += 200.0 * DT;
        move_to(&mut world, target, x, 0.0);
        update_camera(&world, &mut cam, DT);
    }
    let lead = cam.position.x - x;
    assert!((lead - 100.0).abs() < 2.0, "leads by velocity x time: {lead}");
    // Turning round swings the lead over - it does not jump.
    let before = cam.look_ahead.x;
    x -= 200.0 * DT;
    move_to(&mut world, target, x, 0.0);
    update_camera(&world, &mut cam, DT);
    assert!(cam.look_ahead.x > 0.0 && cam.look_ahead.x < before, "eased: {} -> {}", before, cam.look_ahead.x);
    // Faster than the cap: the lead stops at look_ahead_max.
    for _ in 0..240 {
        x += 600.0 * DT;
        move_to(&mut world, target, x, 0.0);
        update_camera(&world, &mut cam, DT);
    }
    assert!((cam.position.x - x - 160.0).abs() < 2.0, "capped: {}", cam.position.x - x);
    // Standing still, the lead settles back to nothing.
    for _ in 0..600 {
        update_camera(&world, &mut cam, DT);
    }
    assert!((cam.position.x - x).abs() < 1.0, "{}", cam.position.x - x);
}

#[test]
fn a_script_sets_a_follow_field_through_the_shared_command() {
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
    let (world, target, _) = setup(CameraFollow::default());
    let mut host = Host(world);
    let args = JsonArgs(serde_json::json!({ "e": target.id(), "f": "dead_zone", "v": [240.0, 120.0] }));
    assert!(handle_world_command(&mut host, "scfl", &args));
    assert_eq!(host.0.get::<&CameraFollow>(target).unwrap().dead_zone, [240.0, 120.0]);
    let args = JsonArgs(serde_json::json!({ "e": target.id(), "f": "look_ahead", "v": 0.4 }));
    assert!(handle_world_command(&mut host, "scfl", &args));
    assert!((host.0.get::<&CameraFollow>(target).unwrap().look_ahead - 0.4).abs() < 1e-6);
    assert!(jengine_core::ecs::components::set_follow_field(&host.0, target, "nonsense", &serde_json::json!(1)).is_err());
}

#[cfg(feature = "native")]
#[test]
fn a_script_asks_where_the_camera_is() {
    use jengine_core::ecs::components::{ScriptComponent, ScriptLanguage, TextComponent};
    use jengine_core::ecs::systems::ScriptSystem;
    use jengine_core::scripting::api::ScriptCommand;
    let mut world = World::new();
    let label = world.spawn((Transform::default(), TextComponent::default()));
    let follower = world.spawn((Transform::default(), CameraFollow::default()));
    let src = format!(
        "function update(e, dt)\n  local x, y = Camera.get_position()\n  Text.set({l}, x .. \",\" .. y)\n  Camera.set_follow({f}, \"dead_zone\", {{ 300, 150 }})\nend\n",
        l = label.id(),
        f = follower.id()
    );
    let dir = std::env::temp_dir().join(format!("jengine_camfollow_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("s.lua");
    std::fs::write(&path, src).unwrap();
    world.spawn((
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    system.set_camera_view((120.0, -45.0, 1280.0, 720.0, 1.0));
    system.run(&mut world, DT, &jengine_core::input::InputState::new(), None, None);
    let text = system.api.commands.iter().find_map(|c| match c {
        ScriptCommand::SetText { text, .. } => Some(text.clone()),
        _ => None,
    });
    assert_eq!(text.as_deref(), Some("120,-45"));
    let set = system.api.commands.iter().find_map(|c| match c {
        ScriptCommand::SetCameraFollow { field, value, .. } => Some((field.clone(), value.clone())),
        _ => None,
    });
    assert_eq!(set, Some(("dead_zone".to_string(), serde_json::json!([300.0, 150.0]))));
}

#[test]
fn the_browsers_answer_get_position_from_the_published_camera() {
    let preamble = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
    assert!(preamble.contains("function get_camera_position()") && preamble.contains("p.camera_x"));
    let lua_rt = include_str!("../../../editor/src/lib/lua-runtime.ts");
    let multi = &lua_rt[lua_rt.find("const MULTI_RETURN").unwrap()..];
    let multi = &multi[..multi.find("]);").unwrap()];
    assert!(multi.contains("'get_camera_position'"), "Fengari would hand back one table, not x, y");
}
