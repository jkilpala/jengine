//! **Paintable sprites** (Oct 2026): `Sprite.paint(e, x, y, size, r, g, b,
//! a, shape, rotation)` stains a sprite's own copy of its texture - blood on
//! a floor, scorch marks, oil - once, kept for good, free every frame after.
//! Before it the only way to leave a mark was a sprite per stain, so a floor
//! after a firefight was thousands of drawables. Found by the showcase's
//! Firefight.
//!
//! Checks the stamps themselves (where a splat, a disc and a streak land,
//! `over` and `multiply`, a hole left alone, the same splat everywhere),
//! that the authored texture is untouched, the script command shared by the
//! browser hosts, the scene key, and that the sprite pass draws the copy.
#![cfg(feature = "paint")]

mod support;

use glam::Vec2;
use hecs::World;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::carve::apply_carves;
use jengine_core::ecs::components::{SpriteComponent, Transform};
use jengine_core::ecs::paint::{PaintBlend, Paintable, Stamp, StampShape};

/// A grey 100 x 100 texture drawn 200 x 200 at the origin: two world units a
/// texel. The left column of texels is a hole (alpha 0).
fn floor(world: &mut World, assets: &mut AssetManager, blend: PaintBlend) -> hecs::Entity {
    let mut rgba = [128u8, 128, 128, 255].repeat(100 * 100);
    for y in 0..100 {
        rgba[(y * 100) * 4 + 3] = 0;
    }
    assets.insert_texture_bytes("floor".into(), RawTextureData { width: 100, height: 100, rgba });
    let sprite = SpriteComponent { texture_id: "floor".into(), size: Some([200.0, 200.0]), ..Default::default() };
    world.spawn((Transform::default(), sprite, Paintable { blend, ..Default::default() }))
}

/// The painted copy's texel under world point `p`.
fn texel(world: &World, assets: &AssetManager, e: hecs::Entity, p: Vec2) -> [u8; 4] {
    let id = world.get::<&Paintable>(e).unwrap().state.texture.clone().expect("a copy was made");
    let t = assets.get_texture_bytes(&id).unwrap();
    let (x, y) = (((p.x + 100.0) / 2.0) as usize, ((100.0 - p.y) / 2.0) as usize);
    let i = (y.min(99) * 100 + x.min(99)) * 4;
    [t.rgba[i], t.rgba[i + 1], t.rgba[i + 2], t.rgba[i + 3]]
}

fn stamp(x: f32, y: f32, size: f32, shape: StampShape, rotation: f32, color: [f32; 4]) -> Stamp {
    Stamp { x, y, size, color, shape, rotation, depth: 0.0, blend: None }
}

fn paint(world: &mut World, assets: &mut AssetManager, e: hecs::Entity, s: Stamp) {
    world.get::<&mut Paintable>(e).unwrap().push(s);
    apply_carves(world, assets);
}

#[test]
fn a_disc_stains_where_it_lands_and_nowhere_else() {
    let (mut world, mut assets) = (World::new(), AssetManager::new());
    let e = floor(&mut world, &mut assets, PaintBlend::Over);
    paint(&mut world, &mut assets, e, stamp(20.0, 10.0, 40.0, StampShape::Disc, 0.0, [0.5, 0.0, 0.0, 1.0]));
    let c = texel(&world, &assets, e, Vec2::new(20.0, 10.0));
    assert!(c[0] > 150 && c[1] < 20, "red at the centre: {c:?}");
    let far = texel(&world, &assets, e, Vec2::new(-60.0, -60.0));
    assert_eq!(far, [128, 128, 128, 255], "untouched away from it");
    let edge = texel(&world, &assets, e, Vec2::new(20.0 + 24.0, 10.0));
    assert_eq!(edge, [128, 128, 128, 255], "the disc is 40 across, so 24 out is clear");
    // The authored texture is not the copy.
    assert_eq!(assets.get_texture_bytes("floor").unwrap().rgba[50 * 400 + 200], 128);
}

#[test]
fn multiply_darkens_and_a_hole_is_left_alone() {
    let (mut world, mut assets) = (World::new(), AssetManager::new());
    let e = floor(&mut world, &mut assets, PaintBlend::Multiply);
    paint(&mut world, &mut assets, e, stamp(-99.0, 0.0, 60.0, StampShape::Disc, 0.0, [0.0, 0.0, 0.0, 0.5]));
    let mid = texel(&world, &assets, e, Vec2::new(-80.0, 0.0));
    assert!((60..=68).contains(&mid[0]), "half-way to black: {mid:?}");
    let hole = texel(&world, &assets, e, Vec2::new(-99.5, 0.0));
    assert_eq!(hole, [128, 128, 128, 0], "the hole is not painted");
}

