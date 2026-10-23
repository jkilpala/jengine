//! `set_body_type` must mean the same thing in every runtime.
//!
//! # The bug this was written for
//!
//! Three runtimes each parsed the body-type name with their own `match`, and
//! they disagreed. Native and the web host accepted `"static"` and
//! `"kinematic"`; the editor viewport also accepted `"Static"` and
//! `"Kinematic"`. Everything else fell through to `Dynamic`.
//!
//! So a script written and tested in the editor as
//! `set_body_type(e, "Static")` shipped as a **dynamic** body: a wall that
//! falls, with no error and no log line. Every existing guard passed —
//! `script_global_parity` saw the function bound everywhere,
//! `host_command_parity` and `preamble_arms_are_handled` saw the `sbt` tag
//! dispatched by both hosts. None of them can see what an arm *does*.
//!
//! `BodyType::from_script_name` is now the one parser. This test pins its
//! semantics, and — the part that actually prevents a relapse — checks by
//! source scan that no runtime has grown its own copy again.

use jengine_core::ecs::components::BodyType;

const NATIVE: &str = include_str!("../src/ecs/systems.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/script_host.rs");


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
fn the_names_a_script_can_pass_are_the_documented_three() {
    assert_eq!(BodyType::from_script_name("static"), Some(BodyType::Static));
    assert_eq!(BodyType::from_script_name("kinematic"), Some(BodyType::Kinematic));
    assert_eq!(BodyType::from_script_name("dynamic"), Some(BodyType::Dynamic));
}

/// The whole point of the shared parser: capitalisation must not decide which
/// runtime a script works in.
#[test]
fn parsing_ignores_case() {
    for name in ["Static", "STATIC", "sTaTiC"] {
        assert_eq!(
            BodyType::from_script_name(name),
            Some(BodyType::Static),
            "`{name}` did not parse as Static - the editor accepted this spelling \
             and the shipped game did not, which is the bug this guards"
        );
    }
    assert_eq!(BodyType::from_script_name("Kinematic"), Some(BodyType::Kinematic));
    assert_eq!(BodyType::from_script_name("Dynamic"), Some(BodyType::Dynamic));
}

/// An unrecognised name is reported rather than silently becoming `Dynamic`.
///
/// Callers still fall back to `Dynamic` — that is long-standing behaviour and
/// changing it would break scenes — but they now log, so a typo is findable.
#[test]
fn an_unknown_name_is_rejected_rather_than_guessed() {
    for name in ["", "stati", "rigid", "Static ", "kinematic2"] {
        assert_eq!(
            BodyType::from_script_name(name),
            None,
            "`{name}` was accepted as a body type"
        );
    }
}

/// The relapse guard: no runtime may parse body-type names for itself again.
///
/// A private `match` is exactly how the three drifted in the first place, and
/// it is invisible to every other check in this repo.
#[test]
fn no_runtime_parses_body_type_names_for_itself() {
    // The distinctive shape of a hand-rolled parser: a match arm mapping the
    // string to the enum. Matching on the arrow avoids flagging the parser's
    // own definition or a doc comment quoting it.
    let smells = ["\"static\" => BodyType", "\"Static\" => BodyType", "\"kinematic\" => BodyType"];
    for (name, src) in [("native", NATIVE), ("viewport", VIEWPORT), ("web", WEB)] {
        for smell in smells {
            assert!(
                !src.contains(smell),
                "{name} parses body-type names itself (`{smell}`) instead of calling \
                 `BodyType::from_script_name`. That is how native, viewport and web \
                 came to disagree about `\"Static\"`; keep one parser."
            );
        }
    }
}

/// And each runtime must actually call the shared parser, or the check above
/// passes for a runtime that stopped handling the command at all.
#[test]
fn every_runtime_calls_the_shared_parser() {
    for (name, src) in [("native", NATIVE.to_string()), ("viewport", with_dispatch(VIEWPORT)), ("web", with_dispatch(WEB))] {
        assert!(
            src.contains("BodyType::from_script_name"),
            "{name} no longer calls `BodyType::from_script_name`"
        );
    }
}

