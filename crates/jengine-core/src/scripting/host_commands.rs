//! Script commands that need nothing but the ECS world, implemented once.
//!
//! # Why this exists
//!
//! The editor viewport and the web runtime each dispatch the browser preamble's
//! 87 command tags with their own `match`, hand-copied between files of 7.3k
//! and 3.3k lines. Adding a script function means editing both, and forgetting
//! one is silent — the command simply does nothing in the runtime you missed.
//! That is the single largest source of bugs in this repo
//! (`docs/architecture-debt.md` #2).
//!
//! `tests/host_command_parity.rs` already stops the two drifting *apart*, by
//! asserting they dispatch the same tag set. What it cannot see is behaviour:
//! two arms with the same tag doing different things pass it happily.
//!
//! # Why this is not just tidying
//!
//! An earlier attempt to extract these arms was backed out, on the grounds that
//! the remaining value was tidiness and tidiness is not worth a silent
//! regression in code no test exercises. The important half of that sentence is
//! **"no test exercises"** — and the reason none does is that the arms live in
//! `wasm_bindgen` functions, take a `js_sys::Object`, and only run in a browser.
//!
//! So the arms are not moved as-is. They are moved behind [`CommandArgs`],
//! which is the only thing they needed a JS object for: reading a few named
//! fields. That leaves the actual behaviour as ordinary Rust operating on a
//! `hecs::World`, which a native test can call directly — the first coverage
//! these commands have ever had. The hosts keep a ~20-line adapter each.
//!
//! # What can and cannot move here
//!
//! Only arms that need nothing but the world. In particular a host's script
//! entity table (`SCRIPT_ENTITY_MAP`) is per-host state, and it hides inside
//! the innocuous-looking `find_entity_by_id`, so [`ScriptWorldHost`] takes the
//! resolver from the host rather than scanning here.
//!
//! Arms needing audio, tweens, the save system or a scene stack stay in the
//! hosts. So do the ones whose two versions genuinely differ — each of those
//! needs a decision about which behaviour is right before it can be shared, and
//! that decision is not a refactor.

use hecs::{Entity, World};

use crate::animation::skeletal::SkeletalPlayer;
use crate::animation::{AnimStateMachine, SpriteAnimator};
use crate::ecs::components::{
    AmbientLight, GuiComponent, Health, Hitbox, MaterialComponent, Parent,
    SpriteComponent, TextComponent, Transform,
};
use crate::renderer::material::UniformValue;
use crate::particles::ParticleEmitter;
use crate::ui::{UIAnchor, UIElement, UIText, apply_ui_prop};

/// Named fields of one command, however the host happens to carry them.
///
/// The browser hosts back this with a `js_sys::Object`; a test backs it with
/// `serde_json::Value` (see [`JsonArgs`]). Abstracting it is what lets the
/// command bodies below be plain Rust rather than wasm-only code.
///
/// Every accessor returns an `Option` rather than erroring: a missing field is
/// normal — the preamble omits optional arguments — and every arm has a
/// documented default for one.
pub trait CommandArgs {
    /// A numeric field, as the script passed it.
    fn number(&self, key: &str) -> Option<f64>;
    /// A string field.
    fn text(&self, key: &str) -> Option<String>;
    /// A boolean field.
    fn flag(&self, key: &str) -> Option<bool>;
    /// A field of no fixed shape, as JSON. `Null` when absent.
    fn json(&self, key: &str) -> serde_json::Value;
}

/// What a runtime must lend the shared commands.
///
/// Two methods, and the second is the interesting one. Script code addresses
/// entities by a `u32` id, and each host keeps its own table mapping those to
/// live `hecs::Entity` values (with a world scan as the fallback when an id is
/// not in the table). That table is host state, so resolution has to come from
/// the host — an extracted function that scanned the world itself would
/// silently disagree with the host on any id the table had remapped.
pub trait ScriptWorldHost {
    /// The live world the commands act on.
    fn world(&mut self) -> &mut World;

    /// Resolve a script-side entity id, or `None` if it names nothing live.
    fn resolve_entity(&mut self, id: u32) -> Option<Entity>;
}

/// [`CommandArgs`] over a JSON object — for tests, and for any host that
/// already has its commands as `serde_json`.
///
/// Public because it is what makes these commands testable, which is the point
/// of the whole module.
pub struct JsonArgs(pub serde_json::Value);

impl JsonArgs {
    /// Build from an object literal, e.g. `JsonArgs::new(json!({"e": 3}))`.
    pub fn new(value: serde_json::Value) -> Self {
        Self(value)
    }
}

impl CommandArgs for JsonArgs {
    fn number(&self, key: &str) -> Option<f64> {
        self.0.get(key)?.as_f64()
    }
    fn text(&self, key: &str) -> Option<String> {
        self.0.get(key)?.as_str().map(str::to_string)
    }
    fn flag(&self, key: &str) -> Option<bool> {
        self.0.get(key)?.as_bool()
    }
    fn json(&self, key: &str) -> serde_json::Value {
        self.0.get(key).cloned().unwrap_or(serde_json::Value::Null)
    }
}

/// The entity a command targets, from its conventional `"e"` field.
///
/// Absent or negative means "no target": the preamble writes `-1` when a script
/// passes something that is not an entity, and every arm treats that as a
/// silent no-op rather than an error, because a script that lost track of an
/// entity should not stop the frame.
fn target(host: &mut impl ScriptWorldHost, args: &impl CommandArgs) -> Option<Entity> {
    let id = args.number("e").unwrap_or(-1.0);
    if id < 0.0 {
        return None;
    }
    host.resolve_entity(id as u32)
}

