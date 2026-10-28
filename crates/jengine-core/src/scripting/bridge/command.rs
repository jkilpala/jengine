//! Typed command queue produced by scripts and consumed by the engine.
//!
//! Phase 1 ships only a minimal subset (transform + log). Subsequent phases extend the
//! enum as the API surface migrates onto the bridge (see execution plan §8).

/// A single deferred mutation produced by a script.
///
/// Commands are queued during script execution and applied to the ECS world after all
/// scripts for the frame have run. This mirrors the existing [`crate::scripting::api::ScriptCommand`]
/// model but is intentionally smaller and bridge-safe.
#[derive(Debug, Clone, PartialEq)]
pub enum BridgeCommand {
    /// A script global contributed by a package.
    ///
    /// Carried as `(tag, args)` rather than a typed variant because core
    /// cannot know what a package registered. The apply step dispatches it
    /// through `package::script_commands::handle_package_command`, which is
    /// the same function the browser hosts call — one implementation of the
    /// command, reached from every runtime.
    PackageCall { tag: String, args: serde_json::Value },
    /// Set an entity's world position.
    SetPosition { entity_id: u64, x: f32, y: f32 },
    /// Set an entity's rotation in radians.
    SetRotation { entity_id: u64, radians: f32 },
    /// Set an entity's scale.
    SetScale { entity_id: u64, x: f32, y: f32 },
    /// Overwrite an entity's [`crate::ecs::components::Health`] (`current`
    /// and `max`). A non-positive `max` leaves the existing `max` untouched
    /// (current-only writes). No-op if the entity lacks a `Health`.
    SetHealth { entity_id: u64, current: f32, max: f32 },
    /// Overwrite an entity's [`crate::ecs::components::Hitbox`] combat stats
    /// (`damage`, `knockback`, `retrigger`). No-op if the entity lacks a
    /// `Hitbox`; `active` and per-victim cooldowns are preserved.
    SetHitbox { entity_id: u64, damage: f32, knockback: f32, retrigger: f32 },
    /// Slice 3.5.A — attach `child_id` under `parent_id` in the transform
    /// hierarchy. `parent_id == None` detaches the child from any current
    /// parent so it becomes a root entity. No-ops if either entity is
    /// missing or the attachment would create a self-loop.
    SetParent { child_id: u64, parent_id: Option<u64> },
    /// Slice 3.5.C — mutate a `SkeletalPlayer` on `entity_id`. Each
    /// field is `Option`; `Some(_)` overwrites, `None` leaves it
    /// untouched. `clip = Some(String::new())` is the documented
    /// "clear clip" signal (player keeps the empty id and the system
    /// silently no-ops it). No-op if the entity lacks `SkeletalPlayer`.
    SetSkeletalPlayer {
        entity_id: u64,
        clip: Option<String>,
        time: Option<f32>,
        speed: Option<f32>,
        looping: Option<bool>,
        playing: Option<bool>,
    },
    /// Emit a log line on behalf of a script.
    Log { message: String },
    /// Spawn a new entity. `prefab` is the prefab name (or `None` for a bare entity
    /// with just a [`crate::ecs::components::Name`] + default
    /// [`crate::ecs::components::Transform`]). `spawn_id` is pre-allocated by the
    /// bridge so the calling script can reference the entity immediately.
    SpawnEntity { name: String, prefab: Option<String>, spawn_id: u64 },
    /// Slice 3.5.D — instantiate a registered [`crate::animation::rig::SkeletonRig`]
    /// from the asset manager. `spawn_id` is allocated client-side and refers to
    /// the rig's root bone; child bones are spawned with fresh hecs entities and
    /// must be reached via [`crate::ecs::skeleton::find_bone`] after the apply
    /// pass. Silent no-op if the asset manager isn't available or the rig is
    /// unknown.
    SpawnRig { rig_id: String, spawn_id: u64 },
    /// Slice 3.5.F — set the world-space target of an IK constraint
    /// (`IkTwoBone` or `IkLookAt`) on the given entity. Silent no-op
    /// if the entity carries neither component.
    SetIkTarget { entity_id: u64, x: f32, y: f32 },
    /// `crossfade_animation(e, clip, seconds)`: start a skeletal clip and
    /// fade the current pose out over `seconds`.
    CrossfadeAnimation { entity_id: u64, clip: String, seconds: f32 },
    /// `set_animation_blend(e, clip, weight)`: mix a second skeletal clip in
    /// at the same phase.
    SetAnimationBlend { entity_id: u64, clip: String, weight: f32 },
    /// `set_animation_blend_position(e, x, y)`: move a skeletal player's
    /// 2D blend space.
    SetAnimationBlendPosition { entity_id: u64, x: f32, y: f32 },
    /// `Animation.play_layer` / `stop_layer` / `set_layer_weight`: a
    /// skeletal player's layer (`SkeletalPlayer::layer_op`), by `op`.
    AnimationLayer { entity_id: u64, op: String, layer: String, clip: String, value: f32, seconds: f32, looping: bool },
    /// `set_conveyor_speed(e, speed)`: how fast a `Conveyor` slides what
    /// rests on it.
    SetConveyorSpeed { entity_id: u64, speed: f32 },
    /// `set_ik_weight(e, weight)`: how much of an `IkTwoBone` solve applies.
    SetIkWeight { entity_id: u64, weight: f32 },
    /// Destroy an entity.
    DestroyEntity { entity_id: u64 },
    /// Add a tag to an entity.
    AddTag { entity_id: u64, tag: String },
    /// Remove a tag from an entity.
    RemoveTag { entity_id: u64, tag: String },
    /// Set an entity's sprite RGBA tint (`SpriteComponent::color`).
    SetSpriteColor { entity_id: u64, r: f32, g: f32, b: f32, a: f32 },
    /// Set an entity's sprite anchor (`SpriteComponent::anchor`).
    SetSpriteAnchor { entity_id: u64, x: f32, y: f32 },
    SetSpriteSize { entity_id: u64, w: f32, h: f32 },
    SetZOrder { entity_id: u64, z: i32 },
    /// Set the colour of an entity's world text (`TextComponent::color`).
    SetTextColor { entity_id: u64, r: f32, g: f32, b: f32, a: f32 },
    SetTrailEmitting { entity_id: u64, on: bool },
    ClearTrail { entity_id: u64 },
    /// Queue a hole in a carvable sprite (`ecs::carve`), in world units.
    CarveSprite { entity_id: u64, x: f32, y: f32, radius: f32 },
    /// `paint_sprite(e, x, y, size, r, g, b, a, shape, rotation)`: a stamp in a paintable sprite;
    /// `press_sprite(e, x, y, size, depth, shape, rotation)` the same with a `depth`, a dent. `blend`
    /// (`over` / `multiply` / `erase`, empty for the component's) is paint's eleventh argument.
    PaintSprite { entity_id: u64, x: f32, y: f32, size: f32, color: [f32; 4], shape: String, rotation: f32, depth: f32, blend: String },
    /// Replace the entity's `mesh` shape (vertices, 1-based indices).
    SetMesh { entity_id: u64, vertices: serde_json::Value, indices: serde_json::Value },
    /// Set a `CharacterController2D`'s velocity (and drop-through).
    MoveCharacter { entity_id: u64, vx: f32, vy: f32, drop_through: Option<bool> },
    /// Swap an entity's sprite to a named region of a runtime-loaded atlas.
    /// Updates `SpriteComponent::texture_id` to `atlas_id` and
    /// `SpriteComponent::uv_rect` to the region's UV rect. Silently
    /// no-ops if the atlas or region is unknown to the asset manager.
    SetSpriteRegion { entity_id: u64, atlas_id: String, region_name: String },
    /// `Sprite.set_label(e, label, category)`.
    SetSpriteLabel { entity_id: u64, label: String, category: Option<String> },
    /// Draw an entity's sprite as an explicit quad given by four local-space
    /// corner offsets `[TL, TR, BR, BL]` from its position. `None` reverts to
    /// a normal rectangle. Sets `SpriteComponent::corners`.
    SetSpriteCorners { entity_id: u64, corners: Option<[[f32; 2]; 4]> },
    /// Switch the material on an entity (`MaterialComponent::material_id`).
    SetMaterial { entity_id: u64, material_id: String },
    /// Give an entity's material a normal map, or take it away (`None`) -
    /// `MaterialComponent::normal_map_id`.
    SetNormalMap { entity_id: u64, normal_map_id: Option<String> },
    /// Set a scalar uniform on an entity's material (`UniformValue::Float`).
    SetMaterialUniformFloat { entity_id: u64, name: String, value: f32 },
    /// Set a vec4 uniform on an entity's material (`UniformValue::Vec4`).
    SetMaterialUniformVec4 { entity_id: u64, name: String, value: [f32; 4] },
    /// Apply a continuous force (accumulated over the frame) to a body.
    ApplyForce { entity_id: u64, fx: f32, fy: f32 },
    /// Apply an instantaneous impulse (velocity change) to a body.
    ApplyImpulse { entity_id: u64, ix: f32, iy: f32 },
    /// Set a body's linear velocity directly.
    SetVelocity { entity_id: u64, vx: f32, vy: f32 },
    /// Set a body's angular velocity (radians per second).
    SetAngularVelocity { entity_id: u64, omega: f32 },
    /// Play a one-shot sound clip. `sound_id` is pre-allocated by the bridge
    /// so the calling script receives the handle synchronously and can later
    /// pass it to [`Self::StopSound`] / [`Self::SetSoundVolume`].
    /// `pan` is -1.0 (hard left) to 1.0 (hard right); 0.0 is centre and is
    /// what `play_sound` sends, so the common call is unchanged.
    /// `looping` repeats it until `stop_sound(sound_id)` - an engine note,
    /// a crackling fire a script owns.
    PlaySound { clip_id: String, volume: f32, pitch: f32, pan: f32, sound_id: u64, looping: bool },
    /// Start (or restart) the music track. Replaces any currently playing
    /// music — the legacy `EngineApi::play_music` returns no handle.
    PlayMusic { clip_id: String, volume: f32, looping: bool },
    /// Stop a sound previously started with [`Self::PlaySound`].
    StopSound { handle_id: u64 },
    /// Stop every playing instance of one clip, by asset path.
    StopSoundByClip { clip_id: String },
    /// Stop the currently playing music track.
    StopMusic,
    /// Pause the currently playing music track (resumable).
    PauseMusic,
    /// Resume a paused music track.
    ResumeMusic,
    /// Set the volume of an in-flight sound handle.
    SetSoundVolume { handle_id: u64, volume: f32 },
    /// Set the global music bus volume (0.0..=1.0).
    SetMusicVolume { volume: f32 },
    /// Set the global master volume (0.0..=1.0).
    SetMasterVolume { volume: f32 },
    /// Stop every currently playing sound (including music).
    StopAllSounds,
    /// Pause every currently playing sound (including music).
    PauseAllSounds,
    /// Resume every currently paused sound (including music).
    ResumeAllSounds,
    /// Play a named sprite-animation clip on an entity.
    PlayAnimation { entity_id: u64, clip_name: String },
    /// Stop the entity's currently playing sprite animation.
    StopAnimation { entity_id: u64 },
    /// Override the playback speed multiplier for an entity's sprite animator.
    SetAnimationSpeed { entity_id: u64, speed: f32 },
    /// Set a boolean parameter on the entity's animation state machine.
    SetAnimBool { entity_id: u64, param: String, value: bool },
    /// Set a float parameter on the entity's animation state machine.
    SetAnimFloat { entity_id: u64, param: String, value: f32 },
    /// Fire a one-shot trigger on the entity's animation state machine.
    SetAnimTrigger { entity_id: u64, param: String },
    /// Reset the entity's animation state machine to a named state.
    StartStateMachine { entity_id: u64, state_name: String },
    /// Begin an interpolation tween. `target` is one of "position", "rotation",
    /// "scale", "sprite_color", or "sprite_alpha"; `easing` is any name
    /// understood by [`crate::animation::easing::EasingFunction::from_name`].
    /// `tween_id` is pre-allocated by the bridge so the script can reference
    /// the tween in the same frame for cancellation.
    StartTween {
        entity_id: u64,
        target: String,
        end_value: [f32; 4],
        duration: f32,
        easing: String,
        tween_id: u32,
    },
    /// Cancel a single in-flight tween by id.
    CancelTween { tween_id: u32 },
    /// Cancel every in-flight tween belonging to an entity.
    CancelAllTweens { entity_id: u64 },
    /// Replace the active scene with the one at `path`.
    LoadScene { path: String },
    QuitGame,
    SetTimeScale { scale: f32 },
    /// Set a `UniformValue::Vec2` uniform on an entity's material.
    SetMaterialUniformVec2 {
        entity_id: u64,
        name: String,
        value: [f32; 2],
    },
    /// Set a `UniformValue::Int` uniform on an entity's material.
    SetMaterialUniformInt {
        entity_id: u64,
        name: String,
        value: i32,
    },
    // ---- physics writes ---------------------------------------------------
    // `ScriptCommand` has carried these since the pre-bridge API; what was
    // missing is a bridge path, which is why the functions existed as
    // metadata in `core_script_api.rs` and were `nil` in Lua on native.
    /// Set world gravity.
    SetGravity { gx: f32, gy: f32 },
    /// Change a body's type: "dynamic", "kinematic" or "static".
    SetBodyType { entity_id: u64, body_type: String },
    /// Attach a joint between two named entities. `joint_json` is the same
    /// payload the scene format uses.
    AddJoint {
        entity_name: String,
        target_name: String,
        joint_json: String,
    },
    /// Remove the joint owned by a named entity.
    RemoveJoint { entity_name: String },
    /// Drive a joint motor on a named entity.
    SetMotorSpeed { entity_name: String, speed: f32 },
    /// Push the current scene onto the stack and switch to `path` (pause-menu pattern).
    PushScene { path: String },
    /// Pop the scene stack, returning to the previously-pushed scene.
    PopScene,
    /// Slice 12.A — start a fade-from-color transition (overlay starts opaque,
    /// fades to clear over `duration_s`). `color` is `[r, g, b]` in 0..=1.
    FadeIn { duration_s: f32, color: [f32; 3] },
    /// Slice 12.A — start a fade-to-color transition (overlay starts clear,
    /// fades to opaque over `duration_s`). `color` is `[r, g, b]`.
    FadeOut { duration_s: f32, color: [f32; 3] },
    /// P1.3 — fullscreen screen-flash overlay. `color.a` is the *initial*
    /// opacity (decays linearly to 0 over `duration_s`); RGB is the tint.
    ScreenFlash { duration_s: f32, color: [f32; 4] },
    /// Dynamically attach a script to an entity. The script is loaded from `path`,
    /// transpiled if Lua, compiled, and attached as a `ScriptComponent`. The script's
    /// `init()` is called on the next frame when the component is processed.
    LoadScript { entity_id: u64, path: String },
    // ---- Phase 5.8: events & timers ----------------------------------------
    /// Fire a named event. `data` is an optional plain-string payload (matches
    /// the package-API surface where `data` is registered as `String`).
    SendEvent { name: String, sender: u64, data: Option<String> },
    /// Register `entity_id` as a listener for `event_name`. `listener_id` is
    /// pre-allocated by the bridge so the calling script can store it before
    /// the legacy engine actually wires it up.
    RegisterEventListener { event_name: String, entity_id: u64, listener_id: u32 },
    /// Remove every listener entry for `(event_name, entity_id)`.
    RemoveEventListener { event_name: String, entity_id: u64 },
    /// Schedule a timer callback. `repeating == false` is a one-shot,
    /// `true` is an interval. `timer_id` is pre-allocated bridge-side.
    SetTimer {
        entity_id: u64,
        delay: f32,
        callback_name: String,
        repeating: bool,
        timer_id: u32,
    },
    /// Cancel a previously-scheduled timer by id.
    CancelTimer { timer_id: u32 },
    // ---- Phase 5.9: UI elements --------------------------------------------
    /// Create a HUD text element. `entity_id` is pre-allocated by the bridge
    /// (shares the spawn counter so ids stay contiguous with `spawn_entity`).
    /// `anchor` is the lowercase anchor name (parsed via `UIAnchor::from_name`).
    /// `color` (RGBA), `font_size`, and `alignment` ("left" / "center" /
    /// "right") default to white / 16.0 / left when scripts omit them.
    CreateUIText {
        text: String,
        anchor: String,
        offset_x: f32,
        offset_y: f32,
        color: [f32; 4],
        font_size: f32,
        alignment: String,
        entity_id: u64,
    },
    /// Create a HUD coloured panel. Same pre-allocation rules as `CreateUIText`.
    CreateUIPanel {
        anchor: String,
        offset_x: f32,
        offset_y: f32,
        width: f32,
        height: f32,
        bg_color: [f32; 4],
        entity_id: u64,
    },
    /// Update text of a previously-created UI text node.
    SetUIText { entity_id: u64, text: String },
    /// Set one property on an entity-UI widget. `value` is raw JSON so a
    /// single command covers text, numbers, flags and colors — the widget
    /// fields have no common type, and one command per field would mean eight
    /// more globals to keep in parity across four runtimes.
    SetUiProp { entity_id: u64, property: String, value: serde_json::Value },
    /// Update the `text` field on a `TextComponent` entity. Falls back to
    /// `UIText` if the target entity has no `TextComponent` (so scripts may
    /// safely call `set_text` on either).
    SetText { entity_id: u64, text: String },
    /// Show / hide a UI element.
    SetUIVisible { entity_id: u64, visible: bool },
    /// Re-anchor an existing UI element.
    SetUIPosition { entity_id: u64, anchor: String, offset_x: f32, offset_y: f32 },
    // ---- Phase 5.9: GUI document API ---------------------------------------
    /// Show a registered GUI document.
    GuiShow { gui_id: String },
    /// Hide a registered GUI document.
    GuiHide { gui_id: String },
    /// Mutate a GUI node property. `value` is an opaque string (matches the
    /// `package_api` surface where this is registered as `String`); host
    /// translates to `serde_json::Value::String` for the legacy path.
    GuiSetNodeProp { gui_id: String, node_id: String, property: String, value: String },
    /// Register a GUI click callback on `(gui_id, node_id)` for `entity_id`.
    GuiOnClick { entity_id: u64, gui_id: String, node_id: String, callback_name: String },
    /// Focus a GUI node (pass `node_id == ""` to clear focus).
    GuiFocus { gui_id: String, node_id: String },
    /// Emit a one-shot burst of `count` particles from an entity's emitter.
    EmitBurst { entity_id: u64, count: u32 },
    /// Enable or disable continuous emission on an emitter.
    SetEmitterActive { entity_id: u64, active: bool },
    /// Set the emission rate (particles per second) on an emitter.
    SetEmitterRate { entity_id: u64, rate: f32 },
    /// Write any emitter field by name: `set_emitter_field(e, field, value)`.
    SetEmitterField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any cloth or rope field by name: `set_cloth_field(e, field, value)`.
    SetClothField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `path_follow` field by name: `set_path_follow_field(e, field, value)`.
    SetPathFollowField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `line` field by name: `set_line_field(e, field, value)`.
    SetLineField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `trail` field by name: `set_trail_field(e, field, value)`.
    SetTrailField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `fluid` field by name: `set_fluid_field(e, field, value)`.
    SetFluidField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `mirror` field by name: `set_mirror_field(e, field, value)`.
    SetMirrorField { entity_id: u64, field: String, value: serde_json::Value },
    /// Dig or build a smooth terrain: `smooth_terrain_edit(e, x, y, radius, amount)`.
    TerrainEdit { entity_id: u64, x: f32, y: f32, radius: f32, amount: f32 },
    /// Drive a conversation: `dialogue_start` / `_advance` / `_choose` / `_stop`.
    DialogueOp { entity_id: u64, op: String, arg: String },
    /// Put an item in a slot: `ui_drop(item, slot)`.
    UiDrop { entity_id: u64, slot: String },
    /// Stand a Mode 7 sprite: `mode7_place(e, x, y, angle)`.
    Mode7Place { entity_id: u64, x: f32, y: f32, angle: f32 },
    /// Any Mode 7 field by name: `mode7_set(e, field, value)`.
    Mode7Set { entity_id: u64, field: String, value: serde_json::Value },
    /// Stand a stage billboard, move a quad or point a stage: `stage_place(e, x, y, z)`.
    StagePlace { entity_id: u64, x: f32, y: f32, z: f32 },
    /// Any stage field by name: `stage_set(e, field, value)`.
    StageSet { entity_id: u64, field: String, value: serde_json::Value },
    /// Any sprite tilt field by name: `sprite_tilt_set(e, field, value)`.
    SpriteTiltSet { entity_id: u64, field: String, value: serde_json::Value },
    /// Any afterimage field by name: `afterimage_set(e, field, value)`.
    AfterimageSet { entity_id: u64, field: String, value: serde_json::Value },
    /// Cut what a segment crosses: `physics_slice(x0, y0, x1, y1)`.
    PhysicsSlice { x0: f32, y0: f32, x1: f32, y1: f32 },
    /// Any portal field by name: `portal_set(e, field, value)`.
    PortalSet { entity_id: u64, field: String, value: serde_json::Value },
    /// Poke a jelly: `jelly_poke(e, x, y, strength, radius)`.
    JellyPoke { entity_id: u64, x: f32, y: f32, strength: f32, radius: f32 },
    /// Any jelly field by name: `jelly_set(e, field, value)`.
    JellySet { entity_id: u64, field: String, value: serde_json::Value },
    /// Any off-screen marker field by name: `marker_set(e, field, value)`.
    MarkerSet { entity_id: u64, field: String, value: serde_json::Value },
    /// Any radial menu field by name: `radial_set(e, field, value)`.
    RadialSet { entity_id: u64, field: String, value: serde_json::Value },
    /// Say something in a bubble: `bubble_say(e, text, seconds)`.
    BubbleSay { entity_id: u64, text: String, seconds: f32 },
    /// Any speech bubble field by name: `bubble_set(e, field, value)`.
    BubbleSet { entity_id: u64, field: String, value: serde_json::Value },
    /// Any sprite outline field by name: `outline_set(e, field, value)`.
    OutlineSet { entity_id: u64, field: String, value: serde_json::Value },
    /// Strike lightning now: `lightning_strike(e, x, y)`.
    LightningStrike { entity_id: u64, x: f32, y: f32 },
    /// Any lightning field by name: `lightning_set(e, field, value)`.
    LightningSet { entity_id: u64, field: String, value: serde_json::Value },
    /// Any flow path field by name: `flow_set(e, field, value)`.
    FlowSet { entity_id: u64, field: String, value: serde_json::Value },
    /// Any navigation link field by name: `nav_link_set(e, field, value)`.
    NavLinkSet { entity_id: u64, field: String, value: serde_json::Value },
    /// Morph a path into another's shape: `morph_to(e, target, seconds)`.
    MorphTo { entity_id: u64, target: String, seconds: f32 },
    /// Any path morph field by name: `morph_set(e, field, value)`.
    MorphSet { entity_id: u64, field: String, value: serde_json::Value },
    /// Queue a toast: `toast_show(e, title, text, icon, seconds)`.
    ToastShow { entity_id: u64, title: String, text: String, icon: String, seconds: f32 },
    /// Any toast stack field by name: `toast_set(e, field, value)`.
    ToastSet { entity_id: u64, field: String, value: serde_json::Value },
    /// Any snow cover field by name: `snow_set(e, field, value)`.
    SnowSet { entity_id: u64, field: String, value: serde_json::Value },
    /// The map unseen again: `fog_of_war_reset(e)`.
    FogOfWarReset { entity_id: u64 },
    /// Any fog of war field by name: `fog_of_war_set(e, field, value)`.
    FogOfWarSet { entity_id: u64, field: String, value: serde_json::Value },
    /// Any beat_clock field by name: `set_beat_clock_field(e, field, value)`.
    SetBeatClockField { entity_id: u64, field: String, value: serde_json::Value },
    /// Any text_path field by name: `set_text_path_field(e, field, value)`.
    SetTextPathField { entity_id: u64, field: String, value: serde_json::Value },
    /// Any trajectory field by name: `set_trajectory_field(e, field, value)`.
    SetTrajectoryField { entity_id: u64, field: String, value: serde_json::Value },
    /// Any tile_light / tile_light_source field by name: `set_tile_light_field(e, field, value)`.
    SetTileLightField { entity_id: u64, field: String, value: serde_json::Value },
    /// Any auto_normal field by name: `set_auto_normal_field(e, field, value)`.
    SetAutoNormalField { entity_id: u64, field: String, value: serde_json::Value },
    /// Any cast_shadow field by name: `set_cast_shadow_field(e, field, value)`.
    SetCastShadowField { entity_id: u64, field: String, value: serde_json::Value },
    /// An entity's own pace: `set_time_scale_field(e, field, value)`.
    SetTimeScaleField { entity_id: u64, field: String, value: serde_json::Value },
    /// A floating_origin field by name: `set_origin_field(e, field, value)`.
    SetOriginField { entity_id: u64, field: String, value: serde_json::Value },
    /// Any day_cycle field by name: `set_day_field(e, field, value)`.
    SetDayField { entity_id: u64, field: String, value: serde_json::Value },
    /// Any vision field by name: `set_vision_field(e, field, value)`.
    SetVisionField { entity_id: u64, field: String, value: serde_json::Value },
    /// A region of a wfc generator's layer to fill: `wfc_generate(e, x, y, w, h)`.
    WfcGenerate { entity_id: u64, x: i32, y: i32, w: i32, h: i32 },
    /// Any fog field by name: `set_fog_field(e, field, value)`.
    SetFogField { entity_id: u64, field: String, value: serde_json::Value },
    /// A rewind clock's mode or speed: `set_rewind_field(e, field, value)`.
    SetRewindField { entity_id: u64, field: String, value: serde_json::Value },
    /// Smoke and a push at a world point: `smoke_puff(e, x, y, r, amount, vx, vy)`.
    SmokePuff { entity_id: u64, x: f32, y: f32, radius: f32, amount: f32, vx: f32, vy: f32 },
    /// Any smoke field by name: `set_smoke_field(e, field, value)`.
    SetSmokeField { entity_id: u64, field: String, value: serde_json::Value },
    /// Paint a material into a pixel simulation: `pixel_paint(e, x, y, r, m)`.
    PixelPaint { entity_id: u64, x: f32, y: f32, radius: f32, material: String },
    /// How much of a bar or radial fill is drawn: `set_sprite_fill(e, amount)`.
    SetSpriteFill { entity_id: u64, amount: f32 },
    /// Write any `water_surface` field by name: `set_water_field(e, field, value)`.
    SetWaterField { entity_id: u64, field: String, value: serde_json::Value },
    /// Push a water's surface: `water_splash(e, x, strength)`.
    WaterSplash { entity_id: u64, x: f32, strength: f32 },
    /// Write any `canvas_group` field by name: `set_canvas_group_field(e, field, value)`.
    SetCanvasGroupField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `video_player` field by name: `set_video_field(e, field, value)`.
    SetVideoField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `ragdoll` field by name: `set_ragdoll_field(e, field, value)`.
    SetRagdollField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `timeline` field by name: `set_timeline_field(e, field, value)`.
    SetTimelineField { entity_id: u64, field: String, value: serde_json::Value },
    /// `BehaviorTree.set(e, field, value)`: root, enabled, loop or restart.
    SetBehaviorTreeField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any area-effect field by name: `set_area_field(e, field, value)`.
    SetAreaField { entity_id: u64, field: String, value: serde_json::Value },
    /// `Camera.set(e, field, value)`.
    SetCameraField { entity_id: u64, field: String, value: serde_json::Value },
    /// `Camera.set_follow(e, field, value)`.
    SetCameraFollow { entity_id: u64, field: String, value: serde_json::Value },
    /// `Camera.set_screen_rect(e, x, y, w, h)`.
    SetCameraScreenRect { entity_id: u64, rect: [f32; 4] },
    /// `Nav.set_target(e, x, y)` / `Nav.stop(e)` (`None`).
    SetNavTarget { entity_id: u64, target: Option<[f32; 2]> },
    /// `Nav.set(e, field, value)`.
    SetNavField { entity_id: u64, field: String, value: serde_json::Value },
    /// Set or clear a tile on a tilemap layer. `tile_id == None` clears the tile.
    SetTile { entity_id: u64, layer_name: String, x: u32, y: u32, tile_id: Option<u32> },
    /// Set the RGB color of a 2D point light.
    SetLightColor { entity_id: u64, r: f32, g: f32, b: f32 },
    /// Set the intensity (brightness multiplier) of a 2D point light.
    SetLightIntensity { entity_id: u64, intensity: f32 },
    SetLightHeight { entity_id: u64, height: f32 },
    SetLightMask { entity_id: u64, mask: u32 },
    SetColorLut { texture_id: String, strength: f32 },
    /// Set the radius of a 2D point light.
    SetLightRadius { entity_id: u64, radius: f32 },
    /// Set the global ambient light (RGB + intensity).
    SetAmbientLight { r: f32, g: f32, b: f32, intensity: f32 },
    /// The scene's toon lighting: bands (0 removes it), softness, dither cell.
    SetToonLighting { bands: f32, softness: Option<f32>, dither: Option<f32> },
    /// The scene's ambient occlusion: radius, strength (0 removes it).
    SetAmbientOcclusion { radius: Option<f32>, strength: Option<f32> },
    /// The scene's global illumination: strength (0 removes it), scale.
    SetGlobalIllumination { strength: Option<f32>, scale: Option<f32> },
    /// Toggle a named post-process effect on or off.
    SetPostProcessEnabled { effect_name: String, enabled: bool },
    /// Set a `UniformValue::Float` parameter on a post-process effect.
    SetPostProcessFloat { effect_name: String, param_name: String, value: f32 },
    /// Set a `UniformValue::Vec4` parameter on a post-process effect.
    SetPostProcessVec4 { effect_name: String, param_name: String, value: [f32; 4] },
    /// Queue a one-frame debug line draw. Bridge-only — has no matching
    /// [`crate::scripting::api::ScriptCommand`] variant; the eventual host
    /// runner pushes onto `EngineApi::debug_draws` directly.
    DrawDebugLine { x1: f32, y1: f32, x2: f32, y2: f32, r: f32, g: f32, b: f32 },
    /// Queue a one-frame debug rectangle outline. Bridge-only (see `DrawDebugLine`).
    DrawDebugRect { x: f32, y: f32, w: f32, h: f32, r: f32, g: f32, b: f32 },
    /// Queue a one-frame debug circle outline. Bridge-only (see `DrawDebugLine`).
    DrawDebugCircle { x: f32, y: f32, radius: f32, r: f32, g: f32, b: f32 },
    /// Toggle the debug overlay visibility. Bridge-only — the host runner
    /// mutates `EngineApi::debug_overlay_visible` directly; no `ScriptCommand`.
    SetDebugOverlay { visible: bool },
    /// Trigger a screen-shake effect on the active camera. Bridge-only —
    /// applied by `route_bridge_commands` to
    /// `ScriptServices::pending_camera_shakes`; the runtime drains that
    /// queue once per frame and calls
    /// [`crate::renderer::camera::Camera2D::start_shake`].
    CameraShake { amplitude: f32, duration: f32, frequency: f32 },
    /// `set_camera_zoom(zoom)` - the active camera's zoom, 1.0 being the
    /// authored framing. The browser hosts reach the same code through the
    /// shared `scz` world command.
    SetCameraZoom { zoom: f32 },
    SetCameraPixelSize { size: f32 },
    // ---- Phase 5.13: custom component writes -------------------------------
    /// Mutate one field of a `CustomComponents` entry. `value` is
    /// `serde_json::Value` to match the legacy
    /// [`crate::scripting::api::ScriptCommand::SetCustomField`] surface 1:1
    /// (translator is a direct copy). Adapters convert their native value
    /// into this JSON representation at the boundary.
    SetCustomField {
        entity_id: u64,
        component_name: std::sync::Arc<str>,
        field_name: std::sync::Arc<str>,
        value: serde_json::Value,
    },
    /// Batched form of [`Self::SetCustomField`] — write N fields on a single
    /// component in one command. Hot path: scripts that mutate multiple
    /// fields per frame (e.g. `velocity_x`/`velocity_y` movers) avoid both
    /// the bridge crossing per call AND the per-call command alloc/drop.
    /// `fields` is `(field_name, value)` pairs. Field/component names go
    /// through [`crate::scripting::intern::intern`] at the adapter boundary
    /// so the steady-state cost is one `Arc::clone` per call.
    SetCustomFields {
        entity_id: u64,
        component_name: std::sync::Arc<str>,
        fields: Vec<(std::sync::Arc<str>, serde_json::Value)>,
    },
    /// Slice A — package-component field write. Resolves the component by
    /// `component_name` against the global [`crate::package::component_registry::ComponentRegistry`],
    /// patches `field_name` to `value` (using `serde_json` round-trip on the
    /// owning component's Serialize impl), then writes it back onto the
    /// entity. Unlike [`Self::SetCustomField`] which targets the dynamic
    /// `CustomComponents` bag, this variant works for any registered
    /// strongly-typed component, including ones contributed by user
    /// packages.
    SetComponentField {
        entity_id: u64,
        component_name: std::sync::Arc<str>,
        field_name: std::sync::Arc<str>,
        value: serde_json::Value,
    },
    // ---- Phase C: targeted cross-script communication ---------------------
    /// Send a named event to a single recipient entity. Same payload shape as
    /// [`Self::SendEvent`] but only the `target` entity's listener fires.
    /// Translator drops this — runner-side dispatch is bridge-only.
    SendEventTo {
        name: String,
        sender: u64,
        target: u64,
        data: Option<String>,
    },
    /// Synchronously invoke a named callback on another entity's script.
    /// `args` carries primitive values (number/string/bool/nil) marshalled
    /// through the bridge `Value` enum. Translator drops this; the runner
    /// resolves `target → ScriptHandle` and dispatches via `named_callback`
    /// with depth-cap protection.
    CallScriptMethod {
        caller: u64,
        target: u64,
        method: String,
        args: Vec<crate::scripting::bridge::value::Value>,
    },
    // ---- 2.3: save game system --------------------------------------------
    /// Write `value` under `(slot, key)` in the live save store. Buffered —
    /// does not touch disk / localStorage until a matching [`Self::SaveCommit`]
    /// for the same slot is queued. Bridge-only (no `ScriptCommand` analogue);
    /// `apply_bridge_commands` mutates `ScriptSystem::save_system` directly.
    SaveSet { slot: String, key: String, value: serde_json::Value },
    /// Flush a save slot to storage. On native this writes the file; on WASM
    /// it writes to `localStorage`. Bridge-only.
    SaveCommit { slot: String },
    /// Drop every key for a slot (in memory and on storage on the next
    /// [`Self::SaveCommit`]). Bridge-only.
    SaveClear { slot: String },
    // ---- 2.4: audio mixer busses ------------------------------------------
    /// Create a new mixer bus. `parent` of `None` makes a root-level bus;
    /// the default tree already contains `master` / `sfx` / `music`.
    /// Silently no-ops if the bus already exists or the parent is unknown.
    /// Bridge-only.
    AudioCreateBus { name: String, parent: Option<String>, volume: f32 },
    /// Remove a custom bus. Refuses to remove `master`. Children are
    /// re-parented to the removed bus's parent; in-flight tracks on the
    /// removed bus fall back to `master`. Bridge-only.
    AudioRemoveBus { name: String },
    /// Update a bus's linear volume (`0.0..=1.0`). Marks the subtree
    /// dirty so the next [`crate::ecs::systems::apply_audio_commands`]
    /// frame re-applies effective volume on in-flight tracks. Bridge-only.
    AudioSetBusVolume { name: String, volume: f32 },
    /// `Audio.set_bus_effect(bus, effect, value)`: a bus's `lowpass`,
    /// `reverb` or `room` (`audio::BusEffects`). Bridge-only.
    AudioSetBusEffect { name: String, effect: String, value: f32 },
    /// Mute/un-mute a bus. Mute silences the bus and every descendant.
    /// Bridge-only.
    AudioSetBusMuted { name: String, muted: bool },
    /// Set the solo flag on a bus. Any solo bus on the tree triggers
    /// solo-isolate: only the solo chain plays. Bridge-only.
    AudioSetBusSolo { name: String, solo: bool },
    // ---- 2.5: input action remapping --------------------------------------
    /// Replace every binding for `action` with `keys`. Creates the action
    /// if it didn't exist. Bridge-only; mutates the runtime
    /// [`crate::input::InputState`] directly.
    InputSetBindings { action: String, keys: Vec<String> },
    /// Append `key` to `action`'s binding list (idempotent). Bridge-only.
    InputAddBinding { action: String, key: String },
    /// Remove `key` from `action`'s binding list. Bridge-only.
    InputRemoveBinding { action: String, key: String },
    /// Clear every key bound to `action` (action still exists, unbound).
    /// Bridge-only.
    InputClearBindings { action: String },
    /// Restore every action to the bindings supplied to the most recent
    /// `load_actions` (the `.input` asset or `project.jengine`). Actions
    /// introduced after load are dropped. Bridge-only.
    InputResetBindings,
    /// Update the per-action dead-zone (reserved for gamepad axes;
    /// keyboard ignores). Bridge-only.
    InputSetDeadZone { action: String, value: f32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_value_equal() {
        let a = BridgeCommand::SetPosition { entity_id: 1, x: 0.0, y: 0.0 };
        let b = BridgeCommand::SetPosition { entity_id: 1, x: 0.0, y: 0.0 };
        assert_eq!(a, b);
    }
}
