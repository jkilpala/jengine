//! Navigation off the grid (`navigation`): a `nav_region` baked from the
//! colliders inside it, and `nav_agent`s that walk it. Every test steps the
//! real `tick_motion` that every runtime calls, and checks the agents'
//! positions frame by frame rather than only where they end up - a path
//! that cuts a corner arrives just as well.

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::ecs::behaviors::tick_motion;
use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D, Transform};
use jengine_core::navigation::{self, NavAgent, NavRegion};

const DT: f32 = 1.0 / 60.0;
const R: f32 = 16.0;

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

fn wall(world: &mut World, x: f32, y: f32, w: f32, h: f32) -> Entity {
    world.spawn((at(x, y), Collider2D { shape: ColliderShape::Box { width: w, height: h }, ..Default::default() }))
}

fn region(world: &mut World, hw: f32, hh: f32) -> Entity {
    world.spawn((
        at(0.0, 0.0),
        NavRegion { outline: vec![[-hw, -hh], [hw, -hh], [hw, hh], [-hw, hh]], agent_radius: R, ..Default::default() },
    ))
}

fn agent(world: &mut World, x: f32, y: f32, to: Option<[f32; 2]>) -> Entity {
    world.spawn((at(x, y), NavAgent { speed: 150.0, radius: R, target: to, ..Default::default() }))
}

fn pos(world: &World, e: Entity) -> Vec2 {
    world.get::<&Transform>(e).unwrap().position
}

fn moving(world: &World, e: Entity) -> bool {
    world.get::<&NavAgent>(e).unwrap().is_moving()
}

/// How far `p` is from the box at `c` with half extents `h` (0 inside).
fn box_distance(p: Vec2, c: Vec2, h: Vec2) -> f32 {
    let d = ((p - c).abs() - h).max(Vec2::ZERO);
    d.length()
}

#[test]
fn an_agent_walks_round_a_wall_and_never_touches_it() {
    let mut world = World::new();
    region(&mut world, 500.0, 300.0);
    wall(&mut world, 0.0, 0.0, 40.0, 400.0);
    let a = agent(&mut world, -300.0, 0.0, Some([300.0, 0.0]));
    let mut closest = f32::MAX;
    for _ in 0..(12.0 / DT) as usize {
        tick_motion(&mut world, DT, None, false);
        closest = closest.min(box_distance(pos(&world, a), Vec2::ZERO, Vec2::new(20.0, 200.0)));
    }
    assert!(!moving(&world, a), "it arrived and stopped");
    assert!(pos(&world, a).distance(Vec2::new(300.0, 0.0)) < 5.0, "{}", pos(&world, a));
    assert!(closest >= R - 0.5, "its body came within {closest} of the wall");
    // ...and went round the end of it, not through a gap that is not there.
    let baked = world.query::<&NavRegion>().iter().next().unwrap().1.baked.clone().expect("baked");
    assert!(!baked.nodes().is_empty());
}

#[test]
fn a_path_is_shortest_and_every_leg_is_clear() {
    let mut world = World::new();
    region(&mut world, 500.0, 300.0);
    wall(&mut world, 0.0, 0.0, 40.0, 400.0);
    let path = navigation::find_path_in(&navigation::graphs(&world), Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0))
        .expect("a way round");
    // Round the wall's end, grown by the radius: two straight legs and a
    // corner that is the grown wall's.
    let length: f32 = path.windows(2).map(|w| w[0].distance(w[1])).sum();
    let ideal = 2.0 * (Vec2::new(-20.0 - R, 200.0 + R) - Vec2::new(-200.0, 0.0)).length() + 2.0 * (20.0 + R);
    assert!(length < ideal + 12.0, "{length} against about {ideal}: {path:?}");
    let graph = &navigation::graphs(&world)[0];
    for w in path.windows(2) {
        assert!(graph.clear(w[0], w[1]), "{} -> {} crosses something", w[0], w[1]);
    }
}

