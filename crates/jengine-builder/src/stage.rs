//! Laying out a game's files the way a native runtime reads them.
//!
//! A native build runs from a folder holding `project.jengine` and `assets/`:
//! beside the `.exe` on Windows, unpacked into the app's storage on Android.
//! Both builds lay that folder out here, so what the Android runtime unpacks
//! is exactly what a Windows build ships.

use std::path::Path;

/// Copy `project_path`'s game files into `dest` and run the asset pipeline
/// over the copy.
///
/// `debug_overlay` is stamped into the copied `project.jengine`, which is
/// where the runtime reads whether F3 / F12 work. The project's asset packs
/// (`settings.asset_packs`) are moved out of the copy into `packs_out`
/// ([`crate::asset_packs::split`]).
/// The engine's licence and the notices of everything it is built on, which
/// a shipped game carries: the MIT and Apache licences of the libraries
/// compiled into the runtime require their notices to go with every copy.
/// Embedded, so a build needs nothing beside the builder to find them.
const ENGINE_LICENSE: &str = include_str!("../../../LICENSE");
const THIRD_PARTY_NOTICES: &str = include_str!("../../../THIRD-PARTY-NOTICES.md");

/// Write `licenses/jengine.txt` and `licenses/third-party.md` into a build.
pub fn write_licenses(dest: &Path) -> anyhow::Result<()> {
    let dir = dest.join("licenses");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("jengine.txt"), ENGINE_LICENSE)?;
    std::fs::write(dir.join("third-party.md"), THIRD_PARTY_NOTICES)?;
    Ok(())
}

pub fn stage_game(project_path: &Path, dest: &Path, debug_overlay: bool, packs_out: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dest)?;
    write_licenses(dest)?;

    // 1. Assets.
    let source_assets = project_path.join("assets");
    let dest_assets = dest.join("assets");
    if source_assets.exists() {
        copy_dir_recursive(&source_assets, &dest_assets)?;
        crate::progress::step(&format!("Copied assets to {}", dest_assets.display()));
    }

    // 2. project.jengine, the settings the runtime reads.
    let project_file = project_path.join("project.jengine");
    if project_file.exists() {
        // Copied with the build's own `debug_overlay` decision stamped in,
        // since the runtime reads F3 / F12 availability out of this file.
        // A project file that will not parse is copied verbatim rather than
        // failing the build - the runtime treats a missing key as enabled.
        let raw = std::fs::read_to_string(&project_file)?;
        match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(mut doc) => {
                crate::set_debug_overlay(&mut doc, debug_overlay);
                std::fs::write(dest.join("project.jengine"), serde_json::to_string_pretty(&doc)?)?;
            }
            Err(e) => {
                log::warn!("project.jengine is not valid JSON ({e}); copying as-is");
                std::fs::copy(&project_file, dest.join("project.jengine"))?;
            }
        }
        if !debug_overlay {
            log::info!("Debug overlay disabled for this build (F3 / F12 inert)");
        }
        log::info!("Copied project.jengine");
    }

    // 3. Scenes carry the files they point at, so a shipped scene loads the
    //    same whatever runtime or path loads it.
    inline_scene_sources(&dest.join("assets"))?;

    // 4. Asset pipeline (atlas packing, definitions, compression). Always
    //    run - atlas definitions (.atlas) need processing even if packing is
    //    disabled.
    let mut pipeline_config = crate::asset_pipeline::read_pipeline_config(project_path);
    // A pack's sprites stay out of the atlases, or the game would carry their
    // pixels after all.
    let packs = crate::asset_packs::read_settings(project_path);
    pipeline_config.atlas.exclude_patterns.extend(crate::asset_packs::atlas_excludes(&packs));
    crate::progress::step("Running asset pipeline...");
    let pipeline_result = crate::asset_pipeline::run_asset_pipeline(dest, &pipeline_config)?;
    if pipeline_result.atlases_created > 0 {
        log::info!(
            "Asset pipeline: {} atlases ({} sprites packed)",
            pipeline_result.atlases_created,
            pipeline_result.sprites_packed
        );
    }
    // Remap scene references to use atlas textures
    if let Some(manifest) = &pipeline_result.manifest {
        crate::asset_pipeline::remap_all_references(&dest.join("assets"), manifest)?;
    }

    // 5. Asset packs: last, so each carries its files as the game would
    //    have shipped them.
    let entry = crate::asset_packs::entry_scene(project_path);
    crate::asset_packs::split(&dest.join("assets"), packs_out, &packs, entry.as_deref())?;
    Ok(())
}

