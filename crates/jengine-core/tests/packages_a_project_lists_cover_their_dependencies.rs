//! **Listing a package does not list what it depends on.**
//!
//! This is the sibling of `projects_link_the_packages_their_scenes_use`, one
//! level further out, and it exists because a package can depend on others -
//! say a dungeon game's package built on a crawler package for the corridor
//! and a battle package for the fights, or `jengine-online` on `jengine-net`.
//!
//! The trap is that a Cargo dependency **compiles a package in and does not
//! register it**. `jengine-builder` writes one `host.add_package(Box::new(..))`
//! per entry in the project's top-level `"packages"` array, and nothing else.
//! So a project that lists the dungeon package and not the crawler builds
//! cleanly, links every line of the crawler's code, and then runs with the
//! crawler's components unregistered and its script commands absent - which
//! is the exact failure `projects_link_the_packages_their_scenes_use` guards
//! against, arrived at by a different road.
//!
//! What makes it worse than the plain case is that the symptom need not point
//! anywhere near the dependency. The dungeon game reads the crawler's *data
//! tables* and calls its code directly, so it works; what silently stops
//! working is a different game in the same project that uses the crawler's
//! own component, which is now unregistered. A missing entry for a package
//! you are not thinking about breaks a game you are not looking at.
//!
//! So: read each package's declared `dependencies`, and require any project
//! that lists a package to list that package's dependencies too.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `package -> the packages it names in `PackageMetadata::dependencies`.
///
/// Read from `metadata()` rather than from `Cargo.toml`, because it says more:
/// a package can need another to be *present* without linking its crate. The
/// second test below closes the gap that leaves - a crate dependency nobody
/// declared is a trap this guard cannot see.
fn declared_dependencies() -> BTreeMap<String, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(repo().join("packages")).expect("packages/").flatten() {
        let pkg = entry.file_name().to_string_lossy().to_string();
        let Ok(src) = std::fs::read_to_string(entry.path().join("src/lib.rs")) else { continue };
        let Some(i) = src.find("dependencies:") else {
            out.insert(pkg, BTreeSet::new());
            continue;
        };
        // `dependencies: vec!["jengine-net".into(), ...],` - take every
        // quoted string up to the closing bracket.
        //
        // **Only when there is a `vec![`.** `dependencies: Vec::new()` has no
        // bracket at all, so scanning to the next `]` runs off into whatever
        // code happens to follow and reports the package as depending on
        // itself - which is what the first draft of this did to
        // `jengine-parallax`.
        let rest = &src[i + "dependencies:".len()..];
        let Some(open) = rest.find("vec![").filter(|o| rest[..*o].trim().is_empty()) else {
            out.insert(pkg, BTreeSet::new());
            continue;
        };
        let rest = &rest[open..];
        let end = rest.find(']').unwrap_or(rest.len());
        let mut names = BTreeSet::new();
        let mut scan = &rest[..end];
        while let Some(q) = scan.find('"') {
            scan = &scan[q + 1..];
            let Some(e) = scan.find('"') else { break };
            let name = &scan[..e];
            if name.starts_with("jengine-") {
                names.insert(name.to_string());
            }
            scan = &scan[e + 1..];
        }
        out.insert(pkg, names);
    }
    // The extraction checked against a package whose declaration is known:
    // `jengine-online` needs `jengine-net` beside it. A count would do as
    // well here and not in the public tree, which ships far fewer packages.
    assert!(
        out.get("jengine-online").is_some_and(|d| d.contains("jengine-net")),
        "did not read `jengine-online`'s dependency on `jengine-net` - the scan broke: {out:?}"
    );
    out
}

