//! Guard for the Engine Test Lab project.
//!
//! Every `.scene` / `.prefab` / `.gui` in `projects/engine-test-lab` is parsed
//! with the real deserializers and every asset it references is checked to
//! exist. The scenes there are hand-authored JSON that nothing else compiles,
//! so a renamed component field or a typo'd script path used to surface only
//! as "that scene is empty when I run it" — which, in a project whose whole
//! job is catching regressions, is the one failure mode that must not be
//! silent.
//!
//! It also enforces the wiring every scene needs to be reachable: an entry in
//! `nav.lua`'s TOUR, a button in `hub.gui`, and a route in `hub.lua`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use hecs::World;
use jengine_core::scene::format::{load_scene, SceneData};

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("projects")
        .join("engine-test-lab")
}

fn assets() -> PathBuf {
    project_root().join("assets")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Every file in a directory with the given extension, sorted.
fn files_with(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {}", dir.display(), e))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some(ext))
        .collect();
    out.sort();
    out
}

#[test]
fn every_scene_parses_and_loads() {
    let scenes = files_with(&assets().join("scenes"), "scene");
    assert!(scenes.len() >= 30, "expected the full scene set, got {}", scenes.len());

    for path in &scenes {
        let text = read(path);
        let scene: SceneData = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
        // Loading is where a component bundle with a wrong shape actually
        // bites; parsing alone would accept a field that silently defaults.
        let mut world = World::new();
        load_scene(&scene, &mut world);
        assert!(
            world.iter().count() > 0,
            "{} loaded zero entities",
            path.display()
        );
    }
}

#[test]
fn every_prefab_and_gui_parses() {
    for path in files_with(&assets().join("prefabs"), "prefab") {
        let text = read(&path);
        serde_json::from_str::<serde_json::Value>(&text)
            .unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
    }
    every_prefab_component_is_a_real_component();
    for path in files_with(&assets().join("ui"), "gui") {
        let text = read(&path);
        jengine_core::ui::gui_document::load_gui(&text)
            .unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
    }
}

/// Collect every asset path a scene references, as `assets/`-relative strings.
fn referenced_assets(scene: &serde_json::Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    // Keys whose value is an asset path. `texture_id` is deliberately absent:
    // sprites may name an atlas region rather than a file on disk.
    const PATH_KEYS: &[&str] = &[
        "source_path",
        "gui_id",
        "prefab_path",
        "graph_path",
        "material_id",
        "clip_id",
    ];
    fn walk(v: &serde_json::Value, out: &mut BTreeSet<String>, keys: &[&str]) {
        match v {
            serde_json::Value::Object(map) => {
                for (k, val) in map {
                    if keys.contains(&k.as_str())
                        && let Some(s) = val.as_str()
                        && !s.is_empty()
                    {
                        out.insert(s.to_string());
                    }
                    walk(val, out, keys);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    walk(item, out, keys);
                }
            }
            _ => {}
        }
    }
    walk(scene, &mut out, PATH_KEYS);
    out
}

#[test]
fn referenced_assets_exist() {
    let root = assets();
    let mut missing: Vec<String> = Vec::new();

    for path in files_with(&root.join("scenes"), "scene") {
        let json: serde_json::Value = serde_json::from_str(&read(&path)).unwrap();
        for rel in referenced_assets(&json) {
            // Material ids are not paths: they name either a built-in shader
            // (`default_sprite`, `lit_sprite`) or a `.material` asset written
            // without its extension.
            if !rel.contains('/') && !rel.contains('.') {
                continue;
            }
            if root.join(format!("{rel}.material")).exists() {
                continue;
            }
            if !root.join(&rel).exists() {
                missing.push(format!(
                    "{} references missing asset '{}'",
                    path.file_name().unwrap().to_string_lossy(),
                    rel
                ));
            }
        }
    }
    assert!(missing.is_empty(), "\n{}", missing.join("\n"));
}

/// True for a scene entered from another scene rather than from the hub —
/// `19a-coin`, `20-pause-overlay`. They carry no tour navigation of their own.
fn is_sub_scene(path: &Path) -> bool {
    let stem = path.file_stem().unwrap().to_string_lossy().to_string();
    stem.contains("overlay")
        || stem
            .chars()
            .skip_while(|c| c.is_ascii_digit())
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
}

/// Scene number prefix, e.g. `34-skeletal.scene` → `Some(34)`.
fn scene_number(path: &Path) -> Option<u32> {
    let stem = path.file_stem()?.to_str()?;
    let digits: String = stem.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[test]
fn every_numbered_scene_is_reachable() {
    let root = assets();
    let nav = read(&root.join("scripts").join("lua").join("nav.lua"));
    let hub_lua = read(&root.join("scripts").join("lua").join("hub.lua"));
    let hub_gui = read(&root.join("ui").join("hub.gui"));

    let mut problems: Vec<String> = Vec::new();
    for path in files_with(&root.join("scenes"), "scene") {
        let Some(n) = scene_number(&path) else { continue };
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if is_sub_scene(&path) {
            continue;
        }
        let rel = format!("scenes/{name}");
        if !nav.contains(&rel) {
            problems.push(format!("{name}: missing from nav.lua TOUR"));
        }
        if !hub_lua.contains(&rel) {
            problems.push(format!("{name}: missing from hub.lua BUTTONS"));
        }
        let btn = format!("btn_scene_{n:02}");
        if !hub_gui.contains(&btn) {
            problems.push(format!("{name}: no '{btn}' node in hub.gui"));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

/// Wired *once*, not twice. The reachability check above asks whether a
/// route is present, which a copy-pasted row satisfies twice over - and
/// that is how scene 81 came to be listed twice in the tour and three
/// times in the hub, where `]` from scene 80 landed on it, `]` again
/// reloaded it, and two buttons sat one above the other. A duplicate id
/// is worse than untidy: `gui_on_click` binds by id, so only one of them
/// answers.
#[test]
fn each_scene_is_wired_exactly_once() {
    let root = assets();
    let nav = read(&root.join("scripts").join("lua").join("nav.lua"));
    let hub_lua = read(&root.join("scripts").join("lua").join("hub.lua"));
    let hub_gui = read(&root.join("ui").join("hub.gui"));

    let mut problems: Vec<String> = Vec::new();
    for path in files_with(&root.join("scenes"), "scene") {
        let Some(n) = scene_number(&path) else { continue };
        if is_sub_scene(&path) {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let rel = format!("\"scenes/{name}\"");
        for (where_, src) in [("nav.lua TOUR", &nav), ("hub.lua BUTTONS", &hub_lua)] {
            let count = src.matches(&rel).count();
            if count > 1 {
                problems.push(format!("{name}: listed {count} times in {where_}"));
            }
        }
        let btn = format!("\"id\": \"btn_scene_{n:02}\"");
        let count = hub_gui.matches(&btn).count();
        if count > 1 {
            problems.push(format!("{name}: {count} 'btn_scene_{n:02}' nodes in hub.gui"));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

/// Every scene script referenced by a scene must define `update`, or the
/// scene loses its hub/next/prev navigation and strands the player.
#[test]
fn scene_scripts_have_nav() {
    let root = assets();
    let mut problems: Vec<String> = Vec::new();
    for path in files_with(&root.join("scenes"), "scene") {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name == "hub.scene" || name == "placeholder.scene" || is_sub_scene(&path) {
            continue;
        }
        let json: serde_json::Value = serde_json::from_str(&read(&path)).unwrap();
        let scripts: Vec<String> = referenced_assets(&json)
            .into_iter()
            .filter(|s| s.ends_with(".lua"))
            .collect();
        if scripts.is_empty() {
            problems.push(format!("{name}: no script, so no way back to the hub"));
            continue;
        }
        let has_nav = scripts.iter().any(|rel| {
            let src = read(&root.join(rel));
            src.contains("nav.tick") || src.contains("nav_tick")
        });
        if !has_nav {
            problems.push(format!("{name}: no script calls nav.tick"));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

/// Scene 34's rig is hand-authored JSON: bone names, per-vertex weights and
/// pre-baked bind-time offsets all have to agree with each other, and none of
/// it is checked by anything else. Drive the real IK solver and skinner and
/// assert the mesh actually deforms — a typo in a bone name would otherwise
/// just render a stiff mesh that still parses.
#[test]
fn scene34_skinned_mesh_deforms_with_ik() {
    use jengine_core::animation::skinned_mesh::{compute_skinned_positions, SkinnedMesh};
    use jengine_core::ecs::components::{IkTwoBone, SkeletonRoot};

    let path = assets().join("scenes").join("34-skeleton-ik.scene");
    let scene: SceneData = serde_json::from_str(&read(&path)).unwrap();
    let mut world = World::new();
    load_scene(&scene, &mut world);

    let root = world
        .query::<&SkeletonRoot>()
        .iter()
        .map(|(e, _)| e)
        .next()
        .expect("scene 34 must carry a SkeletonRoot");
    let mesh: SkinnedMesh = {
        let m = world
            .get::<&SkinnedMesh>(root)
            .expect("the rig root must carry the SkinnedMesh");
        (*m).clone()
    };
    assert_eq!(mesh.bones, vec!["upper".to_string(), "lower".to_string()]);
    assert_eq!(mesh.indices.len() % 3, 0, "indices must be whole triangles");

    jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world);
    jengine_core::animation::ik::solve_ik(&mut world);
    jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world);
    let before = compute_skinned_positions(&mesh, &world, root);

    for (_, ik) in world.query::<&mut IkTwoBone>().iter() {
        ik.target = [-200.0, 220.0];
    }
    jengine_core::animation::ik::solve_ik(&mut world);
    jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world);
    let after = compute_skinned_positions(&mesh, &world, root);

    assert_eq!(before.len(), mesh.vertices.len());
    assert_ne!(
        before, after,
        "skinned vertices did not move when the IK target changed —          check the bone names in `bones` against the rig"
    );
}

/// Scene 51's maze routes the walker to the goal *through* it, in both modes.
///
/// The first version of this scene had its walker and goal in an open border
/// round a few wall blocks, so every route simply went round the outside and
/// clicking a wall changed nothing worth seeing. The maze now has a closed
/// outer wall and the two sit at dead ends deep inside; this reads their
/// positions from the scene, paths between their tiles, and checks the route
/// is long, stays inside, and re-plans when a cell on it is walled.
#[test]
fn scene51_maze_is_solvable_in_both_modes() {
    use jengine_core::tilemap::{find_path, PathMode, Tilemap};

    let path = assets().join("tilemaps").join("51-maze.tilemap");
    let mut tm: Tilemap = serde_json::from_str(&read(&path))
        .unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
    let scene: serde_json::Value =
        serde_json::from_str(&read(&assets().join("scenes").join("51-tilemap-pathfinding.scene"))).unwrap();
    let pos = |name: &str| -> (f64, f64) {
        let e = scene["entities"].as_array().unwrap().iter().find(|e| e["name"] == name).unwrap();
        let p = &e["components"]["transform"]["position"];
        (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())
    };
    // The tilemap hangs from its top-left corner (see CLAUDE.md).
    let (mx, my) = pos("Maze");
    let tile_of = |name: &str| {
        let (x, y) = pos(name);
        let t = tm.tile_size[0] as f64;
        (((x - mx) / t).floor() as i32, ((my - y) / t).floor() as i32)
    };
    let (start, goal) = (tile_of("Walker"), tile_of("Goal"));

    let route = |tm: &Tilemap, mode| find_path(tm, "ground", start, goal, mode);
    let manhattan = route(&tm, PathMode::Manhattan)
        .expect("no 4-way route from the walker to the goal");
    let diagonal = route(&tm, PathMode::Diagonal).expect("no 8-way route from the walker to the goal");
    assert!(manhattan.len() > 30, "route is suspiciously short ({}) - is it going round the maze?", manhattan.len());
    assert!(diagonal.len() <= manhattan.len());
    assert_eq!((manhattan.first(), manhattan.last()), (Some(&start), Some(&goal)));

    // No cell on the outer edge is open, so there is no way round.
    let layer = tm.layers.iter().find(|l| l.name == "ground").unwrap();
    let (w, h) = (layer.width as i32, layer.height as i32);
    for x in 0..w {
        for y in [0, h - 1] {
            assert!(layer.get_tile(x as u32, y as u32).is_some(), "outer edge open at ({x}, {y})");
        }
    }

    // Walling some cell on the route forces a longer one - what clicking shows.
    let base = manhattan.len();
    let mut rerouted = false;
    for &(x, y) in &manhattan[1..manhattan.len() - 1] {
        let layer = tm.layers.iter_mut().find(|l| l.name == "ground").unwrap();
        layer.set_tile(x as u32, y as u32, Some(2));
        let longer = route(&tm, PathMode::Manhattan).is_some_and(|p| p.len() > base);
        let layer = tm.layers.iter_mut().find(|l| l.name == "ground").unwrap();
        layer.set_tile(x as u32, y as u32, None);
        if longer {
            rerouted = true;
            break;
        }
    }
    assert!(rerouted, "no single wall on the route forces a detour - the maze has no loops");
}

/// Compile every scene script with the *Luau* compiler — the dialect the
/// native runtime actually runs. A syntax error in a hand-edited scene script
/// would otherwise only show up as "that scene does nothing" the next time
/// somebody opened it, and the editor runs a different VM (Fengari, Lua 5.3),
/// so a Luau-only mistake can hide there.
///
/// This is a syntax check, not an execution: the engine globals do not exist
/// here, and a script that merely *calls* a missing function still compiles.
#[cfg(feature = "native")]
#[test]
fn every_scene_script_compiles_under_luau() {
    let dir = assets().join("scripts").join("lua");
    let lua = mlua::Lua::new();
    let mut checked = 0;
    let mut errors: Vec<String> = Vec::new();

    for path in files_with(&dir, "lua") {
        let src = read(&path);
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if let Err(e) = lua.load(&src).set_name(&name).into_function() {
            errors.push(format!("{name}: {e}"));
        }
        checked += 1;
    }
    assert!(checked >= 40, "expected the full script set, found {checked}");
    assert!(errors.is_empty(), "
{}", errors.join("
"));
}

/// The hub list must be fully scrollable, and stay inside the window, at any
/// window size.
///
/// Two failures live here. A scroll container is clipped to its own box, but
/// nothing clips that box to the screen, so a container taller than the window
/// pushes its lower rows — and half its scrollbar — somewhere scrolling cannot
/// reach. And a frame whose height is computed by a *script* can disagree with
/// the layout engine outright: in the editor's Game view the camera is sized to
/// the project's virtual resolution while the GUI lays out in canvas pixels, so
/// `get_screen_size()` and the GUI's own screen height are different numbers.
/// The document therefore sizes itself with `Percent`, which the layout engine
/// resolves against the height it is already using. This test asserts the
/// outcome both ways: nothing overflows the screen, and the last button is
/// fully visible at maximum scroll.
#[test]
fn hub_last_button_is_reachable_at_every_window_size() {
    use jengine_core::ui::layout::{apply_gui_scroll, compute_layout};

    let json = read(&assets().join("ui").join("hub.gui"));
    let mut failures: Vec<String> = Vec::new();

    for (w, h) in [
        (1215.0_f32, 843.0_f32),
        (1000.0, 700.0),
        (810.0, 573.0),
        (900.0, 560.0),
        (800.0, 480.0),
        (700.0, 420.0),
        (1600.0, 1200.0),
    ] {
        let mut doc = jengine_core::ui::gui_document::load_gui(&json).unwrap();

        // Nothing may sit outside the screen, at any scroll position.
        let initial = compute_layout(&doc, w, h);
        for e in &initial {
            if e.id == "root" {
                continue;
            }
            let r = e.rect;
            if r.y < -0.01 || r.y + r.height > h + 0.01 {
                if e.id.starts_with("btn_scene_") {
                    continue; // rows legitimately extend past the clip
                }
                failures.push(format!(
                    "{w}x{h}: '{}' is off-screen at y {:.0}..{:.0} (screen 0..{h})",
                    e.id,
                    r.y,
                    r.y + r.height
                ));
            }
        }

        // Scroll well past the end; the layout clamps to the real range.
        for _ in 0..500 {
            apply_gui_scroll(&mut doc, w / 2.0, h / 2.0, -1.0, w, h);
        }

        let layout = compute_layout(&doc, w, h);
        let last = layout
            .iter().rfind(|e| e.id.starts_with("btn_scene_"))
            .expect("hub.gui has no scene buttons");
        let v = last.visible_rect();
        let ok = !v.is_empty()
            && v.y >= -0.01
            && v.y + v.height <= h + 0.01
            && (v.height - 44.0).abs() < 0.01;
        if !ok {
            failures.push(format!(
                "{w}x{h}: {} ends at y {:.0}..{:.0} (h {:.0}) — screen is 0..{h}",
                last.id,
                v.y,
                v.y + v.height,
                v.height
            ));
        }
    }
    assert!(failures.is_empty(), "
{}", failures.join("
"));
}

/// Skeletal clips have to be *discoverable*, not just loadable.
///
/// `AssetManager::load_skeletal_clip` existed with no callers and nothing
/// walked the project for `.skelanim` files, so every runtime ticked
/// `update_skeletal_players` against an empty clip map: a `SkeletalPlayer`
/// held its pose forever and clip playback could not be authored at all.
/// This drives the whole chain — walk the directory, bind the scene's player,
/// advance time, and check a bone actually moved.
#[test]
fn scene34_skeletal_clip_loads_and_drives_the_rig() {
    use jengine_core::animation::skeletal::{update_skeletal_players, SkeletalPlayer};
    use jengine_core::ecs::components::{Bone, Transform};

    let root = assets();
    let mut am = jengine_core::asset::AssetManager::new();
    let found = am.load_skeletal_clips_from_dir(&root);
    assert!(found >= 1, "the walker should find the project's .skelanim files");

    let scene: SceneData =
        serde_json::from_str(&read(&root.join("scenes").join("34-skeleton-ik.scene"))).unwrap();
    let mut world = World::new();
    load_scene(&scene, &mut world);

    // The scene's player names a clip the walker just loaded — a typo here is
    // exactly the silent nothing this test exists to catch.
    let clip_id = world
        .query::<&SkeletalPlayer>()
        .iter()
        .map(|(_, p)| p.clip.clone())
        .next()
        .expect("scene 34 should carry a SkeletalPlayer");
    assert!(
        am.skeletal_clips().contains_key(&clip_id),
        "scene 34 names clip '{clip_id}', which was not among the loaded clips"
    );

    // It starts paused, so nothing should move until it is playing.
    let rotation_of = |world: &World, name: &str| -> f32 {
        for (e, b) in world.query::<&Bone>().iter() {
            if b.name == name {
                return world.get::<&Transform>(e).unwrap().rotation;
            }
        }
        panic!("no bone named {name}");
    };
    let before = rotation_of(&world, "upper");
    update_skeletal_players(&mut world, am.skeletal_clips(), 0.5);
    assert_eq!(
        rotation_of(&world, "upper"),
        before,
        "a paused player must not move the rig"
    );

    for (_, p) in world.query::<&mut SkeletalPlayer>().iter() {
        p.playing = true;
    }
    update_skeletal_players(&mut world, am.skeletal_clips(), 0.5);
    assert_ne!(
        rotation_of(&world, "upper"),
        before,
        "a playing clip should rotate the bone it has a track for"
    );
}

/// A prefab's components must actually deserialize into components.
///
/// Valid JSON is not enough. `spawn_prefab` routes the bundle through
/// `ComponentRegistry::deserialize_into_builder`, and anything the registry
/// does not recognise — a misspelled key, a renamed component, a field of the
/// wrong shape — is quietly swept into `CustomComponents`. The prefab still
/// spawns, just without the thing it was for.
///
/// That is not hypothetical: `PhysCrate.prefab` exists so scene 63 can drive a
/// **script-spawned rigid body**, which is the case BUGS #43/#45 were broken
/// in. A typo in `rigid_body` would give it no body at all, the scene would
/// demonstrate nothing, and every other test here would pass.
fn every_prefab_component_is_a_real_component() {
    let mut registry = jengine_core::package::component_registry::ComponentRegistry::new();
    jengine_core::package::core_registration::register_core_components(&mut registry);

    for path in files_with(&assets().join("prefabs"), "prefab") {
        let text = read(&path);
        let doc: serde_json::Value = serde_json::from_str(&text).expect("prefab json");
        let Some(components) = doc.get("components").and_then(|c| c.as_object()) else {
            panic!("{} has no `components` object", path.display());
        };
        let map: std::collections::HashMap<String, serde_json::Value> = components
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let mut builder = hecs::EntityBuilder::new();
        let unknown = registry.deserialize_into_builder(&mut builder, &map);
        assert!(
            unknown.is_empty(),
            "{} carries component keys the registry does not know: {:?}
             They would be swept into `CustomComponents` and the prefab would              spawn without them. Check the spelling against              `package/core_registration.rs`, or register the component.",
            path.display(),
            unknown.keys().collect::<Vec<_>>()
        );

        // Spawning proves the bundle is coherent, not just recognised.
        let mut world = hecs::World::new();
        let entity = world.spawn(builder.build());
        assert!(
            world.entity(entity).map(|e| e.component_types().count()).unwrap_or(0) > 0,
            "{} deserialized to an entity with no components",
            path.display()
        );
    }
}

/// The prefab scene 63 spawns must really carry a physics body.
///
/// Stated separately from the generic check above because this one is load
/// bearing for what the scene demonstrates: without a `RigidBody2D` the crate
/// is a sprite that ignores every force, which looks exactly like the bug the
/// scene exists to prove is fixed.
#[test]
fn the_scene_63_prefab_has_a_body_and_a_collider() {
    use jengine_core::ecs::components::{Collider2D, RigidBody2D};

    let path = assets().join("prefabs").join("PhysCrate.prefab");
    let doc: serde_json::Value = serde_json::from_str(&read(&path)).expect("prefab json");
    let map: std::collections::HashMap<String, serde_json::Value> = doc["components"]
        .as_object()
        .expect("components")
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let mut registry = jengine_core::package::component_registry::ComponentRegistry::new();
    jengine_core::package::core_registration::register_core_components(&mut registry);
    let mut builder = hecs::EntityBuilder::new();
    registry.deserialize_into_builder(&mut builder, &map);
    let mut world = hecs::World::new();
    let e = world.spawn(builder.build());

    assert!(
        world.get::<&RigidBody2D>(e).is_ok(),
        "PhysCrate has no RigidBody2D, so scene 63 would spawn a crate that          ignores every physics command - indistinguishable from BUGS #43/#45          coming back"
    );
    assert!(
        world.get::<&Collider2D>(e).is_ok(),
        "PhysCrate has no Collider2D, so it would fall through the floor"
    );
}

/// Every key name a script asks for must be a key the engine has.
///
/// `is_key_pressed` takes a **canonical `KeyCode::name()`** - which names the
/// physical key, not the character a layout prints on it - and a name it does
/// not recognise is not an error. It is simply false, forever, in every
/// runtime. Scene 08 asked for `"="`, `"+"`, `"-"` and `"_"`; the names are
/// `Equal` and `Minus`, so its light-radius controls had never once worked and
/// nothing said so.
///
/// The scene's own hint text advertised them, which is the shape this catches:
/// a control the scene promises, the script sincerely tries to read, and the
/// engine has no key for.
#[test]
fn every_key_name_a_script_asks_for_is_a_real_key() {
    use jengine_core::input::KeyCode;

    let scripts = assets().join("scripts");
    let mut bad: Vec<String> = Vec::new();

    for path in lua_files(&scripts) {
        let src = read(&path);
        for (lineno, line) in src.lines().enumerate() {
            let mut rest = line;
            while let Some(at) = rest.find("is_key_") {
                rest = &rest[at..];
                let Some(open) = rest.find("(\"") else { break };
                let after = &rest[open + 2..];
                let Some(close) = after.find('"') else { break };
                let name = &after[..close];
                if KeyCode::from_name(name).is_none() {
                    bad.push(format!(
                        "{}:{}  is_key_*(\"{name}\")",
                        path.file_name().unwrap().to_string_lossy(),
                        lineno + 1
                    ));
                }
                rest = &after[close..];
            }
        }
    }

    assert!(
        bad.is_empty(),
        "{} script key name(s) the engine has no key for:\n  {}\n\n\
         Names are physical keys from `KeyCode::name()`, not the characters a \
         layout prints: the `+` key is `Equal`, the `-` key is `Minus`. An \
         unrecognised name reads as false forever, in every runtime. The full \
         list is in docs/scripting-api.md under Key Names.",
        bad.len(),
        bad.join("\n  ")
    );
}

/// Every `.lua` under a directory, recursively.
fn lua_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(lua_files(&path));
        } else if path.extension().is_some_and(|e| e == "lua") {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// No scene may tell the reader to press the bracket keys.
///
/// Key names are **physical** keys, and `[` and `]` are not on
/// `BracketLeft` / `BracketRight` on every layout - a Finnish keyboard puts
/// them on AltGr+8 and AltGr+9, so a hint saying "press ] for the next scene"
/// is simply wrong there. `nav.lua` binds PageUp / PageDown as well, and those
/// are the same physical keys everywhere, so the hints name those.
///
/// Fourteen scenes said it, in five different spellings, and they were found
/// by hand three separate times - twice by me and once by the person sweeping.
/// Hence a check rather than another read-through.
///
/// Brackets as *notation* around a key name are fine and common here:
/// `[S] spawn crate`, `[1]-[6]`, `[Minus]/[Equal]`. Only the spellings that
/// mean "press this bracket" are refused.
#[test]
fn no_scene_tells_you_to_press_a_bracket() {
    // The spellings that have actually appeared. A bracket directly beside a
    // slash, an "or", or another bracket is one being offered as a key.
    const BAD: &[&str] = &["]/[", "[/]", "[ / ]", "[ or ]", "] or [", "[ and ]"];

    let mut found: Vec<String> = Vec::new();
    for path in files_with(&assets().join("scenes"), "scene") {
        let text = read(&path);
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        for e in v.get("entities").and_then(|e| e.as_array()).into_iter().flatten() {
            let Some(s) = e.pointer("/components/text/text").and_then(|t| t.as_str()) else {
                continue;
            };
            for bad in BAD {
                // `]/[` also falls out of notation like `[Minus]/[Equal]`,
                // where the brackets belong to the names either side. A
                // bracket *offered as a key* has no word character against
                // it.
                if s.match_indices(bad).any(|(at, _)| {
                    let before = s[..at].chars().next_back();
                    let after = s[at + bad.len()..].chars().next();
                    !before.is_some_and(|c| c.is_alphanumeric())
                        && !after.is_some_and(|c| c.is_alphanumeric())
                }) {
                    found.push(format!(
                        "{} / {}: {bad:?} in {:?}",
                        path.file_stem().unwrap().to_string_lossy(),
                        e.get("name").and_then(|n| n.as_str()).unwrap_or("?"),
                        s.chars().take(60).collect::<String>()
                    ));
                }
            }
        }
    }

    assert!(
        found.is_empty(),
        "{} hint(s) tell the reader to press a bracket key:\n  {}\n\n\
         `[` and `]` are not on the BracketLeft/BracketRight keys on every \
         layout, and key names are physical. Use the standard nav hint: \
         `[H/Esc] hub   [PgUp/PgDn] prev/next`.",
        found.len(),
        found.join("\n  ")
    );
}

/// Every scene's text must fit inside the camera it is drawn with.
///
/// World-space `TextComponent` has no alignment: it draws rightward from its
/// transform, so a label is centred only by the author having picked an `x`
/// equal to minus half its width - and that arithmetic goes stale the moment
/// anyone edits the string. Sixteen scenes had titles and hints running off
/// the right-hand edge, mid-word, including the first two anybody opens.
///
/// Nothing caught it because nothing measured: the scene parses, the entity
/// exists, the text renders. It is only wrong once you look at where it lands.
///
/// Measured with the built-in font. A scene using a custom `.font` is measured
/// with the wrong advances, but the built-in face is the wider of the two here,
/// so the answer errs towards reporting an overflow rather than missing one.
#[test]
fn every_scene_text_fits_on_screen() {
    use jengine_core::renderer::text::built_in_font_atlas;
    use jengine_core::renderer::text_layout::{layout_text, measure_line};

    let (_, _, _, font) = built_in_font_atlas();
    let mut over: Vec<String> = Vec::new();

    for path in files_with(&assets().join("scenes"), "scene") {
        let text = std::fs::read_to_string(&path).expect("read scene");
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let entities = v
            .get("entities")
            .and_then(|e| e.as_array())
            .cloned()
            .unwrap_or_default();
        // A camera that follows something scrolls: what is on screen is
        // wherever it goes, so text past the first view is the point of the
        // scene (166's distance markers), not an overflow. So does one whose
        // scene is about things coming on screen and leaving it (170's
        // lantern labels, along a street its camera pans). And a split
        // screen's cameras follow their players (184's numbered posts).
        if entities.iter().any(|e| {
            e.pointer("/components/camera_follow").is_some()
                || e.pointer("/components/visibility_notifier").is_some()
                || e.pointer("/components/camera/screen_rect").is_some()
        }) {
            continue;
        }

        // The scene's own camera decides what is on screen - its zoom too: a
        // camera at 0.5 shows twice as much world, which is how scene 94 fits
        // its whole map (and why its text had to be twice the usual size).
        let mut half_w = 640.0f32;
        for e in &entities {
            // A camera with a target draws into a texture, not the screen.
            if e.pointer("/components/camera/target").and_then(|t| t.as_str()).is_some_and(|t| !t.is_empty()) {
                continue;
            }
            if let Some(w) = e
                .pointer("/components/camera/viewport_width")
                .and_then(|w| w.as_f64())
            {
                let zoom = e
                    .pointer("/components/camera/zoom")
                    .and_then(|z| z.as_f64())
                    .filter(|z| *z > 0.0)
                    .unwrap_or(1.0);
                half_w = (w / 2.0 / zoom) as f32;
            }
        }

        for e in &entities {
            let Some(t) = e.pointer("/components/text") else {
                continue;
            };
            let s = t.get("text").and_then(|s| s.as_str()).unwrap_or("");
            if s.is_empty() {
                continue;
            }
            let scale = t.get("font_scale").and_then(|f| f.as_f64()).unwrap_or(1.0) as f32;
            let wrap = t.get("wrap_width").and_then(|w| w.as_f64()).unwrap_or(0.0) as f32;
            let x = e
                .pointer("/components/transform/position/0")
                .and_then(|p| p.as_f64())
                .unwrap_or(0.0) as f32;

            // A wrapped paragraph is only as wide as its wrap width allows;
            // measuring the whole string would call every one an overflow.
            let widest = if wrap > 0.0 {
                layout_text(&font, s, scale, Some(wrap))
                    .iter()
                    .map(|l| l.width)
                    .fold(0.0f32, f32::max)
            } else {
                s.split('\n')
                    .map(|line| measure_line(&font, line, scale))
                    .fold(0.0f32, f32::max)
            };

            // `align` moves where the text starts: centred text spans half its
            // width either side of x, right-aligned text ends at x.
            let align: jengine_core::ecs::components::TextAlign = t
                .get("align")
                .and_then(|a| serde_json::from_value(a.clone()).ok())
                .unwrap_or_default();
            let x = x - align.offset(widest);

            let name = e.get("name").and_then(|n| n.as_str()).unwrap_or("?");
            let scene = path.file_stem().unwrap().to_string_lossy();
            // A single pixel of slack: these are hand-placed round numbers and
            // a label landing exactly on the edge is fine.
            if x + widest > half_w + 1.0 {
                over.push(format!(
                    "{scene} / {name}: runs {:.0} past the right edge ({:?})",
                    x + widest - half_w,
                    s.chars().take(48).collect::<String>()
                ));
            }
            if x < -half_w - 1.0 {
                over.push(format!(
                    "{scene} / {name}: starts {:.0} left of the screen",
                    -half_w - x
                ));
            }
        }
    }

    assert!(
        over.is_empty(),
        "text off the edge of its camera in {} place(s):\n  {}\n\n\
         `align` decides where the text sits relative to its x: the default \
         draws rightward from it, `center` spans half its width either side, \
         and `right` *ends* at it - so a right-aligned label needs its x at \
         least its own width inside the edge, not at the edge. This check \
         already accounts for `align`, so a failure here is the position and \
         not the alignment. Re-measure after editing the string.",
        over.len(),
        over.join("\n  ")
    );
}

/// A script's lifecycle hooks must be globals.
///
/// Every runtime looks `init`, `update` and the `on_*` callbacks up by name
/// in the script's globals, so `local function update(dt)` is simply never
/// called - no error, no warning, the script just does nothing. Scene 18
/// shipped that way: its status text never changed and neither R nor the
/// hub keys worked (BUGS #92). Scans every project, not only the lab.
#[test]
fn no_script_hides_a_lifecycle_hook_in_a_local() {
    const HOOKS: &[&str] = &[
        "init", "update", "on_collision_enter", "on_collision_exit",
        "on_trigger_enter", "on_trigger_exit", "on_entered_trigger",
        "on_exited_trigger", "on_spawn", "on_despawn", "on_resume", "on_gui_click",
    ];
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "lua") {
                out.push(p);
            }
        }
    }
    let mut scripts = Vec::new();
    for project in std::fs::read_dir(project_root().join("..")).unwrap().flatten() {
        walk(&project.path().join("assets"), &mut scripts);
    }
    assert!(scripts.len() > 50, "only {} scripts found - the scan broke", scripts.len());

    let mut bad = Vec::new();
    for path in &scripts {
        for (n, line) in read(path).lines().enumerate() {
            for hook in HOOKS {
                if line.trim_start().starts_with(&format!("local function {hook}(")) {
                    bad.push(format!("{}:{}  {}", path.display(), n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "these lifecycle hooks are `local`, so no runtime will ever call them:\n  {}",
        bad.join("\n  ")
    );
}

/// Text drawn with the built-in font must only use characters it has.
///
/// A character outside the font renders as `?`, and nothing else notices:
/// the scene loads, the entity exists, the string is exactly what the author
/// typed. Eight lab scenes shipped em dashes that way, one a box-drawing rule,
/// and scene 23 labelled its angular-velocity demo `angular ? = ? rad/s` - an
/// omega and a pi. Checked against the real atlas's glyph list, so this moves
/// with the font if it is ever widened. Every project, not only the lab.
#[test]
fn builtin_font_text_uses_only_glyphs_the_font_has() {
    use jengine_core::renderer::text::built_in_font_atlas;

    let (_, _, _, font) = built_in_font_atlas();
    let have: std::collections::HashSet<char> = font.glyphs.iter().map(|g| g.ch).collect();

    fn walk(v: &serde_json::Value, have: &std::collections::HashSet<char>, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                let builtin = matches!(
                    m.get("font_id").and_then(|f| f.as_str()),
                    None | Some("") | Some("default")
                );
                if let (true, Some(t)) = (builtin, m.get("text").and_then(|t| t.as_str())) {
                    let missing: String =
                        t.chars().filter(|c| *c != '\n' && !have.contains(c)).collect();
                    if !missing.is_empty() {
                        out.push(format!("{:?} is missing {:?}", t.chars().take(50).collect::<String>(), missing));
                    }
                }
                m.values().for_each(|x| walk(x, have, out));
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, have, out)),
            _ => {}
        }
    }

    let mut bad = Vec::new();
    let mut checked = 0;
    for project in std::fs::read_dir(project_root().join("..")).unwrap().flatten() {
        let assets = project.path().join("assets");
        for dir in ["scenes", "prefabs"] {
            if !assets.join(dir).is_dir() {
                continue;
            }
            for ext in ["scene", "prefab"] {
                for path in files_with(&assets.join(dir), ext) {
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(&read(&path)) else {
                        continue;
                    };
                    checked += 1;
                    let mut out = Vec::new();
                    walk(&v, &have, &mut out);
                    bad.extend(out.into_iter().map(|m| format!("{}: {m}", path.display())));
                }
            }
        }
    }
    assert!(checked > 60, "only {checked} files checked - the scan broke");
    assert!(
        bad.is_empty(),
        "text the built-in font cannot draw (it renders as `?`). Use ASCII, or ship \
         a `.font` that has the glyph:\n  {}",
        bad.join("\n  ")
    );
}

/// **Every shipped `.shader` graph has to compile.**
///
/// A graph is hand-written JSON and nothing validates it until something
/// draws with it - at which point `GraphPipelineCache` records the failure and
/// falls back to the default sprite shader, so a broken graph looks like "the
/// material did nothing", which is indistinguishable from the material not
/// being wired at all.
///
/// Measured, rather than assumed: this catches an edge from a node id that
/// does not exist, an unknown node type, and a `FragmentOutput` nothing
/// reaches. It does **not** catch an out-of-range *pin index* - a graph whose
/// edge names pin 9 of a two-pin node still validates and still generates
/// WGSL. So this proves a graph compiles, not that it was wired the way its
/// author meant.
#[test]
fn every_shader_graph_generates_wgsl() {
    let dir = assets().join("shaders");
    let mut checked = 0;
    for path in files_with(&dir, "shader") {
        let src = read(&path);
        let graph: jengine_core::renderer::shader_graph::ShaderGraph =
            serde_json::from_str(&src)
                .unwrap_or_else(|e| panic!("{}: not a valid graph: {e}", path.display()));
        let errors = graph.validate();
        assert!(errors.is_empty(), "{}: {errors:?}", path.display());
        jengine_core::renderer::shader_codegen::generate(&graph)
            .unwrap_or_else(|e| panic!("{}: codegen failed: {e:?}", path.display()));
        checked += 1;
    }
    assert!(checked >= 2, "expected at least the pulse and rim graphs, found {checked}");
}

/// And a `.material` naming a graph must name one that is there.
#[test]
fn every_graph_material_points_at_a_shader_that_exists() {
    for path in files_with(&assets().join("materials"), "material") {
        let v: serde_json::Value = serde_json::from_str(&read(&path))
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let Some(graph) = v["shader_graph_path"].as_str() else { continue };
        let target = project_root().join(graph);
        assert!(
            target.exists(),
            "{} names `{graph}`, which does not exist",
            path.display()
        );
    }
}

/// Every numbered scene has a row in `SWEEP.md`.
///
/// The sweep list is how a scene gets looked at in each runtime. Scenes 82 to
/// 102 were added after it was written and had no row, so a sweep in tour
/// order simply ran out at 81 and those twenty-one were never on anyone's list.
#[test]
fn every_scene_has_a_sweep_row() {
    let sweep = read(&project_root().join("SWEEP.md"));
    let mut missing = Vec::new();
    for path in files_with(&assets().join("scenes"), "scene") {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let Some(num) = stem.split('-').next().filter(|n| n.chars().all(|c| c.is_ascii_digit())) else {
            continue;
        };
        // Rows spell 1-9 with a leading zero ("| 01 |"), later ones without.
        let bare = num.trim_start_matches('0');
        let rows = [format!("| {num} |"), format!("| {bare} |")];
        if !sweep.lines().any(|l| rows.iter().any(|r| l.starts_with(r.as_str()))) {
            missing.push(stem);
        }
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "scenes with no row in SWEEP.md: {missing:?}");
}

/// A sprite's `uv_rect` must cover at least one visible pixel of its texture.
///
/// `LabStranger` (scene 87) drew `[0, 0, 0.25, 0.25]` of a 16x64 strip - a
/// 4x16 sliver of fully transparent pixels - so the prefab the scene exists
/// to show spawned, moved and drew nothing in every runtime, which reads
/// exactly like the preload bug the scene is about. Checks every sprite in
/// every lab scene and prefab against the real image.
#[test]
fn every_sprite_uv_rect_shows_something() {
    use jengine_core::asset::RawTextureData;

    fn sprites(v: &serde_json::Value, out: &mut Vec<(String, [f64; 4])>) {
        match v {
            serde_json::Value::Object(m) => {
                if let Some(s) = m.get("sprite")
                    && let (Some(tex), Some(uv)) = (s["texture_id"].as_str(), s["uv_rect"].as_array())
                    && uv.len() == 4
                    && s.get("region").is_none()
                {
                    let r = [0, 1, 2, 3].map(|i| uv[i].as_f64().unwrap_or(0.0));
                    out.push((tex.to_string(), r));
                }
                m.values().for_each(|x| sprites(x, out));
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| sprites(x, out)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    for dir in ["scenes", "prefabs"] {
        for ext in ["scene", "prefab"] {
            for path in files_with(&assets().join(dir), ext) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&read(&path)) {
                    let mut s = Vec::new();
                    sprites(&v, &mut s);
                    found.extend(s.into_iter().map(|(t, r)| (path.clone(), t, r)));
                }
            }
        }
    }
    assert!(found.len() > 10, "only {} uv_rect sprites found - the scan broke", found.len());

    let mut blank = Vec::new();
    for (path, tex, [u0, v0, u1, v1]) in found {
        let file = assets().join(&tex);
        let Ok(bytes) = std::fs::read(&file) else { continue }; // missing textures are another test's job
        let Ok(img) = RawTextureData::from_bytes(&bytes) else { continue };
        let (w, h) = (img.width as f64, img.height as f64);
        let (x0, x1) = ((u0.min(u1) * w).floor() as u32, ((u0.max(u1) * w).ceil() as u32).min(img.width));
        let (y0, y1) = ((v0.min(v1) * h).floor() as u32, ((v0.max(v1) * h).ceil() as u32).min(img.height));
        let visible = (y0..y1).any(|y| (x0..x1).any(|x| img.rgba[((y * img.width + x) * 4 + 3) as usize] > 0));
        if !visible {
            blank.push(format!("{}: {tex} [{u0}, {v0}, {u1}, {v1}]", path.display()));
        }
    }
    assert!(blank.is_empty(), "sprites whose uv_rect covers only transparent pixels:\n  {}", blank.join("\n  "));
}
