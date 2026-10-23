//! The transform mirror between the hosts and the browser preamble.
//!
//! Scripts read and write entity transforms through flat `Float32Array`s that
//! the WASM hosts fill once per frame (BUGS.md #30). That replaced per-entity
//! JS objects, which cost 31.4 ms of a 52 ms frame at 8,000 entities in each
//! direction — the hydration built them, and the writeback then walked the
//! whole world looking for changes that were almost never there.
//!
//! Both sides of that contract are implicit: the preamble knows the array
//! layout, and the host knows the preamble records what it wrote. Nothing
//! type-checks the agreement, and every way of breaking it is silent — a
//! script reads zeros, or a write never reaches the ECS.
//!
//! The preamble is `#[cfg(target_arch = "wasm32")]`, so it cannot be executed
//! from a native test. These read the source, like the other preamble tests.

/// The two-character CRLF sequence, spelled without an escape so this file
/// survives being rewritten by tooling that mangles backslashes.
const CRLF: &str = "
";

const BROWSER: &str = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
/// The mirror itself, which both hosts call since Oct 2026 rather than each
/// carrying a copy (`both_browser_hosts_use_the_shared_mirror` checks they
/// do). Each contract below is about the mirror, so it is checked there - a
/// host's wrapper of the same name is two lines and carries none of it.
const MIRROR: &str = include_str!("../src/scripting/bridge/adapters/js/mirror.rs");

#[test]
fn each_host_publishes_the_transform_arrays_exactly_once() {
    // This is the bug this test exists for. The old hydration ended by setting
    // `play.positions = <plain object>`; leaving that in place after the
    // rewrite silently overwrote the Float32Array with an empty object, and
    // every accessor would have read zeros with nothing to say why.
    for (name, src) in [("the shared mirror", MIRROR)] {
        for key in ["positions", "rotations", "scales", "sprite_colors"] {
            let n = src.matches(&format!("from_str(\"{key}\")")).count();
            assert_eq!(
                n, 1,
                "{name} sets `{key}` on the play namespace {n} times; it must be \
                 exactly once (in ScriptMirror::ensure_capacity) or the later \
                 write clobbers the typed array"
            );
        }
    }
}

#[test]
fn every_transform_setter_records_what_it_wrote() {
    // The writeback applies only the recorded ids. A setter that forgets to
    // record is a script write that reaches JS and never reaches the ECS —
    // visible only as an entity that refuses to move.
    for (func, list) in [
        ("function set_position(", "dirty_positions"),
        ("function set_rotation(", "dirty_rotations"),
        ("function set_scale(", "dirty_scales"),
        ("function set_sprite_color(", "dirty_colors"),
    ] {
        let start = BROWSER
            .find(func)
            .unwrap_or_else(|| panic!("{func} missing from the preamble"));
        let body_end = BROWSER[start..]
            .find("\n  }}")
            .unwrap_or_else(|| panic!("{func} body not delimited as expected"));
        let body = &BROWSER[start..start + body_end];
        assert!(
            body.contains(list),
            "{func} does not record into `{list}`, so its writes never reach the ECS"
        );
    }
}

#[test]
fn the_hosts_drain_every_dirty_list() {
    for (name, src) in [("the shared mirror", MIRROR)] {
        for list in [
            "dirty_positions",
            "dirty_rotations",
            "dirty_scales",
            "dirty_colors",
        ] {
            assert!(
                src.contains(&format!("drain_dirty(&play, \"{list}\")")),
                "{name} never drains `{list}`, so those writes are dropped"
            );
        }
    }
}

/// A script's `set_position` must reach the world before the commands it
/// queued after it. `juice:burst` moves a pooled emitter and calls
/// `Particles.emit_burst` in one breath; with the drain first, every coin and
/// kill burst in every web game came from where the *previous* one was. And a
/// `fixed_update`'s writes, drained with no writeback after, were wiped by the
/// next publish's reset of the dirty lists. So each browser host's drain
/// writes back before its loop.
#[test]
fn the_hosts_write_back_transforms_before_draining_commands() {
    for (name, src) in [
        ("jengine-web", include_str!("../../jengine-web/src/script_host.rs")),
        ("jengine-editor-viewport", include_str!("../../jengine-editor-viewport/src/lib.rs")),
    ] {
        let start = src
            .find("fn drain_play_commands_into(")
            .unwrap_or_else(|| panic!("{name} has no command drain"));
        let body = &src[start..];
        let writeback = body
            .find("sync_play_namespace_to_world(app)")
            .unwrap_or_else(|| panic!("{name}'s drain never writes back script transforms"));
        let loop_start = body
            .find("for i in 0..snapshot_len")
            .unwrap_or_else(|| panic!("{name}'s drain has no command loop"));
        assert!(
            writeback < loop_start,
            "{name} drains script commands before writing back what the scripts \
             set: a burst after `set_position` fires from the old place"
        );
    }
}