/// Physics commands address bodies by the entity's own id, so a runtime must
/// resolve the script's handle before calling the backend.
///
/// The viewport used to pass the raw script id straight through, in **four**
/// places. That is only the entity id by coincidence: `spawn_entity` and
/// `spawn_prefab` return a script-allocated index that `SCRIPT_ENTITY_MAP`
/// maps to the real entity. So pushing anything a script had spawned moved the
/// wrong body, or none — with no error anywhere.
///
/// The method list is scraped from `PhysicsWorld` rather than written out, so
/// a new entity-keyed call is covered the day it is added. Writing the list by
/// hand is how `set_velocity` escaped the first version of this test.
#[test]
fn physics_commands_resolve_the_script_handle_first() {
    let entity_keyed = entity_keyed_physics_methods();
    assert!(
        entity_keyed.len() >= 5,
        "only found {} entity-keyed physics methods — the scrape has stopped          matching and this test is no longer checking anything: {entity_keyed:?}",
        entity_keyed.len()
    );

    for (name, src) in [("viewport", VIEWPORT), ("web", WEB)] {
        for method in &entity_keyed {
            let call = format!("{method}(");
            for (idx, _) in src.match_indices(&call) {
                let line_start = src[..idx].rfind('\n').map_or(0, |n| n + 1);
                let line_end = src[idx..].find('\n').map_or(src.len(), |n| idx + n);
                let line = &src[line_start..line_end];
                // Definition sites and doc comments are not calls.
                if line.contains("fn ") || line.trim_start().starts_with("//") {
                    continue;
                }
                assert!(
                    line.contains(".id()"),
                    "{name} calls `{method}` with something that is not an entity id:
  {}
                     Physics bodies are keyed by `entity.id()`; a script handle from                      `spawn_entity` is not one. Resolve with `find_entity_by_id` first.",
                    line.trim()
                );
            }
        }
    }
}

/// Names of `PhysicsWorld` methods whose first argument is an entity id.
///
/// Scraped from the source so the guard above cannot go stale.
fn entity_keyed_physics_methods() -> Vec<String> {
    const PHYSICS: &str = include_str!("../src/physics/mod.rs");
    let mut out: Vec<String> = Vec::new();
    for line in PHYSICS.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("pub fn ") else { continue };
        let Some((name, args)) = rest.split_once('(') else { continue };
        // Both the real and the no-op stub signatures, hence the leading
        // underscore being optional.
        if args.contains("entity_id: u64") || args.contains("_entity_id: u64") {
            let name = name.to_string();
            if !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out.sort();
    out
}

/// A scene transition must leave the session with a working physics world.
///
/// # The bug
///
/// Replacing `app.world` invalidates every physics body handle, so the old
/// `PhysicsWorld` has to go. The editor viewport used to just drop it — set to
/// `None` and never rebuilt — and only `viewport_play` ever created one. A
/// transition does not re-enter play, so **physics was dead for the rest of the
/// session**: every scene reached from a hub menu ran without physics in the
/// editor while working natively and on web, which both rebuild.
///
/// That is also why the physics command divergences (#41-#47) went unnoticed
/// for so long. The commands were broken *and* nothing was simulating, so
/// there was nothing to notice.
///
/// This checks the shape rather than the behaviour: a runtime that drops
/// physics must also construct one nearby. Crude, but it is the failure that
/// happened, and no test in this repo can run a browser.
#[test]
fn dropping_physics_on_a_scene_transition_is_always_paired_with_a_rebuild() {
    // Functions allowed to drop physics without rebuilding, with the reason.
    // `viewport_play` sets it up again on the next play; the unsupported-backend
    // branch means there is deliberately nothing to build.
    const ALLOWED: &[&str] = &["viewport_stop", "viewport_play"];

    let mut offenders: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in VIEWPORT.lines() {
        let t = line.trim_start();
        if ((t.starts_with("pub fn ") || t.starts_with("fn ")) && line.starts_with(char::is_alphabetic)
            || t.starts_with("pub fn "))
            && let Some(rest) = t.split("fn ").nth(1)
                && let Some((name, _)) = rest.split_once('(')
            {
                current = name.to_string();
            }
        if line.contains("physics = None") && !ALLOWED.contains(&current.as_str()) {
            offenders.push(current.clone());
        }
    }
    assert!(
        offenders.is_empty(),
        "these viewport functions drop the physics world without rebuilding it:          {offenders:?}
         Only `viewport_play` and `viewport_stop` may, because play builds a new          one. Anywhere else — a scene transition, a pop — physics stays dead for          the rest of the session (BUGS #48). Rebuild from `app.physics_config`          the way the web runtime does."
    );

    // And the rebuild must actually be there, or the check above passes for a
    // viewport that simply stopped mentioning physics.
    assert!(
        VIEWPORT.contains("physics_config"),
        "the viewport no longer remembers what to rebuild physics from"
    );
}
