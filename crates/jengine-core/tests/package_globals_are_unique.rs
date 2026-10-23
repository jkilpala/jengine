//! No two packages register the same script global or the same component key.
//!
//! Every package's commands land in one `PackageCommandRegistry`, which
//! refuses a second `name` - correctly, since the second global would shadow
//! the first. But it refuses at load, into the log, and a package's
//! `register_script_commands` stops at its first error: so a clash on one
//! name leaves the package's *later* commands unregistered too, and the
//! clashing global quietly calls the *other* package's handler. A game
//! shipped that way - its package named its commands `sf_*`, and another
//! package already owned `sf_start`. The script's `sf_start`
//! ran the other package's (which found no match and warned), and the next line,
//! `sf_autopilot`, was `attempt to call a nil value`. Each package's own
//! scene test hosts only that package, so none of them could see it.
//!
//! A source scan, like `projects_link_the_packages_their_scenes_use`, over
//! every package in the repo rather than per project: a project may link any
//! set, so a name has to be unique among all of them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Every package's sources, keyed by the package directory's name.
fn package_sources() -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    let mut roots = vec![repo().join("packages")];
    if let Ok(projects) = std::fs::read_dir(repo().join("projects")) {
        roots.extend(projects.flatten().map(|p| p.path().join("packages")));
    }
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        for e in entries.flatten() {
            let src = e.path().join("src");
            if !src.is_dir() {
                continue;
            }
            let mut files = Vec::new();
            rust_files(&src, &mut files);
            let texts = files.iter().map(|f| std::fs::read_to_string(f).unwrap()).collect();
            out.insert(e.file_name().to_string_lossy().to_string(), texts);
        }
    }
    // The engine's own packages, which every checkout has (a count would fail
    // the public repository, which ships only these).
    for pkg in ["jengine-physics", "jengine-net", "jengine-online", "jengine-tracker"] {
        assert!(out.contains_key(pkg), "no {pkg} among {} packages - has the layout moved?", out.len());
    }
    out
}

fn is_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The string literal at the start of `s`, if there is one.
fn leading_literal(s: &str) -> Option<&str> {
    let body = s.trim_start().strip_prefix('"')?;
    body.find('"').map(|end| &body[..end])
}

/// The command name in each `.register(package, "name"` call; the package
/// argument is a string literal or a constant.
fn command_names(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for piece in src.split(".register(").skip(1) {
        let Some(comma) = piece.find(',') else { continue };
        let first = piece[..comma].trim();
        let literal = first.len() >= 2 && first.starts_with('"') && first.ends_with('"');
        if !literal && !is_ident(first) {
            continue;
        }
        if let Some(name) = leading_literal(&piece[comma + 1..]).filter(|n| is_ident(n)) {
            out.push(name.to_string());
        }
    }
    out
}

/// The key in each `register::<T>("key"` call.
fn component_keys(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    // `register_defaulted` registers the same way, with a `Default` for the editor.
    let src = src.replace("register_defaulted::<", "register::<");
    for piece in src.split("register::<").skip(1) {
        let Some(open) = piece.find(">(") else { continue };
        if let Some(key) = leading_literal(&piece[open + 2..]).filter(|k| is_ident(k)) {
            out.push(key.to_string());
        }
    }
    out
}

/// Names registered by more than one package. `must_find` are names of the
/// engine's own packages - every checkout has them, where a count would fail
/// the public repository, which ships only those - so a scan that stopped
/// finding registrations fails instead of passing on nothing.
fn clashes(scan: fn(&str) -> Vec<String>, must_find: &[&str], what: &str) -> Vec<String> {
    let mut owners: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (package, texts) in package_sources() {
        for text in texts {
            for name in scan(&text) {
                owners.entry(name).or_default().insert(package.clone());
            }
        }
    }
    for name in must_find {
        assert!(
            owners.contains_key(*name),
            "{name} is not among {} {what} - has registration changed shape?",
            owners.len()
        );
    }
    owners
        .iter()
        .filter(|(_, pkgs)| pkgs.len() > 1)
        .map(|(name, pkgs)| format!("{name}: {}", pkgs.iter().cloned().collect::<Vec<_>>().join(", ")))
        .collect()
}

#[test]
fn no_two_packages_register_the_same_script_global() {
    let clashes = clashes(command_names, &["online_join", "parallax_set"], "package commands");
    assert!(
        clashes.is_empty(),
        "script globals registered by more than one package - the second is refused at load, \
         and its package's later commands with it:\n  {}",
        clashes.join("\n  ")
    );
}

#[test]
fn no_two_packages_register_the_same_component_key() {
    // Core is not in this scan: the engine's own packages re-register core's
    // types under core's keys deliberately.
    let clashes = clashes(component_keys, &["online", "swarm_spawner"], "package components");
    assert!(clashes.is_empty(), "component keys registered by more than one package:\n  {}", clashes.join("\n  "));
}
