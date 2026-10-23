//! Session state must not outlive the scene that created it.
//!
//! This is the third instance of a class of bug the repo has met twice
//! already, and lost both times before noticing: a **post-process chain** and
//! **looping music** each survived a scene change and followed the player into
//! the next scene, because both are state the *world* does not own and
//! replacing the world therefore does not touch them (BUGS #84,
//! `post_process_resets_between_scenes.rs`).
//!
//! Script **timers** and **event listeners** are the same shape and were never
//! cleared by anything, in any runtime. Nothing noticed, because every game in
//! the repo loads a scene once and stays in it - so a strand has nowhere to
//! land. A WarioWare-style compilation loads a new scene every three or four
//! seconds, which turns "nothing noticed" into "every few seconds".
//!
//! # The two failures, concretely
//!
//! A `Timer` carries a raw `entity_id: u64`, and the next scene **reuses those
//! ids**. So a microgame that starts a two-second timer and is torn down after
//! one and a half seconds fires its callback half a second into the *next*
//! microgame, on whatever entity inherited the id. The symptom is one
//! microgame occasionally behaving as though it were a different one.
//!
//! `event_listeners` is a `HashMap<String, Vec<(u64, u32)>>` that only ever
//! grows. Fifty scenes in, `send_event("hit")` dispatches to fifty scenes'
//! worth of dead registrations.

#![cfg(feature = "native")]

use jengine_core::ecs::systems::ScriptSystem;