/// Handle one command if it is one of the shared world-only commands.
///
/// Returns `true` when handled, so a host dispatches through this first and
/// falls through to its own `match` for everything else. Returning `true` for a
/// command that turned out to be a no-op (unknown entity, missing component) is
/// deliberate: the tag *was* ours, and the host must not try it again.
pub fn handle_world_command(
    host: &mut impl ScriptWorldHost,
    tag: &str,
    args: &impl CommandArgs,
) -> bool {
    match tag {
        // ── sprite animation ──────────────────────────────────────────────
        // Sprite and skeletal animation alike, as the native arms do.
        "pa" => {
            let clip = args.text("clip").unwrap_or_default();
            if let Some(e) = target(host, args) {
                if let Ok(mut a) = host.world().get::<&mut SpriteAnimator>(e) {
                    a.play(&clip);
                }
                if let Ok(mut p) = host.world().get::<&mut SkeletalPlayer>(e) {
                    p.play(&clip);
                }
            }
        }
        "sa" => {
            if let Some(e) = target(host, args) {
                if let Ok(mut a) = host.world().get::<&mut SpriteAnimator>(e) {
                    a.stop();
                }
                if let Ok(mut p) = host.world().get::<&mut SkeletalPlayer>(e) {
                    p.playing = false;
                }
            }
        }
        "sas" => {
            let speed = args.number("speed").unwrap_or(1.0) as f32;
            if let Some(e) = target(host, args) {
                if let Ok(mut a) = host.world().get::<&mut SpriteAnimator>(e) {
                    a.speed = speed;
                }
                if let Ok(mut p) = host.world().get::<&mut SkeletalPlayer>(e) {
                    p.speed = speed;
                }
            }
        }
        "caf" => {
            let clip = args.text("clip").unwrap_or_default();
            let seconds = args.number("s").unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Ok(mut p) = host.world().get::<&mut SkeletalPlayer>(e)
            {
                p.crossfade(&clip, seconds);
            }
        }
        "sabl" => {
            let clip = args.text("clip").unwrap_or_default();
            let weight = args.number("w").unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Ok(mut p) = host.world().get::<&mut SkeletalPlayer>(e)
            {
                p.set_blend(&clip, weight);
            }
        }
        // `set_conveyor_speed(e, speed)` - `Physics.set_conveyor_speed`; the
        // physics sync reads the component each frame.
        "scvs" => {
            let speed = args.number("s").unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Ok(mut c) = host.world().get::<&mut crate::ecs::components::Conveyor>(e)
            {
                c.speed = speed;
            }
        }
        // `animation_play_layer` / `_stop_layer` / `_set_layer_weight` -
        // `Animation.play_layer` and kin, through `SkeletalPlayer::layer_op`.
        "anly" => {
            let op = args.text("op").unwrap_or_default();
            let layer = args.text("l").unwrap_or_default();
            let clip = args.text("c").unwrap_or_default();
            let value = args.number("v").unwrap_or(0.0) as f32;
            let seconds = args.number("s").unwrap_or(0.0) as f32;
            let looping = args.flag("lp").unwrap_or(false);
            if let Some(e) = target(host, args)
                && let Ok(mut p) = host.world().get::<&mut SkeletalPlayer>(e)
            {
                p.layer_op(&op, &layer, &clip, value, seconds, looping);
            }
        }
        // `set_animation_blend_position(e, x, y)` - `Animation.set_blend_position`.
        "sabp" => {
            let (x, y) = (args.number("x").unwrap_or(0.0) as f32, args.number("y").unwrap_or(0.0) as f32);
            if let Some(e) = target(host, args)
                && let Ok(mut p) = host.world().get::<&mut SkeletalPlayer>(e)
            {
                p.set_blend_position(x, y);
            }
        }
        "ikw" => crate::feature!("ik", "Skeleton.set_ik_weight", {
            let weight = args.number("w").unwrap_or(1.0) as f32;
            if let Some(e) = target(host, args) {
                if let Ok(mut ik) = host.world().get::<&mut crate::animation::ik::IkTwoBone>(e) {
                    ik.weight = weight.clamp(0.0, 1.0);
                } else if let Ok(mut ik) = host.world().get::<&mut crate::animation::ik::IkChain>(e) {
                    ik.weight = weight.clamp(0.0, 1.0);
                }
            }
        }),

        // ── animation state machine ───────────────────────────────────────
        "sat" => {
            let param = args.text("param").unwrap_or_default();
            if let Some(e) = target(host, args)
                && let Ok(mut fsm) = host.world().get::<&mut AnimStateMachine>(e)
            {
                fsm.set_trigger(&param);
            }
        }
        "saf" => {
            let param = args.text("param").unwrap_or_default();
            let value = args.number("value").unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Ok(mut fsm) = host.world().get::<&mut AnimStateMachine>(e)
            {
                fsm.set_float(&param, value);
            }
        }
        "sab" => {
            let param = args.text("param").unwrap_or_default();
            let value = args.flag("value").unwrap_or(false);
            if let Some(e) = target(host, args)
                && let Ok(mut fsm) = host.world().get::<&mut AnimStateMachine>(e)
            {
                fsm.set_bool(&param, value);
            }
        }
        "ssm" => {
            let state = args.text("state").unwrap_or_default();
            if let Some(e) = target(host, args) {
                // `start` needs the state machine and the animator mutably at
                // once, which hecs forbids on a single entity. Removing the
                // fsm, mutating, and putting it back is cheaper than splitting
                // the world — and it goes back even if the animator is missing.
                if let Ok(mut fsm) = host.world().remove_one::<AnimStateMachine>(e) {
                    if let Ok(mut anim) = host.world().get::<&mut SpriteAnimator>(e) {
                        fsm.start(&state, &mut anim);
                    }
                    let _ = host.world().insert_one(e, fsm);
                }
            }
        }

        // ── skeletal + IK ─────────────────────────────────────────────────
        "sat2" => {
            let time = args.number("v").unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Ok(mut player) = host.world().get::<&mut SkeletalPlayer>(e)
            {
                player.time = time.max(0.0);
            }
        }
        "ikt" => crate::feature!("ik", "Skeleton.set_ik_target", {
            let x = args.number("x").unwrap_or(0.0) as f32;
            let y = args.number("y").unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args) {
                // Two-bone first, then look-at — the order
                // `ScriptCommand::SetIkTarget` uses natively.
                if let Ok(mut ik) = host.world().get::<&mut crate::animation::ik::IkTwoBone>(e) {
                    ik.target = [x, y];
                } else if let Ok(mut ik) = host.world().get::<&mut crate::animation::ik::IkLookAt>(e) {
                    ik.target = [x, y];
                } else if let Ok(mut ik) = host.world().get::<&mut crate::animation::ik::IkChain>(e) {
                    ik.target = [x, y];
                }
            }
        }),

        // ── particles ─────────────────────────────────────────────────────
        "sea" => {
            let active = args.flag("a").unwrap_or(false);
            if let Some(e) = target(host, args)
                && let Ok(mut em) = host.world().get::<&mut ParticleEmitter>(e)
            {
                em.active = active;
            }
        }
        "ser" => {
            let rate = args.number("r").unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Ok(mut em) = host.world().get::<&mut ParticleEmitter>(e)
            {
                em.emission_rate = rate;
            }
        }
        // `set_emitter_field(entity, field, value)`. Mirrors
        // `ScriptCommand::SetEmitterField`.
        "sef" => {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args) {
                // A CPU emitter or a GPU one (`particles::gpu`).
                let result = if let Ok(mut em) = host.world().get::<&mut ParticleEmitter>(e) {
                    crate::particles::set_emitter_field(&mut em, &field, &value)
                } else if let Ok(mut g) = host.world().get::<&mut crate::particles::gpu::GpuParticles>(e) {
                    crate::particles::gpu::set_field(&mut g, &field, &value)
                } else {
                    Ok(())
                };
                if let Err(err) = result {
                    log::warn!("Particles.set: {err}");
                }
            }
        }
        // `set_video_field(entity, field, value)` - `Video.set`.
        "svf" => crate::feature!("video", "Video.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::video::set_video_field(host.world(), e, &field, &value)
            {
                log::warn!("Video.set: {err}");
            }
        }),
        // `set_ragdoll_field(entity, field, value)` - `Ragdoll.set`.
        "srdf" => crate::feature!("ragdoll", "Ragdoll.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::ragdoll::set_ragdoll_field(host.world(), e, &field, &value)
            {
                log::warn!("Ragdoll.set: {err}");
            }
        }),
        // `set_behavior_tree_field(entity, field, value)` - `BehaviorTree.set`.
        "sbtf" => crate::feature!("behavior_tree", "BehaviorTree.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::behavior_tree::set_behavior_tree_field(host.world(), e, &field, &value)
            {
                log::warn!("BehaviorTree.set: {err}");
            }
        }),
        // `set_timeline_field(entity, field, value)` - `Timeline.set`.
        "stlf" => crate::feature!("timeline", "Timeline.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::timeline::set_timeline_field(host.world(), e, &field, &value)
            {
                log::warn!("Timeline.set: {err}");
            }
        }),
        // `set_canvas_group_field(entity, field, value)` - `CanvasGroup.set`.
        "scgf" => {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::canvas_group::set_canvas_group_field(host.world(), e, &field, &value)
            {
                log::warn!("CanvasGroup.set: {err}");
            }
        }
        // `set_fluid_field(entity, field, value)` - `Fluid.set`.
        "sffl" => crate::feature!("fluid", "Fluid.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::fluid::set_fluid_field(host.world(), e, &field, &value)
            {
                log::warn!("Fluid.set: {err}");
            }
        }),
        // `smooth_terrain_edit(entity, x, y, radius, amount)` - `Terrain.edit`.
        "stte" => crate::feature!("smooth_terrain", "Terrain.edit", {
            let num = |k: &str| args.number(k).unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::smooth_terrain::edit(host.world(), e, num("x"), num("y"), num("r"), num("a"))
            {
                log::warn!("Terrain.edit: {err}");
            }
        }),
        // `dialogue_start` / `_advance` / `_choose` / `_stop` - `Dialogue.*`.
        "dlg" => crate::feature!("dialogue", "Dialogue.*", {
            let op = args.text("op").unwrap_or_default();
            let arg = args.text("a").unwrap_or_default();
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::dialogue::op(host.world(), e, &op, &arg)
            {
                log::warn!("Dialogue.{op}: {err}");
            }
        }),
        // `mode7_place(entity, x, y, angle)` - `Mode7.place`.
        "m7p" => crate::feature!("mode7", "Mode7.place", {
            let num = |k: &str| args.number(k).unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::mode7::place(host.world(), e, num("x"), num("y"), num("a"))
            {
                log::warn!("Mode7.place: {err}");
            }
        }),
        // `mode7_set(entity, field, value)` - `Mode7.set`.
        "m7s" => crate::feature!("mode7", "Mode7.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::mode7::set_field(host.world(), e, &field, &value)
            {
                log::warn!("Mode7.set: {err}");
            }
        }),
        // `stage_place(entity, x, y, z)` - `Stage.place`.
        "stgp" => crate::feature!("stage", "Stage.place", {
            let num = |k: &str| args.number(k).unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::stage::place(host.world(), e, num("x"), num("y"), num("z"))
            {
                log::warn!("Stage.place: {err}");
            }
        }),
        // `stage_set(entity, field, value)` - `Stage.set`.
        "stgs" => crate::feature!("stage", "Stage.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::stage::set_field(host.world(), e, &field, &value)
            {
                log::warn!("Stage.set: {err}");
            }
        }),
        // `sprite_tilt_set(entity, field, value)` - `SpriteTilt.set`.
        "tilt" => crate::feature!("sprite_tilt", "SpriteTilt.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::sprite_tilt::set_field(host.world(), e, &field, &value)
            {
                log::warn!("SpriteTilt.set: {err}");
            }
        }),
        // `afterimage_set(entity, field, value)` - `Afterimage.set`.
        "aimg" => crate::feature!("afterimage", "Afterimage.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::afterimage::set_field(host.world(), e, &field, &value)
            {
                log::warn!("Afterimage.set: {err}");
            }
        }),
        // `physics_slice(x0, y0, x1, y1)` - `Physics.slice`.
        "pslc" => crate::feature!("slice", "Physics.slice", {
            let num = |k: &str| args.number(k).unwrap_or(0.0) as f32;
            crate::ecs::slice::script_slice(host.world(), num("x0"), num("y0"), num("x1"), num("y1"));
        }),
        // `portal_set(entity, field, value)` - `Portal.set`.
        "prtl" => crate::feature!("portal", "Portal.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::portal::set_field(host.world(), e, &field, &value)
            {
                log::warn!("Portal.set: {err}");
            }
        }),
        // `jelly_poke(entity, x, y, strength, radius)` - `Jelly.poke`.
        "jlpk" => crate::feature!("jelly", "Jelly.poke", {
            let num = |k: &str, d: f64| args.number(k).unwrap_or(d) as f32;
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::jelly::poke(host.world(), e, glam::Vec2::new(num("x", 0.0), num("y", 0.0)), num("s", 300.0), num("r", 40.0))
            {
                log::warn!("Jelly.poke: {err}");
            }
        }),
        // `jelly_set(entity, field, value)` - `Jelly.set`.
        "jlst" => crate::feature!("jelly", "Jelly.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::jelly::set_field(host.world(), e, &field, &value)
            {
                log::warn!("Jelly.set: {err}");
            }
        }),
        // `marker_set(entity, field, value)` - `Marker.set`.
        "mkst" => crate::feature!("offscreen_marker", "Marker.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::offscreen_marker::set_field(host.world(), e, &field, &value)
            {
                log::warn!("Marker.set: {err}");
            }
        }),
        // `radial_set(entity, field, value)` - `Radial.set`.
        "rdst" => crate::feature!("radial_menu", "Radial.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ui::radial_menu::set_field(host.world(), e, &field, &value)
            {
                log::warn!("Radial.set: {err}");
            }
        }),
        // `bubble_say(entity, text, seconds)` - `Bubble.say`.
        "bbsy" => crate::feature!("speech_bubble", "Bubble.say", {
            let text = args.text("x").unwrap_or_default();
            let seconds = args.number("s").unwrap_or(-1.0) as f32;
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ui::speech_bubble::say(host.world(), e, &text, seconds)
            {
                log::warn!("Bubble.say: {err}");
            }
        }),
        // `bubble_set(entity, field, value)` - `Bubble.set`.
        "bbst" => crate::feature!("speech_bubble", "Bubble.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ui::speech_bubble::set_field(host.world(), e, &field, &value)
            {
                log::warn!("Bubble.set: {err}");
            }
        }),
        // `outline_set(entity, field, value)` - `Outline.set`.
        "olst" => crate::feature!("sprite_outline", "Outline.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::sprite_outline::set_field(host.world(), e, &field, &value)
            {
                log::warn!("Outline.set: {err}");
            }
        }),
        // `lightning_strike(entity, x, y)` - `Lightning.strike`.
        "ltsk" => crate::feature!("lightning", "Lightning.strike", {
            let x = args.number("x").unwrap_or(0.0) as f32;
            let y = args.number("y").unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::lightning::strike(host.world(), e, x, y)
            {
                log::warn!("Lightning.strike: {err}");
            }
        }),
        // `lightning_set(entity, field, value)` - `Lightning.set`.
        "ltst" => crate::feature!("lightning", "Lightning.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::lightning::set_field(host.world(), e, &field, &value)
            {
                log::warn!("Lightning.set: {err}");
            }
        }),
        // `flow_set(entity, field, value)` - `Flow.set`.
        "flst" => crate::feature!("flow", "Flow.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::flow::set_field(host.world(), e, &field, &value)
            {
                log::warn!("Flow.set: {err}");
            }
        }),
        // `nav_link_set(entity, field, value)` - `NavLink.set`.
        "nvlk" => crate::feature!("navigation", "NavLink.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::nav_links::set_field(host.world(), e, &field, &value)
            {
                log::warn!("NavLink.set: {err}");
            }
        }),
        // `morph_to(entity, target, seconds)` - `Morph.to`.
        "mrto" => crate::feature!("path_morph", "Morph.to", {
            let target_name = args.text("n").unwrap_or_default();
            let seconds = args.number("s").unwrap_or(-1.0) as f32;
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::path_morph::morph_to(host.world(), e, &target_name, seconds)
            {
                log::warn!("Morph.to: {err}");
            }
        }),
        // `morph_set(entity, field, value)` - `Morph.set`.
        "mrst" => crate::feature!("path_morph", "Morph.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::path_morph::set_field(host.world(), e, &field, &value)
            {
                log::warn!("Morph.set: {err}");
            }
        }),
        // `toast_show(entity, title, text, icon, seconds)` - `Toast.show`.
        "tsho" => crate::feature!("toast", "Toast.show", {
            let title = args.text("n").unwrap_or_default();
            let text = args.text("x").unwrap_or_default();
            let icon = args.text("i").unwrap_or_default();
            let seconds = args.number("s").unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ui::toast::show(host.world(), e, &title, &text, &icon, seconds)
            {
                log::warn!("Toast.show: {err}");
            }
        }),
        // `toast_set(entity, field, value)` - `Toast.set`.
        "tsst" => crate::feature!("toast", "Toast.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ui::toast::set_field(host.world(), e, &field, &value)
            {
                log::warn!("Toast.set: {err}");
            }
        }),
        // `fog_of_war_reset(entity)` - `FogOfWar.reset`.
        "fwrs" => crate::feature!("fog_of_war", "FogOfWar.reset", {
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::fog_of_war::reset_fog(host.world(), e)
            {
                log::warn!("FogOfWar.reset: {err}");
            }
        }),
        // `fog_of_war_set(entity, field, value)` - `FogOfWar.set`.
        "fwsf" => crate::feature!("fog_of_war", "FogOfWar.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::fog_of_war::set_fog_of_war_field(host.world(), e, &field, &value)
            {
                log::warn!("FogOfWar.set: {err}");
            }
        }),
        // `snow_set(entity, field, value)` - `Snow.set`. (`snst` is
        // `stop_nav`'s.)
        "snwc" => crate::feature!("snow", "Snow.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::snow::set_field(host.world(), e, &field, &value)
            {
                log::warn!("Snow.set: {err}");
            }
        }),
        // `ui_drop(item, slot)` - `UI.drop`.
        "uidp" => crate::feature!("drag_drop", "UI.drop", {
            let slot = args.text("s").unwrap_or_default();
            if let Some(e) = target(host, args) {
                crate::ui::drag_drop::drop_named(host.world(), e, &slot);
            }
        }),
        // `set_beat_clock_field(entity, field, value)` - `BeatClock.set`.
        "sbcf" => crate::feature!("beat_clock", "BeatClock.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::beat_clock::set_beat_clock_field(host.world(), e, &field, &value)
            {
                log::warn!("BeatClock.set: {err}");
            }
        }),
        // `set_text_path_field(entity, field, value)` - `TextPath.set`.
        "stxp" => crate::feature!("text_path", "TextPath.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::text_path::set_text_path_field(host.world(), e, &field, &value)
            {
                log::warn!("TextPath.set: {err}");
            }
        }),
        // `set_trajectory_field(entity, field, value)` - `Trajectory.set`.
        "strj" => crate::feature!("trajectory", "Trajectory.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::trajectory::set_trajectory_field(host.world(), e, &field, &value)
            {
                log::warn!("Trajectory.set: {err}");
            }
        }),
        // `set_tile_light_field(entity, field, value)` - `TileLight.set`.
        "stlt" => crate::feature!("tile_light", "TileLight.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::tile_light::set_tile_light_field(host.world(), e, &field, &value)
            {
                log::warn!("TileLight.set: {err}");
            }
        }),
        // `set_auto_normal_field(entity, field, value)` - `AutoNormal.set`.
        "sanf" => crate::feature!("auto_normal", "AutoNormal.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::auto_normal::set_auto_normal_field(host.world(), e, &field, &value)
            {
                log::warn!("AutoNormal.set: {err}");
            }
        }),
        // `set_cast_shadow_field(entity, field, value)` - `CastShadow.set`.
        "scsh" => crate::feature!("cast_shadow", "CastShadow.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::cast_shadow::set_shadow_field(host.world(), e, &field, &value)
            {
                log::warn!("CastShadow.set: {err}");
            }
        }),
        // `set_time_scale_field(entity, field, value)` - `TimeScale.set`.
        "stsf" => {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::time_scale::set_time_scale_field(host.world(), e, &field, &value)
            {
                log::warn!("TimeScale.set: {err}");
            }
        }
        // `set_origin_field(entity, field, value)` - `FloatingOrigin.set`.
        "sfof" => {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::floating_origin::set_origin_field(host.world(), e, &field, &value)
            {
                log::warn!("FloatingOrigin.set: {err}");
            }
        }
        // `set_day_field(entity, field, value)` - `DayCycle.set`.
        "sdcf" => crate::feature!("day_cycle", "DayCycle.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::day_cycle::set_day_field(host.world(), e, &field, &value)
            {
                log::warn!("DayCycle.set: {err}");
            }
        }),
        // `set_vision_field(entity, field, value)` - `Vision.set`.
        "svsf" => crate::feature!("vision", "Vision.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::vision::set_vision_field(host.world(), e, &field, &value)
            {
                log::warn!("Vision.set: {err}");
            }
        }),
        // `wfc_generate(entity, x, y, w, h)` - `Wfc.generate`.
        "swfc" => crate::feature!("wfc", "Wfc.generate", {
            let n = |k: &str| args.number(k).unwrap_or(0.0).floor() as i32;
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::wfc::queue_region(host.world(), e, n("x"), n("y"), n("w"), n("h"))
            {
                log::warn!("Wfc.generate: {err}");
            }
        }),
        // `set_fog_field(entity, field, value)` - `Fog.set`.
        "sfgf" => crate::feature!("fog", "Fog.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::fog::set_fog_field(host.world(), e, &field, &value)
            {
                log::warn!("Fog.set: {err}");
            }
        }),
        // `set_rewind_field(entity, field, value)` - `Rewind.set`.
        "srwf" => crate::feature!("rewind", "Rewind.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::rewind::set_rewind_field(host.world(), e, &field, &value)
            {
                log::warn!("Rewind.set: {err}");
            }
        }),
        // `smoke_puff(entity, x, y, radius, amount, vx, vy)` - `Smoke.puff`.
        "smpf" => crate::feature!("smoke", "Smoke.puff", {
            let n = |k: &str| args.number(k).unwrap_or(0.0) as f32;
            let puff = crate::ecs::smoke::Puff {
                at: glam::Vec2::new(n("x"), n("y")),
                radius: n("r"),
                amount: n("a"),
                velocity: glam::Vec2::new(n("vx"), n("vy")),
            };
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::smoke::queue_puff(host.world(), e, puff)
            {
                log::warn!("Smoke.puff: {err}");
            }
        }),
        // `set_smoke_field(entity, field, value)` - `Smoke.set`.
        "ssmf" => crate::feature!("smoke", "Smoke.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::smoke::set_smoke_field(host.world(), e, &field, &value)
            {
                log::warn!("Smoke.set: {err}");
            }
        }),
        // `pixel_paint(entity, x, y, radius, material)` - `PixelSim.paint`.
        "spxp" => crate::feature!("pixel_sim", "PixelSim.paint", {
            let x = args.number("x").unwrap_or(0.0) as f32;
            let y = args.number("y").unwrap_or(0.0) as f32;
            let r = args.number("r").unwrap_or(0.0) as f32;
            let m = args.text("m").unwrap_or_default();
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::pixel_sim::queue_paint(host.world(), e, x, y, r, &m)
            {
                log::warn!("PixelSim.paint: {err}");
            }
        }),
        // `set_sprite_fill(entity, amount)` - `Sprite.set_fill`.
        "ssfl" => {
            let amount = args.number("a").unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Ok(mut sprite) = host.world().get::<&mut crate::ecs::components::SpriteComponent>(e)
                && !sprite.set_fill_amount(amount)
            {
                log::warn!("Sprite.set_fill: the sprite has no bar or radial fill");
            }
        }
        // `set_water_field(entity, field, value)` - `Water.set`.
        "swtf" => crate::feature!("water", "Water.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::water::set_water_field(host.world(), e, &field, &value)
            {
                log::warn!("Water.set: {err}");
            }
        }),
        // `water_splash(entity, x, strength)` - `Water.splash`.
        "swsp" => crate::feature!("water", "Water.splash", {
            let x = args.number("x").unwrap_or(0.0) as f32;
            let strength = args.number("s").unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::water::splash_world(host.world(), e, x, strength)
            {
                log::warn!("Water.splash: {err}");
            }
        }),
        // `set_mirror_field(entity, field, value)` - `Mirror.set`.
        "smrf" => crate::feature!("mirror", "Mirror.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::mirror::set_mirror_field(host.world(), e, &field, &value)
            {
                log::warn!("Mirror.set: {err}");
            }
        }),
        // `set_trail_field(entity, field, value)` - `Trail.set`.
        "strl" => crate::feature!("trails", "Trail.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::trails::set_trail_field(host.world(), e, &field, &value)
            {
                log::warn!("Trail.set: {err}");
            }
        }),
        // `set_line_field(entity, field, value)` - `Line.set`.
        "slf" => crate::feature!("line2d", "Line.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::renderer::line2d::set_line_field(host.world(), e, &field, &value)
            {
                log::warn!("Line.set: {err}");
            }
        }),
        // `set_path_follow_field(entity, field, value)` - `PathFollow.set`.
        "spff" => {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::path_follow::set_path_follow_field(host.world(), e, &field, &value)
            {
                log::warn!("PathFollow.set: {err}");
            }
        }
        // `set_cloth_field(entity, field, value)`. Mirrors
        // `ScriptCommand::SetClothField`.
        "scf" => crate::feature!("cloth", "Cloth.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::cloth::set_cloth_field(host.world(), e, &field, &value)
            {
                log::warn!("Cloth.set: {err}");
            }
        }),
        // `set_area_field(entity, field, value)` - `Area.set`.
        "saef" => {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::area_effect::set_area_field(host.world(), e, &field, &value)
            {
                log::warn!("Area.set: {err}");
            }
        }

        // `set_camera_screen_rect(entity, x, y, w, h)` - `Camera.set_screen_rect`.
        "scsr" => {
            let n = |k: &str| args.number(k).unwrap_or(0.0) as f32;
            let rect = [n("x"), n("y"), n("w"), n("h")];
            if let Some(e) = target(host, args) {
                crate::ecs::views::set_screen_rect(host.world(), e, rect);
            }
        }
        // `set_sprite_label(entity, label, category)` - `Sprite.set_label`.
        "spl" => crate::feature!("sprite_library", "Sprite.set_label", {
            let label = args.text("l").unwrap_or_default();
            let category = args.text("c");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::sprite_library::set_label(host.world(), e, &label, category.as_deref())
            {
                log::warn!("Sprite.set_label: {err}");
            }
        }),
        // `set_camera_field(entity, field, value)` - `Camera.set`.
        "scmf" => {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::camera_rig::set_camera_field(host.world(), e, &field, &value)
            {
                log::warn!("Camera.set: {err}");
            }
        }
        // `set_camera_follow(entity, field, value)` - `Camera.set_follow`.
        "scfl" => {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::ecs::components::set_follow_field(host.world(), e, &field, &value)
            {
                log::warn!("Camera.set_follow: {err}");
            }
        }
        // `set_nav_target(entity, x, y)` / `stop_nav(entity)` - `Nav.set_target`
        // and `Nav.stop` (`snst`, no coordinates).
        "snt" | "snst" => crate::feature!("navigation", "Nav.set_target", {
            let goal = match (args.number("x"), args.number("y")) {
                (Some(x), Some(y)) => Some([x as f32, y as f32]),
                _ => None,
            };
            if let Some(e) = target(host, args)
                && let Err(err) = crate::navigation::set_nav_target(host.world(), e, goal)
            {
                log::warn!("Nav.set_target: {err}");
            }
        }),
        // `set_nav_field(entity, field, value)` - `Nav.set`.
        "snvf" => crate::feature!("navigation", "Nav.set", {
            let field = args.text("f").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && let Err(err) = crate::navigation::set_nav_field(host.world(), e, &field, &value)
            {
                log::warn!("Nav.set: {err}");
            }
        }),

        // ── combat ────────────────────────────────────────────────────────
        // Overwrite `Health`. Mirrors the native `ScriptCommand::SetHealth`.
        "shp" => {
            let current = args.number("cur").unwrap_or(0.0) as f32;
            let max = args.number("max").unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Ok(mut h) = host.world().get::<&mut Health>(e)
            {
                h.current = current;
                // A non-positive max means "leave the maximum alone" — scripts
                // commonly set only the current value, and the field defaults
                // to 0.0 when omitted.
                if max > 0.0 {
                    h.max = max;
                }
            }
        }
        // Overwrite `Hitbox` combat stats. Mirrors `ScriptCommand::SetHitbox`.
        "shb" => {
            let dmg = args.number("dmg").unwrap_or(0.0) as f32;
            let kb = args.number("kb").unwrap_or(0.0) as f32;
            let rt = args.number("rt").unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Ok(mut hb) = host.world().get::<&mut Hitbox>(e)
            {
                hb.damage = dmg;
                hb.knockback = kb;
                hb.retrigger = rt;
            }
        }

        // ── sprites and text ──────────────────────────────────────────────
        // `set_sprite_anchor(entity, ax, ay)` — the pivot a sprite is drawn
        // around. Neither browser host handled this until Sep 2026, so the
        // function was callable and inert in the editor and in web builds
        // alike; it is the kind of gap having one implementation prevents.
        // `set_sprite_size(entity, w, h)` - the size the sprite is drawn
        // at, in world units (the transform's scale still applies on top).
        // Found missing by a game that sized every cell by
        // scaling children because a slot's own scale is the package's.
        "sssz" => {
            let w = args.number("w").unwrap_or(0.0) as f32;
            let h = args.number("h").unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Ok(mut sprite) = host.world().get::<&mut SpriteComponent>(e)
            {
                sprite.size = Some([w.max(0.0), h.max(0.0)]);
            }
        }
        // `set_z_order(entity, z)` - the draw order of the entity's sprite,
        // its text and its mesh, whichever it has.
        "szo" => {
            let z = args.number("z").unwrap_or(0.0) as i32;
            if let Some(e) = target(host, args) {
                let world = host.world();
                if let Ok(mut sprite) = world.get::<&mut SpriteComponent>(e) {
                    sprite.z_order = z;
                }
                if let Ok(mut text) = world.get::<&mut crate::ecs::components::TextComponent>(e) {
                    text.z_order = z;
                }
                if let Ok(mut mesh) = world.get::<&mut crate::ecs::components::Mesh2D>(e) {
                    mesh.z_order = z;
                }
            }
        }
        // `move_character(entity, vx, vy[, drop_through])` - what a
        // `CharacterController2D` asks the next physics step for.
        "mvc" => {
            let vx = args.number("vx").unwrap_or(0.0) as f32;
            let vy = args.number("vy").unwrap_or(0.0) as f32;
            let drop = args.flag("drop");
            if let Some(e) = target(host, args)
                && let Ok(mut cc) = host.world().get::<&mut crate::ecs::components::CharacterController2D>(e)
            {
                cc.velocity = [vx, vy];
                if let Some(d) = drop {
                    cc.drop_through = d;
                }
            }
        }
        // `set_text_color(entity, r, g, b, a)` - the entity's world text.
        "stc" => {
            let c = |k: &str, d: f64| args.number(k).unwrap_or(d) as f32;
            let color = [c("r", 1.0), c("g", 1.0), c("b", 1.0), c("a", 1.0)];
            if let Some(e) = target(host, args)
                && let Ok(mut text) = host.world().get::<&mut TextComponent>(e)
            {
                text.color = color;
            }
        }
        // `set_mesh(entity, vertices, indices)`.
        "smsh" => {
            let (vertices, indices) = (args.json("vs"), args.json("ix"));
            if let Some(e) = target(host, args) {
                set_mesh(host.world(), e, &vertices, &indices);
            }
        }
        // `set_trail_emitting(entity, on)` / `clear_trail(entity)`.
        "tre" => {
            let on = args.flag("on").unwrap_or(true);
            if let Some(e) = target(host, args)
                && let Ok(mut trail) = host.world().get::<&mut crate::ecs::components::Trail>(e)
            {
                trail.emitting = on;
            }
        }
        "trc" => {
            if let Some(e) = target(host, args)
                && let Ok(mut trail) = host.world().get::<&mut crate::ecs::components::Trail>(e)
            {
                trail.points.clear();
            }
        }
        // `paint_sprite(entity, x, y, size, r, g, b, a, shape, rotation, blend)` and
        // `press_sprite(entity, x, y, size, depth, shape, rotation)` (`dp`):
        // queued on the `paintable`, drawn by `ecs::paint::apply_paints`.
        "spnt" => crate::feature!("paint", "paint", {
            let n = |k: &str, d: f64| args.number(k).unwrap_or(d) as f32;
            let stamp = crate::ecs::paint::Stamp {
                x: n("x", 0.0),
                y: n("y", 0.0),
                size: n("s", 0.0),
                color: [n("cr", 1.0), n("cg", 1.0), n("cb", 1.0), n("ca", 1.0)],
                shape: crate::ecs::paint::StampShape::named(&args.text("sh").unwrap_or_default()),
                rotation: n("rot", 0.0),
                depth: n("dp", 0.0),
                blend: crate::ecs::paint::PaintBlend::named(&args.text("bl").unwrap_or_default()),
            };
            if let Some(e) = target(host, args) {
                crate::ecs::paint::paint(host.world(), e, stamp);
            }
        }),
        // `carve_sprite(entity, x, y, radius)`: queued on the `carvable`,
        // cut by `ecs::carve::apply_carves`.
        "scv" => {
            let n = |k: &str| args.number(k).unwrap_or(0.0) as f32;
            let carve = crate::ecs::carve::Carve { x: n("x"), y: n("y"), radius: n("r") };
            if let Some(e) = target(host, args)
                && let Ok(mut c) = host.world().get::<&mut crate::ecs::carve::Carvable>(e)
            {
                c.pending.push(carve);
            }
        }
        // `set_light_color(entity, r, g, b)`, `set_light_intensity(entity, i)`
        // and `set_light_height(entity, h)`: whichever light the entity has -
        // point, spot or sun. They were copied into each browser host, and
        // knew point and spot lights only.
        "slc" => {
            let c = |k: &str| args.number(k).unwrap_or(1.0) as f32;
            let color = [c("r"), c("g"), c("b")];
            if let Some(e) = target(host, args) {
                with_lights(host.world(), e, |l| match l {
                    AnyLight::Point(l) => l.color = color,
                    AnyLight::Spot(l) => l.color = color,
                    AnyLight::Sun(l) => l.color = color,
                    #[cfg(feature = "freeform_light")]
                    AnyLight::Freeform(l) => l.color = color,
                });
            }
        }
        "sli" => {
            let i = args.number("i").unwrap_or(1.0) as f32;
            if let Some(e) = target(host, args) {
                with_lights(host.world(), e, |l| match l {
                    AnyLight::Point(l) => l.intensity = i,
                    AnyLight::Spot(l) => l.intensity = i,
                    AnyLight::Sun(l) => l.intensity = i,
                    #[cfg(feature = "freeform_light")]
                    AnyLight::Freeform(l) => l.intensity = i,
                });
            }
        }
        "stoon" => {
            let bands = args.number("b").unwrap_or(0.0) as f32;
            let soft = args.number("s").map(|v| v as f32);
            let dither = args.number("d").map(|v| v as f32);
            set_toon_lighting(host.world(), bands, soft, dither);
        }
        // `set_global_illumination(strength, scale)` - `Lighting.set_global_illumination`.
        "sgi" => {
            let strength = args.number("s").map(|v| v as f32);
            let scale = args.number("k").map(|v| v as f32);
            set_global_illumination(host.world(), strength, scale);
        }
        "sao" => {
            let radius = args.number("r").map(|v| v as f32);
            let strength = args.number("s").map(|v| v as f32);
            set_ambient_occlusion(host.world(), radius, strength);
        }
        // `set_color_lut(texture, strength)`: the frame's colour grade.
        "slut" => {
            let id = args.text("id").unwrap_or_default();
            let strength = args.number("s").unwrap_or(1.0) as f32;
            set_color_lut(host.world(), &id, strength);
        }
        // `set_light_mask(entity, mask)`: the light layers it lights.
        "slm" => {
            let m = args.number("m").map_or(u32::MAX, |v| v.clamp(0.0, u32::MAX as f64) as u32);
            if let Some(e) = target(host, args) {
                with_lights(host.world(), e, |l| match l {
                    AnyLight::Point(l) => l.light_mask = m,
                    AnyLight::Spot(l) => l.light_mask = m,
                    AnyLight::Sun(l) => l.light_mask = m,
                    #[cfg(feature = "freeform_light")]
                    AnyLight::Freeform(_) => {}
                });
            }
        }
        "slh" => {
            let h = args.number("h").unwrap_or(0.5) as f32;
            if let Some(e) = target(host, args) {
                with_lights(host.world(), e, |l| match l {
                    AnyLight::Point(l) => l.height = h,
                    AnyLight::Spot(l) => l.height = h,
                    AnyLight::Sun(l) => l.height = h,
                    #[cfg(feature = "freeform_light")]
                    AnyLight::Freeform(_) => {}
                });
            }
        }
        "ssa" => {
            let ax = args.number("ax").unwrap_or(0.5) as f32;
            let ay = args.number("ay").unwrap_or(0.5) as f32;
            if let Some(e) = target(host, args)
                && let Ok(mut sprite) = host.world().get::<&mut SpriteComponent>(e)
            {
                sprite.anchor = [ax, ay];
            }
        }
        "st" => {
            let txt = args.text("txt").unwrap_or_default();
            if let Some(e) = target(host, args) {
                // World-space text first, UI text as the fallback — one script
                // function serves both, which is why the native
                // `ScriptCommand::SetText` has the same two-step.
                if let Ok(mut tc) = host.world().get::<&mut TextComponent>(e) {
                    tc.text = txt;
                } else if let Ok(mut t) = host.world().get::<&mut UIText>(e) {
                    t.text = txt;
                }
            }
        }

        // ── UI ────────────────────────────────────────────────────────────
        "sut" => {
            let txt = args.text("txt").unwrap_or_default();
            if let Some(e) = target(host, args)
                && let Ok(mut t) = host.world().get::<&mut UIText>(e)
            {
                t.text = txt;
            }
        }
        "suv" => {
            let visible = args.flag("v").unwrap_or(false);
            if let Some(e) = target(host, args)
                && let Ok(mut el) = host.world().get::<&mut UIElement>(e)
            {
                el.visible = visible;
            }
        }
        "sup" => {
            let anchor = args.text("a").unwrap_or_default();
            let ox = args.number("ox").unwrap_or(0.0) as f32;
            let oy = args.number("oy").unwrap_or(0.0) as f32;
            if let Some(e) = target(host, args)
                && let Ok(mut el) = host.world().get::<&mut UIElement>(e)
            {
                el.anchor = UIAnchor::from_name(&anchor);
                el.offset = glam::Vec2::new(ox, oy);
            }
        }
        // `set_ui_prop(entity, property, value)`. Mirrors
        // `ScriptCommand::SetUiProp`; `apply_ui_prop` owns the per-widget
        // property table so both paths agree on what a property means.
        "uip" => {
            let property = args.text("p").unwrap_or_default();
            let value = args.json("v");
            if let Some(e) = target(host, args)
                && !apply_ui_prop(host.world(), e, &property, &value)
            {
                log::warn!(
                    "set_ui_prop: entity has no UI widget with property '{property}'"
                );
            }
        }
        "gfo" => {
            let gid = args.text("gid").unwrap_or_default();
            let nid = args.text("nid").unwrap_or_default();
            for (_, gui) in host.world().query_mut::<&mut GuiComponent>() {
                if gui.gui_id == gid {
                    // An empty node id clears focus rather than focusing a
                    // node named "".
                    gui.focused_node_id = if nid.is_empty() { None } else { Some(nid.clone()) };
                    break;
                }
            }
        }

        // ── deferred transform / colour writes ────────────────────────────
        // `set_position` and friends normally write the script mirror directly,
        // which is a typed-array store and the reason those calls are cheap.
        // An entity the mirror has no slot for cannot be written that way, and
        // the preamble sends this command instead of dropping the write.
        //
        // The case that matters is spawn-then-place: `spawn_entity` and
        // `spawn_prefab` hand back a script-side handle immediately, but the
        // entity is not created until the host drains commands — after the
        // script returns. So `set_position` on the line after a spawn wrote
        // past the end of the mirror and vanished, which is about the most
        // natural thing a script can do (BUGS #52). Commands drain in order, so
        // the spawn has already happened by the time this arm runs.
        "dtw" => {
            let kind = args.text("k").unwrap_or_default();
            let a = args.number("a").unwrap_or(0.0) as f32;
            let b = args.number("b").unwrap_or(0.0) as f32;
            let Some(e) = target(host, args) else { return true };
            match kind.as_str() {
                "p" => {
                    if let Ok(mut t) = host.world().get::<&mut Transform>(e) {
                        t.position = glam::Vec2::new(a, b);
                    }
                }
                "r" => {
                    if let Ok(mut t) = host.world().get::<&mut Transform>(e) {
                        t.rotation = a;
                    }
                }
                "s" => {
                    if let Ok(mut t) = host.world().get::<&mut Transform>(e) {
                        t.scale = glam::Vec2::new(a, b);
                    }
                }
                "c" => {
                    let c = args.number("c").unwrap_or(1.0) as f32;
                    let d = args.number("d").unwrap_or(1.0) as f32;
                    // No sprite is not an error: `spawn_entity` makes a bare
                    // entity, and colouring one before giving it a sprite is a
                    // script mistake rather than something to materialize a
                    // sprite for — unlike `scor`, where the corners *are* the
                    // sprite's geometry.
                    if let Ok(mut sprite) = host.world().get::<&mut SpriteComponent>(e) {
                        sprite.color = [a, b, c, d];
                    }
                }
                _ => {
                    log::warn!("deferred transform write: unknown kind '{kind}'");
                }
            }
        }

        // ── hierarchy ─────────────────────────────────────────────────────
        // `set_parent(child, parent)`. A null or negative parent un-parents.
        "sprt" => {
            let child_id = args.number("c").unwrap_or(-1.0);
            if child_id < 0.0 {
                return true;
            }
            let Some(child) = host.resolve_entity(child_id as u32) else {
                return true;
            };
            match args.number("p") {
                Some(pid) if pid >= 0.0 => {
                    if let Some(parent) = host.resolve_entity(pid as u32) {
                        if parent == child {
                            // Self-parenting would make `resolve_world_transforms`
                            // chase its own tail. The editor logged this and the
                            // web host silently skipped it; logging is the more
                            // useful of the two, so both do now.
                            log::warn!("set_parent: ignoring self-parent");
                        } else {
                            let _ = host.world().insert_one(child, Parent(parent));
                        }
                    }
                }
                // No parent given, or a negative one: detach.
                _ => {
                    let _ = host.world().remove_one::<Parent>(child);
                }
            }
        }

        // ── materials ─────────────────────────────────────────────────────
        // `set_material_uniform_{float,int,vec2,vec4}`. One tag; `ut` says
        // which, and `v` carries a number or an array to match.
        "snm" => {
            if let Some(e) = target(host, args) {
                let id = normal_map_id(args.text("m").as_deref());
                let mut comp = host.world().remove_one::<MaterialComponent>(e).unwrap_or_default();
                comp.normal_map_id = id;
                let _ = host.world().insert_one(e, comp);
            }
        }
        "smu" => {
            let name = args.text("n").unwrap_or_default();
            let utype = args.text("ut").unwrap_or_default();
            let raw = args.json("v");
            let nth = |i: usize| raw.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            // A vector uniform whose value is not an array is left alone rather
            // than written as zeros. The two hosts disagreed here — the editor
            // ignored it, the web runtime set the uniform to the origin, which
            // silently blanks a material. Unreachable through the preamble,
            // which always sends an array literal, but the safe reading is the
            // one to keep.
            let value: Option<UniformValue> = match utype.as_str() {
                "f" => raw.as_f64().map(|v| UniformValue::Float(v as f32)),
                "i" => raw.as_f64().map(|v| UniformValue::Int(v as i32)),
                "v2" => raw.is_array().then(|| UniformValue::Vec2([nth(0), nth(1)])),
                "v4" => raw
                    .is_array()
                    .then(|| UniformValue::Vec4([nth(0), nth(1), nth(2), nth(3)])),
                _ => None,
            };
            if name.is_empty() {
                return true;
            }
            if let (Some(e), Some(v)) = (target(host, args), value) {
                // Taken out and put back: `MaterialComponent` may not exist
                // yet, and this both creates and updates in one step.
                let mut comp = host.world().remove_one::<MaterialComponent>(e).unwrap_or_default();
                comp.uniforms.insert(name, v);
                let _ = host.world().insert_one(e, comp);
            }
        }

        // `set_sprite_corners(entity, x0,y0, x1,y1, x2,y2, x3,y3)` — free-form
        // quad corners, for skew and perspective-ish effects.
        "scor" => {
            let c = |k: &str| args.number(k).unwrap_or(0.0) as f32;
            let corners = [
                [c("x0"), c("y0")],
                [c("x1"), c("y1")],
                [c("x2"), c("y2")],
                [c("x3"), c("y3")],
            ];
            if let Some(e) = target(host, args) {
                // Materialize a default sprite for runtime-spawned entities, so
                // this works on something a script has just created. Native and
                // the editor both did; the web host did not, so the call was
                // silently inert in a shipped build for exactly the entities
                // most likely to use it.
                if host.world().get::<&SpriteComponent>(e).is_err() {
                    let _ = host.world().insert_one(e, SpriteComponent::default());
                }
                if let Ok(mut sprite) = host.world().get::<&mut SpriteComponent>(e) {
                    sprite.corners = Some(corners);
                }
            }
        }

        // `set_camera_zoom(zoom)` - the active camera's zoom, 1.0 being the
        // authored framing and higher values closer in.
        //
        // The engine could always zoom: `update_camera` copies this field into
        // the renderer's camera every frame. Nothing could *ask* it to, which
        // left `camera_shake` as the only camera control a script had - so a
        // scene wanting to zoom had to move every entity instead.
        // ── persistent store (browser writes, applied to the shared store) ─
        "psn" => {
            if let (Some(key), Some(n)) = (args.text("k"), args.number("n")) {
                crate::scripting::persistent::set_number(&key, n);
            }
        }
        "pss" => {
            if let (Some(key), Some(v)) = (args.text("k"), args.text("s")) {
                crate::scripting::persistent::set_string(&key, &v);
            }
        }
        "pcl" => crate::scripting::persistent::clear(),

        // `set_camera_pixel_size(size)`. Mirrors `ScriptCommand::SetCameraPixelSize`.
        "scpx" => {
            let size = args.number("s").unwrap_or(0.0) as f32;
            if !crate::ecs::systems::set_active_camera_pixel_size(host.world(), size) {
                log::warn!(
                    "set_camera_pixel_size({size}) had no effect: no entity carries an active Camera2DComponent"
                );
            }
        }
        "scz" => {
            let Some(zoom) = args.number("z") else { return true };
            let zoom = zoom as f32;
            // A scene with no camera entity renders through the runtime's own
            // default camera, which this cannot reach. Saying so beats a
            // silent no-op.
            if !crate::ecs::systems::set_active_camera_zoom(host.world(), zoom) {
                log::warn!(
                    "set_camera_zoom({zoom}) had no effect: no entity carries an active Camera2DComponent"
                );
            }
        }

        // ── lighting ──────────────────────────────────────────────────────
        "sal" => {
            let r = args.number("r").unwrap_or(1.0) as f32;
            let g = args.number("g").unwrap_or(1.0) as f32;
            let b = args.number("b").unwrap_or(1.0) as f32;
            let i = args.number("i").unwrap_or(0.3) as f32;
            // The renderer only reads the first ambient light, so this updates
            // that one — and spawns it if the scene authored none, so a script
            // can set ambient light without the scene having to declare it.
            let first = host.world().query_mut::<&mut AmbientLight>().into_iter().next();
            let updated = first.is_some();
            if let Some((_, ambient)) = first {
                ambient.color = [r, g, b];
                ambient.intensity = i;
            }
            if !updated {
                host.world().spawn((AmbientLight { color: [r, g, b], intensity: i },));
            }
        }

        // ── custom data, tags, materials, tiles, .gui visibility ──────────
        // `set_custom(entity, component, field, value)`. The bag is made if
        // the entity has none, and the component's object inside it.
        "sc" => {
            let comp = args.text("cn").unwrap_or_default();
            let field = args.text("fn").unwrap_or_default();
            if !comp.is_empty()
                && !field.is_empty()
                && let Some(e) = target(host, args)
            {
                let value = args.json("v");
                let world = host.world();
                let mut bag = world
                    .remove_one::<crate::ecs::components::CustomComponents>(e)
                    .unwrap_or_else(|_| crate::ecs::components::CustomComponents(Default::default()));
                let entry = bag.0.entry(comp).or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
                if let serde_json::Value::Object(map) = entry {
                    map.insert(field, value);
                }
                let _ = world.insert_one(e, bag);
            }
        }
        // `add_tag` / `remove_tag`, the tag set made if the entity has none.
        "at" | "rt" => {
            let adding = tag == "at";
            let name = args.text("tag").unwrap_or_default();
            if !name.is_empty()
                && let Some(e) = target(host, args)
            {
                let world = host.world();
                let mut bag = world
                    .remove_one::<crate::ecs::components::Tags>(e)
                    .unwrap_or_else(|_| crate::ecs::components::Tags(Default::default()));
                if adding {
                    bag.0.insert(name);
                } else {
                    bag.0.remove(&name);
                }
                let _ = world.insert_one(e, bag);
            }
        }
        // `set_material(entity, id)`: a default material made if there is none.
        "smat" => {
            let mat = args.text("m").unwrap_or_default();
            if !mat.is_empty()
                && let Some(e) = target(host, args)
            {
                let world = host.world();
                let mut comp = world.remove_one::<crate::ecs::components::MaterialComponent>(e).unwrap_or_default();
                comp.material_id = mat.into();
                let _ = world.insert_one(e, comp);
            }
        }
        // `set_tile(entity, layer, x, y, id)`. `null`, a negative and `0` all
        // mean "clear it" - `script_tile_id`, the one rule every host uses.
        "stl" => {
            let layer = args.text("ln").unwrap_or_default();
            let x = args.number("x").unwrap_or(0.0) as u32;
            let y = args.number("y").unwrap_or(0.0) as u32;
            let tile_id = crate::tilemap::script_tile_id(args.number("tid"));
            if !layer.is_empty()
                && let Some(e) = target(host, args)
                && let Ok(mut tm) = host.world().get::<&mut crate::tilemap::Tilemap>(e)
                && let Some(l) = tm.find_layer_mut(&layer)
            {
                l.set_tile(x, y, tile_id);
            }
        }
        // `gui_show(id)` / `gui_hide(id)`: every `gui` naming the document, or
        // every one with no id.
        "gs" | "gh" => {
            let visible = tag == "gs";
            let gid = args.text("gid").unwrap_or_default();
            for (_, gui) in host.world().query_mut::<&mut crate::ecs::components::GuiComponent>() {
                if gid.is_empty() || gui.gui_id == gid {
                    gui.visible = visible;
                }
            }
        }

        _ => return false,
    }
    true
}