/// `gui_get` in the browsers reads the published `gui_state`; natively it
/// asks `snapshot::gui_node_prop`. Every property native answers must be
/// published, or a script reads "" in the editor and on the web - `submitted`
/// was missing, so no chat line was ever sent from a web game.
#[test]
fn the_browsers_publish_every_gui_property_native_answers() {
    let snapshot = include_str!("../src/scripting/bridge/snapshot.rs");
    let start = snapshot.find("pub fn gui_node_prop(").expect("gui_node_prop");
    let body = &snapshot[start..];
    let body = &body[..body.find("\n}").expect("end of gui_node_prop")];
    let gui_state = &MIRROR[MIRROR.find("let gui_state = js_sys::Object::new()").expect("gui_state")..];
    let gui_state = &gui_state[..gui_state.find("from_str(\"gui_state\")").expect("gui_state set")];
    let mut props: Vec<&str> = body.split('"').skip(1).step_by(2).collect();
    props.sort_unstable();
    props.dedup();
    assert!(props.contains(&"submitted"), "read no properties from gui_node_prop: {props:?}");
    for p in props {
        assert!(
            gui_state.contains(&format!("\"{p}\"")),
            "native's gui_get answers `{p}` and the browsers' gui_state never publishes it"
        );
    }
}

/// The preamble with `//` comment lines removed.
///
/// These checks look for old *code* patterns, and the comments explaining why
/// those patterns are gone naturally quote them.
fn browser_code_only() -> String {
    BROWSER
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("
")
}

#[test]
fn nothing_still_treats_the_arrays_as_per_entity_objects() {
    let code = browser_code_only();
    // The old shape was `positions[entity] = [x, y]`. Against a flat typed
    // array that silently writes to the wrong slot — index `entity` rather
    // than `entity * 2` — so an entity's position would land in some other
    // entity's slot.
    for pattern in [
        "positions[entity]",
        "rotations[entity]",
        "scales[entity]",
        "sprite_colors[entity]",
    ] {
        assert!(
            !code.contains(pattern),
            "the preamble still indexes the mirror as an object: `{pattern}`"
        );
    }
    // And the one place that enumerated it. `for (var e in <Float32Array>)`
    // walks every slot including the gaps.
    assert!(
        !code.contains("for (var e in positions)"),
        "the preamble still enumerates `positions`; it must walk `live_ids`"
    );
}

#[test]
fn script_handles_are_resolved_through_the_alias_map() {
    // Entities a script spawned are addressed by an allocated index, not an
    // `Entity::id()`. The old scheme mirrored values under both keys in one
    // namespace, so an index could collide with a real id and silently
    // address the wrong entity. Resolution is now explicit and one-way.
    assert!(
        BROWSER.contains("function __ent(entity)"),
        "the alias resolver is gone; script-spawned entities will miss"
    );
    for (name, src) in [("the shared mirror", MIRROR)] {
        assert!(
            src.contains("from_str(\"alias\")"),
            "{name} does not publish the alias map"
        );
    }
}

#[test]
fn the_mirror_allocates_nothing_that_scales_with_the_world() {
    // The rewrite removed per-entity JS objects but left three per-frame
    // allocations behind: a fresh `Uint32Array` for `live_ids` (32 KB a frame
    // at 8,000 entities - about a megabyte a second), four fresh `Array`s for
    // the dirty lists, and a fresh alias `Object`. That churn showed up as a
    // regular sawtooth in the profiler and a heap swinging between 3.5 and
    // 8.6 MB, with frame time far above the sum of its parts.
    //
    // Small per-component maps (skeletons, input actions, healths) still
    // allocate and that is fine: they are bounded by how many entities carry
    // those components, not by the entity count. What must never come back is
    // an allocation whose size follows the world.
    for (name, src) in [("the shared mirror", MIRROR)] {
        let src = src.replace(CRLF, "
");

        let hydrate = {
            let start = src
                .find("fn sync_world_to_play_namespace(")
                .expect("hydration fn");
            let rest = &src[start + 1..];
            // The next item ends it - `pub fn` in the shared mirror, where
            // every function is public.
            let end = [rest.find("
fn "), rest.find("
pub fn "), rest.find("
#[wasm_bindgen]")]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(rest.len());
            rest[..end].to_string()
        };
        assert!(
            !hydrate.contains("Uint32Array::new_with_length"),
            "{name} allocates the `live_ids` array inside the per-frame              hydration; it belongs in `ensure_capacity`, reused across frames"
        );

        let reset = {
            let start = src.find("fn reset_dirty_lists(").expect("reset fn");
            let rest = &src[start + 1..];
            let end = [rest.find("
fn "), rest.find("
pub fn ")].into_iter().flatten().min().unwrap_or(rest.len());
            rest[..end].to_string()
        };
        assert!(
            !reset.contains("Array::new()"),
            "{name} replaces the dirty lists each frame instead of truncating them"
        );

        assert!(
            hydrate.contains("alias_len"),
            "{name} rebuilds the alias object unconditionally; it should only              be rebuilt when the set of script-spawned entities changes"
        );
    }
}

#[test]
fn readers_respect_the_live_count() {
    // `live_ids` is reused between frames, so its tail is stale. A reader that
    // trusted `.length` would walk entities that no longer exist.
    assert!(
        BROWSER.contains("live_count"),
        "the preamble reads `live_ids` without bounding it by `live_count`"
    );
    for (name, src) in [("the shared mirror", MIRROR)] {
        assert!(
            src.contains("from_str(\"live_count\")"),
            "{name} does not publish `live_count`, so readers cannot tell how              much of the reused array is valid"
        );
    }
}
