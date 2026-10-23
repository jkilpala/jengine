//! `visibility_notifier` (`ecs::visibility`): an entity's script hears when
//! it comes on screen and when it leaves, and `pause_offscreen` holds its
//! animation and particles while it is out of view.

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::animation::SpriteAnimator;
use jengine_core::ecs::components::{Parent, SpriteComponent, Transform};
use jengine_core::ecs::hierarchy::resolve_world_transforms;
use jengine_core::ecs::visibility::{VisibilityNotifier, tick_visibility};
use jengine_core::particles::ParticleEmitter;

/// A 200 x 100 view about the origin.
const VIEW: (Vec2, Vec2) = (Vec2::ZERO, Vec2::new(100.0, 50.0));

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

fn step(world: &mut World) -> Vec<(Entity, bool)> {
    resolve_world_transforms(world);
    tick_visibility(world, VIEW).into_iter().map(|c| (c.entity, c.entered)).collect()
}

fn move_to(world: &mut World, e: Entity, x: f32, y: f32) {
    world.get::<&mut Transform>(e).unwrap().position = Vec2::new(x, y);
}

#[test]
fn it_hears_coming_and_going_once_each() {
    let mut world = World::new();
    let far = world.spawn((at(500.0, 0.0), VisibilityNotifier::default()));
    let near = world.spawn((at(0.0, 0.0), VisibilityNotifier::default()));
    // The first frame says what is on screen and nothing about what is not.
    assert_eq!(step(&mut world), vec![(near, true)]);
    assert!(step(&mut world).is_empty(), "nothing changed, nothing said");

    move_to(&mut world, far, 60.0, 0.0);
    assert_eq!(step(&mut world), vec![(far, true)]);
    move_to(&mut world, near, 0.0, -80.0);
    assert_eq!(step(&mut world), vec![(near, false)]);
    move_to(&mut world, near, 0.0, -81.0);
    assert!(step(&mut world).is_empty(), "still gone");
}

#[test]
fn the_box_is_the_sprite_and_the_margin_reaches_out() {
    let mut world = World::new();
    // A 100 x 40 sprite standing on its feet just below the view: its quad
    // reaches up into it.
    let tall = world.spawn((
        at(0.0, -80.0),
        SpriteComponent { size: Some([100.0, 40.0]), anchor: [0.5, 0.0], ..Default::default() },
        VisibilityNotifier::default(),
    ));
    // The same hanging down from its top: entirely below.
    let hung = world.spawn((
        at(0.0, -80.0),
        SpriteComponent { size: Some([100.0, 40.0]), anchor: [0.5, 1.0], ..Default::default() },
        VisibilityNotifier::default(),
    ));
    // A point just past the right edge, noticed early by a margin.
    let early = world.spawn((at(130.0, 0.0), VisibilityNotifier { margin: 40.0, ..Default::default() }));
    // An explicit box, off to one side of its entity.
    let boxed = world.spawn((
        at(-160.0, 0.0),
        VisibilityNotifier { size: [20.0, 20.0], offset: [60.0, 0.0], ..Default::default() },
    ));
    let seen: Vec<Entity> = step(&mut world).into_iter().map(|(e, _)| e).collect();
    assert!(seen.contains(&tall), "the quad reaches into the view");
    assert!(!seen.contains(&hung), "the quad hangs below it");
    assert!(seen.contains(&early), "the margin reaches out to it");
    assert!(seen.contains(&boxed), "the box is where its offset puts it");
}

#[test]
fn a_child_is_seen_where_the_hierarchy_puts_it() {
    let mut world = World::new();
    let root = world.spawn((at(400.0, 0.0),));
    let child = world.spawn((at(-380.0, 0.0), Parent(root), VisibilityNotifier::default()));
    assert_eq!(step(&mut world), vec![(child, true)], "400 - 380 is on screen");
    move_to(&mut world, root, 600.0, 0.0);
    assert_eq!(step(&mut world), vec![(child, false)]);
}

#[test]
fn pausing_offscreen_holds_and_resumes_only_what_was_running() {
    let mut world = World::new();
    let mut anim = SpriteAnimator::default();
    anim.playing = true;
    let emitter = ParticleEmitter { active: false, ..Default::default() };
    let e = world.spawn((
        at(0.0, 0.0),
        anim,
        emitter,
        VisibilityNotifier { pause_offscreen: true, ..Default::default() },
    ));
    step(&mut world);
    move_to(&mut world, e, 1000.0, 0.0);
    step(&mut world);
    assert!(!world.get::<&SpriteAnimator>(e).unwrap().playing, "held while off screen");
    move_to(&mut world, e, 0.0, 0.0);
    step(&mut world);
    assert!(world.get::<&SpriteAnimator>(e).unwrap().playing, "and goes on when back");
    assert!(!world.get::<&ParticleEmitter>(e).unwrap().active, "what was stopped stays stopped");
}

#[test]
fn the_scene_format_keeps_it_but_not_its_state() {
    let mut world = World::new();
    world.spawn((at(0.0, 0.0), VisibilityNotifier { margin: 12.0, pause_offscreen: true, ..Default::default() }));
    step(&mut world);
    let scene = jengine_core::scene::format::save_scene(&world);
    let n = scene.entities.iter().find_map(|e| e.components.visibility_notifier.as_ref()).expect("saved");
    assert_eq!(n.margin, 12.0);
    assert!(n.pause_offscreen);
    assert_eq!(n.on_screen, None);
}