#[test]
fn a_streak_runs_along_its_rotation() {
    let (mut world, mut assets) = (World::new(), AssetManager::new());
    let e = floor(&mut world, &mut assets, PaintBlend::Over);
    let up = std::f32::consts::FRAC_PI_2;
    paint(&mut world, &mut assets, e, stamp(0.0, 0.0, 120.0, StampShape::Streak, up, [0.0, 0.0, 0.0, 1.0]));
    assert!(texel(&world, &assets, e, Vec2::new(0.0, 40.0))[0] < 40, "painted along +y");
    assert!(texel(&world, &assets, e, Vec2::new(0.0, -40.0))[0] < 40, "and back along -y");
    assert_eq!(texel(&world, &assets, e, Vec2::new(40.0, 0.0))[0], 128, "but not across it");
}

#[test]
fn a_splat_throws_its_droplets_forward_and_is_the_same_every_time() {
    let count = |world: &World, assets: &AssetManager, e, dir: f32| {
        // Painted texels in a band ahead of (dir 1) or behind (dir -1) it.
        (0..100)
            .flat_map(|x| (0..100).map(move |y| (x, y)))
            .filter(|&(x, y)| {
                let p = Vec2::new(x as f32 * 2.0 - 99.0, 99.0 - y as f32 * 2.0);
                p.x * dir > 26.0 && texel(world, assets, e, p)[0] < 100
            })
            .count()
    };
    let make = || {
        let (mut world, mut assets) = (World::new(), AssetManager::new());
        let e = floor(&mut world, &mut assets, PaintBlend::Over);
        paint(&mut world, &mut assets, e, stamp(0.0, 0.0, 70.0, StampShape::Splat, 0.0, [0.0, 0.0, 0.0, 1.0]));
        (world, assets, e)
    };
    let (world, assets, e) = make();
    let ahead = count(&world, &assets, e, 1.0);
    let behind = count(&world, &assets, e, -1.0);
    assert!(ahead > 10 && ahead > behind * 3, "droplets fly along +x: {ahead} ahead, {behind} behind");
    let (world2, assets2, e2) = make();
    let a = assets.get_texture_bytes(world.get::<&Paintable>(e).unwrap().state.texture.as_deref().unwrap()).unwrap();
    let b = assets2.get_texture_bytes(world2.get::<&Paintable>(e2).unwrap().state.texture.as_deref().unwrap()).unwrap();
    assert!(a.rgba == b.rgba, "a splat's shape is a hash of where it landed");
}

#[test]
fn the_script_command_queues_a_stamp() {
    use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
    struct Host(World);
    impl ScriptWorldHost for Host {
        fn world(&mut self) -> &mut World {
            &mut self.0
        }
        fn resolve_entity(&mut self, id: u32) -> Option<hecs::Entity> {
            self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
        }
    }
    let mut world = World::new();
    let mut assets = AssetManager::new();
    let e = floor(&mut world, &mut assets, PaintBlend::Over);
    let mut host = Host(world);
    let args = JsonArgs(serde_json::json!({
        "e": e.id(), "x": 10.0, "y": -4.0, "s": 30.0, "cr": 0.4, "cg": 0.0, "cb": 0.0, "ca": 0.8, "sh": "streak", "rot": 1.0
    }));
    assert!(handle_world_command(&mut host, "spnt", &args), "`spnt` is a shared world command");
    let p = host.0.get::<&Paintable>(e).unwrap();
    assert_eq!(p.pending, vec![stamp(10.0, -4.0, 30.0, StampShape::Streak, 1.0, [0.4, 0.0, 0.0, 0.8])]);
}

#[test]
fn the_scene_key_round_trips_and_the_sprite_pass_draws_the_copy() {
    let bundle: jengine_core::scene::format::ComponentBundle =
        serde_json::from_value(serde_json::json!({ "paintable": { "blend": "multiply" } })).unwrap();
    assert_eq!(bundle.get::<jengine_core::ecs::paint::Paintable>("paintable").expect("the scene key reaches the bundle").blend, PaintBlend::Multiply);
    let src = support::with_frame_draw(include_str!("../src/ecs/wgpu_systems.rs"));
    assert!(src.contains("painted_texture(painted)"), "the sprite pass draws the painted copy");
    let carve = include_str!("../src/ecs/carve.rs");
    assert!(carve.contains("p.and_then(|p| p.state.texture.as_deref())"), "painted_texture reads the painted copy");
}

