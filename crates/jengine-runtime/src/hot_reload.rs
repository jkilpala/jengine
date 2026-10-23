//! P1.5 — Live asset hot-reload for the native runtime.
//!
//! Sits in front of `jengine_core::asset::watcher::AssetWatcher`, classifies
//! the debounced filesystem change events by file extension, and resolves the
//! changed absolute path back into the engine-relative *asset id* the runtime
//! caches use (e.g. `"scripts/player.lua"` or `"player.png"`).
//!
//! Opt-in via the `JENGINE_HOT_RELOAD` env var (any non-empty value enables
//! it). When disabled, `WgpuSceneState::hot_reload` stays `None` and the
//! per-frame `drain_hot_reload` call is a single field check.
//!
//! Scope of this first wedge:
//! - **`.scene`** — if the file matches the currently-loaded scene, queue a
//!   reload through the existing `reload_scene` path.
//! - **`.lua` / `.js`** — forget the cached compiled handle so every entity
//!   whose `ScriptComponent.source_path` matches re-attaches next frame
//!   with the new source.
//! - **`.png` / `.jpg` / `.jpeg`** — drop the raw RGBA + uploaded wgpu
//!   texture caches; the next sprite draw re-decodes + re-uploads.
//! - Anything else (`.material`, `.shader`, `.atlas`, `.tilemap`, fonts,
//!   audio) is logged and ignored — those reload paths are larger and land
//!   in a follow-up wedge.

use std::path::{Path, PathBuf};

use jengine_core::asset::watcher::AssetWatcher;

/// What kind of change `classify` decided a path represents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotReloadKind {
    /// `.scene` file. The runtime should compare `asset_id` against the
    /// currently-loaded scene's relative path before reloading.
    Scene,
    /// `.lua` / `.luau` script.
    LuaScript,
    /// `.js` script.
    JsScript,
    /// `.png` / `.jpg` / `.jpeg` texture.
    Texture,
    /// Recognised engine asset but not yet wired into the hot-reload loop
    /// (`.material`, `.shader`, `.atlas`, `.tilemap`, `.sortlayers`, audio,
    /// fonts). Logged + skipped.
    Unsupported(&'static str),
    /// Not an engine asset — caller should ignore silently.
    Irrelevant,
}

/// Classified change event ready for the runtime to dispatch.
#[derive(Debug, Clone)]
pub struct HotReloadChange {
    /// The absolute path the watcher reported.
    pub absolute: PathBuf,
    /// Engine-relative id used by the runtime caches (e.g. `"scripts/foo.lua"`
    /// or `"player.png"`). Always normalised to forward slashes. Empty when
    /// the path couldn't be located under any candidate root.
    pub asset_id: String,
    /// Kind of asset the extension says this is.
    pub kind: HotReloadKind,
}

/// Optional file-watcher attached to the active scene's candidate roots.
/// Built when `JENGINE_HOT_RELOAD` is set; otherwise the runtime keeps a
/// plain `None` here and the per-frame drain is a no-op.
pub struct HotReload {
    watcher: AssetWatcher,
    /// Roots used to relativise absolute paths back into asset ids. Captured
    /// at construction; `rebind_roots` swaps this out after a scene change so
    /// stripping stays correct for the new scene's layout.
    roots: Vec<PathBuf>,
}

impl HotReload {
    /// `roots` are the directories (in priority order) to try when
    /// stripping an absolute filesystem path down to an asset id —
    /// typically the same list `candidate_roots` produces for the active
    /// scene. The first root must also be the one to recursively watch on
    /// disk; conventionally the project's `assets/` dir or the project root.
    pub fn new(watch_root: &Path, roots: Vec<PathBuf>) -> anyhow::Result<Self> {
        Ok(Self {
            watcher: AssetWatcher::new(watch_root)
                .map_err(|e| anyhow::anyhow!("AssetWatcher::new failed: {e:?}"))?,
            roots,
        })
    }

    /// Replace the strip-roots used to convert absolute paths into asset ids
    /// (call after a scene load/push/pop swaps the active scene path).
    pub fn rebind_roots(&mut self, roots: Vec<PathBuf>) {
        self.roots = roots;
    }

    /// Drain pending filesystem changes and classify each one. Returns an
    /// empty vec on quiet frames (the common case).
    pub fn drain(&self) -> Vec<HotReloadChange> {
        let raw = self.watcher.drain_changed();
        raw.into_iter()
            .map(|abs| classify(&abs, &self.roots))
            .collect()
    }
}

/// Pure classifier — no I/O. Picks the *shortest* relative path across the
/// supplied roots (i.e. the most-specific root that contains the file) so
/// asset ids match what the runtime caches use.
pub fn classify(abs: &Path, roots: &[PathBuf]) -> HotReloadChange {
    let mut best: Option<String> = None;
    for root in roots {
        if let Ok(rel) = abs.strip_prefix(root) {
            let s = normalise_id(rel);
            if best.as_ref().is_none_or(|b| s.len() < b.len()) {
                best = Some(s);
            }
        }
    }
    let asset_id = best.unwrap_or_default();
    let kind = kind_for(abs);
    HotReloadChange {
        absolute: abs.to_path_buf(),
        asset_id,
        kind,
    }
}

