//! `line` (`renderer::line2d`): a polyline with a width along it, a
//! gradient, a texture stretched or tiled, joints and caps - built as
//! triangles and drawn in the sprite pass.

use glam::Vec2;
use hecs::World;
use jengine_core::renderer::line2d::{Line2D, LineCap, LineJoint, LineTextureMode, line_mesh, set_line_field};

fn line(points: &[[f32; 2]]) -> Line2D {
    Line2D { points: points.to_vec(), ..Default::default() }
}

fn has_vertex(m: &jengine_core::renderer::line2d::LineMesh, p: Vec2) -> bool {
    m.positions.iter().any(|q| q.distance(p) < 1e-3)
}

fn extent(m: &jengine_core::renderer::line2d::LineMesh) -> (Vec2, Vec2) {
    let lo = m.positions.iter().fold(Vec2::splat(f32::MAX), |a, p| a.min(*p));
    let hi = m.positions.iter().fold(Vec2::splat(f32::MIN), |a, p| a.max(*p));
    (lo, hi)
}

#[test]
fn a_straight_line_is_its_width_and_its_caps_reach_past_the_ends() {
    let mut l = line(&[[0.0, 0.0], [100.0, 0.0]]);
    let (lo, hi) = extent(&line_mesh(&l).unwrap());
    assert!((lo - Vec2::new(0.0, -5.0)).length() < 1e-3 && (hi - Vec2::new(100.0, 5.0)).length() < 1e-3, "{lo} {hi}");
    l.begin_cap = LineCap::Box;
    l.end_cap = LineCap::Round;
    let (lo, hi) = extent(&line_mesh(&l).unwrap());
    assert!((lo.x + 5.0).abs() < 1e-3 && (hi.x - 105.0).abs() < 1e-3, "caps: {lo} {hi}");
    assert!(line_mesh(&line(&[[3.0, 3.0]])).is_none(), "one point is no line");
}

#[test]
fn corners_are_mitred_bevelled_or_rounded() {
    let mut l = line(&[[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]]);
    let m = line_mesh(&l).unwrap();
    assert!(has_vertex(&m, Vec2::new(105.0, -5.0)), "the outer mitre");
    assert!(has_vertex(&m, Vec2::new(95.0, 5.0)), "the inner corner");
    l.joint = LineJoint::Bevel;
    let m = line_mesh(&l).unwrap();
    assert!(!has_vertex(&m, Vec2::new(105.0, -5.0)));
    assert!(has_vertex(&m, Vec2::new(100.0, -5.0)) && has_vertex(&m, Vec2::new(105.0, 0.0)), "cut across");
    l.joint = LineJoint::Round;
    let m = line_mesh(&l).unwrap();
    let corner = Vec2::new(100.0, 0.0);
    let round: Vec<f32> = m.positions.iter().filter(|p| p.x > 100.0 && p.y < 0.0).map(|p| p.distance(corner)).collect();
    assert!(!round.is_empty() && round.iter().all(|d| (d - 5.0).abs() < 1e-3), "on the circle: {round:?}");
    // A hairpin: the mitre would reach far past the limit, so it is cut.
    let hairpin = Line2D { points: vec![[0.0, 0.0], [100.0, 0.0], [0.0, 8.0]], ..Default::default() };
    let m = line_mesh(&hairpin).unwrap();
    // Nothing pokes out past the corner further than the limit allows.
    let past = m.positions.iter().map(|p| p.x - 100.0).fold(f32::MIN, f32::max);
    assert!(past <= 5.0 * hairpin.sharp_limit + 1e-3, "no spike past the corner: {past}");
}

#[test]
fn a_tiled_texture_is_cut_into_whole_tiles() {
    let l = Line2D {
        points: vec![[0.0, 0.0], [100.0, 0.0]],
        texture_mode: LineTextureMode::Tile,
        tile_length: 25.0,
        ..Default::default()
    };
    let m = line_mesh(&l).unwrap();
    assert!(m.uvq.iter().all(|t| (0.0..=1.0).contains(&t[0])), "every u inside the texture");
    // Four tiles: a cut at 25, 50 and 75, each a u of 1 arriving and 0 leaving.
    for x in [25.0, 50.0, 75.0] {
        let us: Vec<f32> = m.positions.iter().zip(&m.uvq).filter(|(p, _)| (p.x - x).abs() < 1e-3).map(|(_, t)| t[0]).collect();
        assert!(us.contains(&0.0) && us.contains(&1.0), "a cut at {x}: {us:?}");
    }
    let stretched = line_mesh(&Line2D { points: l.points.clone(), ..Default::default() }).unwrap();
    let max_u = stretched.uvq.iter().map(|t| t[0]).fold(0.0, f32::max);
    assert!((max_u - 1.0).abs() < 1e-6, "stretched once over the line");
}

