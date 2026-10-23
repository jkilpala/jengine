//! `Mesh.set(entity, vertices, indices)` - a mesh shaped by a script.
//!
//! `Mesh2D` could only be authored in a scene or written by a package, so a
//! script that wanted a waving flag, a ripple or a shape of its own had
//! nothing to call. The vertices and indices are read by one function,
//! `mesh_from_script`, natively and in both browser hosts; indices count from
//! 1, as Lua does.

#![cfg(feature = "native")]

use hecs::{Entity, World};
use jengine_core::ecs::components::{Mesh2D, MeshVertex, Name, ScriptComponent, ScriptLanguage, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command, mesh_from_script};
use serde_json::json;

const SRC: &str = r#"
local frames = 0
function update(entity, dt)
    frames = frames + 1
    local flag = Entity.find_by_name("Flag")
    local blank = Entity.find_by_name("Blank")
    if frames == 1 then
        -- A quad as two triangles, one vertex keyed and tinted.
        Mesh.set(flag, {
            { -10, -5, 0, 1 }, { 10, -5, 1, 1 }, { 10, 5, 1, 0, 0.5 },
            { x = -10, y = 5, u = 0, v = 0, r = 0.5, a = 0.25 },
        }, { 1, 2, 3, 1, 3, 4 })
        -- No indices: a fan. And the entity had no mesh at all.
        Mesh.set(blank, { { 0, 0, 0, 0 }, { 4, 0, 1, 0 }, { 0, 4, 0, 1 } })
    end
end
"#;

#[test]
fn a_script_shapes_a_mesh_natively() {
    let dir = std::env::temp_dir().join(format!(
        "jengine_meshes_from_scripts_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mesh.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    let flag = world.spawn((
        Name("Flag".into()),
        Transform::default(),
        Mesh2D { texture_id: "flag.png".into(), z_order: 3, ..Default::default() },
    ));
    let blank = world.spawn((Name("Blank".into()), Transform::default()));
    world.spawn((
        Name("Driver".into()),
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..2 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }
    let m = world.get::<&Mesh2D>(flag).unwrap();
    assert_eq!((m.texture_id.as_str(), m.z_order), ("flag.png", 3), "the rest of the component is kept");
    assert_eq!(m.vertices.len(), 4);
    assert_eq!(m.indices, vec![0, 1, 2, 0, 2, 3], "1-based in Lua, 0-based in the component");
    assert_eq!(m.vertices[1], MeshVertex { position: [10.0, -5.0], uv: [1.0, 1.0], ..Default::default() });
    assert_eq!(m.vertices[2].q, 0.5, "a fifth number is q");
    assert_eq!(m.vertices[3].color, [0.5, 1.0, 1.0, 0.25], "keyed: what is left out is the default");
    let b = world.get::<&Mesh2D>(blank).expect("a mesh is added where there was none");
    assert_eq!((b.vertices.len(), b.indices.len()), (3, 0), "no indices is a fan");
}

#[test]
fn a_bad_triangle_is_dropped_not_drawn_wrong() {
    let (v, i) = mesh_from_script(&json!([[0, 0, 0, 0], [1, 0, 1, 0], [0, 1, 0, 1]]), &json!([1, 2, 3, 1, 3, 9, 0, 1, 2, 3]));
    assert_eq!(v.len(), 3);
    assert_eq!(i, vec![0, 1, 2], "an index past the vertices, or 0, drops that triangle; a partial one too");
}

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
fn the_browser_hosts_shape_it_too() {
    let mut host = Host(World::new());
    let e = host.0.spawn((Transform::default(),));
    // As Fengari hands the preamble a Lua list: a JS array of arrays.
    let args = JsonArgs::new(json!({ "e": e.id(), "vs": [[0, 0, 0, 0], [8, 0, 1, 0], [8, 8, 1, 1]], "ix": [1, 2, 3] }));
    assert!(handle_world_command(&mut host, "smsh", &args));
    let m = host.0.get::<&Mesh2D>(e).unwrap();
    assert_eq!((m.vertices.len(), m.indices.clone()), (3, vec![0, 1, 2]));
    assert_eq!(m.vertices[2].position, [8.0, 8.0]);
}
