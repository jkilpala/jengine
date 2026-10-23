//! `Parallax.set` / `Parallax.set_auto_scroll` reach the layer through
//! `handle_package_command`, the one dispatch every runtime uses.
//!
//! The package's older `set_parallax` globals are `ScriptApiRegistry`
//! handlers, which only the native `ScriptSystem` installs - in the editor
//! and on the web they were `nil`, while the parallax guide documented them
//! as working. These commands are the cross-runtime form.

use hecs::{Entity, World};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::host::PackageHost;
use jengine_core::package::script_commands::handle_package_command;
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost};
use jengine_core::scripting::namespaces::package_entries;
use jengine_parallax::{ParallaxLayer, ParallaxPackage};
use serde_json::json;

struct TestHost<'a>(&'a mut World);

impl ScriptWorldHost for TestHost<'_> {
    fn world(&mut self) -> &mut World {
        self.0
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
    }
}

fn host() -> PackageHost {
    let mut registry = ComponentRegistry::new();
    jengine_core::package::core_registration::register_core_components(&mut registry);
    let mut host = PackageHost::new();
    host.add_package(Box::new(ParallaxPackage));
    host.initialize(&mut registry);
    host
}

#[test]
fn commands_set_the_layer() {
    let host = host();
    let mut world = World::new();
    let e = world.spawn((ParallaxLayer::default(),));
    let id = e.id();

    for (name, x, y) in [("parallax_set", 0.25, 0.75), ("parallax_set_auto_scroll", 12.0, -3.0)] {
        let tag = format!("pkg/jengine-parallax/{name}");
        let args = JsonArgs::new(json!({ "e": id, "x": x, "y": y }));
        assert!(
            handle_package_command(&mut TestHost(&mut world), &tag, &args, host.script_commands()),
            "{tag} is not registered"
        );
    }

    let layer = world.get::<&ParallaxLayer>(e).unwrap();
    assert_eq!((layer.scroll_factor_x, layer.scroll_factor_y), (0.25, 0.75));
    assert_eq!((layer.auto_scroll_x, layer.auto_scroll_y), (12.0, -3.0));
}

#[test]
fn scripts_call_them_as_parallax_set() {
    let host = host();
    let calls: Vec<String> = package_entries(host.script_commands().commands())
        .into_iter()
        .map(|n| format!("{}.{}", n.namespace, n.short))
        .collect();
    assert_eq!(calls, ["Parallax.set", "Parallax.set_auto_scroll"]);
}