#[test]
fn the_width_and_colour_follow_the_line() {
    let l = Line2D {
        points: vec![[0.0, 0.0], [100.0, 0.0]],
        width_curve: vec![[0.0, 1.0], [1.0, 2.0]],
        gradient: vec![[0.0, 1.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0, 1.0]],
        ..Default::default()
    };
    let m = line_mesh(&l).unwrap();
    let at = |x: f32| {
        m.positions
            .iter()
            .zip(&m.colors)
            .filter(|(p, _)| (p.x - x).abs() < 1e-3)
            .map(|(p, c)| (p.y.abs(), *c))
            .collect::<Vec<_>>()
    };
    let start = at(0.0);
    let end = at(100.0);
    assert!(start.iter().all(|(h, c)| (h - 5.0).abs() < 1e-3 && c[0] > 0.99 && c[2] < 0.01), "{start:?}");
    assert!(end.iter().all(|(h, c)| (h - 10.0).abs() < 1e-3 && c[2] > 0.99 && c[0] < 0.01), "{end:?}");
}

#[test]
fn a_closed_line_joins_up() {
    let l = Line2D { points: vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]], closed: true, ..Default::default() };
    let m = line_mesh(&l).unwrap();
    // Every corner mitred, the first one too.
    for c in [[-5.0, -5.0], [105.0, -5.0], [105.0, 105.0], [-5.0, 105.0]] {
        assert!(has_vertex(&m, Vec2::from(c)), "corner {c:?}");
    }
}

#[test]
fn a_script_reshapes_it_and_the_scene_keeps_it() {
    let mut world = World::new();
    let e = world.spawn((jengine_core::ecs::components::Transform::default(), line(&[[0.0, 0.0], [10.0, 0.0]])));
    set_line_field(&world, e, "points", &serde_json::json!([[0, 0], [50, 0], [50, 50]])).unwrap();
    set_line_field(&world, e, "joint", &serde_json::json!("round")).unwrap();
    set_line_field(&world, e, "gradient", &serde_json::json!([[0, 1, 1, 1, 1], [1, 1, 0, 0, 1]])).unwrap();
    assert!(set_line_field(&world, e, "colour", &serde_json::json!(1)).is_err());
    {
        let l = world.get::<&Line2D>(e).unwrap();
        assert_eq!(l.points.len(), 3);
        assert_eq!(l.joint, LineJoint::Round);
        assert_eq!(l.gradient.len(), 2);
    }
    let scene = jengine_core::scene::format::save_scene(&world);
    let saved = scene.entities.iter().find_map(|e| e.components.line.as_ref()).expect("saved");
    assert_eq!(saved.points.len(), 3);
}

/// A gradient line, red to blue, across a black frame: red at its left end,
/// blue at its right, black above it.
#[cfg(feature = "gpu-wgpu")]
#[test]
fn a_line_draws_in_the_sprite_pass() {
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
    use jengine_core::ecs::components::Transform;
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 64;
    let Some(gpu) = HeadlessGpu::new(W, W) else {
        eprintln!("a_line_draws_in_the_sprite_pass: no GPU adapter - skipped");
        return;
    };
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
    let (_r, _w, _h, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let mut world = World::new();
    world.spawn((
        Transform::default(),
        Line2D {
            points: vec![[-28.0, 0.0], [28.0, 0.0]],
            width: 8.0,
            gradient: vec![[0.0, 1.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0, 1.0]],
            ..Default::default()
        },
    ));
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
    let px = |x: u32, y: u32| {
        let i = ((y * W + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    };
    let left = px(6, 32);
    let right = px(58, 32);
    let above = px(32, 20);
    // The gradient is linear and the screen sRGB: a few percent of the far
    // colour this near the end still shows as a few tens.
    assert!(left[0] > 200 && left[2] < 90, "red at the start: {left:?}");
    assert!(right[2] > 200 && right[0] < 90, "blue at the end: {right:?}");
    assert!(above == [0, 0, 0], "nothing off the line: {above:?}");
}

/// A bevel or round corner leaves no hole: every point of the corner's
/// wedge - out to the outer corners and in to where the inner edges meet -
/// is inside some triangle. The first version fanned the joint from the
/// corner point and left the half towards the inner corner open.
#[test]
fn a_corner_leaves_no_hole() {
    for joint in [LineJoint::Bevel, LineJoint::Round] {
        let l = Line2D {
            points: vec![[-90.0, -30.0], [-30.0, 30.0], [30.0, -30.0]],
            width: 26.0,
            joint,
            ..Default::default()
        };
        let m = line_mesh(&l).unwrap();
        let covered = |p: Vec2| {
            m.indices.chunks(3).any(|t| {
                let (a, b, c) = (m.positions[t[0] as usize], m.positions[t[1] as usize], m.positions[t[2] as usize]);
                let s = |u: Vec2, v: Vec2| (v - u).perp_dot(p - u);
                let (x, y, z) = (s(a, b), s(b, c), s(c, a));
                (x >= -1e-3 && y >= -1e-3 && z >= -1e-3) || (x <= 1e-3 && y <= 1e-3 && z <= 1e-3)
            })
        };
        // Down the middle of the corner, from just under the outer edge to
        // just over the inner corner (which is 18.4 under the point).
        for k in 0..20 {
            let p = Vec2::new(-30.0, 38.0 - k as f32 * 1.3);
            assert!(covered(p), "{joint:?}: a hole at {p}");
        }
    }
}
