//! The editor's "your viewport is stale" check.
//!
//! Rust packages are linked at compile time, and the editor viewport is one
//! prebuilt WASM module shared by every project — so it carries whichever
//! project it was last built for. Left unchecked, the symptom of a mismatch is
//! a package that silently does nothing, which is what makes package
//! development guesswork. These pin down when the editor nags and when it
//! stays quiet, because a check that cries wolf gets ignored.

use std::path::{Path, PathBuf};

use jengine_builder::viewport::{ViewportStamp, cache_dir, dep_info_sources, status, switch};

struct Fixture {
    _tmp: tempfile::TempDir,
    engine: PathBuf,
    project: PathBuf,
}

fn fixture(project_packages: &[&str]) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let engine = tmp.path().join("engine");
    let project = tmp.path().join("projects").join("my-game");
    std::fs::create_dir_all(engine.join("editor/src/lib/viewport-wasm")).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    // A package must exist on disk or `resolve_packages` errors out.
    let mut decls = Vec::new();
    for name in project_packages {
        let dir = project.join("packages").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("package.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\npackage_type = \"full\"\n"),
        )
        .unwrap();
        decls.push(serde_json::json!({
            "name": name,
            "source": { "type": "path", "path": format!("packages/{name}") }
        }));
    }
    std::fs::write(
        project.join("project.jengine"),
        serde_json::to_string_pretty(&serde_json::json!({ "packages": decls })).unwrap(),
    )
    .unwrap();

    Fixture { _tmp: tmp, engine, project }
}

fn write_stamp(engine: &Path, stamp: &ViewportStamp) {
    std::fs::write(
        engine.join("editor/src/lib/viewport-wasm/.jengine-viewport.json"),
        serde_json::to_string_pretty(stamp).unwrap(),
    )
    .unwrap();
}

#[test]
fn a_project_with_no_rust_packages_never_nags() {
    // The stock viewport already carries every built-in package, so there is
    // nothing to rebuild — including right after switching away from a project
    // that did have packages. Nagging here would train people to ignore it.
    let f = fixture(&[]);
    assert!(!status(&f.engine, &f.project).needs_rebuild);

    write_stamp(
        &f.engine,
        &ViewportStamp {
            project: "/somewhere/else".into(),
            packages: vec!["other-fx".into()],
            stock: false,
            ..Default::default()
        },
    );
    assert!(!status(&f.engine, &f.project).needs_rebuild);
}

#[test]
fn a_matching_stamp_is_quiet() {
    let f = fixture(&["cool-fx"]);
    write_stamp(
        &f.engine,
        &ViewportStamp {
            project: f.project.display().to_string(),
            packages: vec!["cool-fx".into()],
            stock: false,
            ..Default::default()
        },
    );
    let s = status(&f.engine, &f.project);
    assert!(!s.needs_rebuild, "nagged despite a match: {}", s.reason);
}

#[test]
fn a_viewport_built_for_another_project_is_flagged() {
    // The case that makes this whole check worth having.
    let f = fixture(&["cool-fx"]);
    write_stamp(
        &f.engine,
        &ViewportStamp {
            project: "/projects/other-game".into(),
            packages: vec!["other-fx".into()],
            stock: false,
            ..Default::default()
        },
    );
    let s = status(&f.engine, &f.project);
    assert!(s.needs_rebuild);
    // The message has to name both sides, or it does not help anyone.
    assert!(s.reason.contains("cool-fx"), "{}", s.reason);
    assert!(s.reason.contains("other-fx"), "{}", s.reason);
    assert!(s.reason.contains("other-game"), "{}", s.reason);
}

#[test]
fn the_stock_viewport_is_flagged_for_a_project_that_has_packages() {
    let f = fixture(&["cool-fx"]);
    write_stamp(&f.engine, &ViewportStamp { stock: true, ..Default::default() });
    let s = status(&f.engine, &f.project);
    assert!(s.needs_rebuild);
    assert!(s.reason.contains("stock viewport"), "{}", s.reason);
}

#[test]
fn a_missing_stamp_is_flagged_rather_than_assumed_fine() {
    // A module built before stamping existed cannot be assumed to carry
    // anything. Guessing "probably fine" is how a package ends up silently
    // dead.
    let f = fixture(&["cool-fx"]);
    let s = status(&f.engine, &f.project);
    assert!(s.needs_rebuild);
    assert!(s.reason.contains("no package stamp"), "{}", s.reason);
}

#[test]
fn package_order_does_not_matter() {
    // Both sides are sorted before comparison; declaration order in
    // project.jengine is not meaningful and must not trigger a rebuild.
    let f = fixture(&["b-pkg", "a-pkg"]);
    write_stamp(
        &f.engine,
        &ViewportStamp {
            project: f.project.display().to_string(),
            packages: vec!["a-pkg".into(), "b-pkg".into()],
            stock: false,
            ..Default::default()
        },
    );
    assert!(!status(&f.engine, &f.project).needs_rebuild);
}

#[test]
fn a_stamp_with_a_utf8_bom_still_parses() {
    // An earlier build wrote the stamp with PowerShell's `Set-Content
    // -Encoding utf8`, which emits a BOM that serde_json rejects. A stamp that
    // fails to parse reads as "no stamp" and nags forever.
    let f = fixture(&["cool-fx"]);
    let stamp = ViewportStamp {
        project: f.project.display().to_string(),
        packages: vec!["cool-fx".into()],
        stock: false,
        ..Default::default()
    };
    let json = format!("\u{feff}{}", serde_json::to_string_pretty(&stamp).unwrap());
    std::fs::write(
        f.engine.join("editor/src/lib/viewport-wasm/.jengine-viewport.json"),
        json,
    )
    .unwrap();

    let s = status(&f.engine, &f.project);
    assert!(!s.needs_rebuild, "BOM defeated the stamp: {}", s.reason);
}