/// The same floor with a flat normal map (`floor_n`, specular alpha 200).
fn pressable(world: &mut World, assets: &mut AssetManager) -> hecs::Entity {
    let e = floor(world, assets, PaintBlend::Over);
    let rgba = [128u8, 128, 255, 200].repeat(100 * 100);
    assets.insert_texture_bytes("floor_n".into(), RawTextureData { width: 100, height: 100, rgba });
    let material = jengine_core::ecs::components::MaterialComponent {
        normal_map_id: Some("floor_n".into()),
        ..Default::default()
    };
    world.insert_one(e, material).unwrap();
    e
}

/// The pressed normal map's texel under world point `p`.
fn normal_at(world: &World, assets: &AssetManager, e: hecs::Entity, p: Vec2) -> [u8; 4] {
    let id = world.get::<&Paintable>(e).unwrap().state.normal.clone().expect("a pressed copy was made");
    let t = assets.get_texture_bytes(&id).unwrap();
    let (x, y) = (((p.x + 100.0) / 2.0) as usize, ((100.0 - p.y) / 2.0) as usize);
    let i = (y.min(99) * 100 + x.min(99)) * 4;
    [t.rgba[i], t.rgba[i + 1], t.rgba[i + 2], t.rgba[i + 3]]
}

fn press(world: &mut World, assets: &mut AssetManager, e: hecs::Entity, depth: f32) {
    let s = Stamp { depth, ..stamp(0.0, 0.0, 40.0, StampShape::Disc, 0.0, [1.0; 4]) };
    paint(world, assets, e, s);
}

#[test]
fn a_press_dents_the_normal_map_and_its_rims_face_in() {
    let (mut world, mut assets) = (World::new(), AssetManager::new());
    let e = pressable(&mut world, &mut assets);
    press(&mut world, &mut assets, e, 1.0);
    let left = normal_at(&world, &assets, e, Vec2::new(-20.0, 0.0));
    assert!(left[0] > 150, "the left rim of a pit faces right: {left:?}");
    let top = normal_at(&world, &assets, e, Vec2::new(0.0, 20.0));
    assert!(top[1] < 106, "its top rim faces down: {top:?}");
    let mid = normal_at(&world, &assets, e, Vec2::ZERO);
    assert!(mid[0].abs_diff(128) < 4 && mid[1].abs_diff(128) < 4, "its floor is flat: {mid:?}");
    assert_eq!(left[3], 200, "the specular mask is kept");
    assert_eq!(normal_at(&world, &assets, e, Vec2::new(-80.0, -80.0)), [128, 128, 255, 200], "away from it, as authored");
    assert_eq!(assets.get_texture_bytes("floor_n").unwrap().rgba[..4], [128, 128, 255, 200], "the authored map is untouched");
    // The colour is not painted by a press.
    assert!(world.get::<&Paintable>(e).unwrap().state.texture.is_some());
    assert_eq!(texel(&world, &assets, e, Vec2::ZERO), [128, 128, 128, 255]);
}

#[test]
fn a_press_sets_a_depth_and_a_negative_one_raises() {
    let (mut world, mut assets) = (World::new(), AssetManager::new());
    let e = pressable(&mut world, &mut assets);
    press(&mut world, &mut assets, e, 1.0);
    let once = normal_at(&world, &assets, e, Vec2::new(-20.0, 0.0));
    press(&mut world, &mut assets, e, 1.0);
    assert_eq!(normal_at(&world, &assets, e, Vec2::new(-20.0, 0.0)), once, "a print over a print is no deeper");

    let (mut world, mut assets) = (World::new(), AssetManager::new());
    let e = pressable(&mut world, &mut assets);
    press(&mut world, &mut assets, e, -1.0);
    assert!(normal_at(&world, &assets, e, Vec2::new(-20.0, 0.0))[0] < 106, "the left side of a ridge faces left");
}

#[test]
fn a_press_on_a_sprite_with_no_normal_map_does_nothing() {
    let (mut world, mut assets) = (World::new(), AssetManager::new());
    let e = floor(&mut world, &mut assets, PaintBlend::Over);
    press(&mut world, &mut assets, e, 1.0);
    let p = world.get::<&Paintable>(e).unwrap();
    assert!(p.state.normal.is_none() && p.pending.is_empty(), "dropped, not queued forever");
}

#[test]
fn the_normal_pass_draws_the_pressed_copy() {
    let src = support::with_frame_draw(include_str!("../src/ecs/wgpu_systems.rs"));
    assert!(src.contains("pressed_normal(pressed)"), "the normal pass draws the pressed normal map");
    let carve = include_str!("../src/ecs/carve.rs");
    assert!(carve.contains("p.and_then(|p| p.state.normal.as_deref())"), "pressed_normal reads the pressed copy");
    let assets = include_str!("../src/asset/mod.rs");
    assert!(assets.contains("self.patch_dirty(id, &key, queue)"), "a pressed normal map is patched on the GPU");
}