/// [`CommandArgs`] over the `js_sys::Object` the browser hosts carry commands in.
///
/// Both hosts had their own byte-identical `js_value_f64` / `js_value_string` /
/// `js_value_bool` / `js_value_json` helpers; this is the one copy. Note
/// [`Self::json`] round-trips through `JSON.stringify` rather than
/// `serde_wasm_bindgen`, so a Lua table arriving as an array still lands as a
/// JSON array — the hosts both did that deliberately and it must not be
/// "simplified" away.
#[cfg(feature = "web")]
pub struct JsArgs<'a>(pub &'a js_sys::Object);

#[cfg(feature = "web")]
impl CommandArgs for JsArgs<'_> {
    fn number(&self, key: &str) -> Option<f64> {
        js_sys::Reflect::get(self.0, &wasm_bindgen::JsValue::from_str(key))
            .ok()
            .and_then(|v| v.as_f64())
    }
    fn text(&self, key: &str) -> Option<String> {
        js_sys::Reflect::get(self.0, &wasm_bindgen::JsValue::from_str(key))
            .ok()
            .and_then(|v| v.as_string())
    }
    fn flag(&self, key: &str) -> Option<bool> {
        js_sys::Reflect::get(self.0, &wasm_bindgen::JsValue::from_str(key))
            .ok()
            .and_then(|v| v.as_bool())
    }
    fn json(&self, key: &str) -> serde_json::Value {
        let Ok(v) = js_sys::Reflect::get(self.0, &wasm_bindgen::JsValue::from_str(key)) else {
            return serde_json::Value::Null;
        };
        js_sys::JSON::stringify(&v)
            .ok()
            .and_then(|s| String::from(s).parse::<serde_json::Value>().ok())
            .unwrap_or(serde_json::Value::Null)
    }
}

