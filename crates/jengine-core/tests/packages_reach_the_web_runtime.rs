//! **A web build hosts packages the way native and the editor do.**
//!
//! Until Sep 2026 `jengine-web` built no `PackageHost` at all: a web build of
//! the arcade loaded every scene, turned each package component into custom
//! data, and died on the first package global a script called (`attempt to
//! call a nil value`) - most of the arcade, broken in the one runtime nobody
//! tests in. Nothing failed at build time.
//!
//! The web host's frame and command arms run only in a browser, so no native
//! test can call them. This reads the source instead, for each link in the
//! chain the editor viewport already has - any one missing is silent:
//!
//! * packages registered into the component registry **before** the scene is
//!   parsed, or their components deserialize as custom data;
//! * their script globals in the JS preamble, and their names on
//!   `__jengine_play` for Fengari, before any script compiles;
//! * their commands installed process-wide and dispatched from the drain;
//! * data tables published for `data::global()`, which packages read;
//! * the six phases, at the viewport's points in the frame;
//! * and one module-start function, whichever crate supplies it.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
    // Line endings normalized: the checks below span lines, and a checkout
    // with `core.autocrlf` writes some of these files with CRLF.
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
        .replace("\r\n", "\n")
}

fn at(src: &str, needle: &str, what: &str) -> usize {
    src.find(needle).unwrap_or_else(|| panic!("the web runtime no longer {what} (`{needle}`)"))
}

#[test]
fn packages_are_registered_before_the_entry_scene_is_parsed() {
    let lib = read("crates/jengine-web/src/lib.rs");
    let host = at(&lib, "PackageHost::new()", "builds a PackageHost");
    let project = at(&lib, "PROJECT_PACKAGES.with(|p| {", "adds the project's own packages");
    let init = at(&lib, "package_host.initialize(&mut registry)", "initializes its packages");
    let load = at(&lib, "load_scene_auto(&scene_json, &mut world, &registry)", "loads the entry scene");
    assert!(host < project && project < init, "built-ins, then the project's, then initialize");
    assert!(init < load, "packages must be in the registry before the scene is parsed");
}

#[test]
fn package_script_globals_reach_both_script_paths_and_the_drain() {
    let lib = read("crates/jengine-web/src/lib.rs");
    let host = read("crates/jengine-web/src/script_host.rs");
    let globals = at(&lib, "set_package_script_globals(", "publishes package globals to the preamble");
    let names = at(&lib, "publish_package_api_names(", "publishes package names for Fengari");
    let install = at(&lib, "script_commands::install_global(", "installs package commands");
    let compile = at(&lib, "script_host::install_scripts(", "compiles scripts");
    assert!(
        globals < compile && names < compile && install < compile,
        "package globals have to exist before the first script compiles"
    );
    assert!(host.contains("\"__package_api_names\""), "the names go where lua-runtime.ts reads them");

    let drain = at(&host, "fn drain_play_commands_into(", "has a command drain");
    let dispatch = drain + at(&host[drain..], "script_commands::dispatch_global(", "dispatches package commands");
    let own_arms = drain + at(&host[drain..], "match kind.as_str()", "has its own command arms");
    assert!(dispatch < own_arms, "package commands are dispatched before the host's own match");
}

#[test]
fn packages_can_read_the_data_tables() {
    let lib = read("crates/jengine-web/src/lib.rs");
    // One helper seeds both copies - scripts' and packages' - at boot and
    // again when an asset pack brings tables.
    let helper = at(&lib, "fn seed_data_tables(", "has the seeding helper");
    let rest = &lib[helper..];
    let end = rest[3..].find("\nfn ").map_or(rest.len(), |e| e + 3);
    let body = &rest[..end];
    assert!(body.contains("script_host::seed_data_tables("), "seeds data tables for scripts");
    assert!(body.contains("jengine_core::data::install_global("), "publishes data tables for packages");
    let seed = at(&lib, "seed_data_tables(&tables);", "seeds the tables at boot");
    let init = at(&lib, "package_host.initialize(&mut registry)", "initializes its packages");
    assert!(seed < init, "tables published before any package runs");
}

#[test]
fn the_phases_run_at_the_viewports_points_in_the_frame() {
    let lib = read("crates/jengine-web/src/lib.rs");
    // Indented eight spaces in `tick_world`, twelve inside render_frame's
    // `!transitioning` block.
    let find = |name: &str| {
        [8, 12]
            .iter()
            .filter_map(|n| lib.find(&format!("run_phase_with(\n{}SystemPhase::{name},", " ".repeat(*n))))
            .min()
            .unwrap_or_else(|| panic!("the web runtime no longer runs the {name} phase"))
    };
    let pre_update = find("PreUpdate");
    let script_exec = find("ScriptExec");
    let physics = find("Physics");
    let post_physics = find("PostPhysics");
    let animation = find("Animation");
    let pre_render = find("PreRender");
    // The frame's physics: `begin_frame`, then a step at a time (with
    // `fixed_update` between) or `step`, then `end_frame`.
    let step = at(&lib, "begin_frame(dt)", "steps physics");
    let barrier = at(&lib, "flush_commands_and_route_events(", "flushes the behaviour barrier");
    let camera = at(&lib, "update_camera(&app.world, &mut app.camera, dt)", "updates the camera");

    assert!(pre_update < script_exec && script_exec < step, "PreUpdate and ScriptExec before the step");
    assert!(barrier < physics && physics < post_physics, "Physics and PostPhysics after the step and barrier");
    assert!(camera < animation && animation < pre_render, "Animation and PreRender after the camera moved");
    assert!(
        !lib.contains("SystemPhase::Render,") && !lib.contains("SystemPhase::PostRender,"),
        "no host runs Render / PostRender: nothing is shared to draw with"
    );
}

#[test]
fn a_module_has_exactly_one_start_function() {
    let lib = read("crates/jengine-web/src/lib.rs");
    let cargo = read("crates/jengine-web/Cargo.toml");
    let web = read("crates/jengine-builder/src/web.rs");

    let start = at(&lib, "#[wasm_bindgen(start)]", "has a stock start function");
    assert!(
        lib[..start].trim_end().ends_with("#[cfg(feature = \"standalone\")]"),
        "the stock start must be behind `standalone`, or a project crate's start collides with it"
    );
    assert_eq!(lib.matches("wasm_bindgen(start)").count(), 1);
    assert!(
        cargo.contains("default = [\"standalone\""),
        "`standalone` must be on by default, so a plain `wasm-pack build` still starts"
    );
    // The builder passes `--no-default-features`, so it has to ask for the
    // stock start explicitly - and only for the stock crate.
    assert!(
        web.contains("if project_crate.is_none() {\n        features.push(\"standalone\""),
        "build_web must turn `standalone` on for a stock build and only for one"
    );
    assert!(
        web.contains("[\"--out-name\", \"jengine_web\"]"),
        "either crate must emit `jengine_web.js`, which index.html imports"
    );
}