#[test]
fn a_wall_that_moves_is_rebaked_and_an_agent_walking_replans() {
    let mut world = World::new();
    let r = region(&mut world, 600.0, 300.0);
    let a = agent(&mut world, -400.0, 0.0, Some([400.0, 0.0]));
    tick_motion(&mut world, DT, None, false);
    let key0 = world.get::<&NavRegion>(r).unwrap().baked.as_ref().unwrap().key();
    // A gate slides across its way once it has set off.
    let gate = wall(&mut world, 0.0, 400.0, 40.0, 440.0);
    for _ in 0..30 {
        tick_motion(&mut world, DT, None, false);
    }
    world.get::<&mut Transform>(gate).unwrap().position.y = 80.0;
    tick_motion(&mut world, DT, None, false);
    let key1 = world.get::<&NavRegion>(r).unwrap().baked.as_ref().unwrap().key();
    assert_ne!(key0, key1, "the region rebaked when the gate moved");
    let mut closest = f32::MAX;
    for _ in 0..(14.0 / DT) as usize {
        tick_motion(&mut world, DT, None, false);
        closest = closest.min(box_distance(pos(&world, a), Vec2::new(0.0, 80.0), Vec2::new(20.0, 220.0)));
    }
    assert!(!moving(&world, a) && pos(&world, a).distance(Vec2::new(400.0, 0.0)) < 5.0, "{}", pos(&world, a));
    assert!(closest >= R - 0.5, "it walked into the gate ({closest})");
    // Nothing moved since: the bake is kept, not redone every frame.
    let before = world.get::<&NavRegion>(r).unwrap().baked.clone().unwrap();
    tick_motion(&mut world, DT, None, false);
    let after = world.get::<&NavRegion>(r).unwrap().baked.clone().unwrap();
    assert!(std::sync::Arc::ptr_eq(&before, &after));
}

#[test]
fn two_agents_meeting_in_a_corridor_pass_each_other() {
    let mut world = World::new();
    region(&mut world, 500.0, 300.0);
    // A corridor 130 wide between two long walls.
    wall(&mut world, 0.0, 165.0, 800.0, 200.0);
    wall(&mut world, 0.0, -165.0, 800.0, 200.0);
    let a = agent(&mut world, -350.0, 0.0, Some([350.0, 0.0]));
    let b = agent(&mut world, 350.0, 0.0, Some([-350.0, 0.0]));
    let mut nearest = f32::MAX;
    for _ in 0..(12.0 / DT) as usize {
        tick_motion(&mut world, DT, None, false);
        nearest = nearest.min(pos(&world, a).distance(pos(&world, b)));
    }
    assert!(!moving(&world, a) && !moving(&world, b), "both got through");
    assert!(pos(&world, a).x > 340.0 && pos(&world, b).x < -340.0, "{} {}", pos(&world, a), pos(&world, b));
    assert!(nearest > 2.0 * R * 0.75, "they walked through each other ({nearest})");
}

#[test]
fn a_crowd_sent_to_one_point_stops_round_it() {
    let mut world = World::new();
    region(&mut world, 600.0, 400.0);
    wall(&mut world, 0.0, 0.0, 120.0, 120.0);
    let crowd: Vec<Entity> = (0..12)
        .map(|i| agent(&mut world, -450.0 + (i % 4) as f32 * 45.0, -150.0 + (i / 4) as f32 * 45.0, Some([300.0, 0.0])))
        .collect();
    let mut seconds = 0.0;
    while crowd.iter().any(|&e| moving(&world, e)) && seconds < 20.0 {
        tick_motion(&mut world, DT, None, false);
        seconds += DT;
    }
    assert!(seconds < 20.0, "someone never settled");
    for (i, &e) in crowd.iter().enumerate() {
        let p = pos(&world, e);
        assert!(p.distance(Vec2::new(300.0, 0.0)) < 160.0, "agent {i} stopped far off at {p}");
        assert!(box_distance(p, Vec2::ZERO, Vec2::splat(60.0)) >= R - 0.5, "agent {i} is in the wall at {p}");
        for &f in &crowd[i + 1..] {
            assert!(p.distance(pos(&world, f)) > 2.0 * R * 0.8, "two agents stand in each other");
        }
    }
}