/// What a documented event handler, `on_<event>(entity, data)`, is handed as
/// `data`: the payload as a **value** - a table in Lua - not the JSON text it
/// travels as between script and host. Text that is not JSON is passed as the
/// string it is; no payload is nil.
///
/// The browser hosts used to call `on_<event>` with the general `on_event`
/// argument list, `(self, name, sender, data_text)`, so a handler written
/// the documented way got the event's *name* as its `data` - `data.button`
/// on a `ui_click` was nil, and a game whose every button is a
/// `ui_click` did nothing at all in the editor. Native passes
/// `(entity, data)` with `data` a table, and so do these now.
pub fn event_handler_data(data: Option<&str>) -> serde_json::Value {
    match data {
        None => serde_json::Value::Null,
        Some(text) => serde_json::from_str(text).unwrap_or_else(|_| serde_json::Value::String(text.to_string())),
    }
}

/// The arguments a browser host calls an event handler with: `(entity,
/// data)` for the documented `on_<event>`, with `data` per
/// [`event_handler_data`]; and the general `on_event(self, name, sender,
/// data_text)` exactly as it always was, since scripts written against that
/// spelling parse the text themselves. The one place both hosts build them.
#[cfg(feature = "web")]
pub fn event_handler_args(
    specific: bool,
    listener: u32,
    name: &str,
    sender: u32,
    data_js: &wasm_bindgen::JsValue,
) -> js_sys::Array {
    use wasm_bindgen::JsValue;
    let args = js_sys::Array::new();
    args.push(&JsValue::from_f64(listener as f64));
    if specific {
        let value = event_handler_data(data_js.as_string().as_deref());
        let js = if value.is_null() {
            JsValue::NULL
        } else {
            js_sys::JSON::parse(&value.to_string()).unwrap_or(JsValue::NULL)
        };
        args.push(&js);
    } else {
        args.push(&JsValue::from_str(name));
        args.push(&JsValue::from_f64(sender as f64));
        args.push(data_js);
    }
    args
}

