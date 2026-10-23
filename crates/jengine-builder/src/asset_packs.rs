//! Moving a staged game's asset packs out of it (`settings.asset_packs`,
//! `jengine_core::asset::packs`).
//!
//! After a build has laid out its `assets/` and run the asset pipeline over
//! them, [`split`] takes every file a pack claims out of that folder and
//! writes each pack as one `.jpak` archive into the build's packs folder,
//! named for its hash - then `assets/packs.json` says what went where. That
//! folder is what to upload to `settings.asset_packs.url`; it is not needed
//! beside the game (on Windows, Linux and the web it sits there anyway, so a
//! fresh build downloads from itself).
//!
//! Two rules keep a split game working. Input maps, read before the game
//! starts, stay in the base game ([`stays_in_base`]), and so does the entry
//! scene - a pack cannot be there before the game has started. Scripts,
//! prefabs and data tables can go: each runtime adds a pack's to what it
//! loaded at startup when the pack mounts (`asset::packs::content`). And sprites
//! a pack claims are left out of atlas packing ([`atlas_excludes`]), or the
//! base game's atlas would carry their pixels after all.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use jengine_core::asset::packs::{self, PackEntry, PackManifest, PackSettings, archive, stays_in_base};

/// `settings.asset_packs` from a project's `project.jengine`.
pub fn read_settings(project_path: &Path) -> PackSettings {
    std::fs::read_to_string(project_path.join("project.jengine"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("settings").map(PackSettings::from_settings))
        .unwrap_or_default()
}

/// The atlas packer's exclude patterns (relative to `sprites/`) for the
/// sprites a pack claims.
pub fn atlas_excludes(settings: &PackSettings) -> Vec<String> {
    settings
        .packs
        .iter()
        .flat_map(|p| p.include.iter())
        .filter_map(|inc| packs::normalize(inc).strip_prefix("sprites/").map(str::to_string))
        .filter(|s| !s.is_empty())
        .collect()
}

/// What [`split`] did.
#[derive(Debug, Default)]
pub struct SplitReport {
    pub manifest: Option<PackManifest>,
    /// Files a pack claims that stayed in the base game, and why.
    pub kept: Vec<(String, &'static str)>,
}

/// Move every pack's files out of `assets` into an archive in `packs_out`,
/// and write `assets/packs.json`. `entry_scene` (relative to `assets/`)
/// stays. Nothing to do without packs.
pub fn split(assets: &Path, packs_out: &Path, settings: &PackSettings, entry_scene: Option<&str>) -> anyhow::Result<SplitReport> {
    let mut report = SplitReport::default();
    if settings.packs.is_empty() {
        return Ok(report);
    }
    let mut seen = std::collections::HashSet::new();
    for p in &settings.packs {
        if !packs::valid_name(&p.name) {
            bail!("asset pack name '{}': use lowercase letters, digits, '-' and '_'", p.name);
        }
        if !seen.insert(p.name.as_str()) {
            bail!("two asset packs are named '{}'", p.name);
        }
    }
    let entry = entry_scene.map(packs::normalize);

    let mut files = Vec::new();
    collect(assets, assets, &mut files)?;
    files.sort();
    let mut claimed: Vec<Vec<String>> = vec![Vec::new(); settings.packs.len()];
    for rel in &files {
        let owners: Vec<usize> = (0..settings.packs.len()).filter(|&i| settings.packs[i].claims(rel)).collect();
        match owners.as_slice() {
            [] => {}
            [i] => {
                if let Some(why) = stays_in_base(rel) {
                    report.kept.push((rel.clone(), why));
                } else if entry.as_deref() == Some(rel.as_str()) {
                    report.kept.push((rel.clone(), "the game starts with it"));
                } else {
                    claimed[*i].push(rel.clone());
                }
            }
            many => bail!(
                "'{rel}' is claimed by more than one asset pack ({}) - a file can be in one",
                many.iter().map(|&i| settings.packs[i].name.as_str()).collect::<Vec<_>>().join(", ")
            ),
        }
    }
    for (rel, why) in &report.kept {
        log::info!("Asset packs: '{rel}' stays in the game ({why})");
    }

    std::fs::create_dir_all(packs_out).with_context(|| format!("creating {}", packs_out.display()))?;
    let mut manifest = PackManifest { version: 1, url: settings.url.clone(), packs: Vec::new() };
    for (def, rels) in settings.packs.iter().zip(&claimed) {
        remove_old_archives(packs_out, &def.name)?;
        if rels.is_empty() {
            log::warn!("Asset pack '{}' takes no files - check its include paths", def.name);
            continue;
        }
        let entry = write_pack(assets, packs_out, &def.name, rels)?;
        crate::progress::step(&format!(
            "Asset pack '{}': {} files, {:.1} MB -> {}",
            def.name,
            rels.len(),
            entry.size as f64 / 1_048_576.0,
            entry.file
        ));
        for rel in rels {
            std::fs::remove_file(assets.join(rel)).with_context(|| format!("moving {rel} out of the game"))?;
        }
        manifest.packs.push(entry);
    }
    remove_empty_dirs(assets);
    std::fs::write(assets.join(packs::MANIFEST_FILE), serde_json::to_string_pretty(&manifest)?)?;
    if !manifest.packs.is_empty() {
        let url = if settings.url.is_empty() { packs::DEFAULT_URL } else { settings.url.as_str() };
        log::info!("Asset packs written to {} - serve them at {url}", packs_out.display());
    }
    report.manifest = Some(manifest);
    Ok(report)
}

/// Write `rels` (under `assets`) as one archive in `packs_out`, named for its
/// hash.
fn write_pack(assets: &Path, packs_out: &Path, name: &str, rels: &[String]) -> anyhow::Result<PackEntry> {
    use sha2::Digest;
    let sizes: Vec<(String, u64)> = rels
        .iter()
        .map(|r| Ok((r.clone(), std::fs::metadata(assets.join(r))?.len())))
        .collect::<std::io::Result<_>>()?;
    let tmp = packs_out.join(format!("{name}.jpak.tmp"));
    let mut out = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
    let mut hasher = sha2::Sha256::new();
    let mut size = 0u64;
    let head = archive::header(&archive::index_for(&sizes));
    hasher.update(&head);
    out.write_all(&head)?;
    size += head.len() as u64;
    for rel in rels {
        let bytes = std::fs::read(assets.join(rel))?;
        hasher.update(&bytes);
        out.write_all(&bytes)?;
        size += bytes.len() as u64;
    }
    out.flush()?;
    drop(out);
    let sha256: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    let file = format!("{name}-{}.jpak", &sha256[..12]);
    std::fs::rename(&tmp, packs_out.join(&file))?;
    Ok(PackEntry { name: name.to_string(), file, size, sha256, files: rels.to_vec() })
}

/// A pack's archives from an earlier build, which no manifest will name.
fn remove_old_archives(packs_out: &Path, name: &str) -> anyhow::Result<()> {
    let Ok(entries) = std::fs::read_dir(packs_out) else { return Ok(()) };
    for e in entries.flatten() {
        let file = e.file_name().to_string_lossy().into_owned();
        let hash = file.strip_prefix(&format!("{name}-")).and_then(|r| r.strip_suffix(".jpak"));
        if hash.is_some_and(|h| h.len() == 12 && h.bytes().all(|b| b.is_ascii_hexdigit())) {
            std::fs::remove_file(e.path())?;
        }
    }
    Ok(())
}

fn collect(base: &Path, dir: &Path, out: &mut Vec<String>) -> anyhow::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let path = e?.path();
        if path.is_dir() {
            collect(base, &path, out)?;
        } else if let Ok(rel) = path.strip_prefix(base) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// Folders the move left empty.
fn remove_empty_dirs(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    let mut empty = true;
    for e in entries.flatten() {
        let path = e.path();
        if !(path.is_dir() && remove_empty_dirs(&path) && std::fs::remove_dir(&path).is_ok()) {
            empty = false;
        }
    }
    empty
}

/// `settings.entry_scene` (or the top-level `default_scene`), relative to
/// `assets/`, for [`split`].
pub fn entry_scene(project_path: &Path) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(project_path.join("project.jengine")).ok()?).ok()?;
    v.get("settings")
        .and_then(|s| s.get("entry_scene"))
        .or_else(|| v.get("default_scene"))
        .and_then(|s| s.as_str())
        .map(packs::normalize)
}

