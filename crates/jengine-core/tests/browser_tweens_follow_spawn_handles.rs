//! A tween on an entity a script spawned must move *that* entity in the
//! browser runtimes.
//!
//! `spawn_prefab` in the browser preamble returns a handle from a counter
//! that only goes up, while hecs reuses the ids of despawned entities - so
//! after the first despawn a handle and the real id stop agreeing. The
//! editor and web tween arms resolved the handle to find the entity, then
//! keyed the tween by the **handle**; `tick_tweens` looks targets up by real
//! id, so the tween moved nothing, or whatever entity owned that number.
//! A deckbuilder spawns every card at the draw pile and tweens it into the
//! hand, and in the editor every card stayed on the pile.
//!
//! Native is not affected: its handles start at `FIRST_SPAWN_ID` and the
//! tween tick resolves them through `spawn_aliases`.

const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/script_host.rs");

/// The first non-comment argument after `tid,` in each `add_tween_with_id(`.
fn tween_keys(src: &str) -> Vec<String> {
    src.split("add_tween_with_id(")
        .skip(1)
        .map(|call| {
            call.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with("//"))
                .nth(1)
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}


/// The shared command arms both browser hosts call (`adapters::js::host_dispatch`).
const DISPATCH: &str = include_str!("../src/scripting/bridge/adapters/js/host_dispatch.rs");

/// A host's source as far as its commands go: its own, plus the shared arms -
/// but only if it calls them, so a host that dropped the call fails.
fn with_dispatch(src: &str) -> String {
    if src.contains("host_dispatch::handle_host_command(") {
        format!("{src}
{DISPATCH}")
    } else {
        src.to_string()
    }
}

#[test]
fn browser_tweens_are_keyed_by_the_real_entity() {
    for (name, src) in [("viewport", with_dispatch(VIEWPORT)), ("web", with_dispatch(WEB))] {
        let keys = tween_keys(&src);
        assert_eq!(keys.len(), 4, "{name}: expected the four tween arms (tp, tr, ts, tc), found {keys:?}");
        for key in keys {
            assert_eq!(
                key, "entity.id() as u64,",
                "{name} keys a tween by `{key}` - a spawn handle is not an entity id once hecs reuses a slot"
            );
        }
    }
}

#[test]
fn browser_cancel_all_tweens_resolves_the_handle() {
    for (name, src) in [("viewport", with_dispatch(VIEWPORT)), ("web", with_dispatch(WEB))] {
        let arm = &src[src.find("\"cat\" =>").unwrap_or_else(|| panic!("{name} has no `cat` arm"))..];
        let arm = &arm[..arm.find("cancel_all(").expect("cat arm calls cancel_all")];
        assert!(
            // The shared arm calls the host's resolver through `HostParts`.
            arm.contains("find_entity_by_id(") || arm.contains("find_entity_by_id)("),
            "{name}'s cancel_all_tweens does not resolve a spawn handle, so it cancels nothing on a spawned entity"
        );
    }
}

/// Keyed by real id, a tween outlives its entity unless the despawn drops
/// it - and hecs gives that id to the next spawn, which the tween would then
/// drive. Native prunes tweens whose entity is gone every frame.
#[test]
fn browser_destroy_entity_cancels_its_tweens() {
    for (name, src) in [("viewport", with_dispatch(VIEWPORT)), ("web", with_dispatch(WEB))] {
        let arm = &src[src.find("\"de\" =>").unwrap_or_else(|| panic!("{name} has no `de` arm"))..];
        let arm = &arm[..arm.find("world.despawn(e)").expect("de arm despawns")];
        assert!(
            arm.contains("tween_manager.cancel_all(e.id() as u64)"),
            "{name}'s destroy_entity leaves the entity's tweens running on a reusable id"
        );
    }
}
