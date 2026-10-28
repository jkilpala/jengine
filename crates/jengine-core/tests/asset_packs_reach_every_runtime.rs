//! Asset packs (`asset::packs`) are wired into every host, and a file a pack
//! brought loads through the same loaders a file of the build does.
//!
//! The wiring is read from the hosts' source, as the other `*_reach_*`
//! guards do: a host that forgets the per-frame sync is a game whose
//! `Packs.download` never starts, and one whose fetch helpers skip
//! `web::read` is a pack that downloads and is never seen - both silent.
//! So is a host that never adds what a pack brought to the maps it filled at
//! startup (prefabs, data tables, clips, `require`'s modules): the pack
//! mounts and `spawn_prefab` still says the prefab does not exist.
#![cfg(feature = "packs")]

use std::path::{Path, PathBuf};

use jengine_core::asset::packs::{self, PackEntry, PackManifest, PackSettings, archive};

const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const NATIVE_LIB: &str = include_str!("../../jengine-runtime/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/lib.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const VIEWPORT_SVELTE: &str = include_str!("../../../editor/src/lib/Viewport.svelte");
const EDITOR_SETTINGS: &str = include_str!("../../../editor/src-tauri/src/commands/project.rs");
const SCRIPT_SYSTEM: &str = include_str!("../src/ecs/systems.rs");
const MODULES: &str = include_str!("../src/scripting/bridge/module.rs");

/// The body of `fn name` in `src`, up to the next top-level `fn`.
fn body<'a>(src: &'a str, name: &str) -> &'a str {
    let start = src.find(&format!("fn {name}(")).unwrap_or_else(|| panic!("no fn {name}"));
    let rest = &src[start..];
    let end = rest[3..].find("\nfn ").or_else(|| rest[3..].find("\nasync fn ")).map_or(rest.len(), |e| e + 3);
    &rest[..end]
}

#[test]
fn the_native_runner_mounts_syncs_and_downloads() {
    assert!(NATIVE.contains("native::init("), "the native runner never reads the build's packs");
    assert!(body(NATIVE, "build_state").contains("init_asset_packs(scene_path)"), "packs must be set up before the first scene loads");
    assert!(body(NATIVE, "sync_asset_packs").contains("packs::native::sync()"));
    assert!(NATIVE.contains("sync_asset_packs(state);"), "nothing starts a download natively");
    assert!(NATIVE_LIB.contains("packs::native::set_http("), "native has no HTTP client for packs");
    let sync = body(NATIVE, "sync_asset_packs");
    assert!(sync.contains("packs::native::arrived()"), "native never hears what a mounted pack brought");
    for add in ["add_prefabs(state.script_system.api.services.prefabs_mut())", "add_data(", "add_skeletal(&mut state.assets)"] {
        assert!(sync.contains(add), "native does not {add} a mounted pack's files");
    }
    assert!(sync.contains("set_data_registry("), "a pack's tables reach packages but not scripts");
    // Scripts are read lazily, so they need the pack lookup instead.
    assert!(SCRIPT_SYSTEM.contains("crate::asset::find_in_roots(source_path, &roots)"), "a pack's scripts are never read natively");
    assert!(body(MODULES, "fs_resolver").contains("packs::locate("), "`require` cannot reach a pack's modules natively");
}

#[test]
fn the_web_runtime_mounts_syncs_and_reads_through_packs() {
    assert!(body(WEB, "init_and_run").contains("packs::web::init("), "the web runtime never reads the build's packs");
    assert!(body(WEB, "poll_asset_packs").contains("packs::web::sync()"));
    assert!(WEB.contains("poll_asset_packs(&app_loop);"), "nothing starts a download on the web");
    for f in ["fetch_text", "fetch_bytes"] {
        assert!(body(WEB, f).contains("packs::web::read(url)"), "{f} does not ask the mounted packs first");
    }
    let poll = body(WEB, "poll_asset_packs");
    assert!(poll.contains("packs::web::arrived()"), "the web never hears what a mounted pack brought");
    for add in ["add_prefabs(&mut app.prefab_map)", "seed_prefabs(", "add_require_modules(", "seed_data_tables(", "add_skeletal(&mut app.assets)"] {
        assert!(poll.contains(add), "the web does not {add} for a mounted pack");
    }
    let modules = body(WEB, "add_require_modules");
    assert!(modules.contains("add_prefab_scripts(") && modules.contains("MODULE_SOURCES"), "a pack's modules miss one of the two `require`s");
    assert!(body(WEB, "install_module_resolver").contains("MODULE_SOURCES"), "the fallback `require` reads a copy taken at boot");
}

#[test]
fn the_editor_knows_the_projects_packs_and_keeps_them() {
    assert!(body(VIEWPORT, "viewport_set_asset_packs").contains("packs::configure(None"));
    assert!(VIEWPORT_SVELTE.contains("viewport_set_asset_packs(JSON.stringify("), "the editor never tells the viewport its packs");
    // The editor's typed settings are what a save writes: a field missing
    // there is erased by the first save from Project Settings.
    assert!(EDITOR_SETTINGS.contains("pub asset_packs: jengine_core::asset::packs::PackSettings"));
}

