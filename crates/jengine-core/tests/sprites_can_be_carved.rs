//! **Carvable sprites** (Oct 2026): `Sprite.carve(e, x, y, radius)` blows a
//! hole in a sprite's pixels and its collider follows. Before it a texture
//! could not change after it loaded, so destructible terrain - a Worms
//! island, a wall a cannon chews through - could only be faked with stacks
//! of small sprites. Found by the showcase's Crater Isle.
//!
//! The cut itself is unit-tested in `ecs::carve`; this checks what reads it:
//! the script command, the physics (a body resting on carved-away land
//! falls), the GPU (a texture already uploaded shows the hole, with and
//! without a mip chain), the scene format, and that every host cuts.

mod support;

use glam::Vec2;
use hecs::World;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::carve::{Carvable, apply_carves};
use jengine_core::ecs::components::{SpriteComponent, Transform};

static NATIVE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs")));
static VIEWPORT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs")));
static WEB: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs")));

/// An opaque 100 x 50 texture drawn 200 x 100 at the origin: two world units
/// a texel.
fn land(world: &mut World, assets: &mut AssetManager) -> hecs::Entity {
    assets.insert_texture_bytes(
        "land".into(),
        RawTextureData { width: 100, height: 50, rgba: [90u8, 160, 60, 255].repeat(100 * 50) },
    );
    let sprite = SpriteComponent { texture_id: "land".into(), size: Some([200.0, 100.0]), ..Default::default() };
    world.spawn((Transform::default(), sprite, Carvable::default()))
}

#[test]
fn every_host_cuts_the_queued_holes() {
    for (name, src) in [("native", NATIVE.as_str()), ("viewport", VIEWPORT.as_str()), ("web", WEB.as_str())] {
        assert!(src.contains("carve::apply_carves("), "{name} never cuts a carve - `Sprite.carve` does nothing there");
    }
}

#[test]
fn the_script_command_queues_a_hole() {
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
    let e = land(&mut world, &mut assets);
    let mut host = Host(world);
    let args = JsonArgs(serde_json::json!({ "e": e.id(), "x": 10.0, "y": -4.0, "r": 12.0 }));
    assert!(handle_world_command(&mut host, "scv", &args), "`scv` is a shared world command");
    let c = host.0.get::<&Carvable>(e).unwrap();
    assert_eq!(c.pending.len(), 1);
    assert_eq!((c.pending[0].x, c.pending[0].y, c.pending[0].radius), (10.0, -4.0, 12.0));
}

#[test]
fn a_carvable_saves_its_settings_and_not_its_holes() {
    let mut world = World::new();
    let mut assets = AssetManager::new();
    let e = land(&mut world, &mut assets);
    world.get::<&mut Carvable>(e).unwrap().scorch = 6.0;
    apply_carves(&mut world, &mut assets);
    world.get::<&mut Carvable>(e).unwrap().pending.push(jengine_core::ecs::carve::Carve { x: 0.0, y: 0.0, radius: 20.0 });
    let scene = jengine_core::scene::format::save_scene(&world);
    let saved = scene.entities[0].components.carvable.as_ref().expect("saved with the scene");
    assert_eq!(saved.scorch, 6.0);
    assert!(saved.pending.is_empty() && saved.state.texture.is_none(), "runtime state is not part of the scene");
    let json = serde_json::to_string(saved).unwrap();
    assert!(!json.contains("pending") && !json.contains("state"), "{json}");
}

#[cfg(feature = "physics")]
#[test]
fn what_rests_on_carved_away_land_falls() {
    use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D};
    use jengine_core::ecs::systems::ensure_physics_bodies;
    use jengine_core::physics::PhysicsWorld;
    const DT: f32 = 1.0 / 60.0;

    let mut world = World::new();
    let mut assets = AssetManager::new();
    let e = land(&mut world, &mut assets);
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    apply_carves(&mut world, &mut assets);
    ensure_physics_bodies(&world, &mut physics);
    physics.step(DT);
    // The land's top edge is y = 50; a ray down the middle meets it there.
    let hit = physics.raycast(0.0, 300.0, 0.0, -1.0, 1000.0).expect("the land is solid");
    assert!((hit.point_y - 50.0).abs() < 4.0, "met at the top, got {}", hit.point_y);
    assert_eq!(hit.entity_id, e.id() as u64, "and the hit names the sprite");

    let ball = world.spawn((
        Transform { position: Vec2::new(0.0, 80.0), ..Default::default() },
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Circle { radius: 10.0 }, ..Default::default() },
    ));
    for _ in 0..180 {
        ensure_physics_bodies(&world, &mut physics);
        physics.step(DT);
        physics.sync_transforms_to_ecs(&mut world);
    }
    let resting = world.get::<&Transform>(ball).unwrap().position.y;
    assert!((resting - 60.0).abs() < 4.0, "it rests on the land: {resting}");

    // A crater under it, deeper than the ball.
    world.get::<&mut Carvable>(e).unwrap().pending.push(jengine_core::ecs::carve::Carve { x: 0.0, y: 50.0, radius: 40.0 });
    apply_carves(&mut world, &mut assets);
    for _ in 0..120 {
        ensure_physics_bodies(&world, &mut physics);
        physics.step(DT);
        physics.sync_transforms_to_ecs(&mut world);
    }
    let after = world.get::<&Transform>(ball).unwrap().position.y;
    assert!(after < resting - 25.0, "it fell into the crater: {resting} -> {after}");
    assert!(after > -60.0, "and landed in it rather than through the land: {after}");
    let hit = physics.raycast(0.0, 300.0, 0.0, -1.0, 1000.0).unwrap();
    assert!(hit.point_y < 40.0, "the ray now meets the ball down in the crater, not the old surface, got {}", hit.point_y);
}

