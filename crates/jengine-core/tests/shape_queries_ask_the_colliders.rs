//! **Shape queries** (Oct 2026): `Physics.overlap_circle`, `overlap_box`,
//! `overlap_point` and `circle_cast` ask the colliders what a circle, a box
//! or a point touches, and where a swept circle first meets something.
//! Before them a script had `raycast` (a line with no width) and
//! `find_in_radius` (transforms, not colliders), so a blast missed a long
//! wall whose centre was out of its radius. Found by the showcase's Fuse &
//! Fortress.
//!
//! The geometry is unit-tested in `physics::query`; this checks the two ends
//! that feed it - the live collider set natively, the ray snapshot in the
//! browsers - agree, a real Luau script, and the browser preamble's calls.

#![cfg(feature = "physics")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{
    BodyType, Collider2D, ColliderShape, CustomComponents, RigidBody2D, ScriptComponent, ScriptLanguage, Transform,
};
use jengine_core::ecs::systems::{ScriptSystem, ensure_physics_bodies};
use jengine_core::physics::PhysicsWorld;
use jengine_core::physics::query::QueryShape;
use jengine_core::physics::ray::RaySnapshot;
use jengine_core::scripting::bridge::BridgeReadSnapshot;

fn body(world: &mut World, x: f32, y: f32, shape: ColliderShape, kind: BodyType) -> hecs::Entity {
    world.spawn((
        Transform { position: Vec2::new(x, y), ..Default::default() },
        RigidBody2D { body_type: kind, ..Default::default() },
        Collider2D { shape, ..Default::default() },
    ))
}

/// A long wall along y = 0, a crate on it, a ball floating up and right.
fn scene() -> (World, [hecs::Entity; 3]) {
    let mut world = World::new();
    let wall = body(&mut world, 0.0, 0.0, ColliderShape::Box { width: 600.0, height: 20.0 }, BodyType::Static);
    let crate_ = body(&mut world, -100.0, 30.0, ColliderShape::Box { width: 40.0, height: 40.0 }, BodyType::Static);
    let ball = body(&mut world, 200.0, 150.0, ColliderShape::Circle { radius: 25.0 }, BodyType::Static);
    (world, [wall, crate_, ball])
}

fn queries() -> Vec<QueryShape> {
    vec![
        QueryShape::Circle { x: 280.0, y: 30.0, radius: 25.0 },
        QueryShape::Circle { x: -100.0, y: 60.0, radius: 300.0 },
        QueryShape::Box { x: 200.0, y: 90.0, width: 10.0, height: 170.0, rotation: 0.0 },
        QueryShape::Box { x: 200.0, y: 90.0, width: 10.0, height: 120.0, rotation: 1.5708 },
        QueryShape::Point { x: -90.0, y: 40.0 },
        QueryShape::Point { x: 0.0, y: 100.0 },
    ]
}

#[test]
fn native_and_the_browser_snapshot_give_the_same_answers() {
    let (world, [wall, crate_, ball]) = scene();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    ensure_physics_bodies(&world, &mut physics);
    physics.step(1.0 / 60.0);
    let native = BridgeReadSnapshot { physics_ptr: Some(&physics as *const _), ..Default::default() };
    let mut browser = RaySnapshot::new();
    browser.refresh(&world);
    let id = |e: hecs::Entity| e.id() as u64;
    let expected: Vec<Vec<u64>> = vec![
        vec![id(wall)],                     // the wall's far end, its centre 280 away
        vec![id(wall), id(crate_), id(ball)],
        vec![id(wall), id(ball)],           // upright: from the wall up into the ball
        vec![],                             // turned flat between them
        vec![id(crate_)],
        vec![],
    ];
    for (q, want) in queries().iter().zip(&expected) {
        let mut a = native.overlap(q);
        a.sort_unstable();
        let b = browser.overlap(q);
        assert_eq!(&a, want, "native, {q:?}");
        assert_eq!(&b, want, "browser, {q:?}");
    }
    // A circle swept down beside the ball's middle: a ray would miss it.
    let n = native.circle_cast(230.0, 400.0, 0.0, -1.0, 12.0, 1000.0).expect("native hit");
    let b = browser.circle_cast(230.0, 400.0, 0.0, -1.0, 12.0, 1000.0).expect("browser hit");
    assert_eq!((n.entity_id, b.entity_id), (id(ball), id(ball)));
    assert!((n.distance - b.distance).abs() < 0.01 && (n.point_y - b.point_y).abs() < 0.01, "{n:?} {b:?}");
    assert!(native.raycast(230.0, 400.0, 0.0, -1.0, 1000.0).is_some_and(|h| h.entity_id == id(wall)), "the ray goes past it");
}

