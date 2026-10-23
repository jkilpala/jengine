//! A package command receives table arguments whole on native.
//!
//! The Luau binding for package commands converted arguments with a flat
//! converter that kept only arrays of numbers: a list of strings or a keyed
//! table arrived as null, while the editor's runtime passed both through.
//! Found building the arcade's Snake, whose levels are passed to the grid
//! package as a list of row strings.

#![cfg(feature = "native")]

use std::sync::Mutex;

use hecs::World;
use jengine_core::ecs::components::{Name, ScriptComponent, ScriptLanguage, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::host::PackageHost;
use jengine_core::package::script_commands::{install_global, PackageCommandRegistry};
use jengine_core::package::script_api::ScriptApiRegistry;
use jengine_core::package::system_schedule::SystemSchedule;
use jengine_core::package::{JEnginePackage, PackageMetadata};
use serde_json::json;

static SEEN: Mutex<Option<serde_json::Value>> = Mutex::new(None);

struct Recorder;

impl JEnginePackage for Recorder {
    fn metadata(&self) -> PackageMetadata {
        PackageMetadata {
            name: "recorder".into(),
            version: "0".into(),
            description: String::new(),
            author: String::new(),
            engine_version: None,
            dependencies: vec![],
        }
    }
    fn register_components(&self, _registry: &mut ComponentRegistry) {}
    fn register_systems(&self, _schedule: &mut SystemSchedule) {}
    fn register_script_api(&self, _api: &mut ScriptApiRegistry) {}
    fn register_script_commands(&self, commands: &mut PackageCommandRegistry) {
        commands
            .register("recorder", "record_args", &["rows", "nested"], |_host, args| {
                *SEEN.lock().unwrap() = Some(json!({ "rows": args.json("rows"), "nested": args.json("nested") }));
            })
            .unwrap();
    }
}

const SRC: &str = r##"
function init(entity)
    record_args({ "#..#", "...." }, { name = "level", sizes = { 3, 4 } })
end
"##;

#[test]
fn table_arguments_reach_the_handler_intact() {
    let dir = std::env::temp_dir().join(format!(
        // Unique per run: a fixed name here is a race between parallel
        // test binaries and anything still holding the last run's file.
        "{}_{}_{}",
        "jengine_package_table_args",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("args.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut registry = ComponentRegistry::new();
    jengine_core::package::core_registration::register_core_components(&mut registry);
    let mut host = PackageHost::new();
    host.add_package(Box::new(Recorder));
    host.initialize(&mut registry);

    let mut system = ScriptSystem::new().expect("script system");
    system.install_package_commands(host.script_commands());
    install_global(host.take_script_commands());

    let mut world = World::new();
    world.spawn((
        Name("Caller".into()),
        Transform::default(),
        ScriptComponent {
            language: ScriptLanguage::Lua,
            source_path: path.to_string_lossy().into(),
            cull_distance: None,
        },
    ));
    let input = InputState::new();
    for _ in 0..2 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }

    let seen = SEEN.lock().unwrap().clone().expect("the command never ran");
    assert_eq!(seen["rows"], json!(["#..#", "...."]), "a list of strings: {seen}");
    assert_eq!(seen["nested"], json!({ "name": "level", "sizes": [3, 4] }), "a keyed table: {seen}");
}