#[cfg(feature = "physics")]
#[test]
fn a_body_put_down_inside_the_land_is_pushed_out_of_the_top() {
    use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D};
    use jengine_core::ecs::systems::ensure_physics_bodies;
    use jengine_core::physics::PhysicsWorld;
    // A bare outline is a surface with no inside: in the showcase's Crater
    // Isle, crates put down on a slope - a corner a few units into the land -
    // were pushed out of the far side and fell through the island. The
    // collider is solid now (`ecs::carve::solid_pieces`); this one starts
    // 6 units deep.
    let mut world = World::new();
    let mut assets = AssetManager::new();
    land(&mut world, &mut assets);
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    apply_carves(&mut world, &mut assets);
    let crate_e = world.spawn((
        Transform { position: Vec2::new(-40.0, 59.0), ..Default::default() },
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 30.0, height: 30.0 }, ..Default::default() },
    ));
    for _ in 0..180 {
        ensure_physics_bodies(&world, &mut physics);
        physics.step(1.0 / 60.0);
        physics.sync_transforms_to_ecs(&mut world);
    }
    let y = world.get::<&Transform>(crate_e).unwrap().position.y;
    assert!((y - 65.0).abs() < 6.0, "it ends up standing on the land (y 65), not in or under it: {y}");
}

#[cfg(feature = "gpu-wgpu")]
mod gpu {
    use super::*;
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 240;
    const H: u32 = 120;

    /// Draws as every host does: ask for the texture, and upload only when
    /// there is none - so a carve after the first upload has to reach the
    /// screen through that question, not through a test calling `ensure`.
    fn draw(gpu: &HeadlessGpu, world: &World, assets: &mut AssetManager) -> RawTextureData {
        let ids: Vec<String> = world
            .query::<&Carvable>()
            .iter()
            .filter_map(|(_, c)| c.state.texture.clone())
            .collect();
        for id in &ids {
            if assets.get_wgpu_texture(id).is_none() {
                assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).unwrap();
            }
        }
        let (_, _, _, font) = built_in_font_atlas();
        let fonts = FrameFonts::builtin_only(&font, "__font");
        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
        let vp = Camera2D::new(W as f32, H as f32).view_projection();
        let mut encoder = gpu.encoder();
        {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            batch.begin_frame();
            render_sprites_with_shadows_wgpu_materialized(
                world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
                |b, tex, kind| {
                    let t = assets.get_wgpu_texture(tex).unwrap_or_else(|| panic!("{tex} uploaded"));
                    match kind {
                        SpriteShadowFlush::Shadow => b.flush_multiply(&gpu.device, &gpu.queue, &mut pass, &vp, t),
                        SpriteShadowFlush::Sprite { .. } => b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t),
                    };
                },
            );
        }
        gpu.submit(encoder);
        gpu.read_rgba()
    }

    fn green(img: &RawTextureData, x: f32, y: f32) -> u8 {
        let px = (x + W as f32 / 2.0) as u32;
        let py = (H as f32 / 2.0 - y) as u32;
        img.rgba[((py * img.width + px) * 4 + 1) as usize]
    }

    fn hole_after_upload(filter: &str) -> Option<(u8, u8, u8)> {
        let gpu = HeadlessGpu::new(W, H).or_else(|| {
            eprintln!("sprites_can_be_carved: no GPU adapter - skipped");
            None
        })?;
        let mut world = World::new();
        let mut assets = AssetManager::new();
        let rules: jengine_core::renderer::texture_filter::TextureFilterRules =
            serde_json::from_value(serde_json::json!({ "default": filter })).unwrap();
        assets.set_texture_filters(rules);
        let e = land(&mut world, &mut assets);
        apply_carves(&mut world, &mut assets);
        let before = draw(&gpu, &world, &mut assets);
        // Carve after the copy is on the GPU: the next draw must show it.
        world.get::<&mut Carvable>(e).unwrap().pending.push(jengine_core::ecs::carve::Carve { x: 0.0, y: 0.0, radius: 30.0 });
        apply_carves(&mut world, &mut assets);
        let after = draw(&gpu, &world, &mut assets);
        Some((green(&before, 0.0, 0.0), green(&after, 0.0, 0.0), green(&after, 80.0, 0.0)))
    }

    #[test]
    fn an_uploaded_texture_shows_the_hole() {
        for filter in ["nearest", "linear"] {
            let Some((before, hole, beside)) = hole_after_upload(filter) else { return };
            assert!(before > 100, "{filter}: the land is drawn before the carve ({before})");
            assert!(hole < 10, "{filter}: the hole shows the black under it ({hole})");
            assert!(beside > 100, "{filter}: the land beside it is untouched ({beside})");
        }
    }
}