#[test]
fn sensors_dynamic_bodies_and_masked_colliders_are_not_obstacles() {
    let mut world = World::new();
    let r = region(&mut world, 500.0, 300.0);
    let sensor = wall(&mut world, -150.0, 0.0, 40.0, 500.0);
    world.get::<&mut Collider2D>(sensor).unwrap().is_sensor = true;
    let crate_ = wall(&mut world, 0.0, 0.0, 40.0, 500.0);
    world.insert_one(crate_, RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() }).unwrap();
    let masked = wall(&mut world, 150.0, 0.0, 40.0, 500.0);
    world.get::<&mut Collider2D>(masked).unwrap().collision_membership = 0b10;
    world.get::<&mut NavRegion>(r).unwrap().obstacle_mask = 0b01;
    let path = navigation::find_path_in(&navigation::graphs(&world), Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)).unwrap();
    assert_eq!(path.len(), 2, "straight across: {path:?}");
    // A static body's collider is an obstacle.
    world.insert_one(crate_, RigidBody2D { body_type: BodyType::Static, ..Default::default() }).unwrap();
    let path = navigation::find_path_in(&navigation::graphs(&world), Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)).unwrap();
    assert!(path.len() > 2, "round the static wall: {path:?}");
}

#[test]
fn a_point_in_an_obstacle_snaps_to_its_rim() {
    let mut world = World::new();
    region(&mut world, 500.0, 300.0);
    world.spawn((at(0.0, 0.0), Collider2D { shape: ColliderShape::Circle { radius: 60.0 }, ..Default::default() }));
    let graphs = navigation::graphs(&world);
    let q = navigation::closest_point_in(&graphs, Vec2::new(10.0, 5.0)).unwrap();
    let d = q.length();
    assert!(d >= 60.0 + R && d < 60.0 + R + 4.0, "{q} is {d} from the centre");
    // A path to a click on the fountain ends at that rim.
    let path = navigation::find_path_in(&graphs, Vec2::new(-300.0, 0.0), Vec2::new(0.0, 0.0)).unwrap();
    let end = *path.last().unwrap();
    assert!(end.length() >= 60.0 + R - 0.1 && end.x < 0.0, "{end}");
    // A point outside every region is answered from the nearest.
    assert!(navigation::closest_point_in(&graphs, Vec2::new(900.0, 0.0)).unwrap().x <= 500.0 - R + 0.5);
}

#[test]
fn the_shared_commands_steer_an_agent() {
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
    let mut world = World::new();
    region(&mut world, 500.0, 300.0);
    let a = agent(&mut world, 0.0, 0.0, None);
    let mut host = Host(world);
    let send = |host: &mut Host, tag: &str, v: serde_json::Value| assert!(handle_world_command(host, tag, &JsonArgs(v)), "{tag}");
    send(&mut host, "snt", serde_json::json!({ "e": a.id(), "x": 200.0, "y": 50.0 }));
    assert_eq!(host.0.get::<&NavAgent>(a).unwrap().target, Some([200.0, 50.0]));
    send(&mut host, "snvf", serde_json::json!({ "e": a.id(), "f": "speed", "v": 40.0 }));
    assert_eq!(host.0.get::<&NavAgent>(a).unwrap().speed, 40.0);
    send(&mut host, "snt", serde_json::json!({ "e": a.id() }));
    assert_eq!(host.0.get::<&NavAgent>(a).unwrap().target, None, "no coordinates stops it");
    assert!(navigation::set_nav_field(&host.0, a, "nonsense", &serde_json::json!(1)).is_err());
}