#[cfg(feature = "native")]
#[test]
fn a_script_asks_the_colliders() {
    const SRC: &str = r#"
function update(entity, dt)
    local near = Physics.overlap_circle(280, 30, 25)
    Entity.set_custom(entity, "Q", "circle_n", #near)
    Entity.set_custom(entity, "Q", "circle_first", near[1] or -1)
    Entity.set_custom(entity, "Q", "box_n", #Physics.overlap_box(200, 90, 10, 170))
    Entity.set_custom(entity, "Q", "flat_n", #Physics.overlap_box(200, 90, 10, 120, 1.5708))
    Entity.set_custom(entity, "Q", "point_n", #Physics.overlap_point(-90, 40))
    local hit = Physics.circle_cast(230, 400, 0, -1, 12, 1000)
    Entity.set_custom(entity, "Q", "cast_id", hit and hit.entity_id or -1)
    Entity.set_custom(entity, "Q", "cast_d", hit and hit.distance or -1)
    Entity.set_custom(entity, "Q", "miss", Physics.circle_cast(230, 400, 0, 1, 12, 1000) == nil)
end
"#;
    let (mut world, [wall, _, ball]) = scene();
    let dir = std::env::temp_dir().join(format!("jengine_shape_queries_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("q.lua");
    std::fs::write(&path, SRC).unwrap();
    let e = world.spawn((
        Transform::default(),
        CustomComponents::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    ensure_physics_bodies(&world, &mut physics);
    physics.step(1.0 / 60.0);
    let mut system = ScriptSystem::new().expect("script system");
    system.set_physics_ptr(&physics);
    system.run(&mut world, 1.0 / 60.0, &jengine_core::input::InputState::new(), None, None);
    system.clear_physics_ptr();
    let custom = world.get::<&CustomComponents>(e).unwrap();
    let q = custom.0.get("Q").expect("the script wrote its answers");
    let n = |k: &str| q.get(k).and_then(|v| v.as_f64()).unwrap_or(f64::NAN);
    assert_eq!(n("circle_n"), 1.0, "{q}");
    assert_eq!(n("circle_first"), wall.id() as f64);
    assert_eq!((n("box_n"), n("flat_n"), n("point_n")), (2.0, 0.0, 1.0));
    assert_eq!(n("cast_id"), ball.id() as f64);
    assert!(n("cast_d") > 200.0 && n("cast_d") < 240.0, "stopped above the ball: {q}");
    assert_eq!(q.get("miss").and_then(|v| v.as_bool()), Some(true));
}

/// The browser preamble passes each query to the host's one overlap hook with
/// the right kind and arguments, and a missing hook answers empty / nil.
#[test]
fn the_browser_preamble_asks_the_host() {
    const SRC: &str = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
    let lift = |name: &str| -> String {
        let start = SRC.find(&format!("  function {name}(")).unwrap_or_else(|| panic!("{name} is in the preamble"));
        let end = start + SRC[start..].find("\n").unwrap();
        SRC[start..end].replace("{{", "{").replace("}}", "}")
    };
    let script = format!(
        "var window = {{ __jengine_play: {{ state: {{\n\
           physics_overlap: function (k, x, y, a, b, c) {{ return [k, x, y, a, b, c]; }},\n\
           physics_circle_cast: function (ox, oy, dx, dy, r, m) {{ return {{ d: ox + oy + dx + dy + r + m }}; }} }} }} }};\n\
         {}\n{}\n{}\n{}\n\
         var got = [overlap_circle(1, 2, 3), overlap_box(1, 2, 3, 4), overlap_box(1, 2, 3, 4, 5), overlap_point(1, 2), circle_cast(1, 2, 3, 4, 5, 6).d];\n\
         window.__jengine_play.state = {{}};\n\
         got.push(overlap_circle(1, 2, 3).length, circle_cast(1, 2, 3, 4, 5, 6));\n\
         console.log(JSON.stringify(got));",
        lift("overlap_circle"),
        lift("overlap_box"),
        lift("overlap_point"),
        lift("circle_cast"),
    );
    let out = match std::process::Command::new("node").arg("-e").arg(&script).output() {
        Ok(out) => out,
        Err(_) => {
            eprintln!("the_browser_preamble_asks_the_host: no node - skipped");
            return;
        }
    };
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        text.trim(),
        "[[0,1,2,3,0,0],[1,1,2,3,4,0],[1,1,2,3,4,5],[2,1,2,0,0,0],21,0,null]",
        "{text}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
