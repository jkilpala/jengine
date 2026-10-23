//! Every runtime must draw the same set of components.
//!
//! # Why this exists
//!
//! `VectorPath` is a *core* component - registered in `core_registration.rs`,
//! carried by `ComponentBundle`, authored in scenes, and drawn by
//! `ecs::wgpu_systems::render_shapes_wgpu`. That function had **exactly one
//! caller in the whole repo**: the editor viewport. Neither the native runtime
//! nor the web one so much as mentioned a shape batch, so an entity with a
//! `vector_path` was silently invisible in three of the four runtimes.
//! `projects/engine-test-lab/assets/scenes/22-vector-paths.scene` exists to
//! exercise the feature and showed an empty screen when run natively; scene 27
//! lost its trigger outlines the same way. Found 2026-09-21, fixed the same
//! day - see `docs/parity-gaps.md`.
//!
//! Nothing caught it, and nothing *could* have: every ratchet in this repo is
//! about **script globals**, and this is a render pass. A component can be
//! wired everywhere a component is wired and still never reach a screen,
//! because drawing it is a separate call that one runtime happens to make.
//! Same family as BUGS #144 - a missing call is invisible to any test of the
//! function it should have called.
//!
//! # What it checks
//!
//! Every `pub fn render_*_wgpu` in `wgpu_systems.rs` has to be **classified**:
//! drawn by every runtime, deliberately editor-only, or not wired anywhere.
//! Adding a render pass and forgetting a runtime now fails here rather than
//! in a screenshot nobody takes.
//!
//! It is a source-level check, like `preamble_command_tags` - the real pass
//! needs a GPU, and what went wrong was never the drawing but the *calling*.

mod support;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    let p = root().join(rel);
    support::with_frame_draw(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("reading {}: {e}", p.display())))
}