/// Where a build whose game sits inside an app bundle (macOS, iOS, Android)
/// writes its packs: beside the bundle, since a download cannot come from
/// inside it. Such a build needs a `url`, which this says once.
pub fn packs_beside_bundle(output_path: &Path, platform: &str, settings: &PackSettings) -> PathBuf {
    if !settings.packs.is_empty() && settings.url.is_empty() {
        log::warn!(
            "settings.asset_packs.url is empty: a {platform} build cannot download its packs from inside \
             its own app - set a url and upload {}",
            output_path.join("packs").display()
        );
    }
    output_path.join("packs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use jengine_core::asset::packs::PackDef;

    fn tmp(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jengine-split-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn forest() -> PackSettings {
        PackSettings {
            url: "https://cdn.example.com/g/".into(),
            packs: vec![PackDef { name: "forest".into(), include: vec!["sprites/forest/".into(), "scenes/forest/".into(), "music/forest.ogg".into()] }],
        }
    }

    #[test]
    fn a_pack_leaves_the_game_as_one_archive() {
        let dir = tmp("split");
        let assets = dir.join("game/assets");
        write(&assets.join("sprites/forest/tree.png"), &[1; 50]);
        write(&assets.join("scenes/forest/glade.scene"), b"{}");
        write(&assets.join("scenes/forest/glade.lua"), b"-- script");
        write(&assets.join("music/forest.ogg"), &[2; 70]);
        write(&assets.join("sprites/hero.png"), &[3; 10]);
        write(&assets.join("scenes/forest/controls.input"), b"{}");
        let out = dir.join("game/packs");
        let report = split(&assets, &out, &forest(), Some("scenes/main.scene")).unwrap();
        let manifest = report.manifest.unwrap();
        let entry = &manifest.packs[0];
        assert_eq!(
            entry.files,
            vec!["music/forest.ogg", "scenes/forest/glade.lua", "scenes/forest/glade.scene", "sprites/forest/tree.png"],
            "a script goes with its pack"
        );
        assert!(!assets.join("sprites/forest").exists(), "moved out, and its folder with it");
        assert!(assets.join("sprites/hero.png").is_file(), "the rest stays");
        assert!(assets.join("scenes/forest/controls.input").is_file(), "an input map stays in the game");
        assert_eq!(report.kept.len(), 1);
        let archive_path = out.join(&entry.file);
        let bytes = std::fs::read(&archive_path).unwrap();
        assert_eq!(bytes.len() as u64, entry.size);
        assert_eq!(packs::sha256_hex(&bytes), entry.sha256);
        assert!(entry.file.starts_with("forest-") && entry.file.ends_with(".jpak"));
        let written: PackManifest = serde_json::from_str(&std::fs::read_to_string(assets.join("packs.json")).unwrap()).unwrap();
        assert_eq!(written, manifest);
        assert_eq!(written.url, "https://cdn.example.com/g/");

        // And the engine installs what the builder wrote.
        let cache = dir.join("cache");
        let installed = jengine_core::asset::packs::native::install(entry, archive_path.to_str().unwrap(), &cache, &dir, &mut |_| {}).unwrap();
        assert_eq!(std::fs::read(installed.join("music/forest.ogg")).unwrap(), vec![2; 70]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rebuild_replaces_the_old_archive() {
        let dir = tmp("rebuild");
        let assets = dir.join("assets");
        let out = dir.join("packs");
        write(&assets.join("sprites/forest/tree.png"), &[1; 5]);
        let first = split(&assets, &out, &forest(), None).unwrap().manifest.unwrap();
        write(&assets.join("sprites/forest/tree.png"), &[9; 5]);
        let second = split(&assets, &out, &forest(), None).unwrap().manifest.unwrap();
        assert_ne!(first.packs[0].file, second.packs[0].file, "a changed pack is a new file");
        let left: Vec<_> = std::fs::read_dir(&out).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left.len(), 1, "only the new archive: {left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_entry_scene_stays_and_a_shared_file_is_refused() {
        let dir = tmp("refuse");
        let assets = dir.join("assets");
        write(&assets.join("scenes/forest/start.scene"), b"{}");
        let report = split(&assets, &dir.join("packs"), &forest(), Some("assets/scenes/forest/start.scene")).unwrap();
        assert!(assets.join("scenes/forest/start.scene").is_file());
        assert_eq!(report.kept[0].1, "the game starts with it");

        let mut two = forest();
        two.packs.push(PackDef { name: "music".into(), include: vec!["music/".into()] });
        write(&assets.join("music/forest.ogg"), &[0; 3]);
        let err = split(&assets, &dir.join("packs"), &two, None).unwrap_err();
        assert!(format!("{err}").contains("more than one"), "{err}");
        let mut bad = forest();
        bad.packs[0].name = "Forest Pack".into();
        assert!(split(&assets, &dir.join("packs"), &bad, None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pack_sprites_are_not_atlas_packed() {
        assert_eq!(atlas_excludes(&forest()), vec!["forest/"]);
    }
}
