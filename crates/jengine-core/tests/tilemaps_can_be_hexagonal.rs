//! **Hexagonal tilemaps** (Oct 2026): `"orientation": "hexagonal"` lays a
//! map out in hexagons - pointy-topped in rows or flat-topped in columns,
//! every other row (or column) pushed half a cell, as Tiled has them. Before
//! it a hex map from Tiled was read as square, and a strategy board could
//! not be made of tiles at all. Found by the showcase's Hex Strategy Board.
//!
//! The grid's shape is `Tilemap::cell_centre`, `cell_outline`, `cell_at` and
//! `neighbours`; this checks them, and what reads them - picking, the
//! distance and the path, the colliders, the scene format, Tiled, a script,
//! the browser's hooks and, on the GPU, where a tile is drawn.

use jengine_core::tilemap::{
    PathMode, StaggerAxis, StaggerIndex, TileOrientation, Tilemap, TilemapLayer, TilesetDef, find_path,
};

const TW: f32 = 56.0;
const TH: f32 = 64.0;
const SIDE: f32 = 32.0;

fn hex(axis: StaggerAxis, index: StaggerIndex) -> Tilemap {
    let (tw, th) = if axis == StaggerAxis::Y { (TW, TH) } else { (TH, TW) };
    Tilemap {
        tile_size: [tw, th],
        orientation: TileOrientation::Hexagonal,
        hex_side: SIDE,
        stagger_axis: axis,
        stagger_index: index,
        ..Default::default()
    }
}

fn every_kind() -> Vec<Tilemap> {
    let mut out = Vec::new();
    for axis in [StaggerAxis::Y, StaggerAxis::X] {
        for index in [StaggerIndex::Odd, StaggerIndex::Even] {
            out.push(hex(axis, index));
        }
    }
    out
}

fn layer(name: &str, tiles: Vec<u32>, w: u32, h: u32) -> TilemapLayer {
    TilemapLayer { name: name.into(), tiles, width: w, height: h, ..Default::default() }
}

#[test]
fn rows_are_pushed_half_a_hexagon() {
    let tm = hex(StaggerAxis::Y, StaggerIndex::Odd);
    // Cell (0, 0)'s box hangs from the origin; a row steps down by the
    // hexagon's height less its point, and an odd row is pushed right.
    assert_eq!(tm.cell_centre(0, 0), (TW / 2.0, -TH / 2.0));
    assert_eq!(tm.cell_centre(1, 0), (TW * 1.5, -TH / 2.0));
    assert_eq!(tm.cell_centre(0, 1), (TW, -TH / 2.0 - (TH + SIDE) / 2.0), "row 1 is pushed");
    let even = hex(StaggerAxis::Y, StaggerIndex::Even);
    assert_eq!(even.cell_centre(0, 0), (TW, -TH / 2.0), "pushing the even rows pushes row 0");
    // Flat-topped: columns step right and an odd column is pushed down.
    let flat = hex(StaggerAxis::X, StaggerIndex::Odd);
    let [fw, fh] = flat.tile_size;
    assert_eq!(flat.cell_centre(1, 0), ((fw + SIDE) / 2.0 + fw / 2.0, -fh), "column 1 is pushed down");
}

#[test]
fn every_point_picks_the_hexagon_it_is_in() {
    for tm in every_kind() {
        for r in -2..6 {
            for c in -2..6 {
                let (x, y) = tm.cell_centre(c, r);
                assert_eq!(tm.cell_at(x, y), (c, r), "its own centre, {:?}", tm.stagger_axis);
                // Most of the way to each corner is still inside.
                for (px, py) in tm.cell_outline(c, r) {
                    let p = (x + (px - x) * 0.9, y + (py - y) * 0.9);
                    assert_eq!(tm.cell_at(p.0, p.1), (c, r), "near a corner of ({c}, {r})");
                }
                // And world_to_tile / tile_to_world are the same thing.
                assert_eq!(tm.tile_to_world(5.0, 7.0, c, r), (x + 5.0, y + 7.0));
                assert_eq!(tm.world_to_tile(5.0, 7.0, x + 5.0, y + 7.0), (c, r));
            }
        }
    }
}

#[test]
fn hexagons_tile_the_plane() {
    // Every neighbour shares an edge: two of its corners are this cell's.
    for tm in every_kind() {
        for (c, r) in [(0, 0), (1, 1), (2, 3), (3, 2)] {
            let mine = tm.cell_outline(c, r);
            let ns = tm.neighbours(c, r);
            assert_eq!(ns.len(), 6);
            for (nc, nr) in ns {
                let shared = tm
                    .cell_outline(nc, nr)
                    .iter()
                    .filter(|p| mine.iter().any(|q| (p.0 - q.0).abs() < 1e-3 && (p.1 - q.1).abs() < 1e-3))
                    .count();
                assert_eq!(shared, 2, "({c}, {r}) and ({nc}, {nr}) share an edge, {:?} {:?}", tm.stagger_axis, tm.stagger_index);
                assert!(tm.neighbours(nc, nr).contains(&(c, r)), "and it is a neighbour back");
            }
        }
    }
}