#[test]
fn the_press_command_is_shared() {
    use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
    struct Host(World);
    impl ScriptWorldHost for Host {
        fn world(&mut self) -> &mut World {
            &mut self.0
        }
        fn resolve_entity(&mut self, id: u32) -> Option<hecs::Entity> {
            self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
        }
    }
    let mut world = World::new();
    let mut assets = AssetManager::new();
    let e = floor(&mut world, &mut assets, PaintBlend::Over);
    let mut host = Host(world);
    let args = JsonArgs(serde_json::json!({ "e": e.id(), "x": 1.0, "y": 2.0, "s": 30.0, "dp": 0.8, "sh": "disc", "rot": 0.0 }));
    assert!(handle_world_command(&mut host, "spnt", &args));
    let erase = JsonArgs(serde_json::json!({ "e": e.id(), "x": 1.0, "y": 2.0, "s": 30.0, "bl": "erase" }));
    assert!(handle_world_command(&mut host, "spnt", &erase));
    let p = host.0.get::<&Paintable>(e).unwrap();
    assert_eq!(p.pending[0].depth, 0.8);
    assert_eq!(p.pending[0].shape, StampShape::Disc);
    assert_eq!((p.pending[0].blend, p.pending[1].blend), (None, Some(PaintBlend::Erase)), "a stamp names its blend");
}

/// A transparent 100 x 100 canvas drawn 200 x 200, painted on as a layer.
fn canvas(world: &mut World, assets: &mut AssetManager) -> hecs::Entity {
    assets.insert_texture_bytes("canvas".into(), RawTextureData { width: 100, height: 100, rgba: vec![0; 100 * 100 * 4] });
    let sprite = SpriteComponent { texture_id: "canvas".into(), size: Some([200.0, 200.0]), ..Default::default() };
    world.spawn((Transform::default(), sprite, Paintable { blend: PaintBlend::Over, alpha_cutoff: 0.0, ..Default::default() }))
}

#[test]
fn a_transparent_canvas_takes_paint_and_erase_takes_it_away() {
    let (mut world, mut assets) = (World::new(), AssetManager::new());
    let e = canvas(&mut world, &mut assets);
    paint(&mut world, &mut assets, e, stamp(0.0, 0.0, 40.0, StampShape::Disc, 0.0, [1.0, 0.5, 0.0, 0.8]));
    let c = texel(&world, &assets, e, Vec2::ZERO);
    assert_eq!(c[3], 204, "the canvas gains the stamp's alpha: {c:?}");
    assert!(c[0] == 255 && c[1] > 180 && c[1] < 195 && c[2] == 0, "and takes its colour whole, not darkened: {c:?}");
    assert_eq!(texel(&world, &assets, e, Vec2::new(-60.0, 60.0))[3], 0, "away from it the canvas is clear");

    // One stamp rubs out, over the canvas's own `over` (`Sprite.paint`'s
    // eleventh argument).
    let rub = Stamp { blend: Some(PaintBlend::Erase), ..stamp(0.0, 0.0, 40.0, StampShape::Disc, 0.0, [0.0, 0.0, 0.0, 0.5]) };
    paint(&mut world, &mut assets, e, rub);
    let erased = texel(&world, &assets, e, Vec2::ZERO);
    assert_eq!(erased[3], 102, "erase halves the alpha: {erased:?}");
    assert_eq!(erased[..3], c[..3], "and leaves the colour");
}