/// A project's own build, as `viewport::build` leaves it: the module, its
/// stamp, and Cargo's list of what it was compiled from - here one source.
fn write_cache(f: &Fixture, packages: &[&str], built_at_ms: u64) -> PathBuf {
    let source = f.project.join("packages/src.rs");
    std::fs::write(&source, "// a source").unwrap();
    let cache = cache_dir(&f.project);
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("jengine_editor_viewport.js"), "// glue").unwrap();
    std::fs::write(cache.join("jengine_editor_viewport_bg.wasm"), b"wasm").unwrap();
    std::fs::write(
        cache.join("sources.d"),
        format!("{}: {}\n\n{}:\n", cache.join("x.wasm").display(), source.display(), source.display()),
    )
    .unwrap();
    std::fs::write(
        cache.join(".jengine-viewport.json"),
        serde_json::to_string_pretty(&ViewportStamp {
            project: f.project.display().to_string(),
            packages: packages.iter().map(|s| s.to_string()).collect(),
            stock: false,
            built_at_ms,
        })
        .unwrap(),
    )
    .unwrap();
    source
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn other_project_stamp(f: &Fixture) {
    write_stamp(
        &f.engine,
        &ViewportStamp {
            project: "/projects/other-game".into(),
            packages: vec!["other-fx".into()],
            stock: false,
            ..Default::default()
        },
    );
}

#[test]
fn a_current_cached_build_is_switched_to_without_compiling() {
    // Opening a project another project's viewport was built over used to
    // mean a compile of minutes every time. Its own build is kept, so if
    // nothing changed since, loading it is a copy.
    let f = fixture(&["cool-fx"]);
    other_project_stamp(&f);
    write_cache(&f, &["cool-fx"], now_ms() + 60_000);

    let s = status(&f.engine, &f.project);
    assert!(s.needs_rebuild && s.cached, "{s:?}");
    assert!(switch(&f.engine, &f.project).unwrap());

    let editor = f.engine.join("editor/src/lib/viewport-wasm");
    assert_eq!(std::fs::read_to_string(editor.join("jengine_editor_viewport.js")).unwrap(), "// glue");
    assert!(!editor.join("sources.d").exists(), "the source list is the cache's own");
    let s = status(&f.engine, &f.project);
    assert!(!s.needs_rebuild, "still nags after the switch: {}", s.reason);
}

#[test]
fn a_cached_build_older_than_a_source_is_not_used() {
    // The editor's TypeScript calls into the module, so a build that predates
    // a change to what it was compiled from is a build to redo, not reuse.
    let f = fixture(&["cool-fx"]);
    other_project_stamp(&f);
    write_cache(&f, &["cool-fx"], now_ms() - 60_000);

    assert!(!status(&f.engine, &f.project).cached);
    assert!(!switch(&f.engine, &f.project).unwrap());
    assert!(
        !f.engine.join("editor/src/lib/viewport-wasm/jengine_editor_viewport.js").exists(),
        "a refused switch must leave the editor's module alone"
    );
}

#[test]
fn a_cached_build_of_other_packages_is_not_used() {
    let f = fixture(&["cool-fx", "new-fx"]);
    other_project_stamp(&f);
    write_cache(&f, &["cool-fx"], now_ms() + 60_000);
    assert!(!status(&f.engine, &f.project).cached);
    assert!(!switch(&f.engine, &f.project).unwrap());
}

#[test]
fn a_cached_build_whose_source_is_gone_is_not_used() {
    let f = fixture(&["cool-fx"]);
    other_project_stamp(&f);
    let source = write_cache(&f, &["cool-fx"], now_ms() + 60_000);
    std::fs::remove_file(source).unwrap();
    assert!(!switch(&f.engine, &f.project).unwrap());
}

#[test]
fn dep_info_reads_windows_paths_and_escaped_spaces() {
    let text = r"C:\a\x.wasm: C:\a\src\lib.rs C:\my\ dir\b.rs

C:\a\src\lib.rs:
";
    let got = dep_info_sources(text);
    assert_eq!(
        got,
        vec![PathBuf::from(r"C:\a\src\lib.rs"), PathBuf::from(r"C:\my dir\b.rs")]
    );
}

/// Against the live repository, not a fixture.
///
/// `#[ignore]`d because it depends on which project the viewport was last
/// built for, which is machine state rather than something the suite controls.
/// Run it to see what the editor would say right now:
///
/// ```text
/// cargo test -p jengine-builder --test viewport_status -- --ignored --nocapture
/// ```
#[test]
#[ignore = "depends on local build state"]
fn report_live_repository_status() {
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut projects: Vec<_> = std::fs::read_dir(engine.join("projects"))
        .expect("projects/")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("project.jengine").exists())
        .collect();
    projects.sort();
    for project in projects {
        let name = project.file_name().unwrap().to_string_lossy().to_string();
        let s = status(&engine, &project);
        println!(
            "{name}: needs_rebuild={} linked={:?} wanted={:?}
  {}",
            s.needs_rebuild, s.linked, s.wanted, s.reason
        );
    }
}