#[test]
fn distance_is_the_number_of_steps() {
    // Against a breadth-first walk over `neighbours`.
    for tm in every_kind() {
        let start = (3, 3);
        let mut dist = std::collections::HashMap::from([(start, 0u32)]);
        let mut queue = std::collections::VecDeque::from([start]);
        while let Some(p) = queue.pop_front() {
            let d = dist[&p];
            if d >= 5 {
                continue;
            }
            for n in tm.neighbours(p.0, p.1) {
                if !dist.contains_key(&n) {
                    dist.insert(n, d + 1);
                    queue.push_back(n);
                }
            }
        }
        for (p, d) in dist {
            assert_eq!(tm.cell_distance(start, p), d, "{p:?}, {:?} {:?}", tm.stagger_axis, tm.stagger_index);
        }
    }
}

#[test]
fn a_path_steps_from_hexagon_to_hexagon_round_a_wall() {
    let mut tm = hex(StaggerAxis::Y, StaggerIndex::Odd);
    let (w, h) = (8u32, 7u32);
    let mut tiles = vec![0; (w * h) as usize];
    // A wall down column 4, rows 0..=5: the way round is under it.
    for r in 0..6 {
        tiles[(r * w + 4) as usize] = 1;
    }
    tm.layers.push(layer("walls", tiles, w, h));
    let open = find_path(&tm, "walls", (1, 1), (6, 1), PathMode::Manhattan).expect("a way round");
    for pair in open.windows(2) {
        assert!(tm.neighbours(pair[0].0, pair[0].1).contains(&pair[1]), "each step to a neighbour: {open:?}");
    }
    assert!(open.iter().all(|&(c, r)| !(c == 4 && r < 6)), "never through the wall");
    assert!(open.iter().any(|&(_, r)| r == 6), "round its end");
    // On an empty map the path is as short as the distance says.
    let mut empty = hex(StaggerAxis::X, StaggerIndex::Even);
    empty.layers.push(layer("walls", vec![0; 64], 8, 8));
    let p = find_path(&empty, "walls", (0, 0), (7, 5), PathMode::Diagonal).unwrap();
    assert_eq!(p.len() as u32 - 1, empty.cell_distance((0, 0), (7, 5)));
    // Neighbours a script gets are on the map.
    assert!(empty.neighbours_on_map(0, 0).iter().all(|&(c, r)| c >= 0 && r >= 0));
    assert_eq!(empty.neighbours_on_map(3, 3).len(), 6);
}

#[test]
fn a_tall_tile_stands_on_its_hexagon() {
    let tm = hex(StaggerAxis::Y, StaggerIndex::Odd);
    let ts = TilesetDef { tile_width: 56, tile_height: 112, ..Default::default() };
    let (centre, size, foot) = tm.cell_quad(&ts, 1, 1);
    let (cx, cy) = tm.cell_centre(1, 1);
    assert_eq!(size, [TW, 112.0]);
    assert_eq!(centre[0], cx);
    assert!((centre[1] - size[1] / 2.0 - (cy - TH / 2.0)).abs() < 1e-3, "bottom on the box's bottom");
    assert_eq!(foot, cy, "sorts by the cell's centre");
}

#[test]
fn the_scene_format_and_tiled_keep_it() {
    let tm = hex(StaggerAxis::X, StaggerIndex::Even);
    let json = serde_json::to_value(&tm).unwrap();
    assert_eq!(json["orientation"], "hexagonal");
    assert_eq!(json["stagger_axis"], "x");
    let back: Tilemap = serde_json::from_value(json).unwrap();
    assert_eq!((back.hex_side, back.stagger_axis, back.stagger_index), (SIDE, StaggerAxis::X, StaggerIndex::Even));
    // A square map writes none of it.
    let square = serde_json::to_value(Tilemap { tile_size: [32.0, 32.0], ..Default::default() }).unwrap();
    assert!(square.get("hex_side").is_none() && square.get("stagger_axis").is_none());
    let tiled = r#"{ "width": 2, "height": 2, "tilewidth": 56, "tileheight": 64, "orientation": "hexagonal",
        "hexsidelength": 32, "staggeraxis": "y", "staggerindex": "even", "layers": [], "tilesets": [] }"#;
    let tm = jengine_core::tilemap::tiled::parse_tiled_json(tiled).unwrap();
    assert_eq!(tm.orientation, TileOrientation::Hexagonal);
    assert_eq!((tm.hex_side, tm.stagger_index), (32.0, StaggerIndex::Even));
}