#[test]
fn particles_start_where_the_canvas_is_painted_and_nowhere_before() {
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::particles::{EmitShape, ParticleEmitter, ParticleSystem, tick_particles};
    let (mut world, mut assets) = (World::new(), AssetManager::new());
    let e = canvas(&mut world, &mut assets);
    let emitter = ParticleEmitter {
        emission_rate: 600.0,
        max_particles: 500,
        lifetime: [5.0, 5.0],
        speed: [0.0, 0.0],
        emit_shape: EmitShape::Sprite { edge: false, cutoff: 0.5 },
        ..Default::default()
    };
    world.insert_one(e, emitter).unwrap();
    let mut system = ParticleSystem::new();
    let positions = |world: &World, system: &ParticleSystem| -> Vec<Vec2> {
        let em = world.get::<&ParticleEmitter>(e).unwrap();
        system.draw_data(&[(e.id() as u64, &em, Vec2::ZERO)], &SortLayers::default())
            .iter()
            .flat_map(|g| g.particles.iter().map(|p| p.position))
            .collect()
    };
    for _ in 0..10 {
        apply_carves(&mut world, &mut assets);
        tick_particles(&world, &mut system, 1.0 / 60.0);
    }
    assert!(positions(&world, &system).is_empty(), "an empty canvas starts no particles - not at its middle");

    paint(&mut world, &mut assets, e, stamp(50.0, 40.0, 30.0, StampShape::Disc, 0.0, [1.0; 4]));
    for _ in 0..10 {
        apply_carves(&mut world, &mut assets);
        tick_particles(&world, &mut system, 1.0 / 60.0);
    }
    let at = positions(&world, &system);
    assert!(at.len() > 20, "the painted patch emits: {}", at.len());
    assert!(
        at.iter().all(|p| (*p - Vec2::new(50.0, 40.0)).length() < 18.0),
        "and only there: {:?}",
        at.iter().find(|p| (**p - Vec2::new(50.0, 40.0)).length() >= 18.0)
    );
}

fn gloss(world: &mut World, assets: &mut AssetManager, e: hecs::Entity, amount: f32) {
    let s = Stamp { blend: Some(PaintBlend::Gloss), ..stamp(0.0, 0.0, 40.0, StampShape::Disc, 0.0, [1.0, 1.0, 1.0, amount]) };
    paint(world, assets, e, s);
}

#[test]
fn gloss_wets_the_specular_mask_and_only_it() {
    let (mut world, mut assets) = (World::new(), AssetManager::new());
    let e = pressable(&mut world, &mut assets);
    gloss(&mut world, &mut assets, e, 1.0);
    let wet = normal_at(&world, &assets, e, Vec2::ZERO);
    assert_eq!(wet, [128, 128, 255, 255], "full shine, the normal untouched: {wet:?}");
    assert_eq!(normal_at(&world, &assets, e, Vec2::new(-80.0, -80.0))[3], 200, "away from it, the authored mask");
    assert_eq!(texel(&world, &assets, e, Vec2::ZERO), [128, 128, 128, 255], "no colour");

    // Less than the authored shine leaves it alone.
    let (mut world, mut assets) = (World::new(), AssetManager::new());
    let e = pressable(&mut world, &mut assets);
    gloss(&mut world, &mut assets, e, 0.3);
    assert_eq!(normal_at(&world, &assets, e, Vec2::ZERO)[3], 200);
}

#[test]
fn gloss_dries_back_to_the_authored_mask() {
    let (mut world, mut assets) = (World::new(), AssetManager::new());
    let e = pressable(&mut world, &mut assets);
    world.get::<&mut Paintable>(e).unwrap().dry_seconds = 0.5;
    gloss(&mut world, &mut assets, e, 1.0);
    assert_eq!(normal_at(&world, &assets, e, Vec2::ZERO)[3], 255);
    // Time passes: four seconds, as the wall clock has it.
    let now = jengine_core::profiling::unix_time();
    world.get::<&mut Paintable>(e).unwrap().state.dried_at = now - 4.0;
    apply_carves(&mut world, &mut assets);
    let dried = normal_at(&world, &assets, e, Vec2::ZERO)[3];
    assert!(dried == 200, "eight times its drying time: back to the authored 200, got {dried}");
    assert!(world.get::<&Paintable>(e).unwrap().state.wet.is_none(), "nothing left to dry");
}

#[test]
fn a_spray_is_speckled_and_densest_in_the_middle() {
    let (mut world, mut assets) = (World::new(), AssetManager::new());
    let e = floor(&mut world, &mut assets, PaintBlend::Over);
    paint(&mut world, &mut assets, e, stamp(0.0, 0.0, 80.0, StampShape::Spray, 0.0, [0.0, 0.0, 0.0, 1.0]));
    let ring = |r0: f32, r1: f32| {
        let (mut hit, mut all) = (0, 0);
        for y in 0..100 {
            for x in 0..100 {
                let p = Vec2::new(x as f32 * 2.0 - 99.0, 99.0 - y as f32 * 2.0);
                if p.length() >= r0 && p.length() < r1 {
                    all += 1;
                    hit += usize::from(texel(&world, &assets, e, p)[0] < 100);
                }
            }
        }
        hit as f32 / all as f32
    };
    let (middle, rim) = (ring(0.0, 14.0), ring(28.0, 38.0));
    assert!(middle > 0.6 && middle < 1.0, "a dense speckle in the middle: {middle}");
    assert!(rim > 0.05 && rim < middle * 0.6, "a scatter at the rim: {rim}");
}