/// Inline every `.anim`, `.particle` and `.tilemap` reference into the
/// scenes under `assets` (`jengine_core::scene::sources`), in place.
///
/// Runtimes are meant to resolve these as they load a scene, and not all of
/// them did: the native runner skipped them for every scene a script loaded
/// and the web runtime never resolved tilemaps or particles, so a platformer
/// shipped without its ground. A staged scene that already carries its data
/// cannot be loaded wrong. The references stay, so a runtime that does
/// resolve them just resolves them again.
pub fn inline_scene_sources(assets: &Path) -> anyhow::Result<usize> {
    if !assets.is_dir() {
        return Ok(0);
    }
    let mut scenes = Vec::new();
    collect_scenes(assets, &mut scenes)?;
    let candidates = [assets.to_path_buf()];
    let mut inlined = 0;
    for path in scenes {
        let text = std::fs::read_to_string(&path)?;
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue; // the runtime reports a broken scene better than this can
        };
        let (a, p, t) = jengine_core::scene::sources::count_sources(&value);
        if a + p + t == 0 {
            continue;
        }
        for e in jengine_core::scene::sources::inline_sources(&mut value, &candidates) {
            log::warn!("{}: {e:#}", path.display());
        }
        std::fs::write(&path, serde_json::to_string_pretty(&value)?)?;
        inlined += 1;
    }
    if inlined > 0 {
        log::info!("Inlined .anim / .particle / .tilemap files into {inlined} scene(s)");
    }
    Ok(inlined)
}

fn collect_scenes(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_scenes(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "scene") {
            out.push(path);
        }
    }
    Ok(())
}

/// A file kept under `assets/` to make game files from, which the game
/// never reads - a layered Photoshop document, which `jengine psd-import`
/// turns into sprites and a prefab. A build leaves them out; one is often
/// larger than everything made from it.
pub fn is_authoring_source(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("psd") || e.eq_ignore_ascii_case("psb"))
}

/// Recursively copy a directory tree, leaving authoring sources out
/// ([`is_authoring_source`]). Every build's asset copy goes through it.
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if !is_authoring_source(&src_path) {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_carries_the_licences() {
        let dir = tempfile::tempdir().unwrap();
        write_licenses(dir.path()).unwrap();
        let own = std::fs::read_to_string(dir.path().join("licenses/jengine.txt")).unwrap();
        assert!(own.starts_with("MIT License"), "{own}");
        let notices = std::fs::read_to_string(dir.path().join("licenses/third-party.md")).unwrap();
        for crate_name in ["wgpu", "rapier2d", "hecs", "mlua"] {
            assert!(notices.contains(crate_name), "the notices name no {crate_name}");
        }
    }

    #[test]
    fn a_build_leaves_photoshop_documents_behind() {
        let root = std::env::temp_dir().join(format!("jengine-stage-psd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/art")).unwrap();
        std::fs::write(root.join("src/art/hero.PSD"), b"8BPS").unwrap();
        std::fs::write(root.join("src/art/hero.png"), b"png").unwrap();
        copy_dir_recursive(&root.join("src"), &root.join("dst")).unwrap();
        assert!(root.join("dst/art/hero.png").exists());
        assert!(!root.join("dst/art/hero.PSD").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// That platformer's shape: the scene names a `.tilemap`, and carries nothing.
    #[test]
    fn a_staged_scene_carries_its_tilemap() {
        let dir = tempfile::tempdir().unwrap();
        let assets = dir.path().join("assets");
        std::fs::create_dir_all(assets.join("scenes")).unwrap();
        std::fs::create_dir_all(assets.join("tilemaps")).unwrap();
        std::fs::write(
            assets.join("tilemaps/peak.tilemap"),
            r#"{ "tile_size": [32.0, 32.0],
                 "tilesets": [{ "texture_id": "atlases/peak_tiles.png", "tile_width": 32,
                                "tile_height": 32, "columns": 6, "tile_count": 12,
                                "first_gid": 1 }],
                 "layers": [{ "name": "ground", "width": 2, "height": 1, "tiles": [3, 4] }] }"#,
        )
        .unwrap();
        std::fs::write(
            assets.join("scenes/peak.scene"),
            r#"{ "entities": [{ "name": "Level", "components": { "tilemap": {
                   "source": "tilemaps/peak.tilemap", "tile_size": [32.0, 32.0],
                   "tilesets": [], "layers": [] } } }] }"#,
        )
        .unwrap();
        // And a scene with nothing to inline is left byte-for-byte alone.
        std::fs::write(assets.join("scenes/menu.scene"), "{ \"entities\": [] }").unwrap();

        assert_eq!(inline_scene_sources(&assets).unwrap(), 1);
        let scene: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(assets.join("scenes/peak.scene")).unwrap()).unwrap();
        let tilemap = &scene["entities"][0]["components"]["tilemap"];
        assert_eq!(tilemap["tilesets"][0]["texture_id"], "atlases/peak_tiles.png");
        assert_eq!(tilemap["layers"][0]["tiles"], serde_json::json!([3, 4]));
        assert_eq!(std::fs::read_to_string(assets.join("scenes/menu.scene")).unwrap(), "{ \"entities\": [] }");
    }

    #[test]
    fn a_project_without_assets_stages_fine() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(inline_scene_sources(&dir.path().join("assets")).unwrap(), 0);
    }
}