#[cfg(feature = "physics")]
#[test]
fn a_hexagonal_map_collides_in_hexagons() {
    use jengine_core::ecs::components::{TilemapCollider, Transform};
    use jengine_core::ecs::systems::ensure_physics_bodies;
    use jengine_core::physics::PhysicsWorld;
    let mut tm = hex(StaggerAxis::Y, StaggerIndex::Odd);
    let mut tiles = vec![0; 16];
    tiles[4 + 1] = 1; // cell (1, 1)
    tiles[4 + 2] = 1; // and its right-hand neighbour
    tm.layers.push(layer("ground", tiles, 4, 4));
    let mut world = hecs::World::new();
    let spec = TilemapCollider { solid: vec!["ground".into()], one_way: vec![] };
    world.spawn((Transform::default(), tm.clone(), spec));
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    ensure_physics_bodies(&world, &mut physics);
    physics.step(1.0 / 60.0);
    // Straight down through a cell's centre: met at its top point.
    let (cx, cy) = tm.cell_centre(1, 1);
    let hit = physics.raycast(cx, 500.0, 0.0, -1.0, 1000.0).expect("the cell is solid");
    assert!((hit.point_y - (cy + TH / 2.0)).abs() < 0.5, "met at the hexagon's point, got {}", hit.point_y);
    // Where two neighbours meet, the dip between their points.
    let hit = physics.raycast(cx + TW / 2.0, 500.0, 0.0, -1.0, 1000.0).expect("the seam is solid");
    assert!((hit.point_y - (cy + SIDE / 2.0)).abs() < 0.5, "met at the shoulder, got {}", hit.point_y);
    // The ray snapshot the browsers raycast against agrees.
    let mut snap = jengine_core::physics::ray::RaySnapshot::new();
    snap.refresh(&world);
    let hit = snap.cast(cx, 500.0, 0.0, -1.0, 1000.0).expect("the snapshot has it");
    assert!((hit.point_y - (cy + TH / 2.0)).abs() < 0.5);
}

