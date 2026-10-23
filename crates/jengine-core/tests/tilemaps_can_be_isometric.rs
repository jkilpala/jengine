//! **Isometric tilemaps** (Oct 2026): `"orientation": "isometric"` lays a
//! map out in diamonds, and a `y_sort` layer draws its tiles in the sprite
//! pass, sorted by cell with the y-sorted sprites. Before it the tilemap was
//! square-only and drawn under every sprite, so a hamlet could not be built
//! of tiles at all - a villager could not walk behind a house. Found by the
//! showcase's Hillside Hamlet.
//!
//! The grid's shape is decided in one place, `Tilemap::grid_to_local` and
//! its inverse; this checks the arithmetic, what reads it (picking, the
//! colliders, the scene format) and, on the GPU, the sort.

use jengine_core::tilemap::{TileOrientation, Tilemap, TilemapLayer, TilesetDef};

const TW: f32 = 32.0;
const TH: f32 = 16.0;

fn iso(layers: Vec<TilemapLayer>, tilesets: Vec<TilesetDef>) -> Tilemap {
    Tilemap { tile_size: [TW, TH], orientation: TileOrientation::Isometric, layers, tilesets, ..Default::default() }
}

fn layer(name: &str, tiles: Vec<u32>, y_sort: bool) -> TilemapLayer {
    TilemapLayer { name: name.into(), tiles, width: 4, height: 4, y_sort, ..Default::default() }
}

#[test]
fn the_grid_is_diamonds() {
    let tm = iso(vec![], vec![]);
    // The entity's position is the top vertex of cell (0, 0); a column runs
    // down to the right, a row down to the left.
    assert_eq!(tm.tile_to_world(0.0, 0.0, 0, 0), (0.0, -TH / 2.0), "cell (0, 0) hangs under the origin");
    assert_eq!(tm.tile_to_world(0.0, 0.0, 1, 0), (TW / 2.0, -TH), "a column steps down-right");
    assert_eq!(tm.tile_to_world(0.0, 0.0, 0, 1), (-TW / 2.0, -TH), "a row steps down-left");
    // Picking is the inverse, everywhere in a diamond and not past its edges.
    for (c, r) in [(0, 0), (3, 1), (2, 5), (7, 7)] {
        let (x, y) = tm.tile_to_world(100.0, 50.0, c, r);
        assert_eq!(tm.world_to_tile(100.0, 50.0, x, y), (c, r), "its own centre");
        assert_eq!(tm.world_to_tile(100.0, 50.0, x + TW * 0.45, y), (c, r), "near its right vertex");
        assert_eq!(tm.world_to_tile(100.0, 50.0, x, y + TH * 0.45), (c, r), "near its top vertex");
    }
    // Just past the top-right edge of (0, 0) is the cell above it in row -1.
    assert_eq!(tm.world_to_tile(0.0, 0.0, TW * 0.4, -TH * 0.1), (0, -1));
}

#[test]
fn an_orthogonal_map_is_what_it_was() {
    let tm = Tilemap { tile_size: [32.0, 32.0], ..Default::default() };
    assert_eq!(tm.orientation, TileOrientation::Orthogonal);
    assert_eq!(tm.world_to_tile(0.0, 0.0, 40.0, -50.0), (1, 1));
    assert_eq!(tm.tile_to_world(0.0, 0.0, 1, 1), (48.0, -48.0));
    let (cols, rows) = tm.cells_in_box(10, 10, [-1.0, -70.0], [70.0, 1.0]);
    assert_eq!((cols, rows), (0..3, 0..3), "the visible range is the old floor / ceil");
}

