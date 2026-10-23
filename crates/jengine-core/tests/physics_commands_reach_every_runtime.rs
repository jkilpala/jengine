//! Every runtime has to *apply* the physics script commands, not just
//! accept them.
//!
//! Eight script functions - `apply_impulse`, `apply_force`,
//! `set_angular_velocity`, `set_gravity`, `set_body_type`, `add_joint`,
//! `remove_joint`, `set_motor_speed` - reach the engine as
//! `ScriptCommand`s that `apply_script_commands` **deliberately
//! ignores**:
//!
//! ```text
//! ScriptCommand::ApplyForce { .. }
//! | ScriptCommand::ApplyImpulse { .. }
//! | ...
//! | ScriptCommand::SetMotorSpeed { .. } => {}
//! ```
//!
//! The understanding is that something else handles them, because they
//! need the `PhysicsWorld` and that function only gets the ECS. The
//! "something else" is `apply_physics_commands`.
//!
//! **On native there was no landing site.** The only caller of
//! `apply_physics_commands` was the `jengine-physics` package's system,
//! and that system does nothing in the native runtime or the editor
//! viewport, because both drive rapier on their own path and
//! deliberately do not share `PhysicsWorld` as a package resource. The
//! editor got away with it by having its own inline arm; native had
//! neither, so all eight were dropped **in silence** - the name bound,
//! the call succeeded, nothing moved.
//!
//! That is a grade-2 parity gap in `docs/parity-gaps.md`'s terms: the
//! sort no name-level ratchet can see. Test-lab scene 17's jump was dead
//! natively for as long as the scene had existed, and it took a pinball
//! table - a game that cannot work at all without impulses - to notice.
//! BUGS #144.
//!
//! This test reads the runtime sources, because the thing that went
//! wrong was a missing *call*, and no amount of testing the function it
//! should have called would have found it.

const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");

/// Strip `//` line comments, so a mention in prose does not satisfy a
/// check that wants a call. The doc comments in this repo are long and
/// name these functions often.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
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
fn native_applies_physics_script_commands() {
    let code = code_only(NATIVE);
    assert!(
        code.contains("apply_physics_commands("),
        "the native runtime never calls `apply_physics_commands`, so every physics script \
         command - apply_impulse, apply_force, set_gravity, add_joint, set_motor_speed and the \
         rest - is accepted and silently dropped. `apply_script_commands` ignores them all on \
         purpose; this is the only thing that handles them."
    );
}

#[test]
fn the_editor_viewport_applies_them_too() {
    // It does it with its own inline arm rather than through
    // `apply_physics_commands`, which is why native's absence went
    // unnoticed for so long: the runtime people develop in was fine.
    // Through the arms both browser hosts share since Oct 2026.
    let code = code_only(&with_dispatch(VIEWPORT));
    assert!(
        code.contains(".apply_impulse("),
        "the editor viewport no longer applies impulses"
    );
    assert!(
        code.contains(".apply_force("),
        "the editor viewport no longer applies forces"
    );
}

#[test]
fn the_ignore_arm_still_names_what_it_is_ignoring() {
    // If somebody makes `apply_script_commands` handle one of these
    // directly, this list shrinks and the commands above may stop being
    // needed. The check exists so that change is deliberate rather than
    // a silent double-apply.
    let systems = include_str!("../src/ecs/systems.rs");
    for name in [
        "ScriptCommand::ApplyForce",
        "ScriptCommand::ApplyImpulse",
        "ScriptCommand::SetGravity",
        "ScriptCommand::SetBodyType",
        "ScriptCommand::AddJoint",
        "ScriptCommand::RemoveJoint",
        "ScriptCommand::SetMotorSpeed",
    ] {
        assert!(
            systems.contains(name),
            "{name} is gone; check whether `apply_physics_commands` still needs to handle it"
        );
    }
}