fn normalise_id(rel: &Path) -> String {
    // Force forward slashes so the id round-trips on Windows where
    // `strip_prefix` returns `scripts\foo.lua`.
    let mut s = String::with_capacity(rel.as_os_str().len());
    for (i, comp) in rel.components().enumerate() {
        if i > 0 {
            s.push('/');
        }
        s.push_str(&comp.as_os_str().to_string_lossy());
    }
    s
}

fn kind_for(path: &Path) -> HotReloadKind {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());
    match ext.as_deref() {
        Some("scene") => HotReloadKind::Scene,
        Some("lua") | Some("luau") => HotReloadKind::LuaScript,
        Some("js") => HotReloadKind::JsScript,
        Some("png") | Some("jpg") | Some("jpeg") => HotReloadKind::Texture,
        Some("material") => HotReloadKind::Unsupported(".material"),
        Some("shader") => HotReloadKind::Unsupported(".shader"),
        Some("atlas") => HotReloadKind::Unsupported(".atlas"),
        Some("tilemap") => HotReloadKind::Unsupported(".tilemap"),
        Some("sortlayers") => HotReloadKind::Unsupported(".sortlayers"),
        Some("wav") | Some("ogg") | Some("mp3") => HotReloadKind::Unsupported("audio"),
        _ => HotReloadKind::Irrelevant,
    }
}

/// True iff `JENGINE_HOT_RELOAD` is set to a non-empty, non-`"0"` value.
/// `"1"`, `"true"`, `"on"` all work — anything truthy.
pub fn is_enabled_via_env() -> bool {
    match std::env::var("JENGINE_HOT_RELOAD") {
        Ok(v) => {
            let v = v.trim();
            !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false") && !v.eq_ignore_ascii_case("off")
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(p: &str) -> PathBuf {
        PathBuf::from(p)
    }

    #[test]
    fn classify_lua_script_relative_to_assets_root() {
        let abs = root("/proj/assets/scripts/player.lua");
        let roots = vec![root("/proj/assets"), root("/proj")];
        let c = classify(&abs, &roots);
        assert_eq!(c.kind, HotReloadKind::LuaScript);
        // Should pick the most-specific (shortest rel) root.
        assert_eq!(c.asset_id, "scripts/player.lua");
    }

    #[test]
    fn classify_texture_in_project_root_when_no_assets_dir() {
        let abs = root("/proj/player.png");
        let roots = vec![root("/proj/assets"), root("/proj")];
        let c = classify(&abs, &roots);
        assert_eq!(c.kind, HotReloadKind::Texture);
        assert_eq!(c.asset_id, "player.png");
    }

    #[test]
    fn classify_scene_file() {
        let abs = root("/proj/assets/scenes/level1.scene");
        let roots = vec![root("/proj/assets")];
        let c = classify(&abs, &roots);
        assert_eq!(c.kind, HotReloadKind::Scene);
        assert_eq!(c.asset_id, "scenes/level1.scene");
    }

    #[test]
    fn classify_material_is_unsupported_placeholder() {
        let abs = root("/proj/assets/m/foo.material");
        let roots = vec![root("/proj/assets")];
        let c = classify(&abs, &roots);
        assert_eq!(c.kind, HotReloadKind::Unsupported(".material"));
    }

    #[test]
    fn classify_unknown_extension_is_irrelevant() {
        let abs = root("/proj/assets/README.md");
        let roots = vec![root("/proj/assets")];
        let c = classify(&abs, &roots);
        assert_eq!(c.kind, HotReloadKind::Irrelevant);
    }

    #[test]
    fn classify_outside_all_roots_leaves_empty_id() {
        let abs = root("/elsewhere/foo.lua");
        let roots = vec![root("/proj/assets")];
        let c = classify(&abs, &roots);
        assert_eq!(c.kind, HotReloadKind::LuaScript);
        assert!(c.asset_id.is_empty());
    }

    #[test]
    fn env_toggle_truthy_and_falsy() {
        // Save existing value so we don't leak between tests in the same proc.
        let prev = std::env::var("JENGINE_HOT_RELOAD").ok();
        // Safety: tests in this module run single-threaded relative to each
        // other because they all touch the same global env var; cargo
        // serialises within a `#[test]` and we restore on exit.
        unsafe { std::env::set_var("JENGINE_HOT_RELOAD", ""); }
        assert!(!is_enabled_via_env());
        unsafe { std::env::set_var("JENGINE_HOT_RELOAD", "0"); }
        assert!(!is_enabled_via_env());
        unsafe { std::env::set_var("JENGINE_HOT_RELOAD", "false"); }
        assert!(!is_enabled_via_env());
        unsafe { std::env::set_var("JENGINE_HOT_RELOAD", "1"); }
        assert!(is_enabled_via_env());
        unsafe { std::env::set_var("JENGINE_HOT_RELOAD", "true"); }
        assert!(is_enabled_via_env());
        match prev {
            Some(v) => unsafe { std::env::set_var("JENGINE_HOT_RELOAD", v) },
            None => unsafe { std::env::remove_var("JENGINE_HOT_RELOAD") },
        }
    }
}