#[test]
fn a_tall_tile_stands_on_its_cell() {
    let tall = TilesetDef { tile_width: 32, tile_height: 64, ..Default::default() };
    let flat = TilesetDef { tile_width: 32, tile_height: 16, ..Default::default() };
    let tm = iso(vec![], vec![flat.clone(), tall.clone()]);
    let (centre, size, foot) = tm.cell_quad(&flat, 1, 1);
    let (cx, cy) = tm.tile_to_world(0.0, 0.0, 1, 1);
    assert_eq!((centre, size), ([cx, cy], [TW, TH]), "a diamond-sized tile fills its diamond");
    assert_eq!(foot, cy, "and sorts by the cell's centre");
    let (centre, size, _) = tm.cell_quad(&tall, 1, 1);
    assert_eq!(size, [TW, 64.0], "a tall tile keeps its proportions");
    assert_eq!(centre[1] - size[1] / 2.0, cy - TH / 2.0, "its bottom is the cell's bottom vertex");
    assert_eq!(tm.overhang(), 64.0 - TH, "and the cull knows how far it reaches up");
}

/// A square map's tile whose picture is taller than its cell stands on the
/// cell's bottom edge too (Oct 2026) - a tree two cells tall. It used to be
/// squashed into the cell; a picture the cell's shape still fills it.
#[test]
fn a_tall_tile_stands_on_a_square_cell() {
    let tall = TilesetDef { tile_width: 32, tile_height: 64, ..Default::default() };
    let square = TilesetDef { tile_width: 32, tile_height: 32, ..Default::default() };
    let tm = Tilemap { tile_size: [32.0, 32.0], tilesets: vec![square.clone(), tall.clone()], ..Default::default() };
    let (cx, cy) = tm.tile_to_world(0.0, 0.0, 2, 3);
    let (centre, size, foot) = tm.cell_quad(&square, 2, 3);
    assert_eq!((centre, size, foot), ([cx, cy], [32.0, 32.0], cy - 16.0), "a cell-sized tile fills its cell");
    let (centre, size, foot) = tm.cell_quad(&tall, 2, 3);
    assert_eq!(size, [32.0, 64.0], "a tall tile keeps its proportions");
    assert_eq!(centre[1] - size[1] / 2.0, cy - 16.0, "its bottom is the cell's bottom edge");
    assert_eq!(foot, cy - 16.0, "and it sorts by that edge");
    assert_eq!(tm.overhang(), 32.0, "the cull knows how far it reaches up");
    // The row below a view still draws into it.
    let (_, rows) = tm.cells_in_box(10, 10, [0.0, -64.0], [64.0, 0.0]);
    assert!(rows.contains(&2), "a tall tile one row below the view is drawn: {rows:?}");
    let plain = Tilemap { tile_size: [32.0, 32.0], tilesets: vec![square], ..Default::default() };
    assert_eq!(plain.overhang(), 0.0, "a map of cell-sized tiles culls as before");
}

#[test]
fn every_cell_a_view_touches_is_in_its_range() {
    let tall = TilesetDef { tile_width: 32, tile_height: 64, ..Default::default() };
    let tm = iso(vec![], vec![tall]);
    let (lo, hi) = ([-40.0, -60.0], [40.0, -20.0]);
    let (cols, rows) = tm.cells_in_box(20, 20, lo, hi);
    // Brute force: every cell whose quad overlaps the box must be in range.
    for r in 0..20 {
        for c in 0..20 {
            let (centre, size, _) = tm.cell_quad(&tm.tilesets[0], c, r);
            let overlaps = centre[0] + size[0] / 2.0 > lo[0]
                && centre[0] - size[0] / 2.0 < hi[0]
                && centre[1] + size[1] / 2.0 > lo[1]
                && centre[1] - size[1] / 2.0 < hi[1];
            if overlaps {
                assert!(cols.contains(&c) && rows.contains(&r), "cell ({c}, {r}) draws into the view but is culled");
            }
        }
    }
}

