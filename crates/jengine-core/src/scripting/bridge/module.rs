//! Shared types for script-side module loading (`require` / `import`).
//!
//! Phase A of the script-reuse plan adds a `require()` global to every adapter
//! so scripts can pull in shared library code from other files. The resolution
//! step (path → source string) is intentionally adapter-agnostic and host-
//! supplied: the engine wires up a real filesystem resolver that walks
//! `assets/scripts/...`; tests inject a fake closure that returns canned
//! sources.
//!
//! Each adapter owns its own per-VM module cache because the cached export
//! value is language-specific (a Lua table on Luau, a `Persistent<Value>` on
//! the JS host) and cannot cross VM boundaries.
//!
//! ## Path normalisation
//! Resolvers receive the *unmodified* string the script passed to `require`.
//! Cache keys are the same string. Two scripts that spell the path differently
//! (`"lib/foo"` vs `"./lib/foo"`) get separate cache entries — callers that
//! want canonicalisation should normalise inside the resolver before reading
//! from disk.

use std::rc::Rc;

/// Closure type that resolves `require("path")` into raw script source.
///
/// `Ok(source)` indicates success; the adapter then compiles and executes it.
/// `Err(message)` raises a runtime error in the calling script (Lua: as a
/// `LuaError::RuntimeError`; JS: as a thrown `Error`).
///
/// Resolvers run synchronously inside the adapter callback. They must not
/// re-enter the script VM. Filesystem I/O is fine.
pub type ModuleResolver = Rc<dyn Fn(&str) -> Result<String, String>>;

/// Default resolver — always errors. Installed by adapters at construction so
/// `require()` from a script that the host hasn't wired a real resolver into
/// produces a clear diagnostic instead of a panic.
pub fn null_resolver() -> ModuleResolver {
    Rc::new(|path: &str| Err(format!("require('{path}'): no module resolver configured for this adapter")))
}

/// Filesystem-backed resolver rooted at `root` (typically `assets/scripts/`).
///
/// Resolution: `require("lib/util")` reads the first existing of
/// `<root>/lib/util.lua`, `<root>/lib/util.luau`, `<root>/lib/util.js`. If the
/// path already includes one of these extensions it is honoured verbatim.
/// Returns `Err` with the resolved path on miss / read error so the calling
/// script gets an actionable diagnostic.
///
/// Path safety: `..` segments are rejected to prevent scripts escaping the
/// asset root via `require("../../../etc/passwd")`-style traversal.
///
/// A module not under `root` is looked for in the mounted asset packs
/// (`asset::packs`), under `root`'s own folder name - `scripts/lib/util.lua`
/// for a root of `assets/scripts` - since a pack keys its files by their
/// path under `assets/`.
#[cfg(not(target_arch = "wasm32"))]
pub fn fs_resolver(root: std::path::PathBuf) -> ModuleResolver {
    let folder = root.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    Rc::new(move |path: &str| {
        if path.split(['/', '\\']).any(|seg| seg == "..") {
            return Err(format!(
                "require('{path}'): '..' segments are not permitted"
            ));
        }
        let has_ext = path.ends_with(".lua")
            || path.ends_with(".luau")
            || path.ends_with(".js");
        let names: Vec<String> = if has_ext {
            vec![path.to_string()]
        } else {
            ["lua", "luau", "js"].iter().map(|ext| format!("{path}.{ext}")).collect()
        };
        for name in &names {
            let candidate = root.join(name);
            if candidate.is_file() {
                return std::fs::read_to_string(&candidate)
                    .map_err(|e| format!("require('{path}'): read failed: {e}"));
            }
        }
        for name in &names {
            if let Some(candidate) = crate::asset::packs::locate(&format!("{folder}/{name}"))
                && candidate.is_file()
            {
                return std::fs::read_to_string(&candidate)
                    .map_err(|e| format!("require('{path}'): read failed: {e}"));
            }
        }
        Err(format!(
            "require('{path}'): not found under '{}'",
            root.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn null_resolver_errors_with_path() {
        let r = null_resolver();
        let err = r("lib/foo").unwrap_err();
        assert!(err.contains("lib/foo"), "got: {err}");
    }

    #[test]
    fn map_resolver_returns_canned_source() {
        let mut map: HashMap<String, String> = HashMap::new();
        map.insert("lib/foo".into(), "return 42".into());
        let r: ModuleResolver = Rc::new(move |path| {
            map.get(path)
                .cloned()
                .ok_or_else(|| format!("not found: {path}"))
        });
        assert_eq!(r("lib/foo").unwrap(), "return 42");
        assert!(r("lib/bar").is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn fs_resolver_finds_lua_file_with_implicit_extension() {
        let dir = tempdir_unique("fs_resolver_lua");
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::write(dir.join("lib/util.lua"), "return { x = 1 }").unwrap();
        let r = super::fs_resolver(dir.clone());
        assert_eq!(r("lib/util").unwrap(), "return { x = 1 }");
        assert!(r("lib/missing").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn fs_resolver_rejects_parent_traversal() {
        let dir = tempdir_unique("fs_resolver_traversal");
        let r = super::fs_resolver(dir.clone());
        let err = r("../etc/passwd").unwrap_err();
        assert!(err.contains(".."), "got: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn fs_resolver_prefers_lua_then_luau_then_js() {
        let dir = tempdir_unique("fs_resolver_pref");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.luau"), "return 'luau'").unwrap();
        std::fs::write(dir.join("a.js"), "return 'js'").unwrap();
        let r = super::fs_resolver(dir.clone());
        // No .lua present → falls through to .luau (preferred over .js).
        assert_eq!(r("a").unwrap(), "return 'luau'");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn tempdir_unique(tag: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("jengine_{tag}_{nanos}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