/// The three runtimes that draw a game frame. The browser preamble is a
/// scripting path, not a renderer, so it is not one of these.
fn runtimes() -> [(&'static str, String); 3] {
    [
        ("native", read("crates/jengine-runtime/src/wgpu_scene_runner.rs")),
        ("viewport", read("crates/jengine-editor-viewport/src/lib.rs")),
        ("web", read("crates/jengine-web/src/lib.rs")),
    ]
}

/// Drawn by all three. A component whose pass is here reaches every screen.
const EVERY_RUNTIME: &[&str] = &[
    "render_gui_wgpu",
    "render_lightmaps_wgpu",
    "render_normals_wgpu",
    // Per layer of a lit frame, so `unlit` on an emitter is honoured (Oct 2026).
    "render_particles_in_layer",
    // The sprite pass: the world collected once (`render_world_items`) and
    // drawn per layer.
    "render_world_items",
    "render_tilemaps_wgpu",
    "render_touch_controls_wgpu",
    "render_ui_wgpu",
    // Each sprite's motion on screen, for `motion_blur` (Oct 2026).
    "render_velocity_wgpu",
];

/// Deliberately the editor's alone: tooling passes, not the game frame.
/// A game cannot depend on these, so no player can miss them.
const EDITOR_ONLY: &[(&str, &str)] = &[
    ("render_sprites_wgpu_multi", "the asset-preview pass, which has no game equivalent"),
    (
        "render_sprites_wgpu_multi_materialized",
        "the editor's pipeline-variant pass, used for the edit-mode viewport",
    ),
    (
        "render_particles_wgpu",
        "every layer at once, for the edit-mode and asset-preview passes, which have no lighting layers; the game frame draws particles per layer through render_particles_in_layer",
    ),
];

/// Wired to no runtime at all. Superseded rather than forgotten: the shadowed
/// sprite pass does both jobs, and the unlit ones were folded into
/// its `WorldLayer`s (Sep 2026). Listed so that *nothing* is unclassified - an
/// unclassified pass is exactly how `render_shapes_wgpu` hid.
const NOT_WIRED: &[(&str, &str)] = &[
    (
        "render_sprites_with_shadows_wgpu_materialized",
        "one layer in one call - `render_world_items` and a single `draw` - for tests and one-layer callers; every runtime collects the world once a camera and draws each layer from it (Oct 2026), so calling it as well would collect twice",
    ),
    ("render_shadows_wgpu", "superseded by render_sprites_with_shadows_wgpu_materialized"),
    (
        "render_text_wgpu",
        "folded into render_sprites_with_shadows_wgpu_materialized, which draws world text in z order with the sprites (Sep 2026); the golden tests still call it directly",
    ),
    (
        "render_shapes_wgpu",
        "folded into the sprite pass (Sep 2026): vector paths sort with sprites by z_order; calling it too would draw every path twice, the second time over every sprite",
    ),
    (
        "render_skinned_meshes_wgpu",
        "folded into the sprite pass (Sep 2026): skinned meshes sort with sprites by z_order; calling it too would draw every mesh twice, the second time over every sprite",
    ),
];

/// Every `pub fn render_*` the ECS renderer exports.
///
/// Deliberately not filtered on a `_wgpu` suffix: three of them end in
/// `_multi` or `_materialized`, and a scrape that quietly skipped those would
/// leave exactly the kind of unclassified pass this test is here to catch.
fn declared_passes() -> BTreeSet<String> {
    let src = read("crates/jengine-core/src/ecs/wgpu_systems.rs");
    let mut out = BTreeSet::new();
    for line in src.lines() {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix("pub fn render_")
            && let Some(name) = rest.split(['(', '<']).next()
            && !name.is_empty()
        {
            out.insert(format!("render_{name}"));
        }
    }
    assert!(out.len() > 8, "found only {} render passes - the scrape broke", out.len());
    out
}

/// Called, as opposed to merely imported or *talked about*: the name followed
/// by a `(`, on a line that is not a comment.
///
/// Skipping comments is not fussiness. The first version of this counted them,
/// and commenting the native call out - the exact bug, reintroduced by hand to
/// check the test could see it - still passed, because the commented line and
/// the field doc above it both contain `render_shapes_wgpu(`. A ratchet that
/// cannot be made to fail is not a ratchet.
fn calls(src: &str, name: &str) -> usize {
    let needle = format!("{name}(");
    src.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .map(|l| l.matches(&needle).count())
        .sum()
}

#[test]
fn every_render_pass_is_classified() {
    let declared = declared_passes();
    let known: BTreeSet<String> = EVERY_RUNTIME
        .iter()
        .map(|s| s.to_string())
        .chain(EDITOR_ONLY.iter().map(|(n, _)| n.to_string()))
        .chain(NOT_WIRED.iter().map(|(n, _)| n.to_string()))
        .collect();

    let unclassified: Vec<&String> = declared.difference(&known).collect();
    assert!(
        unclassified.is_empty(),
        "these render passes are not classified: {unclassified:?}\n\
         Add each to EVERY_RUNTIME, EDITOR_ONLY or NOT_WIRED in this test. An unclassified \
         pass is exactly how `render_shapes_wgpu` came to be drawn by one runtime out of \
         three, leaving `vector_path` invisible everywhere else."
    );

    let stale: Vec<&String> = known.difference(&declared).collect();
    assert!(stale.is_empty(), "these are listed here but no longer exist: {stale:?}");
}

/// **The one that would have caught it.**
#[test]
fn a_pass_drawn_by_one_runtime_is_drawn_by_all_of_them() {
    let rts = runtimes();
    let mut broken = Vec::new();

    for name in EVERY_RUNTIME {
        let missing: Vec<&str> =
            rts.iter().filter(|(_, src)| calls(src, name) == 0).map(|(n, _)| *n).collect();
        if !missing.is_empty() {
            let drawn: Vec<&str> =
                rts.iter().filter(|(_, src)| calls(src, name) > 0).map(|(n, _)| *n).collect();
            broken.push(format!(
                "  `{name}` is called by {drawn:?} but NOT by {missing:?} - so whatever it draws \
                 is invisible there, with no warning and nothing in any log"
            ));
        }
    }
    assert!(
        broken.is_empty(),
        "a render pass does not reach every runtime:\n{}\n\n\
         Either wire it up, or move it to EDITOR_ONLY with a reason and add a row to \
         docs/parity-gaps.md.",
        broken.join("\n")
    );
}

#[test]
fn an_editor_only_pass_really_is_editor_only() {
    let rts = runtimes();
    for (name, why) in EDITOR_ONLY {
        for (rt, src) in &rts {
            let n = calls(src, name);
            if *rt == "viewport" {
                assert!(n > 0, "`{name}` is listed as editor-only but the viewport does not call it");
            } else {
                assert_eq!(
                    n, 0,
                    "`{name}` is listed as editor-only ({why}) but `{rt}` calls it - move it to \
                     EVERY_RUNTIME"
                );
            }
        }
    }
}

#[test]
fn a_pass_listed_as_unwired_really_is() {
    let rts = runtimes();
    for (name, why) in NOT_WIRED {
        for (rt, src) in &rts {
            assert_eq!(
                calls(src, name),
                0,
                "`{name}` is listed as not wired ({why}) but `{rt}` calls it - it is live again, \
                 so classify it properly"
            );
        }
    }
}

/// The specific regressions, named. `vector_path` is a core component that
/// only the editor drew until Sep 2026 - test-lab scene 22 was an empty screen
/// natively for as long as it existed - and then, drawn everywhere, it was a
/// pass of its own after every sprite, so no `z_order` could put a path under
/// one. It is drawn by the sprite pass now, which every runtime calls; and no
/// runtime may call the old pass as well, because that draws every path a
/// second time, over everything.
#[test]
fn skinned_meshes_are_drawn_by_every_runtime_in_the_sprite_pass() {
    for (rt, src) in runtimes() {
        assert!(
            calls(&src, "render_world_items") > 0,
            "`{rt}` does not run the sprite pass, which is what draws `skinned_mesh` entities"
        );
        assert_eq!(
            calls(&src, "render_skinned_meshes_wgpu"),
            0,
            "`{rt}` still calls the old skinned pass: every skinned mesh would draw twice, the              second time over every sprite"
        );
    }
}

#[test]
fn vector_path_is_drawn_by_every_runtime_in_the_sprite_pass() {
    for (rt, src) in runtimes() {
        assert!(
            calls(&src, "render_world_items") > 0,
            "`{rt}` does not run the sprite pass, which is what draws `vector_path` entities"
        );
        assert_eq!(
            calls(&src, "render_shapes_wgpu"),
            0,
            "`{rt}` still calls the old shape pass: every vector path would draw twice, the \n             second time over every sprite"
        );
    }
}

/// Every host draws its frame - views, world, post chain, UI - through the
/// one function (`ecs::frame_draw::draw_frame`, Oct 2026). Before it, each
/// carried an 800-line copy, and a fix to one reached the others only if
/// someone remembered to copy it.
#[test]
fn every_host_draws_its_frame_through_the_shared_function() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for host in [
        "crates/jengine-runtime/src/wgpu_scene_runner.rs",
        "crates/jengine-editor-viewport/src/lib.rs",
        "crates/jengine-web/src/lib.rs",
    ] {
        let src = std::fs::read_to_string(root.join(host)).unwrap();
        assert!(src.contains("draw_frame(\n"), "{host} no longer draws through ecs::frame_draw");
        assert!(
            !src.contains("light.composite(&mut frame.encoder"),
            "{host} composites the lights itself again - a second copy of the frame"
        );
    }
}
