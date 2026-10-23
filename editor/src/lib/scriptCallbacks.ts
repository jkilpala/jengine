/**
 * The functions a script defines and the engine calls: lifecycle hooks and
 * callbacks.
 *
 * Hand-written, because a callback is a name the engine looks up rather than
 * a function it installs, so there is no table to generate from. It is held
 * to the engine both ways by `editor/scripts/script-api/scriptApi.test.ts`:
 * every name here must be one the runtime source actually calls (its string
 * literal, or `evidence` when the name is put together from an event), and
 * every `ecs::callbacks::push(entity, "on_...")` in core must be listed here.
 */

export interface ScriptCallback {
  name: string;
  /** `(entity, dt)`. */
  args: string;
  info: string;
  /**
   * What to look for in the runtime source when the name itself is not a
   * literal there - `on_ui_click` is `on_` + the `"ui_click"` event.
   */
  evidence?: string;
}

/** A callback queued for an entity's own script by `ecs::callbacks::push`. */
function ownCallback(name: string, info: string): ScriptCallback {
  return { name, args: "(entity)", info: `${info} Called on the entity's own script.` };
}

export const SCRIPT_CALLBACKS: ScriptCallback[] = [
  // Lifecycle
  { name: "init", args: "(entity)", info: "Called once when the script is attached or the scene loads." },
  { name: "update", args: "(entity, dt)", info: "Called every frame; dt is the frame time in seconds." },
  {
    name: "fixed_update",
    args: "(entity, dt)",
    info: "Called before each physics step; dt is the fixed step (Lua scripts only).",
  },
  {
    name: "on_spawn",
    args: "(entity)",
    info: "Once, the frame after init, when every other entity exists. Opt in with script = { events = { \"on_spawn\" } }.",
  },
  {
    name: "on_despawn",
    args: "(entity)",
    info: "Once, when the entity leaves the world - destroyed, expired, or the scene replaced. Opt in through script.events.",
  },
  // Physics and triggers
  {
    name: "on_collision_enter",
    args: "(entity, other)",
    info: "A contact began. Opt in through script.events; the payload carries the impulse and the contact point.",
  },
  { name: "on_collision_exit", args: "(entity, other)", info: "A contact ended. Opt in through script.events." },
  { name: "on_trigger", args: "(entity)", info: "A TriggerArea2D's overlaps changed. Opt in through script.events." },
  { name: "on_trigger_enter", args: "(entity, data)", info: "Something entered this trigger area; data.other is what." },
  { name: "on_trigger_exit", args: "(entity, data)", info: "Something left this trigger area; data.other is what." },
  { name: "on_entered_trigger", args: "(entity, data)", info: "This entity entered a trigger area; data.trigger is which." },
  { name: "on_exited_trigger", args: "(entity, data)", info: "This entity left a trigger area; data.trigger is which." },
  // Events and UI
  {
    name: "on_event",
    args: "(entity, name, sender, data)",
    info: "Any event this entity subscribed to with Event.on; on_<name>(entity, data) wins when both are defined.",
  },
  {
    name: "on_animation_event",
    args: "(entity, event)",
    info: "An animation frame's EmitEvent trigger fired; event.name and event.data.",
  },
  {
    name: "on_ui_click",
    args: "(entity, data)",
    info: "An entity-UI button was clicked (on release); data.button is its id.",
    evidence: '"ui_click"',
  },
  {
    name: "on_gui_click",
    args: "(entity, node_id)",
    info: "The default handler for a .gui button registered with GUI.on_click.",
  },
  // Callbacks queued by components (ecs::callbacks)
  ownCallback("on_screen_entered", "A visibility_notifier came into view."),
  ownCallback("on_screen_exited", "A visibility_notifier left the view."),
  ownCallback("on_path_end", "A path_follow set to `once` reached the end of its path."),
  ownCallback("on_layer_end", "A one-shot skeletal animation layer finished."),
  ownCallback("on_timeline_end", "A timeline finished playing."),
  ownCallback("on_video_end", "A non-looping video_player reached its end."),
  ownCallback("on_joint_break", "A joint with a break_force or break_torque snapped."),
  ownCallback("on_seen", "A vision cone began to see one of its targets."),
  ownCallback("on_lost", "A vision cone lost sight of one of its targets."),
  ownCallback("on_dawn", "A day_cycle reached sunrise."),
  ownCallback("on_dusk", "A day_cycle reached sunset."),
  ownCallback("on_origin_shift", "A floating_origin moved the world back toward the origin."),
  ownCallback("on_wfc_done", "A wave-function-collapse region finished filling."),
  ownCallback("on_nav_link", "A nav_agent started crossing a nav_link."),
  ownCallback("on_morph_end", "A path_morph finished."),
  ownCallback("on_toast", "A toast_stack showed a toast."),
  ownCallback("on_radial_pick", "A ui_radial_menu picked an item."),
  ownCallback("on_beat", "A beat_clock reached a beat."),
  ownCallback("on_bar", "A beat_clock reached the first beat of a bar."),
  ownCallback("on_strike", "A lightning bolt struck."),
  ownCallback("on_dialogue_line", "A dialogue showed a new line."),
  ownCallback("on_dialogue_end", "A dialogue ended."),
  ownCallback("on_ui_drop", "A ui_draggable item was dropped."),
  ownCallback("on_tree_end", "A behavior_tree that does not loop finished."),
];