#[test]
fn the_scene_format_keeps_it_and_old_maps_stay_square() {
    let tm = iso(vec![TilemapLayer { offset: [0.0, 32.0], ..layer("hill", vec![0; 16], true) }], vec![]);
    let json = serde_json::to_value(&tm).unwrap();
    assert_eq!(json["orientation"], "isometric");
    assert_eq!(json["layers"][0]["y_sort"], true);
    assert_eq!(json["layers"][0]["offset"], serde_json::json!([0.0, 32.0]));
    let back: Tilemap = serde_json::from_value(json).unwrap();
    assert_eq!(back.orientation, TileOrientation::Isometric);
    assert!(back.layers[0].y_sort);
    // An orthogonal map writes none of it, so no existing scene changes.
    let square = Tilemap { tile_size: [16.0, 16.0], layers: vec![layer("g", vec![0; 16], false)], ..Default::default() };
    let json = serde_json::to_string(&square).unwrap();
    for key in ["orientation", "y_sort", "offset"] {
        assert!(!json.contains(key), "{key} written for a plain map: {json}");
    }
    let old: Tilemap = serde_json::from_str(r#"{ "tile_size": [16, 16], "layers": [], "tilesets": [] }"#).unwrap();
    assert_eq!(old.orientation, TileOrientation::Orthogonal);
}

#[test]
fn tiled_isometric_maps_import_as_isometric() {
    let json = r#"{ "width": 2, "height": 2, "tilewidth": 64, "tileheight": 32, "orientation": "isometric",
        "layers": [{ "name": "hill", "type": "tilelayer", "data": [1, 0, 0, 1], "width": 2, "height": 2,
                     "visible": true, "offsetx": 0, "offsety": -16 }],
        "tilesets": [] }"#;
    let tm = jengine_core::tilemap::tiled::parse_tiled_json(json).unwrap();
    assert_eq!(tm.orientation, TileOrientation::Isometric);
    assert_eq!(tm.layers[0].offset, [0.0, 16.0], "Tiled's y runs down; a layer lifted 16 px is +16 here");
}

/// The browser preamble does the same arithmetic in JavaScript (it reads the
/// published mirror, not the world), so the editor and web builds pick with
/// their own copy of it. This runs that copy - lifted out of `browser.rs` -
/// under `node` and compares it with the Rust. Skipped without `node`.
#[test]
fn the_browser_preamble_picks_the_same_cells() {
    const SRC: &str = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
    let lift = |name: &str| -> String {
        let start = SRC.find(&format!("  function {name}(")).unwrap_or_else(|| panic!("{name} is in the preamble"));
        let end = start + SRC[start..].find("\n  }}\n").expect("the function ends") + "\n  }}".len();
        SRC[start..end].replace("{{", "{").replace("}}", "}")
    };
    let mut cases = Vec::new();
    for iso_map in [true, false] {
        let tm = Tilemap {
            tile_size: [TW, TH],
            orientation: if iso_map { TileOrientation::Isometric } else { TileOrientation::Orthogonal },
            ..Default::default()
        };
        for (x, y) in [(13.0f32, -7.0f32), (-40.5, -90.25), (100.0, 20.0), (3.0, -1.0)] {
            let (c, r) = tm.world_to_tile(10.0, 20.0, x, y);
            let (wx, wy) = tm.tile_to_world(10.0, 20.0, 3, 2);
            cases.push(format!("[{iso_map}, {x}, {y}, {c}, {r}, {wx}, {wy}]"));
        }
    }
    let script = format!(
        "var window = {{ __jengine_play: {{ tilemaps: {{}} }} }};\n\
         function get_world_position(e) {{ return [10, 20]; }}\n{}\n{}\n\
         var bad = [];\n\
         [{}].forEach(function (k) {{\n\
           window.__jengine_play.tilemaps[1] = {{ tile_size: [{TW}, {TH}], iso: k[0], layers: [] }};\n\
           var t = world_to_tile(1, k[1], k[2]); var w = tile_to_world(1, 3, 2);\n\
           if (t[0] !== k[3] || t[1] !== k[4] || Math.abs(w[0] - k[5]) > 1e-4 || Math.abs(w[1] - k[6]) > 1e-4)\n\
             bad.push(JSON.stringify([k, t, w]));\n\
         }});\n\
         console.log(bad.length ? bad.join('\\n') : 'ok');",
        lift("world_to_tile"),
        lift("tile_to_world"),
        cases.join(", ")
    );
    let out = match std::process::Command::new("node").arg("-e").arg(&script).output() {
        Ok(out) => out,
        Err(_) => {
            eprintln!("the_browser_preamble_picks_the_same_cells: no node - skipped");
            return;
        }
    };
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(text.trim(), "ok", "the preamble disagrees with Tilemap:\n{text}\n{}", String::from_utf8_lossy(&out.stderr));
}

