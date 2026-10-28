//! The engine has one version: the workspace `version` in `Cargo.toml`,
//! which the engine's own crates follow. A handful of files carry a copy of
//! it: the editor's `package.json` and `tauri.conf.json` (the installer's
//! version) and the website's `package.json`. After a release `master` moves
//! every one of them to the next version (CONTRIBUTING.md, "Versions"); this
//! fails if one was missed.
//!
//! A package has a version of its own (since 0.2.0): it moves when the
//! package changes, by what changed, and not with the engine. Its
//! `Cargo.toml` names it, and its `package.toml` - when it has one - must say
//! the same. Its `engine_version` is the *minimum* engine it needs, so it can
//! be older than the engine but never newer.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    let p = root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("reading {}: {e}", p.display()))
}

fn json_version(rel: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(&read(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
    v["version"].as_str().unwrap_or_else(|| panic!("{rel} has no version")).to_string()
}

#[test]
fn every_copy_of_the_engine_version_agrees() {
    let workspace: toml::Table = read("Cargo.toml").parse().expect("the workspace Cargo.toml");
    let version = workspace["workspace"]["package"]["version"].as_str().expect("[workspace.package] version").to_string();
    assert_eq!(version, jengine_core::version::ENGINE_VERSION, "jengine-core follows the workspace version");

    let mut wrong = Vec::new();
    for rel in ["editor/package.json", "editor/src-tauri/tauri.conf.json", "website/package.json"] {
        let found = json_version(rel);
        if found != version {
            wrong.push(format!("{rel}: {found}"));
        }
    }
    let mut packages = 0;
    for entry in std::fs::read_dir(root().join("packages")).expect("packages/") {
        let dir = entry.unwrap().path();
        let Ok(text) = std::fs::read_to_string(dir.join("Cargo.toml")) else { continue };
        let cargo: toml::Table = text.parse().unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        packages += 1;
        // Its own number, written out: `version.workspace = true` would tie
        // it to the engine's again.
        let Some(own) = cargo["package"].get("version").and_then(|v| v.as_str()) else {
            wrong.push(format!("{}/Cargo.toml: no version of its own", dir.display()));
            continue;
        };
        let manifest = dir.join("package.toml");
        let Ok(text) = std::fs::read_to_string(&manifest) else { continue };
        let table: toml::Table = text.parse().unwrap_or_else(|e| panic!("{}: {e}", manifest.display()));
        // Under `[package]`, or flat at the top - the manifest reader takes both.
        let field = |name: &str| {
            table.get("package").and_then(|p| p.get(name)).or_else(|| table.get(name)).and_then(|v| v.as_str())
        };
        let found = field("version").unwrap_or_else(|| panic!("{} has no version", manifest.display()));
        if found != own {
            wrong.push(format!("{}: {found}, but its Cargo.toml says {own}", manifest.display()));
        }
        if let Some(min) = field("engine_version")
            && semver(min) > semver(&version)
        {
            wrong.push(format!("{}: needs engine {min}, newer than this one", manifest.display()));
        }
    }
    assert!(packages > 0, "found no packages/*/Cargo.toml");
    assert!(wrong.is_empty(), "the engine is {version}, but:\n  {}", wrong.join("\n  "));
}

fn semver(v: &str) -> Vec<u64> {
    v.split('.').map(|n| n.parse().unwrap_or_else(|_| panic!("'{v}' is not MAJOR.MINOR.PATCH"))).collect()
}