/// The tags both browser hosts dispatch through
/// `adapters::js::host_dispatch::handle_host_command` - the shared arms that
/// need more of the host than the world (audio, tweens, saves, input, fades),
/// so they live beside the browser code rather than here. Listed here because
/// native tests read it: `host_command_parity` counts these as handled by a
/// host that calls the dispatcher, and checks the list against its arms.
pub const HOST_DISPATCH_TAGS: &[&str] = &[
    "ab",
    "af",
    "ai",
    "aj",
    "cat",
    "cmsh",
    "csm",
    "ct",
    "cup",
    "cut",
    "ddc",
    "ddl",
    "ddr",
    "de",
    "eb",
    "evt",
    "fdin",
    "fdou",
    "goc",
    "gsp",
    "ib",
    "ls",
    "paal",
    "pam",
    "pm",
    "ps",
    "psa",
    "real",
    "rem",
    "rj",
    "sav",
    "sbt",
    "scrf",
    "se",
    "sg",
    "slr",
    "smav",
    "sms",
    "smv",
    "spf",
    "sppe",
    "sppp",
    "srig",
    "ssbc",
    "ssr",
    "ssv",
    "stal",
    "stm",
    "sts",
    "sv",
    "svcl",
    "svcm",
    "svst",
    "tc",
    "tp",
    "tr",
    "ts",
    "tsc",
];

/// Every tag [`handle_world_command`] claims.
///
/// Exists so a host, and `tests/host_command_parity.rs`, can ask rather than
/// re-listing them. A tag here that the function does not actually handle would
/// make the host skip its own arm and do nothing at all, so the list is checked
/// against the function itself in the tests below.
pub const SHARED_WORLD_TAGS: &[&str] = &[
    "pa", "sa", "sas", "sat", "saf", "sab", "ssm", "sat2", "ikt", "caf", "sabl", "sabp", "anly", "scvs", "ikw", "sea", "ser", "sef", "scf", "spff", "slf", "strl", "sffl", "smrf", "swtf", "swsp", "ssfl", "spxp", "smpf", "ssmf", "srwf", "sfgf", "swfc", "svsf", "sdcf", "sfof", "stsf", "scsh", "sanf", "stlt", "strj", "stxp", "sbcf", "stte", "dlg", "uidp", "m7p", "m7s", "stgp", "stgs", "tilt", "aimg", "pslc", "prtl", "jlpk", "jlst", "mkst", "rdst", "bbsy", "bbst", "olst", "ltsk", "ltst", "flst", "nvlk", "mrto", "mrst", "tsho", "tsst", "snwc", "fwrs", "fwsf", "scgf", "svf", "srdf", "scsr", "stlf", "sbtf", "shp", "shb",
    "ssa", "sssz", "st", "sut", "suv", "sup", "uip", "gfo", "sal", "sprt", "smu", "scor", "dtw",
    "scz", "scpx", "psn", "pss", "pcl", "szo", "stc", "mvc", "snm", "tre", "trc",
    "slc", "sli", "slh", "slm", "slut", "stoon", "sao", "smsh", "scv", "spnt", "saef", "snt", "snst", "snvf", "scfl", "scmf", "spl", "sgi",
    "sc", "at", "rt", "smat", "stl", "gs", "gh",
];