#[cfg(feature = "physics")]
#[test]
fn an_isometric_map_collides_in_diamonds() {
    use jengine_core::ecs::components::{TilemapCollider, Transform};
    use jengine_core::ecs::systems::ensure_physics_bodies;
    use jengine_core::physics::PhysicsWorld;
    let mut tiles = vec![0; 16];
    tiles[4 + 2] = 1; // cell (2, 1)
    let mut world = hecs::World::new();
    let spec = TilemapCollider { solid: vec!["ground".into()], one_way: vec![] };
    world.spawn((Transform::default(), iso(vec![layer("ground", tiles, false)], vec![]), spec));
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    ensure_physics_bodies(&world, &mut physics);
    physics.step(1.0 / 60.0);
    let tm = iso(vec![], vec![]);
    let (cx, cy) = tm.tile_to_world(0.0, 0.0, 2, 1);
    // Straight down through the cell's centre: the first thing met is its
    // top vertex - a box would be met at the same height across its width.
    let hit = physics.raycast(cx, 500.0, 0.0, -1.0, 1000.0).expect("the cell is solid");
    assert!((hit.point_y - (cy + TH / 2.0)).abs() < 0.5, "met at the diamond's top vertex, got {}", hit.point_y);
    // Off to the side, over the diamond's corner region a box would fill.
    let miss = physics.raycast(cx + TW * 0.45, 500.0, 0.0, -1.0, 1000.0);
    let near_edge = miss.map(|h| h.point_y).unwrap_or(f32::MIN);
    assert!(near_edge < cy + TH / 2.0 - 4.0, "the corner of the box is not there, got {near_edge}");
    // And the orthogonal box's place, left of the map, is empty.
    assert!(physics.raycast(80.0, 500.0, 0.0, -1.0, 1000.0).is_none());
}

#[cfg(feature = "gpu-wgpu")]
mod gpu {
    use super::*;
    use glam::Vec2;
    use hecs::World;
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
    use jengine_core::ecs::components::{SpriteComponent, Transform};
    use jengine_core::ecs::wgpu_systems::{
        SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized, render_tilemaps_wgpu,
    };
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 160;
    const H: u32 = 120;
    const GREEN: [u8; 3] = [40, 160, 40];
    const BROWN: [u8; 3] = [150, 90, 40];