#[test]
fn apply_physics_commands_is_reachable_at_all() {
    // The function is `#[cfg(feature = "physics")]`. A runtime that
    // builds without the feature cannot call it, which is fine - but
    // `jengine-runtime` builds *with* it, so if this ever stops being
    // true the call above would not compile and somebody might delete
    // the call rather than the cfg.
    let systems = include_str!("../src/ecs/systems.rs");
    assert!(
        systems.contains("pub fn apply_physics_commands("),
        "apply_physics_commands has been renamed or removed"
    );
}

// ---- The `cfg` that is always false ------------------------------------

/// Neither `jengine-runtime` nor `jengine-editor-viewport` declares a
/// feature called `physics`.
///
/// Both turn *core's* `physics` feature on unconditionally, which is why
/// `state.physics` exists in them at all - but a feature a crate does not
/// declare is simply absent, so `#[cfg(feature = "physics")]` inside
/// either crate is **permanently false** and silently deletes whatever it
/// guards.
///
/// This has now bitten twice, both times in exactly the shape that is
/// hardest to notice:
///
/// - the editor viewport published zero gravity whatever the scene said,
///   so `get_gravity` in the editor answered `(0, 0)` forever;
/// - the native runtime's `raycast` support was compiled out whole, so the
///   global bound, the call succeeded, and every ray missed - which is the
///   same silent shape as the bug that work was written to fix.
///
/// Both were found by running something and seeing nothing happen. This is
/// cheaper.
#[test]
fn neither_host_crate_guards_code_behind_a_feature_it_does_not_have() {
    for (name, src) in [("jengine-runtime", NATIVE), ("jengine-editor-viewport", VIEWPORT)] {
        let code = code_only(src);
        let n = code.matches("cfg(feature = \"physics\")").count();
        assert_eq!(
            n, 0,
            "{name} guards {n} place(s) behind `cfg(feature = \"physics\")`, and that crate \
             declares no such feature - so the cfg is always false and the code inside is \
             deleted. It turns core's `physics` feature on unconditionally; drop the cfg. \
             (`jengine-web` does have its own `physics` feature and is not checked here.)"
        );
    }
}

/// And the positive half: the native runtime must actually lend its physics
/// world to the script system, or `raycast` has nothing to cast against.
///
/// A missing *call* is invisible to any test of the function it should have
/// called, which is the lesson the rest of this file was written for.
#[test]
fn native_lends_its_physics_world_for_raycast() {
    let code = code_only(NATIVE);
    assert!(
        code.contains("set_physics_ptr("),
        "the native runtime never calls `set_physics_ptr`, so `raycast` has no world to cast \
         against and every ray misses in silence"
    );
    assert!(
        code.contains("clear_physics_ptr("),
        "the native runtime never calls `clear_physics_ptr`. The borrow is only valid for one \
         `run`, because a scene load replaces the physics world - a pointer left set would \
         outlive what it points at"
    );
}

/// Both browser hosts must refresh the collider snapshot their `raycast`
/// callback answers from, and install the callback at all.
///
/// The install is the part that was missing for years: the preamble's
/// `raycast` reads `state.physics_raycast`, and no host set it, so the
/// function returned `null` while every name-level check counted it as
/// present.
#[test]
fn both_browser_hosts_install_and_feed_the_ray_callback() {
    const WEB_LIB: &str = include_str!("../../jengine-web/src/lib.rs");
    const WEB_HOST: &str = include_str!("../../jengine-web/src/script_host.rs");
    for (name, src) in [
        ("editor viewport", code_only(VIEWPORT)),
        ("web", code_only(&[WEB_LIB, WEB_HOST].concat())),
    ] {
        assert!(
            src.contains("install_ray_callback("),
            "the {name} host never installs the `physics_raycast` callback, so `raycast` there \
             returns null however well the snapshot works"
        );
        assert!(
            src.contains("ray_snapshot.borrow_mut().refresh_named("),
            "the {name} host never refreshes its ray snapshot (with the script's spawn names), so `raycast` answers from the \
             scene as it was when play started - which is worse than answering nothing, because \
             it looks right at first"
        );
    }
}
