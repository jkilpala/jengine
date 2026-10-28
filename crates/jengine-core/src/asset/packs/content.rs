//! What a mounted pack brought that a runtime keeps in memory: the maps a
//! host fills once at startup - prefabs, data tables, skeletal clips and
//! rigs, and in the browser the scripts `require` can reach - which a pack
//! mounted later has to be added to, because nothing reads those files
//! again. A host calls [`take_arrived`] once a frame with its way of reading
//! a mounted file and adds each [`PackContent`] to its maps with the
//! methods here, so the rules for each kind of file are written once.
//!
//! What a removed pack added stays in memory until the game restarts: a
//! prefab already known can still be spawned (its textures are gone with
//! the pack), and a data table can still be read.

use std::collections::HashMap;

use super::PackEntry;
use crate::asset::AssetManager;
use crate::data::DataRegistry;
use crate::scene::format::EntityData;

/// A file's asset path and its text.
pub type TextFile = (String, String);

/// The files of one mounted pack that a runtime keeps in memory, by kind.
#[derive(Debug, Clone, Default)]
pub struct PackContent {
    pub name: String,
    /// `.lua` / `.luau` / `.js`.
    pub scripts: Vec<TextFile>,
    pub prefabs: Vec<TextFile>,
    /// `.dataschema`.
    pub schemas: Vec<TextFile>,
    /// `.data`.
    pub tables: Vec<TextFile>,
    /// `.skelanim`.
    pub clips: Vec<TextFile>,
    pub rigs: Vec<TextFile>,
}

/// Every built pack mounted since the last call, with the files of it that
/// a runtime keeps in memory read through `read` (an asset path to its
/// bytes - a mounted file on disk natively, the archive's slice on the web).
/// A file that cannot be read or is not UTF-8 is logged and left out.
pub fn take_arrived(read: impl Fn(&str) -> Option<Vec<u8>>) -> Vec<PackContent> {
    super::take_arrivals().iter().map(|e| PackContent::read(e, &read)).collect()
}

impl PackContent {
    /// `entry`'s in-memory files, through `read`.
    pub fn read(entry: &PackEntry, read: impl Fn(&str) -> Option<Vec<u8>>) -> Self {
        let mut out = PackContent { name: entry.name.clone(), ..Default::default() };
        for f in &entry.files {
            let rel = super::normalize(f);
            let ext = std::path::Path::new(&rel).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            let list = match ext.as_str() {
                "lua" | "luau" | "js" => &mut out.scripts,
                "prefab" => &mut out.prefabs,
                "dataschema" => &mut out.schemas,
                "data" => &mut out.tables,
                "skelanim" => &mut out.clips,
                "rig" => &mut out.rigs,
                _ => continue,
            };
            match read(&rel).map(String::from_utf8) {
                Some(Ok(text)) => list.push((rel, text)),
                Some(Err(_)) => log::warn!("[packs] '{}': {rel} is not UTF-8 text", entry.name),
                None => log::warn!("[packs] '{}': {rel} could not be read", entry.name),
            }
        }
        out
    }

    /// Nothing here for a runtime to keep.
    pub fn is_empty(&self) -> bool {
        self.scripts.is_empty()
            && self.prefabs.is_empty()
            && self.schemas.is_empty()
            && self.tables.is_empty()
            && self.clips.is_empty()
            && self.rigs.is_empty()
    }

    /// Add the pack's prefabs to a resolved prefab map
    /// ([`crate::scene::add_prefabs`]: a variant may extend a prefab the game
    /// already had). Returns the names added.
    pub fn add_prefabs(&self, map: &mut HashMap<String, EntityData>) -> Vec<String> {
        let mut raw = HashMap::new();
        for (rel, text) in &self.prefabs {
            match crate::scene::parse_prefab(rel, text) {
                Ok((key, data)) => {
                    raw.insert(key, data);
                }
                Err(e) => log::warn!("[packs] '{}': prefab {rel}: {e}", self.name),
            }
        }
        crate::scene::add_prefabs(map, &raw);
        let mut names: Vec<String> = raw.into_keys().collect();
        names.sort();
        names
    }

    /// Add the pack's schemas and tables to `registry`, schemas first, each
    /// table under its id ([`DataRegistry::load_from_assets_dir`]'s rule).
    /// Returns how many tables were added.
    pub fn add_data(&self, registry: &mut DataRegistry) -> usize {
        for (rel, text) in &self.schemas {
            if let Err(e) = registry.load_schema_json(text) {
                log::warn!("[packs] '{}': data schema {rel}: {e}", self.name);
            }
        }
        let mut n = 0;
        for (rel, text) in &self.tables {
            match registry.load_table_json(rel, text) {
                Ok(()) => n += 1,
                Err(e) => log::warn!("[packs] '{}': data table {rel}: {e}", self.name),
            }
        }
        n
    }