#[test]
fn every_core_loader_resolves_through_find_in_roots() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    for f in files {
        let src = std::fs::read_to_string(&f).unwrap();
        assert!(
            !src.contains("fn resolve_against_roots("),
            "{} resolves asset paths with its own copy, which no pack can reach - call asset::find_in_roots",
            f.display()
        );
    }
}

/// A built pack, installed from a folder, mounted, and read by a real loader.
#[test]
fn a_file_from_an_installed_pack_loads_like_one_from_the_build() {
    let dir = std::env::temp_dir().join(format!("jengine-packs-reach-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let game = dir.join("game");
    std::fs::create_dir_all(game.join("assets")).unwrap();
    let wav = b"RIFF....WAVEfmt fake".to_vec();
    let script = b"function update(e, dt) end".to_vec();
    let module = b"return { answer = 42 }".to_vec();
    let prefab = br#"{ "name": "Chime", "components": { "lifetime": { "remaining": 2.0 } } }"#.to_vec();
    let table = br#"{ "version": 1, "schema": "", "rows": [{ "id": "low", "hz": 220 }] }"#.to_vec();
    let files = [
        ("sounds/pack/chime.wav", wav.clone()),
        ("scripts/pack/bell.lua", script.clone()),
        ("scripts/pack/lib/tune.lua", module.clone()),
        ("prefabs/pack/chime.prefab", prefab),
        ("data/pack/notes.data", table),
    ];
    let bytes = archive::encode(&files.iter().map(|(p, b)| (p.to_string(), b.clone())).collect::<Vec<_>>());
    let sha256 = packs::sha256_hex(&bytes);
    let file = format!("chimes-{}.jpak", &sha256[..12]);
    std::fs::create_dir_all(game.join("packs")).unwrap();
    std::fs::write(game.join("packs").join(&file), &bytes).unwrap();
    let entry = PackEntry {
        name: "chimes".into(),
        file,
        size: bytes.len() as u64,
        sha256,
        files: files.iter().map(|(p, _)| p.to_string()).collect(),
    };
    let manifest = PackManifest { version: 1, url: String::new(), packs: vec![entry.clone()] };
    std::fs::write(game.join("assets").join(packs::MANIFEST_FILE), serde_json::to_string(&manifest).unwrap()).unwrap();

    // SAFETY: this test binary reads the variable only here.
    unsafe { std::env::set_var("JENGINE_PACK_DIR", dir.join("cache")) };
    packs::native::init(&game.join("assets"), &PackSettings::default(), vec![game.join("assets")], None);
    assert_eq!(packs::status("chimes").name(), "remote");
    let scene = serde_json::json!({ "audio_refs": ["sounds/pack/chime.wav"] });
    let mut assets = jengine_core::asset::AssetManager::new();
    let errors = jengine_core::audio::audio_asset::resolve_audio_refs(&scene, &[game.join("assets")], &mut assets);
    assert_eq!(errors.len(), 1, "not there before the download");

    packs::download("chimes");
    let source = packs::with(|p| p.source("chimes")).unwrap();
    assert_eq!(source, format!("packs/{}", entry.file), "a build with no url downloads from its own packs/");
    // What `native::sync` does on its worker thread, here in line.
    let _ = packs::take_requests();
    let dirp = packs::native::install(&entry, &source, &dir.join("cache"), &game, &mut |_| {}).unwrap();
    packs::set_installed("chimes", &entry.sha256, Some(&dirp), false);

    let errors = jengine_core::audio::audio_asset::resolve_audio_refs(&scene, &[game.join("assets")], &mut assets);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(assets.get_audio_data("sounds/pack/chime.wav"), Some(&wav[..]), "the loader read the pack's copy");

    // A script is found where the script system looks for one...
    let roots = [game.join("assets"), PathBuf::from("assets"), PathBuf::new()];
    let found = jengine_core::asset::find_in_roots("scripts/pack/bell.lua", &roots).expect("the pack's script");
    assert_eq!(std::fs::read(found).unwrap(), script);
    // ...and a module where `require` looks, with no scripts folder in the game.
    let require = jengine_core::scripting::bridge::fs_resolver(game.join("assets/scripts"));
    assert_eq!(require("pack/lib/tune").unwrap().as_bytes(), &module[..]);
    assert!(require("pack/lib/none").is_err());

    // What a runtime loaded at startup gains the pack's.
    let arrived = packs::native::arrived();
    assert_eq!(arrived.len(), 1, "the mount is news once");
    let content = &arrived[0];
    assert_eq!(content.scripts.len(), 2);
    let mut prefabs = std::collections::HashMap::new();
    assert_eq!(content.add_prefabs(&mut prefabs), vec!["Chime".to_string()]);
    assert_eq!(prefabs["Chime"].components.get::<jengine_core::ecs::components::Lifetime>("lifetime").map(|l| l.remaining), Some(2.0));
    let mut data = jengine_core::data::DataRegistry::new();
    assert_eq!(content.add_data(&mut data), 1);
    assert_eq!(data.get_f64("data/pack/notes", "low", "hz"), Some(220.0));
    assert!(packs::native::arrived().is_empty());

    // The next start of the same build finds it cached.
    packs::native::init(&game.join("assets"), &PackSettings::default(), vec![], None);
    assert_eq!(packs::status("chimes").name(), "installed");
    assert_eq!(packs::native::arrived().len(), 1, "a pack restored at startup is news too");
    let _ = std::fs::remove_dir_all(&dir);
}
