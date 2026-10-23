//! A package system can read the frame: the host's input and camera.
//!
//! Until Sep 2026 `SystemContext` was a world, a delta time and a resource
//! map nothing engine-side was ever put in - no input, no camera - so a
//! package could not read a key. A platformer package had to be fed the
//! player's input by a script command every frame (`plat_input`), which is
//! the shape every performance note here warns about, and the first thing a
//! third party writing a package would hit.
//!
//! `PackageHost::run_phase_with` lends the input and camera for the phase.
//! The input is *moved* in and back out (it owns maps that would allocate to
//! clone), so the second test is that the host has it back afterwards, and
//! the third that every host that runs packages actually goes through the
//! lending form.

use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::input::{InputState, KeyCode};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::host::{FrameCamera, FrameResources, PackageHost};
use jengine_core::package::script_api::ScriptApiRegistry;
use jengine_core::package::system_schedule::{SystemPhase, SystemSchedule};
use jengine_core::package::{JEnginePackage, PackageMetadata};
use jengine_core::renderer::camera::Camera2D;

/// What the system saw, written into the world so the test can read it.
struct Seen {
    space_down: bool,
    camera_zoom: f32,
    mouse_world_x: f32,
    saw_the_probe_texture: bool,
}

struct Peeker;

impl JEnginePackage for Peeker {
    fn metadata(&self) -> PackageMetadata {
        PackageMetadata {
            name: "peeker".into(),
            version: "0".into(),
            description: String::new(),
            author: String::new(),
            engine_version: None,
            dependencies: vec![],
        }
    }
    fn register_components(&self, _: &mut ComponentRegistry) {}
    fn register_systems(&self, schedule: &mut SystemSchedule) {
        schedule.add_system("peek", SystemPhase::PreUpdate, 0, |ctx| {
            let (world, _dt, resources) = ctx.split();
            let Some(input) = resources.get::<InputState>() else { return };
            let Some(cam) = resources.get::<FrameCamera>() else { return };
            let Some(assets) = resources.get::<AssetManager>() else { return };
            let (mx, my) = input.mouse_position;
            let (sw, sh) = input.screen_size;
            let at = cam.0.screen_to_world(glam::Vec2::new(mx, my), glam::Vec2::new(sw, sh));
            world.spawn((Seen {
                space_down: input.is_pressed(KeyCode::Space),
                camera_zoom: cam.0.zoom,
                mouse_world_x: at.x,
                saw_the_probe_texture: assets.has_texture_bytes("probe.png"),
            },));
        });
    }
    fn register_script_api(&self, _: &mut ScriptApiRegistry) {}
}

fn host() -> PackageHost {
    let mut registry = ComponentRegistry::new();
    let mut host = PackageHost::new();
    host.add_package(Box::new(Peeker));
    host.initialize(&mut registry);
    host
}

#[test]
fn a_package_reads_the_input_and_the_camera() {
    let mut host = host();
    let mut world = World::new();
    let mut input = InputState::new();
    input.begin_frame();
    input.key_down(KeyCode::Space);
    input.screen_size = (800.0, 600.0);
    input.mouse_position = (800.0, 300.0); // right edge, vertical centre
    let mut camera = Camera2D::new(800.0, 600.0);
    camera.zoom = 2.0;
    camera.position = glam::Vec2::new(1000.0, 0.0);
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(
        "probe.png".into(),
        jengine_core::asset::RawTextureData { width: 1, height: 1, rgba: vec![255; 4] },
    );

    host.run_phase_with(
        SystemPhase::PreUpdate,
        &mut world,
        1.0 / 60.0,
        FrameResources { input: &mut input, assets: &mut assets, camera },
    );

    let seen = world.query::<&Seen>().iter().map(|(_, s)| (s.space_down, s.camera_zoom, s.mouse_world_x, s.saw_the_probe_texture)).next();
    let (space_down, zoom, mouse_world_x, saw_the_probe_texture) = seen.expect("the system ran and saw the frame");
    assert!(saw_the_probe_texture, "the assets the host holds");
    assert!(assets.has_texture_bytes("probe.png"), "and the host has them back");
    assert!(space_down, "the key the host holds down");
    assert_eq!(zoom, 2.0, "the camera the frame is drawn through");
    // Half a viewport to the right at zoom 2 is 200 world units.
    assert!((mouse_world_x - 1200.0).abs() < 0.01, "the mouse, converted through that camera: {mouse_world_x}");
}

#[test]
fn the_host_has_its_input_back_after_the_phase() {
    let mut host = host();
    let mut world = World::new();
    let mut input = InputState::new();
    input.begin_frame();
    input.key_down(KeyCode::Space);
    input.set_bindings("jump", vec!["Space".into()]);

    let mut assets = AssetManager::new();
    host.run_phase_with(SystemPhase::PreUpdate, &mut world, 0.0, FrameResources { input: &mut input, assets: &mut assets, camera: Camera2D::new(1.0, 1.0) });

    assert!(input.is_pressed(KeyCode::Space), "the key is still down for the rest of the frame");
    assert!(input.is_action_pressed("jump"), "and the bindings came back with it");
}

/// Every host that runs packages lends the frame. A bare `run_phase` in any
/// would hand packages a world and nothing else, silently, in that runtime
/// only - the shape of every parity bug in this repo.
#[test]
fn every_host_lends_the_frame_to_packages() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for (host, path) in [
        ("native runtime", "jengine-runtime/src/wgpu_scene_runner.rs"),
        ("editor viewport", "jengine-editor-viewport/src/lib.rs"),
        ("web runtime", "jengine-web/src/lib.rs"),
    ] {
        let src = std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("read {path}: {e}"));
        let bare: Vec<(usize, &str)> = src
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim_start().starts_with("//") && l.contains(".run_phase(") && !l.contains("run_phase_with("))
            .map(|(i, l)| (i + 1, l.trim()))
            .collect();
        assert!(bare.is_empty(), "{host} calls the bare `run_phase`, which lends packages nothing: {bare:?}");
        let phases = src.matches("run_phase_with(").count();
        assert!(phases >= 6, "{host} runs {phases} package phases through `run_phase_with`; expected all six");
    }
}
