//! `sorting_group` (`ecs::sorting_group`): everything under a parent sorts
//! as one drawable - a character's front arm stays in front of its own body
//! and behind the next character, instead of every arm in the scene drawing
//! over every body.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{Parent, SpriteComponent, Transform};
use jengine_core::ecs::sorting_group::{SortGroup, SortGroups, SortingGroup, outer_key, y_key};

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

#[test]
fn the_outermost_group_decides_and_the_rest_are_ungrouped() {
    let mut world = World::new();
    let loose = world.spawn((at(0.0, 0.0),));
    let none = SortGroups::gather(&world, |_, z| z);
    assert!(none.is_empty());
    assert_eq!(none.of(&world, loose), SortGroup::NONE);

    let outer = world.spawn((at(0.0, 0.0), SortingGroup { z_order: 4, ..Default::default() }));
    let inner = world.spawn((at(0.0, 0.0), Parent(outer), SortingGroup { z_order: 9, ..Default::default() }));
    let hand = world.spawn((at(0.0, 0.0), Parent(inner)));
    let groups = SortGroups::gather(&world, |_, z| z);
    assert_eq!(groups.of(&world, hand).z, 4, "the outer group decides");
    assert_eq!(groups.of(&world, outer).id, outer.id() + 1, "a group's own drawable is in it");
    assert_eq!(groups.of(&world, loose), SortGroup::NONE);
}

#[test]
fn a_group_sorts_as_one_sprite_and_keeps_its_members_in_their_order() {
    // Ungrouped: the item's own key, so nothing that exists moved.
    assert_eq!(outer_key((7, 1, -32, ), SortGroup::NONE), (7, 1, -32, 0));
    // Two characters at z 0, y-sorted: the one lower on screen in front,
    // whatever their parts' own z.
    let back = SortGroup { id: 1, z: 0, y: y_key(20.0) };
    let front = SortGroup { id: 2, z: 0, y: y_key(-20.0) };
    let back_arm = (outer_key((10, 1, 0), back), (10, 1, 0));
    let front_body = (outer_key((5, 1, 0), front), (5, 1, 0));
    let front_arm = (outer_key((10, 1, 0), front), (10, 1, 0));
    assert!(back_arm < front_body, "the back character's arm under the front one's body");
    assert!(front_body < front_arm, "a character's own parts in their own order");
    // A loose sprite at a higher z is over both groups.
    assert!(front_arm < (outer_key((1, 1, 0), SortGroup::NONE), (1, 1, 0)));
}

#[test]
fn the_scene_format_keeps_it() {
    let mut world = World::new();
    world.spawn((at(0.0, 0.0), SortingGroup { z_order: 3, y_sort: true, sort_layer: None }));
    let scene = jengine_core::scene::format::save_scene(&world);
    let g = scene.entities.iter().find_map(|e| e.components.sorting_group.as_ref()).expect("saved");
    assert_eq!((g.z_order, g.y_sort), (3, true));
}

/// Two characters side by side, each a body at z 5 and an arm at z 10
/// reaching over the other's body. Returns the colour where A's arm crosses
/// B's body: yellow (A's arm) when parts sort loose, blue (B's body) when
/// each character is a group and B's is in front.
#[cfg(feature = "gpu-wgpu")]
fn overlap_colour(grouped: bool) -> Option<[u8; 3]> {
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 64;
    let gpu = HeadlessGpu::new(W, W).or_else(|| {
        eprintln!("sorting_groups_keep_parts_together: no GPU adapter - skipped");
        None
    })?;
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
    let (_rgba, _aw, _ah, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let quad = |c: [f32; 4], w: f32, h: f32, z: i32| SpriteComponent {
        color: c,
        size: Some([w, h]),
        z_order: z,
        ..Default::default()
    };
    let mut world = World::new();
    for (x, body, z) in [(-8.0, [1.0, 0.0, 0.0, 1.0], 0), (12.0, [0.0, 0.0, 1.0, 1.0], 1)] {
        let root = world.spawn((at(x, 0.0), quad(body, 24.0, 24.0, 5)));
        if grouped {
            world.insert_one(root, SortingGroup { z_order: z, ..Default::default() }).unwrap();
        }
        world.spawn((at(14.0, 0.0), Parent(root), quad([1.0, 1.0, 0.0, 1.0], 16.0, 6.0, 10)));
    }
    jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world);
    let cam = Camera2D::new(W as f32, W as f32);
    let vp = cam.view_projection();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
            |b, tex, kind| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                if let SpriteShadowFlush::Sprite { .. } = kind {
                    b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
                }
            },
        );
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    // World (8, 0): A's arm over B's body.
    let i = ((32 * W + 40) * 4) as usize;
    Some([img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]])
}

#[cfg(feature = "gpu-wgpu")]
#[test]
fn a_grouped_character_covers_the_one_behind_it_whole() {
    let Some(loose) = overlap_colour(false) else { return };
    assert!(loose[0] > 200 && loose[1] > 200 && loose[2] < 40, "loose parts: A's arm on top {loose:?}");
    let Some(grouped) = overlap_colour(true) else { return };
    assert!(grouped[2] > 200 && grouped[0] < 40, "grouped: B's body covers A's arm {grouped:?}");
}
