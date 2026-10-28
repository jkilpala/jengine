//! **Off-screen markers** (Oct 2026; `ecs::offscreen_marker`): a marker kept
//! at the screen's edge pointing at a target out of sight, an icon over it
//! once in view. The placing is unit-tested in the module; this checks the
//! parts a host relies on - `Marker.set` retargets a marker through the
//! shared host command a browser script reaches, the motion step places it,
//! and the frame records the screen's view for it. Found by the showcase's
//! Beacon Run.
#![cfg(feature = "offscreen_marker")]

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::ecs::components::{Name, SpriteComponent, Transform};
use jengine_core::ecs::offscreen_marker::{MainView, OffscreenMarker, set_main_view};
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

#[test]
fn a_script_retargets_a_marker_and_the_motion_step_places_it() {
    let mut world = World::new();
    world.spawn((Name("North".into()), Transform { position: Vec2::new(0.0, 3000.0), ..Default::default() }));
    world.spawn((Name("East".into()), Transform { position: Vec2::new(3000.0, 0.0), ..Default::default() }));
    let m = world.spawn((Transform::default(), SpriteComponent::default(), OffscreenMarker { target: "North".into(), ..Default::default() }));
    set_main_view(Some(MainView { center: Vec2::ZERO, axis_x: Vec2::X, axis_y: Vec2::Y, half: Vec2::new(640.0, 360.0) }));
    jengine_core::ecs::behaviors::tick_motion(&mut world, 1.0 / 60.0, None, false);
    let at = world.get::<&Transform>(m).unwrap().position;
    assert!((at - Vec2::new(0.0, 312.0)).length() < 1e-3, "at the top edge: {at}");
    let mut host = Host(world);
    let args = JsonArgs(serde_json::json!({ "e": m.id(), "f": "target", "v": "East" }));
    assert!(handle_world_command(&mut host, "mkst", &args), "a shared command");
    jengine_core::ecs::behaviors::tick_motion(&mut host.0, 1.0 / 60.0, None, false);
    let at = host.0.get::<&Transform>(m).unwrap().position;
    assert!((at - Vec2::new(592.0, 0.0)).length() < 1e-3, "now at the right edge: {at}");
    set_main_view(None);
}

#[test]
fn the_frame_records_the_screens_view() {
    let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ecs/frame_draw.rs")).unwrap();
    assert!(src.contains("views::note_main_view(&main_camera)"), "draw_frame notes the main camera's view");
}
