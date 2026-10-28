//! Visual Showcase 2's scenes stand on their stages - the showcase's half
//! of `a_stage_is_drawn_in_perspective.rs`.
#![cfg(feature = "stage")]

use hecs::World;
use jengine_core::ecs::components::{Mesh2D, Name};
use jengine_core::ecs::stage::{StageBillboard, StageQuad};

fn tick(world: &mut World) {
    jengine_core::ecs::behaviors::tick_motion(world, 1.0 / 60.0, None, false);
}

/// Every Visual Showcase 2 scene loads, its stage projects every quad, and
/// in Lantern Square the traveller throws shadows from the lamps beside
/// them - the scene the stage was built for, read the way a runtime reads it.
#[test]
fn the_showcase_scenes_stand_on_their_stages() {
    use jengine_core::package::component_registry::ComponentRegistry;
    use jengine_core::package::core_registration::register_core_components;

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects/visual-showcase-2/assets/scenes");
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).expect("the showcase's scenes") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("scene") {
            continue;
        }
        let mut world = World::new();
        let text = std::fs::read_to_string(&path).unwrap();
        jengine_core::scene::load_scene_auto(&text, &mut world, &registry).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        tick(&mut world);
        tick(&mut world);
        let quads = world.query::<&StageQuad>().iter().count();
        assert!(quads > 0, "{path:?} has no stage quads");
        let empty: Vec<String> = world
            .query::<(&StageQuad, &Mesh2D, &Name)>()
            .iter()
            .filter(|(_, (q, m, _))| !q.two_sided && m.vertices.is_empty() && !q.ground)
            .map(|(_, (_, _, n))| n.0.clone())
            .collect();
        // Walls turned away are culled - some, never all.
        assert!(empty.len() < quads, "{path:?}: nothing projected");
        if path.file_name().unwrap() == "01-lantern-square.scene" {
            let hero = world.query::<(&StageBillboard, &Name)>().iter().find(|(_, (_, n))| n.0 == "Hero").map(|(e, _)| e).unwrap();
            let quads = world.get::<&StageBillboard>(hero).unwrap().shadow_quads.len();
            assert_eq!(quads, 2, "the traveller between two lamps throws two shadows");
        }
        seen += 1;
    }
    assert!(seen >= 7, "the hub and six scenes: {seen}");
}
