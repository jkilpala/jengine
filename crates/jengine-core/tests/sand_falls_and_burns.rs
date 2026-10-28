//! **Falling sand** (Oct 2026; `ecs::pixel_sim`): a grid of material cells
//! - sand, water, oil, fire, smoke, lava, steam, stone, wood - stepped by
//! rules, drawn by its entity's sprite through a texture of its own and
//! solid to bodies through its entity's `carvable`. Found by the showcase's
//! Falling Sands. The rules themselves are tested in the module.
#![cfg(feature = "pixel_sim")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::ecs::carve::Carvable;
use jengine_core::ecs::components::{SpriteComponent, Transform};
use jengine_core::ecs::pixel_sim::{PixelSim, SAND, STONE, queue_paint, sync, texture_id, tick_pixel_sims};

fn sim_world() -> (World, hecs::Entity) {
    let mut world = World::new();
    // 100 x 50 cells over 200 x 100 world units, centred on (0, 0).
    let e = world.spawn((
        Transform::default(),
        SpriteComponent { size: Some([200.0, 100.0]), ..Default::default() },
        PixelSim { cells: [100, 50], collider_interval: 0.0, ..Default::default() },
    ));
    (world, e)
}

#[test]
fn a_simulation_draws_through_its_carvable_and_is_solid_where_its_cells_are() {
    let (mut world, e) = sim_world();
    let mut assets = AssetManager::new();
    tick_pixel_sims(&mut world, 1.0 / 60.0);
    sync(&mut world, &mut assets);
    sync(&mut world, &mut assets);
    let id = texture_id(e);
    let tex = assets.get_texture_bytes(&id).expect("its texture");
    assert_eq!((tex.width, tex.height), (100, 50));
    {
        let c = world.get::<&Carvable>(e).expect("a carvable was added");
        assert_eq!(c.state.texture.as_deref(), Some(id.as_str()), "the sprite pass draws the cells");
    }
    // A floor of stone along the bottom row: solid after the next sync.
    {
        let mut sim = world.get::<&mut PixelSim>(e).unwrap();
        for x in 0..100 {
            sim.state.set(x, 49, STONE);
        }
    }
    tick_pixel_sims(&mut world, 1.0 / 60.0);
    sync(&mut world, &mut assets);
    let c = world.get::<&Carvable>(e).unwrap();
    assert!(c.collider, "a collider is wanted");
    assert_eq!(c.state.mask[(49 * 100 + 10) as usize], 1, "the stone is solid");
    assert_eq!(c.state.mask[10], 0, "the air is not");
    assert_eq!(tex_alpha(&assets, &id, 10, 49), 255, "and the stone is drawn");
}

fn tex_alpha(assets: &AssetManager, id: &str, x: u32, y: u32) -> u8 {
    let t = assets.get_texture_bytes(id).unwrap();
    t.rgba[((y * t.width + x) * 4 + 3) as usize]
}

#[test]
fn a_script_paints_at_a_world_point() {
    let (mut world, e) = sim_world();
    tick_pixel_sims(&mut world, 1.0 / 60.0);
    // World (-50, 25) is a quarter in from the left, a quarter down: cell
    // (25, 12.5).
    queue_paint(&world, e, -50.0, 25.0, 2.0, "sand").unwrap();
    assert!(queue_paint(&world, e, 0.0, 0.0, 2.0, "custard").is_err(), "an unknown material is refused");
    tick_pixel_sims(&mut world, 0.0);
    let sim = world.get::<&PixelSim>(e).unwrap();
    assert_eq!(sim.state.at(25, 12), SAND);
    assert_eq!(sim.state.at(60, 12), 0, "and only there");
}

#[test]
fn a_paint_queued_before_the_first_step_is_kept() {
    // A script paints its level in `init`, before the simulation has built
    // its cells.
    let (mut world, e) = sim_world();
    queue_paint(&world, e, -50.0, 25.0, 2.0, "stone").unwrap();
    tick_pixel_sims(&mut world, 0.0);
    assert_eq!(world.get::<&PixelSim>(e).unwrap().state.at(25, 12), STONE);
}

#[test]
fn its_texture_is_sampled_nearest_whatever_the_projects_default() {
    use jengine_core::renderer::texture_filter::{TextureFilter, TextureFilterRules};
    let rules: TextureFilterRules = serde_json::from_value(serde_json::json!({ "default": "linear" })).unwrap();
    assert_eq!(rules.filter_for("__pixel_sim/7"), TextureFilter::Nearest);
    assert_eq!(rules.filter_for("sprites/a.png"), TextureFilter::Linear);
    let _ = Vec2::ZERO;
}