/// The packages the runtime is **compiled with** and gates by name, read from
/// its own `is_on("…")` calls.
///
/// These need no entry in `"packages"` - they are already linked, which is
/// what makes them built-in. A game package may declare `jengine-physics`,
/// correctly and for a different reason than a package declares a crate it
/// is built on: it is a *runtime* dependency, "this game needs the physics
/// package active", not a crate one.
fn built_in() -> BTreeSet<String> {
    let src = std::fs::read_to_string(
        repo().join("crates/jengine-runtime/src/wgpu_scene_runner.rs"),
    )
    .expect("the runtime source");
    let mut out = BTreeSet::new();
    let mut rest = src.as_str();
    while let Some(i) = rest.find("is_on(\"") {
        rest = &rest[i + "is_on(\"".len()..];
        let Some(end) = rest.find('"') else { break };
        out.insert(rest[..end].to_string());
        rest = &rest[end..];
    }
    assert!(!out.is_empty(), "found no `is_on(..)` calls - the extraction broke");
    out
}

/// The packages a project lists under top-level `"packages"` - the list that
/// actually links and registers.
fn listed(project: &std::path::Path) -> BTreeSet<String> {
    let src = std::fs::read_to_string(project.join("project.jengine")).unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&src).unwrap_or(serde_json::Value::Null);
    v["packages"]
        .as_array()
        .map(|a| a.iter().filter_map(|p| p["name"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn projects() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(repo().join("projects"))
        .expect("projects/")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("project.jengine").exists())
        .collect();
    out.sort();
    out
}

#[test]
fn packages_a_project_lists_cover_their_dependencies() {
    let deps = declared_dependencies();
    let built_in = built_in();
    let mut problems = Vec::new();

    for project in projects() {
        let listed = listed(&project);
        for pkg in &listed {
            let Some(needs) = deps.get(pkg) else { continue };
            for need in needs {
                if !listed.contains(need) && !built_in.contains(need) {
                    problems.push(format!(
                        "{}: lists `{pkg}`, which depends on `{need}`, and does not list \
                         `{need}`",
                        project.file_name().unwrap().to_string_lossy()
                    ));
                }
            }
        }
    }

    problems.sort();
    assert!(
        problems.is_empty(),
        "a project lists a package without listing what that package depends on. The \
         dependency's crate is compiled in, so this builds; its components are never \
         registered and its script commands are never installed, so any scene using \
         them fails at runtime with `attempt to call a nil value`:\n  {}",
        problems.join("\n  ")
    );
}

/// A package's `jengine-*` **Cargo** dependencies are all declared.
///
/// One direction only. `metadata().dependencies` is what the guard above
/// reads and it is hand-written, so a package that gains a crate dependency
/// and forgets to declare it leaves the trap wide open while the guard
/// passes.
///
/// The other direction is deliberately not checked: the field means "this
/// package needs that one *present*", which is a superset of "links against
/// it". A game package may declare `jengine-physics` and not depend on its
/// crate, because what it needs is the physics package running, and that is
/// a true and useful thing to say.
#[test]
fn every_cargo_dependency_on_a_package_is_declared() {
    let declared = declared_dependencies();
    let mut problems = Vec::new();

    for (pkg, names) in &declared {
        let toml = repo().join("packages").join(pkg).join("Cargo.toml");
        let Ok(src) = std::fs::read_to_string(&toml) else { continue };
        // Only the `[dependencies]` section; a dev-dependency does not link
        // into a build.
        let body = match src.find("[dependencies]") {
            Some(i) => {
                let rest = &src[i + "[dependencies]".len()..];
                match rest.find("\n[") {
                    Some(e) => &rest[..e],
                    None => rest,
                }
            }
            None => "",
        };
        let real: BTreeSet<String> = body
            .lines()
            .filter_map(|l| l.split(['=', ' ']).next())
            .map(str::trim)
            .filter(|n| n.starts_with("jengine-") && *n != "jengine-core")
            // Only packages: a plain crate (`jengine-netcode`, under
            // `crates/`) has nothing to register, so a project has nothing to
            // list for it.
            .filter(|n| repo().join("packages").join(n).is_dir())
            .map(str::to_string)
            .collect();

        for missing in real.difference(names) {
            problems.push(format!(
                "{pkg}: Cargo.toml depends on `{missing}` and `metadata().dependencies` \
                 does not name it, so nothing will require a project to list it"
            ));
        }
    }

    problems.sort();
    assert!(
        problems.is_empty(),
        "a package links another and does not declare it:\n  {}",
        problems.join("\n  ")
    );
}