    fn texture(w: u32, h: u32, c: [u8; 3]) -> RawTextureData {
        let mut rgba = Vec::new();
        for _ in 0..w * h {
            rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
        RawTextureData { width: w, height: h, rgba }
    }

    /// A 4 x 4 isometric map of green diamonds (a square texture drawn in a
    /// diamond's box - the corners overlap, which does not matter here), and
    /// on its `y_sort` layer one brown pillar 32 x 96 at cell (1, 1). A red
    /// sprite stands at cell (0, 1) - behind the pillar - and a blue one at
    /// (1, 2), in front of it; both reach into the pillar's picture.
    fn draw(gpu: &HeadlessGpu, y_sort: bool) -> RawTextureData {
        let mut assets = AssetManager::new();
        for (id, tex) in [
            (WHITE_TEXTURE_ID, texture(1, 1, [255, 255, 255])),
            ("ground", texture(32, 16, GREEN)),
            ("pillar", texture(32, 96, BROWN)),
        ] {
            assets.insert_texture_bytes(id.into(), tex);
            assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).unwrap();
        }
        let (rgba, aw, ah, font) = built_in_font_atlas();
        let _ = (rgba, aw, ah);
        let fonts = FrameFonts::builtin_only(&font, "__font");
        let mut objects = vec![0; 16];
        objects[4 + 1] = 2;
        let tm = iso(
            vec![layer("ground", vec![1; 16], false), layer("objects", objects, y_sort)],
            vec![
                TilesetDef { texture_id: "ground".into(), tile_width: 32, tile_height: 16, columns: 1, tile_count: 1,
                             first_gid: 1, image_width: 32, image_height: 16,
                             ..Default::default() },
                TilesetDef { texture_id: "pillar".into(), tile_width: 32, tile_height: 96, columns: 1, tile_count: 1,
                             first_gid: 2, image_width: 32, image_height: 96,
                             ..Default::default() },
            ],
        );
        // Centre the map: its middle (grid (2, 2)) at the origin.
        let origin = Vec2::new(0.0, 2.0 * TH);
        let mut world = World::new();
        let person = |c: [f32; 4]| SpriteComponent {
            texture_id: Default::default(),
            color: c,
            size: Some([10.0, 40.0]),
            anchor: [0.5, 0.0],
            y_sort: true,
            ..Default::default()
        };
        for ((c, r), colour) in [((0, 1), [1.0, 0.0, 0.0, 1.0]), ((1, 2), [0.0, 0.0, 1.0, 1.0])] {
            let (x, y) = tm.tile_to_world(origin.x, origin.y, c, r);
            // Nudged toward the pillar so each overlaps its picture.
            world.spawn((Transform { position: Vec2::new(x + 8.0, y), ..Default::default() }, person(colour)));
        }
        world.spawn((Transform { position: origin, ..Default::default() }, tm));

        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
        let camera = Camera2D::new(W as f32, H as f32);
        let vp = camera.view_projection();
        let mut encoder = gpu.encoder();
        {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            batch.begin_frame();
            render_tilemaps_wgpu(&world, &SortLayers::default(), &mut batch, &camera, |b, tex| {
                b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture(tex).unwrap());
            });
            render_sprites_with_shadows_wgpu_materialized(
                &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
                |b, tex, kind| {
                    let t = assets.get_wgpu_texture(tex).expect("uploaded");
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

    fn pixel(img: &RawTextureData, x: f32, y: f32) -> [u8; 3] {
        let px = (x + W as f32 / 2.0) as u32;
        let py = (H as f32 / 2.0 - y) as u32;
        let i = ((py * img.width + px) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    }

    fn near(a: [u8; 3], b: [u8; 3]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (*x as i16 - *y as i16).abs() <= 8)
    }

    #[test]
    fn a_y_sorted_tile_stands_between_the_sprites_behind_and_in_front() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("tilemaps_can_be_isometric: no GPU adapter - skipped");
            return;
        };
        let img = draw(&gpu, true);
        let tm = iso(vec![], vec![]);
        let origin = (0.0, 2.0 * TH);
        let (px, py) = tm.tile_to_world(origin.0, origin.1, 1, 1);
        // The pillar's picture rises from its cell's bottom vertex.
        let bottom = py - TH / 2.0;
        assert!(near(pixel(&img, px, bottom + 50.0), BROWN), "the pillar is drawn, tall: {:?}", pixel(&img, px, bottom + 50.0));
        // The red sprite behind it: covered where they overlap.
        let (rx, ry) = tm.tile_to_world(origin.0, origin.1, 0, 1);
        let overlap_red = (rx + 11.0, ry + 34.0); // above the blue one
        assert!(overlap_red.0 > px - TW / 2.0, "the test's sprites must overlap the pillar");
        assert!(near(pixel(&img, overlap_red.0, overlap_red.1), BROWN), "the pillar hides the one behind it: {:?}", pixel(&img, overlap_red.0, overlap_red.1));
        // The blue one in front: drawn over it.
        let (bx, by) = tm.tile_to_world(origin.0, origin.1, 1, 2);
        let overlap_blue = (bx + 11.0, by + 20.0);
        assert!(near(pixel(&img, overlap_blue.0, overlap_blue.1), [0, 0, 255]), "the one in front covers it");
        // Without `y_sort` the layer is drawn under every sprite, as before.
        let flat = draw(&gpu, false);
        assert!(near(pixel(&flat, overlap_red.0, overlap_red.1), [255, 0, 0]), "a plain layer is under the sprites");
        // And the ground is diamonds: the map's left vertex is green, the
        // space left of the bottom-left edge is not.
        let (lx, ly) = tm.tile_to_world(origin.0, origin.1, 0, 3);
        assert!(near(pixel(&img, lx, ly), GREEN), "ground at cell (0, 3)");
    }
}