#[cfg(feature = "native")]
#[test]
fn a_script_asks_for_a_path_and_sends_an_agent() {
    use jengine_core::ecs::components::{ScriptComponent, ScriptLanguage};
    use jengine_core::ecs::systems::ScriptSystem;
    use jengine_core::scripting::api::ScriptCommand;
    let mut world = World::new();
    region(&mut world, 500.0, 300.0);
    wall(&mut world, 0.0, 0.0, 40.0, 400.0);
    let a = agent(&mut world, -300.0, 0.0, None);
    let walking = agent(&mut world, 300.0, 200.0, Some([300.0, -200.0]));
    let src = format!(
        r#"
function update(entity, dt)
    local path = Nav.find_path(-300, 0, 300, 0)
    local last = path[#path]
    Nav.set_target({id}, #path, last[2])
    local x, y = Nav.closest_point(5, 0)
    Nav.set_target({id}, x, y)
    if Nav.find_path(-300, 0, 300, 2000) == nil then Nav.stop({id}) end
    Nav.set({id}, "speed", 90)
    if Nav.is_moving({walking}) and not Nav.is_moving({id}) then Nav.set({walking}, "radius", 11) end
end
"#,
        id = a.id(),
        walking = walking.id()
    );
    let dir = std::env::temp_dir().join(format!("jengine_nav_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("s.lua");
    std::fs::write(&path, src).unwrap();
    world.spawn((
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    system.run(&mut world, DT, &jengine_core::input::InputState::new(), None, None);
    let targets: Vec<Option<[f32; 2]>> = system
        .api
        .commands
        .iter()
        .filter_map(|c| match c {
            ScriptCommand::SetNavTarget { target, .. } => Some(*target),
            _ => None,
        })
        .collect();
    assert!(targets.len() >= 2, "{:?}", system.api.commands);
    let [n, y] = targets[0].unwrap();
    assert!(n >= 4.0 && y.abs() < 1e-3, "a path round the wall, ending at the goal: {targets:?}");
    let [x, _] = targets[1].unwrap();
    assert!(x >= 20.0 + R - 0.1, "the point in the wall snapped out of it: {targets:?}");
    // Nowhere off the region is answered from the nearest region - so it
    // has a path, and nothing was stopped.
    assert_eq!(targets.len(), 2, "{targets:?}");
    assert!(system.api.commands.iter().any(|c| matches!(c, ScriptCommand::SetNavField { field, .. } if field == "speed")));
    assert!(
        system.api.commands.iter().any(|c| matches!(c, ScriptCommand::SetNavField { field, .. } if field == "radius")),
        "Nav.is_moving told the walking agent from the idle one"
    );
}

#[test]
fn the_scene_format_keeps_them() {
    let mut world = World::new();
    region(&mut world, 300.0, 200.0);
    agent(&mut world, 10.0, 0.0, Some([50.0, 60.0]));
    tick_motion(&mut world, DT, None, false);
    let scene = jengine_core::scene::format::save_scene(&world);
    let r = scene.entities.iter().find_map(|e| e.components.nav_region.as_ref()).expect("region saved");
    assert_eq!(r.outline[1], [300.0, -200.0]);
    assert!(r.baked.is_none(), "the bake is never saved");
    let a = scene.entities.iter().find_map(|e| e.components.nav_agent.as_ref()).expect("agent saved");
    assert_eq!(a.target, Some([50.0, 60.0]));
    let json = serde_json::to_string(&scene).unwrap();
    assert!(!json.contains("\"path\"") && !json.contains("planned"), "walking state is not saved");
}

#[test]
fn navigation_runs_inside_the_motion_step() {
    let src = include_str!("../src/ecs/behaviors.rs");
    let body = &src[src.find("pub fn tick_motion(").unwrap()..];
    let body = &body[..body.find("\n}\n").unwrap()];
    assert!(body.contains("tick_navigation(world, dt)"), "tick_motion no longer walks agents");
    let nav = include_str!("../src/scripting/bridge/adapters/js/nav.rs");
    for hook in ["\"nav_find_path\"", "\"nav_closest_point\"", "\"nav_is_moving\""] {
        assert!(nav.contains(hook), "the browser hosts never answer {hook}");
    }
    // A spawned agent is known to scripts by its handle, so the browser
    // hosts hand the nav snapshot their spawn map.
    for (name, src) in [
        ("editor", include_str!("../../jengine-editor-viewport/src/lib.rs")),
        ("web", include_str!("../../jengine-web/src/lib.rs")),
        ("web scripts", include_str!("../../jengine-web/src/script_host.rs")),
    ] {
        assert!(src.contains("nav_snapshot.borrow_mut().refresh_named("), "{name} refreshes navigation without its spawn map");
        assert!(!src.contains("nav_snapshot.borrow_mut().refresh(&"), "{name} still refreshes navigation without its spawn map");
    }
}

#[test]
fn two_crowds_swap_sides_through_one_gap() {
    let mut world = World::new();
    region(&mut world, 700.0, 400.0);
    // A wall across the middle with one gap, wide enough for three abreast.
    wall(&mut world, 0.0, 250.0, 40.0, 400.0);
    wall(&mut world, 0.0, -250.0, 40.0, 400.0);
    let mut people = Vec::new();
    for i in 0..15 {
        let (x, y) = (-500.0 + (i % 3) as f32 * 50.0, -120.0 + (i / 3) as f32 * 60.0);
        people.push((agent(&mut world, x, y, Some([-x, y])), Vec2::new(-x, y)));
        people.push((agent(&mut world, -x, y, Some([x, y])), Vec2::new(x, y)));
    }
    let mut seconds = 0.0;
    let mut worst = f32::MAX;
    while people.iter().any(|&(e, _)| moving(&world, e)) && seconds < 40.0 {
        tick_motion(&mut world, DT, None, false);
        seconds += DT;
        for &(e, _) in &people {
            let p = pos(&world, e);
            worst = worst
                .min(box_distance(p, Vec2::new(0.0, 250.0), Vec2::new(20.0, 200.0)))
                .min(box_distance(p, Vec2::new(0.0, -250.0), Vec2::new(20.0, 200.0)));
        }
    }
    let stuck: Vec<Vec2> = people.iter().filter(|&&(e, _)| moving(&world, e)).map(|&(e, _)| pos(&world, e)).collect();
    assert!(stuck.is_empty(), "after {seconds:.1} s, still walking at {stuck:?}");
    assert!(worst >= R - 0.5, "someone came within {worst} of a wall");
    for &(e, goal) in &people {
        assert!(pos(&world, e).distance(goal) < 120.0, "{} stopped far from {goal}", pos(&world, e));
        assert_eq!(pos(&world, e).x.signum(), goal.x.signum(), "on the wrong side");
    }
}

#[test]
fn forty_sent_to_one_point_all_settle() {
    let mut world = World::new();
    region(&mut world, 700.0, 400.0);
    for (x, y) in [(-200.0, 100.0), (-200.0, -150.0), (100.0, 0.0), (100.0, 250.0), (100.0, -250.0)] {
        wall(&mut world, x, y, 80.0, 80.0);
    }
    let crowd: Vec<Entity> = (0..40)
        .map(|i| agent(&mut world, -600.0 + (i % 5) as f32 * 40.0, -280.0 + (i / 5) as f32 * 70.0, Some([450.0, 0.0])))
        .collect();
    let started = std::time::Instant::now();
    let mut seconds = 0.0;
    while crowd.iter().any(|&e| moving(&world, e)) && seconds < 40.0 {
        tick_motion(&mut world, DT, None, false);
        seconds += DT;
    }
    let frames = seconds / DT;
    eprintln!("40 agents settled in {seconds:.1} s, {:.3} ms a frame", started.elapsed().as_secs_f64() * 1000.0 / frames as f64);
    assert!(seconds < 40.0, "someone never settled");
    for (i, &e) in crowd.iter().enumerate() {
        for &f in &crowd[i + 1..] {
            assert!(pos(&world, e).distance(pos(&world, f)) > 2.0 * R * 0.8, "two agents stand in each other");
        }
        assert!(pos(&world, e).distance(Vec2::new(450.0, 0.0)) < 220.0, "agent {i} stopped at {}", pos(&world, e));
    }
}