    /// The pack's tables as the browser's script namespace holds them: the
    /// table id (path without `.data`) to `{ "rows": [...] }`.
    pub fn table_rows(&self) -> Vec<(String, serde_json::Value)> {
        let mut out = Vec::new();
        for (rel, text) in &self.tables {
            match serde_json::from_str::<serde_json::Value>(text) {
                Ok(v) => {
                    let rows = v.get("rows").cloned().unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
                    out.push((rel.trim_end_matches(".data").to_string(), serde_json::json!({ "rows": rows })));
                }
                Err(e) => log::warn!("[packs] '{}': data table {rel}: {e}", self.name),
            }
        }
        out
    }

    /// Add the pack's skeletal clips and rigs to `assets`, each under its
    /// asset path - the id every runtime gives one. Returns how many.
    pub fn add_skeletal(&self, assets: &mut AssetManager) -> usize {
        let mut n = 0;
        for (rel, text) in &self.clips {
            match serde_json::from_str::<crate::animation::skeletal::SkeletalClip>(text) {
                Ok(c) => {
                    assets.insert_skeletal_clip(rel.clone(), c);
                    n += 1;
                }
                Err(e) => log::warn!("[packs] '{}': skeletal clip {rel}: {e}", self.name),
            }
        }
        for (rel, text) in &self.rigs {
            match serde_json::from_str::<crate::animation::rig::SkeletonRig>(text) {
                Ok(r) => {
                    assets.insert_skeleton_rig(rel.clone(), r);
                    n += 1;
                }
                Err(e) => log::warn!("[packs] '{}': skeleton rig {rel}: {e}", self.name),
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(files: &[&str]) -> PackEntry {
        PackEntry { name: "vault".into(), files: files.iter().map(|f| f.to_string()).collect(), ..Default::default() }
    }

    #[test]
    fn a_pack_is_sorted_into_what_runtimes_keep() {
        let files: HashMap<&str, &str> = [
            ("scripts/vault/door.lua", "-- door"),
            ("prefabs/vault/gem.prefab", r#"{ "name": "Gem", "components": {} }"#),
            ("data/vault/loot.data", r#"{ "version": 1, "schema": "", "rows": [{ "id": "a", "gold": 5 }] }"#),
            ("sprites/vault/gem.png", "not kept"),
        ]
        .into();
        let c = PackContent::read(
            &entry(&["scripts/vault/door.lua", "prefabs/vault/gem.prefab", "data/vault/loot.data", "sprites/vault/gem.png", "scenes/missing.prefab"]),
            |rel| files.get(rel).map(|t| t.as_bytes().to_vec()),
        );
        assert_eq!(c.scripts, vec![("scripts/vault/door.lua".to_string(), "-- door".to_string())]);
        assert_eq!(c.prefabs.len(), 1, "the unreadable one is left out");
        assert_eq!(c.tables.len(), 1);
        assert!(c.clips.is_empty() && c.rigs.is_empty() && c.schemas.is_empty());
        assert!(!c.is_empty());

        let mut reg = DataRegistry::new();
        assert_eq!(c.add_data(&mut reg), 1);
        assert_eq!(reg.get_f64("data/vault/loot", "a", "gold"), Some(5.0), "keyed as a table in the game is");
        let rows = c.table_rows();
        assert_eq!(rows[0].0, "data/vault/loot");
        assert_eq!(rows[0].1["rows"][0]["id"], "a");
    }

    #[test]
    fn a_pack_variant_extends_a_prefab_the_game_had() {
        let base: HashMap<String, EntityData> = [(
            "Slime".to_string(),
            serde_json::from_str::<EntityData>(
                r#"{ "name": "Slime", "components": { "lifetime": { "remaining": 3.0 } },
                     "children": [{ "name": "Eye", "components": {} }] }"#,
            )
            .unwrap(),
        )]
        .into();
        let mut map = crate::scene::resolve_prefab_variants(&base);
        let c = PackContent {
            name: "fire".into(),
            prefabs: vec![(
                "prefabs/fire_slime.prefab".into(),
                r#"{ "name": "", "components": { "prefab_instance": { "prefab_id": "Slime", "overrides": {} } } }"#.into(),
            )],
            ..Default::default()
        };
        assert_eq!(c.add_prefabs(&mut map), vec!["fire_slime".to_string()], "named by its file when its name is empty");
        let v = &map["fire_slime"];
        assert_eq!(v.components.get::<crate::ecs::components::Lifetime>("lifetime").map(|l| l.remaining), Some(3.0), "the base's component");
        assert_eq!(v.children.len(), 1, "the base's part, once");
        assert_eq!(map["Slime"].children.len(), 1, "the game's own prefab is untouched");

        // Adding again (a pack restored, then downloaded anew) gives the same.
        c.add_prefabs(&mut map);
        assert_eq!(map["fire_slime"].children.len(), 1);
    }
}