/// The clearing contract of a scene swap, which `reset_initialized` is.
///
/// Every runtime calls it when it replaces the world (the native runtime in
/// `apply_scene_json`, the viewport and web in their own swaps), and it is the
/// single place that decides what does *not* survive a scene.
#[test]
fn a_scene_swap_drops_the_timers_the_old_scene_set() {
    let mut sys = ScriptSystem::new().expect("a script system");

    // A microgame arms a countdown, as one would: "in two seconds, lose".
    let id = sys.api.services.set_timer(7, 2.0, "on_timeout".into());
    sys.api.services.set_timer(9, 0.5, "on_tick".into());
    assert_eq!(sys.api.services.timers.len(), 2, "armed");
    assert!(id < u32::MAX);

    // The compilation moves on before either fires.
    sys.reset_initialized();

    assert!(
        sys.api.services.timers.is_empty(),
        "a timer from the previous scene survived the swap. It carries a raw entity id, and the \
         next scene reuses those ids - so it will fire `on_timeout` on whatever entity inherits \
         id 7, in a scene that never asked for a timer at all. {:?}",
        sys.api
            .services
            .timers
            .iter()
            .map(|t| (t.entity_id, t.callback_name.clone(), t.delay))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_scene_swap_drops_the_event_listeners_the_old_scene_registered() {
    let mut sys = ScriptSystem::new().expect("a script system");
    sys.api.services.register_event_listener("hit".into(), 3);
    sys.api.services.register_event_listener("hit".into(), 4);
    sys.api.services.register_event_listener("scored".into(), 3);
    assert_eq!(sys.api.services.event_listeners.len(), 2, "registered");

    sys.reset_initialized();

    let left: usize = sys.api.services.event_listeners.values().map(|v| v.len()).sum();
    assert_eq!(
        left, 0,
        "{left} listener(s) from the previous scene survived. This map only ever grows, so a \
         compilation that loads fifty scenes dispatches every `send_event` to fifty scenes' worth \
         of dead registrations: {:?}",
        sys.api.services.event_listeners
    );
}

/// The accumulation, rather than the single strand. A hundred scenes is three
/// or four minutes of a microgame compilation, and is meant to leave the
/// script system exactly as empty as one scene does.
#[test]
fn a_hundred_scene_swaps_accumulate_nothing() {
    let mut sys = ScriptSystem::new().expect("a script system");
    for scene in 0..100u64 {
        // Each "scene" arms what a small game would: a countdown, a repeating
        // tick, and a couple of listeners.
        sys.api.services.set_timer(scene % 16, 2.0, "on_timeout".into());
        sys.api.services.set_timer(scene % 16 + 1, 0.25, "on_tick".into());
        sys.api.services.register_event_listener("hit".into(), scene % 16);
        sys.api.services.register_event_listener("won".into(), scene % 16);
        sys.reset_initialized();
    }
    let timers = sys.api.services.timers.len();
    let listeners: usize = sys.api.services.event_listeners.values().map(|v| v.len()).sum();
    assert_eq!(
        (timers, listeners),
        (0, 0),
        "after 100 scene loads - about four minutes of a microgame compilation - the script \
         system is holding {timers} timer(s) and {listeners} listener(s) from scenes that no \
         longer exist"
    );
}

/// The tween manager is the one thing of this shape that *was* already
/// handled, and the comment on it names exactly the reason the others need it
/// too: "Tweens name entities of the scene being replaced; the next scene
/// reuses those ids." Pinned so a refactor cannot quietly drop it.
#[test]
fn the_tween_manager_is_still_replaced_too() {
    let mut sys = ScriptSystem::new().expect("a script system");
    sys.api.services.pending_tweens.push(jengine_core::scripting::api::PendingTween {
        tween_id: 1,
        entity_id: 5,
        target: jengine_core::animation::tween::TweenTarget::Position,
        end_value: glam::Vec4::ZERO,
        duration: 1.0,
        easing: jengine_core::animation::easing::EasingFunction::Linear,
    });
    sys.reset_initialized();
    assert!(sys.api.services.pending_tweens.is_empty());
}

/// GUI click handlers are the third of the same shape: `(gui_id, node_id,
/// entity_id, callback_name)`. A menu button registered by one scene, still
/// bound after the swap, calls its handler on whatever entity inherits the id
/// the moment the next scene's GUI happens to use the same node name - and
/// node names like `btn_start` are exactly the ones every scene reuses.
#[test]
fn a_scene_swap_drops_the_gui_click_handlers_too() {
    let mut sys = ScriptSystem::new().expect("a script system");
    sys.api.services.gui_click_handlers.push(jengine_core::scripting::api::GuiClickHandler {
        gui_id: "ui/menu.gui".into(),
        node_id: "btn_start".into(),
        entity_id: 4,
        callback_name: "on_start".into(),
    });
    sys.reset_initialized();
    assert!(
        sys.api.services.gui_click_handlers.is_empty(),
        "a handler bound to `btn_start` survived into a scene that has its own `btn_start`"
    );
}

/// The other half, and the one a careless fix breaks: clearing **too much**.
///
/// A compilation of microgames has nowhere but the save store to keep its
/// score across a scene, so emptying it would silently reset the run at every
/// load; dropping the prefab cache would turn every scene into a reload; and
/// rewinding the monotonic counters would let a stale id from an old scene
/// match a freshly issued one, which is the very bug this is fixing.
#[test]
fn what_a_scene_does_not_own_survives_the_swap() {
    let mut sys = ScriptSystem::new().expect("a script system");

    sys.api.services.prefabs.insert(
        "shard".into(),
        jengine_core::scene::format::EntityData {
            children: Vec::new(),
            name: "shard".into(),
            parent: None,
            components: Default::default(),
        },
    );
    sys.api.services.next_timer_id = 4321;
    sys.api.services.next_spawn_id = 99;
    sys.api.services.last_sound_handle = 77;
    sys.api.services.debug_overlay_visible = true;

    sys.reset_initialized();

    assert!(
        sys.api.services.prefabs.contains_key("shard"),
        "prefab templates are shared between scenes; reloading them per scene is a stall"
    );
    assert_eq!(
        sys.api.services.next_timer_id, 4321,
        "rewinding the timer counter lets an id from a dead scene match a live one"
    );
    assert_eq!(sys.api.services.next_spawn_id, 99, "same for spawn ids");
    assert_eq!(sys.api.services.last_sound_handle, 77);
    assert!(sys.api.services.debug_overlay_visible, "an overlay toggle is a setting, not scene state");
}

/// Every runtime has to do this, and only one of them can be tested from here.
///
/// The native path calls `reset_initialized` in `apply_scene_json`. The
/// browser hosts keep their own copies on `window.__jengine_play`, whose
/// accessor reuses whatever object is already there - so they do **not** clear
/// any of this, and cannot be reached from a native test. That half is open
/// and recorded in `docs/parity-gaps.md`; this checks the native call site is
/// still wired, because a swap that forgets it puts the bug straight back.
#[test]
fn the_native_scene_swap_still_calls_the_reset() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let src = std::fs::read_to_string(root.join("crates/jengine-runtime/src/wgpu_scene_runner.rs"))
        .expect("the native runtime source");
    let swap = src.find("fn apply_scene_json").expect("the scene swap");
    let tail = &src[swap..];
    let end = tail.find("\nfn ").unwrap_or(tail.len());
    assert!(
        tail[..end].contains("reset_initialized()"),
        "apply_scene_json must call reset_initialized, or timers, listeners and GUI handlers \
         from the outgoing scene fire into the incoming one"
    );
}

/// The browser halves, which cannot be *run* from a native test - they need a
/// `window` - so this checks the thing that actually went wrong instead: the
/// **call**.
///
/// A missing call is invisible to any test of the function it should have
/// called, and that is not theoretical here. Twice in one day: the handler-name
/// fix went into `dispatch_event_to_listeners` while the `evt` command arm kept
/// a private copy of it, and before that `apply_physics_commands` was correct
/// and had no caller (BUGS #144). Both times the function was right and the
/// path did not use it.
#[test]
fn both_browser_hosts_clear_their_play_state_on_a_scene_swap() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let shared = include_str!("../src/scripting/bridge/adapters/js/scripts.rs");
    let at = shared.find("pub fn forget_scene()").expect("scripts::forget_scene");
    assert!(
        shared[at..].contains("clear_scene_scoped_play_state"),
        "scripts::forget_scene no longer clears the scene's timers and listeners"
    );
    for (file, func) in [
        ("crates/jengine-editor-viewport/src/lib.rs", "fn load_and_run_scripts"),
        ("crates/jengine-web/src/script_host.rs", "pub fn install_scripts"),
    ] {
        let src = std::fs::read_to_string(root.join(file))
            .unwrap_or_else(|e| panic!("{file}: {e}"));
        let at = src.find(func).unwrap_or_else(|| panic!("{file}: no {func}"));
        let tail = &src[at..];
        // The body ends at the next top-level item.
        let end = tail[1..].find("
pub fn ").map(|i| i + 1).unwrap_or(tail.len());
        assert!(
            tail[..end].contains("clear_scene_scoped_play_state")
                // Both hosts forget a scene through `scripts::forget_scene`
                // since Oct 2026, which does the clearing (checked below).
                || tail[..end].contains("scripts::forget_scene()"),
            "{file}'s {func} rebuilds the scripts for a new scene but never clears the play              state, so the outgoing scene's timers and event listeners survive into it - each              carrying an entity id the new scene reuses"
        );
    }
}

/// And the rule itself, which is shared: what a scene owns, and what must
/// outlive it. A browser host clearing too much is the same bug wearing a
/// different hat - a microgame compilation keeps its run in `save_data`.
#[test]
fn the_browser_rule_names_what_survives_as_well_as_what_does_not() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let src =
        std::fs::read_to_string(root.join("crates/jengine-core/src/scripting/timers.rs")).unwrap();
    let at = src.find("pub fn clear_scene_scoped_play_state").expect("the shared rule");
    let body = &src[at..];
    let end = body[1..].find("
pub fn ").map(|i| i + 1).unwrap_or(body.len());
    let body = &body[..end];

    for cleared in ["timers", "event_listeners", "commands"] {
        assert!(body.contains(cleared), "`{cleared}` is entity-keyed and has to go");
    }
    for kept in ["save_data", "next_timer_id", "data_tables"] {
        assert!(
            !body.contains(&format!("from_str(\"{kept}\")")),
            "`{kept}` must survive a scene: clearing it is the same bug from the other side"
        );
    }
}