#[cfg(feature = "native")]
#[test]
fn a_script_counts_steps_and_neighbours() {
    use hecs::World;
    use jengine_core::ecs::components::{CustomComponents, ScriptComponent, ScriptLanguage, Transform};
    use jengine_core::ecs::systems::ScriptSystem;
    use jengine_core::input::InputState;
    const SRC: &str = r#"
function update(entity, dt)
    local ns = Tilemap.neighbors(entity, 2, 2)
    Entity.set_custom(entity, "Read", "count", #ns)
    Entity.set_custom(entity, "Read", "corner", #Tilemap.neighbors(entity, 0, 0))
    Entity.set_custom(entity, "Read", "first_x", ns[1][1])
    Entity.set_custom(entity, "Read", "far", Tilemap.distance(entity, 0, 0, 4, 3))
    local tx, ty = Tilemap.world_to_tile(entity, 200, -150)
    Entity.set_custom(entity, "Read", "tx", tx)
    Entity.set_custom(entity, "Read", "ty", ty)
end
"#;
    let dir = std::env::temp_dir().join(format!("jengine_hex_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hex.lua");
    std::fs::write(&path, SRC).unwrap();
    let mut tm = hex(StaggerAxis::Y, StaggerIndex::Odd);
    tm.layers.push(layer("ground", vec![0; 25], 5, 5));
    let mut world = World::new();
    let e = world.spawn((
        Transform::default(),
        tm.clone(),
        CustomComponents::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    system.run(&mut world, 1.0 / 60.0, &InputState::new(), None, None);
    let custom = world.get::<&CustomComponents>(e).unwrap();
    let read = custom.0.get("Read").expect("the script wrote its readings");
    let n = |k: &str| read.get(k).and_then(|v| v.as_f64()).unwrap_or(f64::NAN);
    assert_eq!(n("count"), 6.0, "six around a middle cell: {read}");
    assert_eq!(n("corner"), 2.0, "two on the map at the corner (the rest are off it)");
    assert_eq!(n("first_x"), tm.neighbours(2, 2)[0].0 as f64);
    assert_eq!(n("far"), tm.cell_distance((0, 0), (4, 3)) as f64);
    assert_eq!((n("tx"), n("ty")), { let (c, r) = tm.cell_at(200.0, -150.0); (c as f64, r as f64) });
}

#[cfg(feature = "gpu-wgpu")]
mod gpu {
    use super::*;
    use glam::Vec2;
    use hecs::World;
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
    use jengine_core::ecs::components::Transform;
    use jengine_core::ecs::wgpu_systems::render_tilemaps_wgpu;
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;

    const W: u32 = 240;
    const H: u32 = 200;

    #[test]
    fn a_tile_is_drawn_on_its_hexagon() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("tilemaps_can_be_hexagonal: no GPU adapter - skipped");
            return;
        };
        let mut assets = AssetManager::new();
        let mut rgba = Vec::new();
        for _ in 0..56 * 64 {
            rgba.extend_from_slice(&[40, 160, 40, 255]);
        }
        for (id, tex) in [
            (WHITE_TEXTURE_ID, RawTextureData { width: 1, height: 1, rgba: vec![255; 4] }),
            ("hex", RawTextureData { width: 56, height: 64, rgba }),
        ] {
            assets.insert_texture_bytes(id.into(), tex);
            assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).unwrap();
        }
        let mut tm = hex(StaggerAxis::Y, StaggerIndex::Odd);
        let mut tiles = vec![0; 9];
        tiles[3] = 1; // cell (0, 1): row 1 is pushed right
        tm.layers.push(layer("ground", tiles, 3, 3));
        tm.tilesets.push(TilesetDef {
            texture_id: "hex".into(), tile_width: 56, tile_height: 64, columns: 1, tile_count: 1, first_gid: 1,
            image_width: 56, image_height: 64,
            ..Default::default()
        });
        let origin = Vec2::new(-100.0, 90.0);
        let mut world = World::new();
        world.spawn((Transform { position: origin, ..Default::default() }, tm.clone()));
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
        }
        gpu.submit(encoder);
        let img = gpu.read_rgba();
        let pixel = |x: f32, y: f32| {
            let (px, py) = ((x + W as f32 / 2.0) as u32, (H as f32 / 2.0 - y) as u32);
            let i = ((py * img.width + px) * 4) as usize;
            [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
        };
        let (cx, cy) = tm.cell_centre(0, 1);
        let (cx, cy) = (cx + origin.x, cy + origin.y);
        assert!(pixel(cx, cy)[1] > 120, "drawn at its pushed centre: {:?}", pixel(cx, cy));
        // Half a hexagon left of it - where an unpushed row would put it - is
        // the left edge of the box, so further left is empty.
        assert_eq!(pixel(cx - TW * 0.75, cy), [0, 0, 0], "nothing where a square grid would draw it");
    }
}

/// The browser preamble keeps no copy of hexagon picking: on a hexagonal map
/// `world_to_tile` and `tile_to_world` ask the host's hooks (`js/nav.rs`,
/// which answer with `Tilemap` itself), as `neighbors` and `distance` do.
#[test]
fn the_browser_preamble_asks_the_host_about_hexagons() {
    const SRC: &str = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
    let lift = |name: &str| -> String {
        let start = SRC.find(&format!("  function {name}(")).unwrap_or_else(|| panic!("{name} is in the preamble"));
        let end = start + SRC[start..].find("\n  }}\n").expect("the function ends") + "\n  }}".len();
        SRC[start..end].replace("{{", "{").replace("}}", "}")
    };
    let script = format!(
        "var window = {{ __jengine_play: {{ tilemaps: {{ 1: {{ tile_size: [56, 64], hex: true, layers: [] }} }}, state: {{\n\
           tilemap_world_to_tile: function (e, x, y) {{ return [e, x + y]; }},\n\
           tilemap_tile_to_world: function (e, c, r) {{ return [c * 10, r * 10]; }},\n\
           tilemap_neighbors: function (e, c, r) {{ return [[c + 1, r]]; }},\n\
           tilemap_distance: function (e, a, b, c, d) {{ return 7; }} }} }} }};\n\
         function get_position(e) {{ return [0, 0]; }}\n{}\n{}\n{}\n{}\n\
         var got = [world_to_tile(1, 3, 4), tile_to_world(1, 2, 5), tilemap_neighbors(1, 4, 4), tilemap_distance(1, 0, 0, 1, 1)];\n\
         console.log(JSON.stringify(got));",
        lift("world_to_tile"),
        lift("tile_to_world"),
        lift("tilemap_neighbors"),
        lift("tilemap_distance"),
    );
    let out = match std::process::Command::new("node").arg("-e").arg(&script).output() {
        Ok(out) => out,
        Err(_) => {
            eprintln!("the_browser_preamble_asks_the_host_about_hexagons: no node - skipped");
            return;
        }
    };
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(text.trim(), "[[1,7],[20,50],[[5,4]],7]", "{text}\n{}", String::from_utf8_lossy(&out.stderr));
}