#[test]
fn every_host_checks_and_tells_the_script() {
    for (name, src, dispatch) in [
        (
            "native",
            include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs"),
            "script_system.queue_entity_callbacks(&jengine_core::ecs::callbacks::take())",
        ),
        ("editor", include_str!("../../jengine-editor-viewport/src/lib.rs"), "for owed in jengine_core::ecs::callbacks::take()"),
        ("web", include_str!("../../jengine-web/src/lib.rs"), "script_host::dispatch_entity_callbacks(app)"),
    ] {
        assert!(src.contains("visibility::tick_visibility("), "{name} never checks visibility notifiers");
        assert!(src.contains(dispatch), "{name} owes scripts callbacks and never pays them");
    }
    let web_host = include_str!("../../jengine-web/src/script_host.rs");
    let at = web_host.find("pub fn dispatch_entity_callbacks(").expect("web dispatches entity callbacks");
    assert!(web_host[at..at + 300].contains("call_lifecycle_handler("));
    let editor = include_str!("../../jengine-editor-viewport/src/lib.rs");
    let at = editor.find("for owed in jengine_core::ecs::callbacks::take()").unwrap();
    assert!(editor[at..at + 200].contains("call_lifecycle_handler("));
}

#[cfg(feature = "native")]
#[test]
fn the_native_script_hears_it() {
    use jengine_core::ecs::components::{CustomComponents, ScriptComponent, ScriptLanguage};
    use jengine_core::ecs::systems::ScriptSystem;
    use jengine_core::input::InputState;

    let dir = std::env::temp_dir().join(format!("jengine_visibility_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("watched.lua");
    std::fs::write(
        &path,
        r#"
function update(entity, dt)
end

function on_screen_entered(entity)
    set_custom(entity, "Probe", "entered", (get_custom(entity, "Probe", "entered") or 0) + 1)
end

function on_screen_exited(entity)
    set_custom(entity, "Probe", "exited", (get_custom(entity, "Probe", "exited") or 0) + 1)
end
"#,
    )
    .unwrap();
    let mut world = World::new();
    let e = world.spawn((
        at(0.0, 0.0),
        CustomComponents(Default::default()),
        VisibilityNotifier::default(),
        ScriptComponent {
            language: ScriptLanguage::Lua,
            source_path: path.to_string_lossy().into(),
            cull_distance: None,
        },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    let frame = |world: &mut World, system: &mut ScriptSystem| {
        system.run(world, 1.0 / 60.0, &input, None, None);
        resolve_world_transforms(world);
        tick_visibility(world, VIEW);
        system.queue_entity_callbacks(&jengine_core::ecs::callbacks::take());
    };
    frame(&mut world, &mut system);
    frame(&mut world, &mut system);
    move_to(&mut world, e, 900.0, 0.0);
    frame(&mut world, &mut system);
    frame(&mut world, &mut system);
    frame(&mut world, &mut system);
    let read = |field: &str| {
        let c = world.get::<&CustomComponents>(e).unwrap();
        c.0.get("Probe").and_then(|p| p.get(field)).and_then(|v| v.as_f64())
    };
    assert_eq!(read("entered"), Some(1.0), "heard it come on screen");
    assert_eq!(read("exited"), Some(1.0), "and leave");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A late callback - this one, `on_collision_enter`, `on_animation_event` -
/// can tell other scripts things. Its `send_event` used to be applied after
/// the frame's listener dispatch and then dropped, natively only: Test Lab
/// scene 170's counter stayed at zero while every lantern lit.
#[cfg(feature = "native")]
#[test]
fn what_a_screen_handler_sends_reaches_a_listener() {
    use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage};
    use jengine_core::ecs::systems::ScriptSystem;
    use jengine_core::input::InputState;

    let dir = std::env::temp_dir().join(format!("jengine_visibility_events_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let lantern = dir.join("lantern.lua");
    let counter = dir.join("counter.lua");
    std::fs::write(&lantern, r#"
function on_screen_entered(entity)
    Event.send("lantern_seen", entity, { d = 1 })
end
"#).unwrap();
    std::fs::write(&counter, r#"
function init(entity)
    Event.on("lantern_seen", entity)
end

function update(entity, dt)
end

-- Counted here: two calls in one frame both read the frame's snapshot, so
-- `get_custom` would give each the same starting value.
local lit = 0
function on_lantern_seen(entity, data)
    lit = lit + data.d
    set_custom(entity, "Probe", "lit", lit)
end
"#).unwrap();
    let script = |p: &std::path::Path| ScriptComponent {
        language: ScriptLanguage::Lua,
        source_path: p.to_string_lossy().into(),
        cull_distance: None,
    };
    let mut world = World::new();
    for x in [0.0, 30.0] {
        world.spawn((at(x, 0.0), VisibilityNotifier::default(), script(&lantern)));
    }
    let c = world.spawn((Name("Counter".into()), Transform::default(), CustomComponents(Default::default()), script(&counter)));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..4 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
        resolve_world_transforms(&mut world);
        tick_visibility(&mut world, VIEW);
        system.queue_entity_callbacks(&jengine_core::ecs::callbacks::take());
    }
    let lit = world
        .get::<&CustomComponents>(c)
        .unwrap()
        .0
        .get("Probe")
        .and_then(|p| p.get("lit"))
        .and_then(|v| v.as_f64());
    assert_eq!(lit, Some(2.0), "both lanterns' events were heard");
    let _ = std::fs::remove_dir_all(&dir);
}