/// One of the lights an entity can carry, for a setter that applies to all.
pub enum AnyLight<'a> {
    Point(&'a mut crate::ecs::components::PointLight2D),
    Spot(&'a mut crate::ecs::components::SpotLight2D),
    Sun(&'a mut crate::ecs::components::DirectionalLight2D),
    #[cfg(feature = "freeform_light")]
    Freeform(&'a mut crate::renderer::freeform_light::FreeformLight),
}

/// Run `f` on every light `entity` carries - the one rule the light setters
/// share, natively (`ecs::systems`) and in the browser hosts.
pub fn with_lights(world: &mut hecs::World, entity: hecs::Entity, mut f: impl FnMut(AnyLight<'_>)) {
    if let Ok(mut l) = world.get::<&mut crate::ecs::components::PointLight2D>(entity) {
        f(AnyLight::Point(&mut l));
    }
    if let Ok(mut l) = world.get::<&mut crate::ecs::components::SpotLight2D>(entity) {
        f(AnyLight::Spot(&mut l));
    }
    if let Ok(mut l) = world.get::<&mut crate::ecs::components::DirectionalLight2D>(entity) {
        f(AnyLight::Sun(&mut l));
    }
    #[cfg(feature = "freeform_light")]
    if let Ok(mut l) = world.get::<&mut crate::renderer::freeform_light::FreeformLight>(entity) {
        f(AnyLight::Freeform(&mut l));
    }
}

/// `Mesh.set(entity, vertices, indices)`: the entity's mesh replaced with
/// the shape a script gives, a `mesh` component added if it had none (drawn
/// with the white texture until something names one). Natively and in the
/// browser hosts, through [`mesh_from_script`].
pub fn set_mesh(world: &mut World, entity: Entity, vertices: &serde_json::Value, indices: &serde_json::Value) {
    let (vertices, indices) = mesh_from_script(vertices, indices);
    if let Ok(mut mesh) = world.get::<&mut crate::ecs::components::Mesh2D>(entity) {
        mesh.vertices = vertices;
        mesh.indices = indices;
        return;
    }
    if world.contains(entity) {
        let mesh = crate::ecs::components::Mesh2D { vertices, indices, ..Default::default() };
        let _ = world.insert_one(entity, mesh);
    }
}

/// A script's mesh, as `Mesh2D` holds it - the one reading of it every
/// runtime shares.
///
/// Each vertex is a table, either a list `{ x, y, u, v }` (a fifth number is
/// the perspective weight `q`) or keyed: `{ x =, y =, u =, v =, q =, r =, g =,
/// b =, a = }`, any of them left out taking the `MeshVertex` default.
/// `indices` is a list of triangles counted **from 1**, as Lua counts - an
/// index outside the vertices drops its triangle - and absent or empty means
/// a fan over the vertices in order, as the component's own `indices` does.
pub fn mesh_from_script(vertices: &serde_json::Value, indices: &serde_json::Value) -> (Vec<crate::ecs::components::MeshVertex>, Vec<u32>) {
    use crate::ecs::components::MeshVertex;
    let num = |v: Option<&serde_json::Value>| v.and_then(|v| v.as_f64()).map(|v| v as f32);
    let list = |v: &serde_json::Value| -> Vec<serde_json::Value> {
        match v {
            serde_json::Value::Array(a) => a.clone(),
            // A Lua list with a hole, or one that came across as `{"1": ..}`:
            // taken in key order, so it is still a list.
            serde_json::Value::Object(m) => {
                let mut kv: Vec<(usize, &serde_json::Value)> =
                    m.iter().filter_map(|(k, v)| Some((k.parse::<f64>().ok()? as usize, v))).collect();
                kv.sort_by_key(|(k, _)| *k);
                kv.into_iter().map(|(_, v)| v.clone()).collect()
            }
            _ => Vec::new(),
        }
    };
    let vertices: Vec<MeshVertex> = list(vertices)
        .iter()
        .map(|v| {
            let d = MeshVertex::default();
            match v {
                serde_json::Value::Array(a) => MeshVertex {
                    position: [num(a.first()).unwrap_or(0.0), num(a.get(1)).unwrap_or(0.0)],
                    uv: [num(a.get(2)).unwrap_or(0.0), num(a.get(3)).unwrap_or(0.0)],
                    q: num(a.get(4)).unwrap_or(1.0),
                    ..d
                },
                serde_json::Value::Object(_) => {
                    let f = |k: &str, or: f32| num(v.get(k)).unwrap_or(or);
                    MeshVertex {
                        position: [f("x", 0.0), f("y", 0.0)],
                        uv: [f("u", 0.0), f("v", 0.0)],
                        color: [f("r", 1.0), f("g", 1.0), f("b", 1.0), f("a", 1.0)],
                        q: f("q", 1.0),
                    }
                }
                _ => d,
            }
        })
        .collect();
    let n = vertices.len();
    let raw: Vec<i64> = list(indices).iter().filter_map(|v| v.as_f64()).map(|v| v as i64).collect();
    let indices = raw
        .chunks_exact(3)
        .filter(|t| t.iter().all(|&i| i >= 1 && (i as usize) <= n))
        .flat_map(|t| t.iter().map(|&i| (i - 1) as u32))
        .collect();
    (vertices, indices)
}

/// `set_color_lut(texture, strength)`: the first `color_lut` in the world
/// takes the strip and strength (one is spawned if there is none); an empty
/// id removes the grade. The strip must be loaded - named by the scene, as a
/// `color_lut` or a sprite does - since nothing draws a LUT to ask for it.
pub fn set_color_lut(world: &mut hecs::World, texture_id: &str, strength: f32) {
    use crate::ecs::components::ColorLut;
    let found = world.query::<&ColorLut>().iter().next().map(|(e, _)| e);
    match found {
        Some(e) => {
            if let Ok(mut l) = world.get::<&mut ColorLut>(e) {
                l.texture_id = texture_id.to_string();
                l.strength = strength;
            }
        }
        None if !texture_id.is_empty() => {
            world.spawn((ColorLut { texture_id: texture_id.to_string(), strength },));
        }
        None => {}
    }
}

/// `Lighting.set_toon(bands, softness, dither)`: the scene's toon lighting,
/// spawned if it has none, and removed by `bands <= 0` - natively and in the
/// browser hosts alike. The renderer reads the first `ToonLighting`, so this
/// sets that one; `nil` softness and dither are the component's defaults.
pub fn set_toon_lighting(world: &mut World, bands: f32, softness: Option<f32>, dither: Option<f32>) {
    let d = crate::ecs::components::ToonLighting::default();
    let toon = (bands >= 1.0).then(|| crate::ecs::components::ToonLighting {
        bands: bands.round() as u32,
        softness: softness.unwrap_or(d.softness).clamp(0.0, 1.0),
        dither: dither.unwrap_or(d.dither).max(0.0),
    });
    set_singleton(world, toon);
}

/// `Lighting.set_occlusion(radius, strength)`: the scene's ambient occlusion,
/// spawned if it has none, and removed by `strength <= 0` (or a radius of
/// none) - so turning it off also stops the pass that draws it.
pub fn set_ambient_occlusion(world: &mut World, radius: Option<f32>, strength: Option<f32>) {
    let d = crate::ecs::components::AmbientOcclusion::default();
    let radius = radius.unwrap_or(d.radius);
    let strength = strength.unwrap_or(d.strength);
    let ao = (radius > 0.0 && strength > 0.0)
        .then(|| crate::ecs::components::AmbientOcclusion { radius, strength: strength.min(1.0) });
    set_singleton(world, ao);
}

/// `Lighting.set_global_illumination(strength, scale)`: the scene's bounce
/// light, spawned if it has none, removed by `strength <= 0`; `scale` (frame
/// pixels to a GI texel) kept when not given.
pub fn set_global_illumination(world: &mut World, strength: Option<f32>, scale: Option<f32>) {
    let current = world.query_mut::<&crate::ecs::components::GlobalIllumination>().into_iter().next().map(|(_, g)| *g);
    let mut gi = current.unwrap_or_default();
    if let Some(s) = strength {
        gi.strength = if s.is_finite() { s.max(0.0) } else { 0.0 };
    }
    if let Some(k) = scale.filter(|k| k.is_finite() && *k >= 1.0) {
        gi.scale = (k.round() as u32).min(16);
    }
    set_singleton(world, (gi.strength > 0.0).then_some(gi));
}

/// A scene-wide setting the renderer reads from the first entity carrying
/// it: `Some` writes that one (or spawns one), `None` takes it off every
/// entity, so no second copy is left to take over.
fn set_singleton<T: hecs::Component>(world: &mut World, value: Option<T>) {
    match value {
        Some(v) => {
            if let Some((_, c)) = world.query_mut::<&mut T>().into_iter().next() {
                *c = v;
            } else {
                world.spawn((v,));
            }
        }
        None => {
            let holders: Vec<Entity> = world.query_mut::<&T>().into_iter().map(|(e, _)| e).collect();
            for e in holders {
                let _ = world.remove_one::<T>(e);
            }
        }
    }
}

/// What `Material.set_normal_map`'s argument means: an id names the normal
/// map, and `nil` or an empty string takes it away. One rule for every
/// runtime - native reads `Option<String>`, the browser hosts an empty
/// string, and both come through here.
pub fn normal_map_id(id: Option<&str>) -> Option<String> {
    id.filter(|s| !s.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::AnimationClip;
    use crate::ecs::components::Transform;
    use serde_json::json;

    /// A `SpriteAnimator` that actually knows the named clip.
    ///
    /// `SpriteAnimator::play` ignores a clip it does not have, so an animator
    /// built with `Default` would make the test below pass for the wrong
    /// reason — nothing set, nothing to see.
    fn animator_knowing(clip: &str) -> SpriteAnimator {
        let mut a = SpriteAnimator::default();
        a.clips.insert(
            clip.to_string(),
            AnimationClip {
                name: clip.to_string(),
                frames: Vec::new(),
                frame_duration: 0.1,
                looping: true,
                property_tracks: Vec::new(),
            },
        );
        a
    }

    /// `Health` has no `Default`, and the i-frame fields are not what these
    /// tests are about.
    fn health(current: f32, max: f32) -> Health {
        Health { current, max, invuln_time: 0.0, invuln_timer: 0.0 }
    }

    /// A host with no id table: resolution is a plain world scan, which is what
    /// the real hosts fall back to for any id not in `SCRIPT_ENTITY_MAP`.
    struct TestHost(World);

    impl ScriptWorldHost for TestHost {
        fn world(&mut self) -> &mut World {
            &mut self.0
        }
        fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
            self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
        }
    }

    /// Run one command against a host, the way a runtime would.
    fn run(host: &mut TestHost, tag: &str, args: serde_json::Value) -> bool {
        handle_world_command(host, tag, &JsonArgs(args))
    }

    fn host_with(components: impl hecs::DynamicBundle) -> (TestHost, u32) {
        let mut world = World::new();
        let e = world.spawn(components);
        (TestHost(world), e.id())
    }

    #[test]
    fn set_camera_zoom_reaches_the_active_camera() {
        use crate::ecs::components::Camera2DComponent;
        let mut world = World::new();
        let inactive = world.spawn((Camera2DComponent { is_active: false, ..Default::default() },));
        let active = world.spawn((Camera2DComponent { is_active: true, ..Default::default() },));
        let mut host = TestHost(world);

        assert!(run(&mut host, "scz", json!({ "z": 2.5 })));

        assert_eq!(host.0.get::<&Camera2DComponent>(active).unwrap().zoom, 2.5);
        assert_eq!(
            host.0.get::<&Camera2DComponent>(inactive).unwrap().zoom,
            1.0,
            "a camera that is not the active one must not move"
        );
    }

    #[test]
    fn a_zoom_of_zero_is_clamped_rather_than_blanking_the_screen() {
        use crate::ecs::components::Camera2DComponent;
        let mut world = World::new();
        let e = world.spawn((Camera2DComponent { is_active: true, ..Default::default() },));
        let mut host = TestHost(world);

        run(&mut host, "scz", json!({ "z": 0.0 }));
        assert!(host.0.get::<&Camera2DComponent>(e).unwrap().zoom > 0.0);

        run(&mut host, "scz", json!({ "z": -4.0 }));
        assert!(
            host.0.get::<&Camera2DComponent>(e).unwrap().zoom > 0.0,
            "a negative zoom collapses the view matrix - it reads as a crash"
        );
    }

    #[test]
    fn a_zoom_with_no_camera_is_a_quiet_no_op() {
        // A scene can render through the runtime's own default camera with no
        // camera entity at all; this must not panic there.
        let mut host = TestHost(World::new());
        assert!(run(&mut host, "scz", json!({ "z": 2.0 })));
    }

    #[test]
    fn every_advertised_tag_is_actually_handled() {
        // The list is what a host trusts to decide it need not keep its own
        // arm. A tag advertised but unhandled would make the command vanish.
        let mut host = TestHost(World::new());
        for tag in SHARED_WORLD_TAGS {
            assert!(
                run(&mut host, tag, json!({ "e": -1 })),
                "SHARED_WORLD_TAGS lists `{tag}` but handle_world_command does not handle it"
            );
        }
    }

    #[test]
    fn an_unknown_tag_falls_through() {
        let mut host = TestHost(World::new());
        assert!(!run(&mut host, "definitely-not-a-tag", json!({})));
    }

    /// The preamble writes `-1` when a script passes something that is not an
    /// entity. Every arm must treat that as a quiet no-op, not a panic.
    #[test]
    fn a_missing_entity_is_a_quiet_no_op() {
        let (mut host, _) = host_with((Transform::default(), SpriteAnimator::default()));
        for tag in SHARED_WORLD_TAGS {
            assert!(run(&mut host, tag, json!({ "e": -1 })), "{tag} refused a -1 entity");
            assert!(run(&mut host, tag, json!({ "e": 9999 })), "{tag} refused an unknown id");
            // No fields at all — the arm has to fall back to its defaults.
            assert!(run(&mut host, tag, json!({})), "{tag} refused an empty command");
        }
    }

    /// An entity that exists but lacks the component the command targets. This
    /// is the common case in a real game (a script calling `play_animation` on
    /// something with no animator) and must not panic.
    #[test]
    fn an_entity_without_the_component_is_a_quiet_no_op() {
        let (mut host, id) = host_with((Transform::default(),));
        for tag in SHARED_WORLD_TAGS {
            assert!(
                run(&mut host, tag, json!({ "e": id, "clip": "x", "param": "p", "txt": "t" })),
                "{tag} refused an entity lacking its component"
            );
        }
    }

    #[test]
    fn play_and_stop_drive_the_sprite_animator() {
        let (mut host, id) = host_with((animator_knowing("walk"),));
        run(&mut host, "pa", json!({ "e": id, "clip": "walk" }));
        let e = host.resolve_entity(id).unwrap();
        {
            let a = host.0.get::<&SpriteAnimator>(e).unwrap();
            assert_eq!(a.current_clip.as_deref(), Some("walk"));
            assert!(a.playing);
        }
        run(&mut host, "sa", json!({ "e": id }));
        assert!(!host.0.get::<&SpriteAnimator>(e).unwrap().playing);
    }

    #[test]
    fn animation_speed_defaults_to_one_when_omitted() {
        let (mut host, id) = host_with((SpriteAnimator { speed: 4.0, ..Default::default() },));
        run(&mut host, "sas", json!({ "e": id }));
        let e = host.resolve_entity(id).unwrap();
        assert_eq!(host.0.get::<&SpriteAnimator>(e).unwrap().speed, 1.0);
    }

    #[test]
    fn health_keeps_its_maximum_when_max_is_omitted() {
        // The documented convention: zero means "leave max alone". A script
        // setting only the current value must not zero the bar's maximum.
        let (mut host, id) = host_with((health(50.0, 100.0),));
        run(&mut host, "shp", json!({ "e": id, "cur": 30.0 }));
        let e = host.resolve_entity(id).unwrap();
        let h = host.0.get::<&Health>(e).unwrap();
        assert_eq!((h.current, h.max), (30.0, 100.0));
    }

    #[test]
    fn health_takes_a_new_maximum_when_one_is_given() {
        let (mut host, id) = host_with((health(50.0, 100.0),));
        run(&mut host, "shp", json!({ "e": id, "cur": 10.0, "max": 20.0 }));
        let e = host.resolve_entity(id).unwrap();
        let h = host.0.get::<&Health>(e).unwrap();
        assert_eq!((h.current, h.max), (10.0, 20.0));
    }

    #[test]
    fn set_text_prefers_world_text_over_ui_text() {
        let (mut host, id) = host_with((
            TextComponent { text: "world".into(), ..Default::default() },
            UIText { text: "ui".into(), ..Default::default() },
        ));
        run(&mut host, "st", json!({ "e": id, "txt": "new" }));
        let e = host.resolve_entity(id).unwrap();
        assert_eq!(host.0.get::<&TextComponent>(e).unwrap().text, "new");
        assert_eq!(
            host.0.get::<&UIText>(e).unwrap().text,
            "ui",
            "the UI fallback fired even though world text was present"
        );
    }

    #[test]
    fn set_text_falls_back_to_ui_text() {
        let (mut host, id) = host_with((UIText { text: "old".into(), ..Default::default() },));
        run(&mut host, "st", json!({ "e": id, "txt": "new" }));
        let e = host.resolve_entity(id).unwrap();
        assert_eq!(host.0.get::<&UIText>(e).unwrap().text, "new");
    }

    #[test]
    fn ik_targets_the_two_bone_chain_before_the_look_at() {
        let (mut host, id) = host_with((
            crate::animation::ik::IkTwoBone { target: [0.0, 0.0], ..Default::default() },
            crate::animation::ik::IkLookAt { target: [0.0, 0.0], ..Default::default() },
        ));
        run(&mut host, "ikt", json!({ "e": id, "x": 3.0, "y": 4.0 }));
        let e = host.resolve_entity(id).unwrap();
        assert_eq!(host.0.get::<&crate::animation::ik::IkTwoBone>(e).unwrap().target, [3.0, 4.0]);
        assert_eq!(
            host.0.get::<&crate::animation::ik::IkLookAt>(e).unwrap().target,
            [0.0, 0.0],
            "the look-at fallback fired even though a two-bone chain was present"
        );
    }

    #[test]
    fn ambient_light_updates_the_existing_one() {
        let mut world = World::new();
        world.spawn((AmbientLight { color: [0.0, 0.0, 0.0], intensity: 0.0 },));
        let mut host = TestHost(world);
        run(&mut host, "sal", json!({ "e": -1, "r": 1.0, "g": 0.5, "b": 0.25, "i": 0.8 }));
        let lights: Vec<[f32; 3]> =
            host.0.query::<&AmbientLight>().iter().map(|(_, a)| a.color).collect();
        assert_eq!(lights.len(), 1, "a second ambient light was spawned");
        assert_eq!(lights[0], [1.0, 0.5, 0.25]);
    }

    #[test]
    fn ambient_light_is_spawned_when_the_scene_authored_none() {
        let mut host = TestHost(World::new());
        run(&mut host, "sal", json!({ "e": -1, "r": 0.2, "g": 0.3, "b": 0.4, "i": 0.6 }));
        let lights: Vec<f32> =
            host.0.query::<&AmbientLight>().iter().map(|(_, a)| a.intensity).collect();
        assert_eq!(lights, vec![0.6]);
    }

    #[test]
    fn gui_focus_clears_on_an_empty_node_id() {
        let mut world = World::new();
        world.spawn((GuiComponent {
            gui_id: "hud".into(),
            focused_node_id: Some("button".into()),
            ..Default::default()
        },));
        let mut host = TestHost(world);
        run(&mut host, "gfo", json!({ "gid": "hud", "nid": "" }));
        let focused: Option<String> = host
            .0
            .query::<&GuiComponent>()
            .iter()
            .next()
            .and_then(|(_, g)| g.focused_node_id.clone());
        assert_eq!(focused, None);
    }

    #[test]
    fn gui_focus_ignores_a_gui_it_does_not_name() {
        let mut world = World::new();
        world.spawn((GuiComponent {
            gui_id: "hud".into(),
            focused_node_id: Some("keep".into()),
            ..Default::default()
        },));
        let mut host = TestHost(world);
        run(&mut host, "gfo", json!({ "gid": "menu", "nid": "other" }));
        let focused: Option<String> = host
            .0
            .query::<&GuiComponent>()
            .iter()
            .next()
            .and_then(|(_, g)| g.focused_node_id.clone());
        assert_eq!(focused, Some("keep".into()));
    }

    #[test]
    fn ui_visibility_and_anchor_are_applied() {
        let (mut host, id) = host_with((UIElement { visible: true, ..Default::default() },));
        run(&mut host, "suv", json!({ "e": id, "v": false }));
        run(&mut host, "sup", json!({ "e": id, "a": "TopRight", "ox": 5.0, "oy": -7.0 }));
        let e = host.resolve_entity(id).unwrap();
        let el = host.0.get::<&UIElement>(e).unwrap();
        assert!(!el.visible);
        assert_eq!(el.anchor, UIAnchor::from_name("TopRight"));
        assert_eq!(el.offset, glam::Vec2::new(5.0, -7.0));
    }

    #[test]
    fn the_state_machine_goes_back_even_when_there_is_no_animator() {
        // `ssm` removes the state machine to mutate it. If it failed to put it
        // back when the animator is missing, the entity would lose its state
        // machine permanently — a much worse outcome than the no-op intended.
        let (mut host, id) = host_with((AnimStateMachine::default(),));
        run(&mut host, "ssm", json!({ "e": id, "state": "idle" }));
        let e = host.resolve_entity(id).unwrap();
        assert!(
            host.0.get::<&AnimStateMachine>(e).is_ok(),
            "the state machine was removed and never restored"
        );
    }

    #[test]
    fn skeletal_time_never_goes_negative() {
        let (mut host, id) = host_with((SkeletalPlayer::default(),));
        run(&mut host, "sat2", json!({ "e": id, "v": -5.0 }));
        let e = host.resolve_entity(id).unwrap();
        assert_eq!(host.0.get::<&SkeletalPlayer>(e).unwrap().time, 0.0);
    }

    #[test]
    fn sprite_anchor_defaults_to_the_centre() {
        let (mut host, id) = host_with((SpriteComponent { anchor: [0.0, 0.0], ..Default::default() },));
        run(&mut host, "ssa", json!({ "e": id }));
        let e = host.resolve_entity(id).unwrap();
        assert_eq!(host.0.get::<&SpriteComponent>(e).unwrap().anchor, [0.5, 0.5]);
    }

    #[test]
    fn z_order_reaches_the_sprite_and_the_text() {
        use crate::ecs::components::TextComponent;
        let (mut host, id) = host_with((SpriteComponent::default(), TextComponent::default()));
        run(&mut host, "szo", json!({ "e": id, "z": 42 }));
        let e = host.resolve_entity(id).unwrap();
        assert_eq!(host.0.get::<&SpriteComponent>(e).unwrap().z_order, 42);
        assert_eq!(host.0.get::<&TextComponent>(e).unwrap().z_order, 42);
    }

    #[test]
    fn z_order_reaches_a_mesh() {
        use crate::ecs::components::Mesh2D;
        let (mut host, id) = host_with((Mesh2D::default(),));
        run(&mut host, "szo", json!({ "e": id, "z": 7 }));
        let e = host.resolve_entity(id).unwrap();
        assert_eq!(host.0.get::<&Mesh2D>(e).unwrap().z_order, 7);
    }

    #[test]
    fn emitter_rate_and_active_flag_are_applied() {
        let (mut host, id) = host_with((ParticleEmitter::default(),));
        run(&mut host, "sea", json!({ "e": id, "a": true }));
        run(&mut host, "ser", json!({ "e": id, "r": 12.5 }));
        let e = host.resolve_entity(id).unwrap();
        let em = host.0.get::<&ParticleEmitter>(e).unwrap();
        assert!(em.active);
        assert_eq!(em.emission_rate, 12.5);
    }

    #[test]
    fn animation_commands_reach_skeletal_players_in_the_browser_hosts() {
        let (mut host, id) = host_with((SkeletalPlayer { clip: "idle".into(), ..Default::default() }, crate::animation::ik::IkTwoBone::default()));
        run(&mut host, "pa", json!({ "e": id, "clip": "walk" }));
        run(&mut host, "sas", json!({ "e": id, "speed": 2.0 }));
        run(&mut host, "sabl", json!({ "e": id, "clip": "run", "w": 0.75 }));
        run(&mut host, "ikw", json!({ "e": id, "w": 0.25 }));
        let e = host.resolve_entity(id).unwrap();
        {
            let p = host.0.get::<&SkeletalPlayer>(e).unwrap();
            assert_eq!((p.clip.as_str(), p.speed, p.blend_clip.as_str(), p.blend), ("walk", 2.0, "run", 0.75));
            assert_eq!(host.0.get::<&crate::animation::ik::IkTwoBone>(e).unwrap().weight, 0.25);
        }
        run(&mut host, "caf", json!({ "e": id, "clip": "run", "s": 0.3 }));
        assert_eq!(host.0.get::<&SkeletalPlayer>(e).unwrap().fade.as_ref().map(|f| f.duration), Some(0.3));
        run(&mut host, "sa", json!({ "e": id }));
        assert!(!host.0.get::<&SkeletalPlayer>(e).unwrap().playing);
    }

    #[test]
    fn any_emitter_field_is_written_by_name() {
        let (mut host, id) = host_with((ParticleEmitter::default(),));
        run(&mut host, "sef", json!({ "e": id, "f": "angle", "v": [-2.0, -1.5] }));
        run(&mut host, "sef", json!({ "e": id, "f": "max_particles", "v": 900.0 }));
        run(&mut host, "sef", json!({ "e": id, "f": "turbulence.strength", "v": 12.0 }));
        run(&mut host, "sef", json!({ "e": id, "f": "no_such_field", "v": 1.0 }));
        let mut probe = ParticleEmitter::default();
        assert!(crate::particles::set_emitter_field(&mut probe, "no_such_field", &json!(1.0)).is_err());
        assert!(crate::particles::set_emitter_field(&mut probe, "particle_source", &json!("x")).is_err());
        assert!(crate::particles::set_emitter_field(&mut probe, "speed", &json!("fast")).is_err());
        let e = host.resolve_entity(id).unwrap();
        let em = host.0.get::<&ParticleEmitter>(e).unwrap();
        assert_eq!(em.angle, [-2.0, -1.5]);
        assert_eq!(em.max_particles, 900);
        let t = em.turbulence.expect("created with its defaults");
        assert_eq!((t.strength, t.scale), (12.0, 120.0));
    }

    #[test]
    fn hitbox_stats_are_overwritten_together() {
        let (mut host, id) = host_with((Hitbox::default(),));
        run(&mut host, "shb", json!({ "e": id, "dmg": 7.0, "kb": 2.0, "rt": 0.5 }));
        let e = host.resolve_entity(id).unwrap();
        let hb = host.0.get::<&Hitbox>(e).unwrap();
        assert_eq!((hb.damage, hb.knockback, hb.retrigger), (7.0, 2.0, 0.5));
    }

    /// The behaviour BUGS #44 was: native and the editor created a sprite so
    /// `set_sprite_corners` worked on a script-spawned entity; the web runtime
    /// did not, so the call was inert in a shipped build for exactly the
    /// entities most likely to use it.
    #[test]
    fn sprite_corners_materialize_a_sprite_on_an_entity_without_one() {
        let (mut host, id) = host_with((Transform::default(),));
        run(
            &mut host,
            "scor",
            json!({ "e": id, "x0": 1.0, "y0": 2.0, "x1": 3.0, "y1": 4.0,
                    "x2": 5.0, "y2": 6.0, "x3": 7.0, "y3": 8.0 }),
        );
        let e = host.resolve_entity(id).unwrap();
        let sprite = host
            .0
            .get::<&SpriteComponent>(e)
            .expect("no sprite was created, so the corners went nowhere");
        assert_eq!(
            sprite.corners,
            Some([[1.0, 2.0], [3.0, 4.0], [5.0, 6.0], [7.0, 8.0]])
        );
    }

    #[test]
    fn sprite_corners_keep_an_existing_sprite() {
        let (mut host, id) = host_with((SpriteComponent {
            color: [0.25, 0.5, 0.75, 1.0],
            ..Default::default()
        },));
        run(&mut host, "scor", json!({ "e": id, "x0": 1.0 }));
        let e = host.resolve_entity(id).unwrap();
        let sprite = host.0.get::<&SpriteComponent>(e).unwrap();
        assert_eq!(sprite.color, [0.25, 0.5, 0.75, 1.0], "the sprite was replaced");
        assert!(sprite.corners.is_some());
    }

    /// The spawn-then-place case (BUGS #52), which is what this command exists
    /// for: a script places an entity the mirror does not know about yet.
    #[test]
    fn a_deferred_write_places_a_transform() {
        let (mut host, id) = host_with((Transform::default(),));
        run(&mut host, "dtw", json!({ "k": "p", "e": id, "a": 220.0, "b": 120.0 }));
        let e = host.resolve_entity(id).unwrap();
        assert_eq!(
            host.0.get::<&Transform>(e).unwrap().position,
            glam::Vec2::new(220.0, 120.0)
        );
    }

    #[test]
    fn deferred_writes_cover_rotation_scale_and_colour() {
        let (mut host, id) = host_with((Transform::default(), SpriteComponent::default()));
        run(&mut host, "dtw", json!({ "k": "r", "e": id, "a": 1.5 }));
        run(&mut host, "dtw", json!({ "k": "s", "e": id, "a": 2.0, "b": 3.0 }));
        run(
            &mut host,
            "dtw",
            json!({ "k": "c", "e": id, "a": 0.1, "b": 0.2, "c": 0.3, "d": 0.4 }),
        );
        let e = host.resolve_entity(id).unwrap();
        let t = host.0.get::<&Transform>(e).unwrap();
        assert_eq!(t.rotation, 1.5);
        assert_eq!(t.scale, glam::Vec2::new(2.0, 3.0));
        drop(t);
        assert_eq!(host.0.get::<&SpriteComponent>(e).unwrap().color, [0.1, 0.2, 0.3, 0.4]);
    }

    /// `spawn_entity` produces a bare entity, so colouring one before it has a
    /// sprite must be a quiet no-op rather than a panic or a materialized
    /// sprite — the script asked to recolour something that cannot be seen.
    #[test]
    fn a_deferred_colour_on_a_spriteless_entity_does_nothing() {
        let (mut host, id) = host_with((Transform::default(),));
        assert!(run(&mut host, "dtw", json!({ "k": "c", "e": id, "a": 1.0 })));
        let e = host.resolve_entity(id).unwrap();
        assert!(host.0.get::<&SpriteComponent>(e).is_err(), "a sprite was created");
    }

    #[test]
    fn an_unknown_deferred_kind_is_handled_but_changes_nothing() {
        let (mut host, id) = host_with((Transform::default(),));
        assert!(run(&mut host, "dtw", json!({ "k": "zzz", "e": id, "a": 9.0 })));
        let e = host.resolve_entity(id).unwrap();
        assert_eq!(host.0.get::<&Transform>(e).unwrap().position, glam::Vec2::ZERO);
    }

    #[test]
    fn set_parent_attaches_and_detaches() {
        let mut world = World::new();
        let parent = world.spawn((Transform::default(),));
        let child = world.spawn((Transform::default(),));
        let mut host = TestHost(world);

        run(&mut host, "sprt", json!({ "c": child.id(), "p": parent.id() }));
        assert_eq!(host.0.get::<&Parent>(child).unwrap().0, parent);

        // A missing `p` means detach, which is how `set_parent(child)` and
        // `set_parent(child, nil)` reach here.
        run(&mut host, "sprt", json!({ "c": child.id() }));
        assert!(host.0.get::<&Parent>(child).is_err(), "the child stayed parented");
    }

    #[test]
    fn set_parent_refuses_to_parent_an_entity_to_itself() {
        // A self-parent makes `resolve_world_transforms` chase its own tail.
        let (mut host, id) = host_with((Transform::default(),));
        run(&mut host, "sprt", json!({ "c": id, "p": id }));
        let e = host.resolve_entity(id).unwrap();
        assert!(host.0.get::<&Parent>(e).is_err(), "an entity became its own parent");
    }

    #[test]
    fn a_negative_parent_detaches_rather_than_resolving() {
        let mut world = World::new();
        let parent = world.spawn((Transform::default(),));
        let child = world.spawn((Transform::default(), Parent(parent)));
        let mut host = TestHost(world);
        run(&mut host, "sprt", json!({ "c": child.id(), "p": -1 }));
        assert!(host.0.get::<&Parent>(child).is_err());
    }

    #[test]
    fn material_uniforms_are_written_by_type() {
        let (mut host, id) = host_with((SpriteComponent::default(),));
        run(&mut host, "smu", json!({ "e": id, "n": "glow", "ut": "f", "v": 0.5 }));
        run(&mut host, "smu", json!({ "e": id, "n": "steps", "ut": "i", "v": 3 }));
        run(&mut host, "smu", json!({ "e": id, "n": "uv", "ut": "v2", "v": [1.0, 2.0] }));
        run(&mut host, "smu", json!({ "e": id, "n": "tint", "ut": "v4", "v": [1.0, 0.0, 0.0, 1.0] }));
        let e = host.resolve_entity(id).unwrap();
        let m = host.0.get::<&MaterialComponent>(e).unwrap();
        // `UniformValue` has no `PartialEq`, hence the patterns.
        assert!(matches!(m.uniforms.get("glow"), Some(UniformValue::Float(v)) if *v == 0.5));
        assert!(matches!(m.uniforms.get("steps"), Some(UniformValue::Int(3))));
        assert!(matches!(m.uniforms.get("uv"), Some(UniformValue::Vec2([1.0, 2.0]))));
        assert!(matches!(
            m.uniforms.get("tint"),
            Some(UniformValue::Vec4([1.0, 0.0, 0.0, 1.0]))
        ));
    }

    /// The two hosts disagreed: one ignored a non-array value, the other wrote
    /// the uniform as zeros, which silently blanks a material.
    #[test]
    fn a_vector_uniform_given_a_non_array_is_left_alone() {
        let (mut host, id) = host_with((SpriteComponent::default(),));
        run(&mut host, "smu", json!({ "e": id, "n": "uv", "ut": "v2", "v": 7.0 }));
        let e = host.resolve_entity(id).unwrap();
        let blank = host.0.get::<&MaterialComponent>(e).is_err();
        let unset = host
            .0
            .get::<&MaterialComponent>(e)
            .map(|m| !m.uniforms.contains_key("uv"))
            .unwrap_or(true);
        assert!(blank || unset, "a malformed vec2 was written to the material anyway");
    }

    #[test]
    fn a_normal_map_is_set_kept_beside_the_material_and_cleared() {
        let (mut host, id) = host_with((
            SpriteComponent::default(),
            MaterialComponent { material_id: "glow".into(), ..Default::default() },
        ));
        let e = host.resolve_entity(id).unwrap();
        assert!(run(&mut host, "snm", json!({ "e": id, "m": "sprites/bricks_n.png" })));
        {
            let m = host.0.get::<&MaterialComponent>(e).unwrap();
            assert_eq!(m.normal_map_id.as_deref(), Some("sprites/bricks_n.png"));
            assert_eq!(m.material_id, "glow", "setting a normal map replaced the material");
        }
        // The preamble sends "" for nil: that takes it away.
        run(&mut host, "snm", json!({ "e": id, "m": "" }));
        assert_eq!(host.0.get::<&MaterialComponent>(e).unwrap().normal_map_id, None);
    }

    #[test]
    fn a_normal_map_on_a_sprite_with_no_material_makes_one() {
        let (mut host, id) = host_with((SpriteComponent::default(),));
        run(&mut host, "snm", json!({ "e": id, "m": "n.png" }));
        let e = host.resolve_entity(id).unwrap();
        let m = host.0.get::<&MaterialComponent>(e).expect("a material component");
        assert_eq!(m.normal_map_id.as_deref(), Some("n.png"));
        assert_eq!(m.material_id, "default_sprite");
    }

    #[test]
    fn a_material_uniform_with_no_name_is_ignored() {
        let (mut host, id) = host_with((SpriteComponent::default(),));
        run(&mut host, "smu", json!({ "e": id, "n": "", "ut": "f", "v": 1.0 }));
        let e = host.resolve_entity(id).unwrap();
        assert!(
            host.0.get::<&MaterialComponent>(e).is_err(),
            "an unnamed uniform created a material component"
        );
    }

    #[test]
    fn state_machine_parameters_take_their_documented_defaults() {
        let (mut host, id) = host_with((AnimStateMachine::default(),));
        // Omitted `value` is 0.0 for a float and false for a bool.
        assert!(run(&mut host, "saf", json!({ "e": id, "param": "speed" })));
        assert!(run(&mut host, "sab", json!({ "e": id, "param": "grounded" })));
        assert!(run(&mut host, "sat", json!({ "e": id, "param": "jump" })));
    }
}
