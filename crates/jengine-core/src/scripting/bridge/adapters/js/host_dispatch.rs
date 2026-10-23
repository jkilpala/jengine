//! Script commands both browser hosts handle the same way, but which need
//! more of the host than the world - audio, tweens, the save system, input,
//! particles, fades, time scale. One implementation.
//!
//! The editor viewport and the web runtime each carried these arms in their
//! command loop (`drain_play_commands_into`). Measured in Oct 2026, 30 of the
//! 64 tags both hosts handle had arms that were the same code once formatting
//! was set aside; these are 29 of them (`evt` calls into the host's script
//! instances, so it stays there). Commands that need only the world live in
//! [`crate::scripting::host_commands`], which a native test can reach; these
//! need browser-only parts (`WebAudioBackend`), so they live here, beside the
//! script mirror. See `docs/architecture-debt.md`, #2.
//!
//! A host lends its parts as [`HostParts`], whose fields carry the hosts' own
//! names, so the arms read as they did in the hosts. A `continue` in a host's
//! arm meant "next command"; here it is `return true` - handled, nothing to do.

use hecs::{Entity, World};

use crate::ecs::components::Transform;
use crate::particles::ParticleEmitter;
use wasm_bindgen::{JsCast, JsValue};

use crate::scripting::bridge::adapters::js::mirror::jsvalue_to_json;

/// The physics world a host lends. Core has a stub `PhysicsWorld` without
/// its `physics` feature, so a host fills the field the same way either way;
/// the physics arms below act only with the feature, and otherwise consume
/// their tags quietly.
pub type PhysicsSlot = crate::physics::PhysicsWorld;


/// What these commands change in a host.
pub struct HostParts<'a> {
    pub world: &'a mut World,
    pub mixer: &'a mut crate::audio::AudioMixer,
    pub web_audio: &'a mut crate::audio::web_audio::WebAudioBackend,
    pub tween_manager: &'a mut crate::animation::tween::TweenManager,
    pub save_system: &'a mut crate::save::SaveSystem,
    pub input: &'a mut crate::input::InputState,
    pub particle_system: &'a mut crate::particles::ParticleSystem,
    pub behavior_system: &'a mut crate::behavior::BehaviorGraphSystem,
    pub fade_active: &'a mut Option<crate::fade_overlay::TransitionState>,
    pub time_scale: &'a mut f32,
    /// The host's handle-or-id resolution (its spawn map first).
    pub find_entity_by_id: fn(&World, u32) -> Option<Entity>,
    /// Record a script-spawned entity under the handle the script holds (the
    /// host's spawn map).
    pub note_spawn: fn(u32, Entity),
    /// Sub-commands no arm knows, reported once by the host after the loop.
    pub unhandled_kinds: &'a mut std::collections::HashSet<String>,
    /// The physics world, when the host has one.
    pub physics: Option<&'a mut PhysicsSlot>,
    /// Where a warning goes: the editor's console panel, the browser console.
    pub warn: &'a mut dyn FnMut(String),
    /// The post-process stack, once the host has a GPU.
    pub post_process: Option<&'a mut crate::renderer::backend::wgpu::WgpuPostProcessStack>,
    /// This frame's `draw_debug_*` shapes. The web runtime draws them; the
    /// editor hands them to its overlay as JSON.
    pub debug_draws: &'a mut Vec<crate::debug_draw::DebugDrawCommand>,
    /// Atlases (`set_sprite_region`) and `.gui` documents (`gui_set`).
    pub assets: &'a mut crate::asset::AssetManager,
    /// Builds a spawned prefab's components.
    pub registry: &'a crate::package::component_registry::ComponentRegistry,
    /// `camera_shake` requests, which the host's camera takes.
    pub pending_camera_shakes: &'a mut Vec<(f32, f32, f32)>,
    /// Clips `play_sound` has warned about once already.
    pub audio_warned: &'a mut std::collections::HashSet<String>,
    /// The running scripts (`adapters::js::scripts`).
    pub script_fns: &'a mut super::scripts::Instances,
    /// Compiled scripts by path, so `load_script` compiles each once.
    pub script_cache: &'a mut std::collections::HashMap<String, super::BrowserScriptFns>,
    /// Spawned entities whose `on_spawn` has run.
    pub spawn_notified: &'a mut std::collections::HashSet<u32>,
    /// `gui_on_click` bindings: `(document, node)` to `(entity, handler)`.
    pub gui_click_callbacks: &'a mut std::collections::HashMap<(String, String), (u32, String)>,
}

/// The tags [`handle_host_command`] handles - kept in `host_commands`, which
/// native tests can read, as [`HOST_DISPATCH_TAGS`](crate::scripting::host_commands::HOST_DISPATCH_TAGS).
pub use crate::scripting::host_commands::HOST_DISPATCH_TAGS as HOST_COMMAND_TAGS;

thread_local! {
    static WARNED: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// True the first time `key` is seen for the life of the page: for a warning
/// about something a script names that does not exist, which it will name
/// again every frame. `unhandled_kinds` is not that - it lives for one drain
/// and the host reports it afterwards as an unsupported command, so a missing
/// atlas region was announced every frame as a command the runtime lacked.
fn warn_once(key: String) -> bool {
    WARNED.with(|w| w.borrow_mut().insert(key))
}

pub fn js_value_f64(obj: &js_sys::Object, key: &str) -> Option<f64> {
    js_sys::Reflect::get(obj, &JsValue::from_str(key)).ok().and_then(|v| v.as_f64())
}

/// A string off a command object.
pub fn js_value_string(obj: &js_sys::Object, key: &str) -> Option<String> {
    js_sys::Reflect::get(obj, &JsValue::from_str(key)).ok().and_then(|v| v.as_string())
}

/// Apply one command whose tag is in [`HOST_COMMAND_TAGS`]; `false` for any
/// other tag, which the host then handles itself.
pub fn handle_host_command(kind: &str, cmd_obj: &js_sys::Object, app: &mut HostParts<'_>) -> bool {
    match kind {
        // set_time_scale(). Tag is `tsc`, not `ts` — `ts` is tween_scale.
        "tsc" => {
            let s = js_value_f64(&cmd_obj, "s").unwrap_or(1.0) as f32;
            *app.time_scale = s.max(0.0);
            crate::time::set_world_scale(*app.time_scale);
        }

        // Tweens.
        "tp" => {
            use crate::animation::easing::EasingFunction;
            use crate::animation::tween::TweenTarget;
            let eid = js_value_f64(&cmd_obj, "e").unwrap_or(0.0) as u64;
            let x = js_value_f64(&cmd_obj, "x").unwrap_or(0.0) as f32;
            let y = js_value_f64(&cmd_obj, "y").unwrap_or(0.0) as f32;
            let dur = js_value_f64(&cmd_obj, "dur").unwrap_or(0.0) as f32;
            let ease = js_value_string(&cmd_obj, "ease").unwrap_or_default();
            let tid = js_value_f64(&cmd_obj, "tid").unwrap_or(0.0) as u32;
            if let Some(entity) = (app.find_entity_by_id)(&app.world, eid as u32) {
                let start = match app.world.get::<&Transform>(entity) {
                    Ok(t) => glam::Vec4::new(t.position.x, t.position.y, 0.0, 0.0),
                    Err(_) => return true,
                };
                app.tween_manager.add_tween_with_id(
                    tid,
                    // The real id: `tick_tweens` resolves by it, and a spawn handle
                    // is not one once hecs has reused a freed slot.
                    entity.id() as u64,
                    TweenTarget::Position,
                    start,
                    glam::Vec4::new(x, y, 0.0, 0.0),
                    dur,
                    EasingFunction::from_name(&ease),
                );
            }
        }

        "tr" => {
            use crate::animation::easing::EasingFunction;
            use crate::animation::tween::TweenTarget;
            let eid = js_value_f64(&cmd_obj, "e").unwrap_or(0.0) as u64;
            let r = js_value_f64(&cmd_obj, "r").unwrap_or(0.0) as f32;
            let dur = js_value_f64(&cmd_obj, "dur").unwrap_or(0.0) as f32;
            let ease = js_value_string(&cmd_obj, "ease").unwrap_or_default();
            let tid = js_value_f64(&cmd_obj, "tid").unwrap_or(0.0) as u32;
            if let Some(entity) = (app.find_entity_by_id)(&app.world, eid as u32) {
                let start = match app.world.get::<&Transform>(entity) {
                    Ok(t) => glam::Vec4::new(t.rotation, 0.0, 0.0, 0.0),
                    Err(_) => return true,
                };
                app.tween_manager.add_tween_with_id(
                    tid,
                    // The real id: `tick_tweens` resolves by it, and a spawn handle
                    // is not one once hecs has reused a freed slot.
                    entity.id() as u64,
                    TweenTarget::Rotation,
                    start,
                    glam::Vec4::new(r, 0.0, 0.0, 0.0),
                    dur,
                    EasingFunction::from_name(&ease),
                );
            }
        }

        "ts" => {
            use crate::animation::easing::EasingFunction;
            use crate::animation::tween::TweenTarget;
            let eid = js_value_f64(&cmd_obj, "e").unwrap_or(0.0) as u64;
            let sx = js_value_f64(&cmd_obj, "sx").unwrap_or(1.0) as f32;
            let sy = js_value_f64(&cmd_obj, "sy").unwrap_or(1.0) as f32;
            let dur = js_value_f64(&cmd_obj, "dur").unwrap_or(0.0) as f32;
            let ease = js_value_string(&cmd_obj, "ease").unwrap_or_default();
            let tid = js_value_f64(&cmd_obj, "tid").unwrap_or(0.0) as u32;
            if let Some(entity) = (app.find_entity_by_id)(&app.world, eid as u32) {
                let start = match app.world.get::<&Transform>(entity) {
                    Ok(t) => glam::Vec4::new(t.scale.x, t.scale.y, 0.0, 0.0),
                    Err(_) => return true,
                };
                app.tween_manager.add_tween_with_id(
                    tid,
                    // The real id: `tick_tweens` resolves by it, and a spawn handle
                    // is not one once hecs has reused a freed slot.
                    entity.id() as u64,
                    TweenTarget::Scale,
                    start,
                    glam::Vec4::new(sx, sy, 0.0, 0.0),
                    dur,
                    EasingFunction::from_name(&ease),
                );
            }
        }

        "tc" => {
            use crate::animation::easing::EasingFunction;
            use crate::animation::tween::TweenTarget;
            let eid = js_value_f64(&cmd_obj, "e").unwrap_or(0.0) as u64;
            let r = js_value_f64(&cmd_obj, "r").unwrap_or(1.0) as f32;
            let g = js_value_f64(&cmd_obj, "g").unwrap_or(1.0) as f32;
            let b = js_value_f64(&cmd_obj, "b").unwrap_or(1.0) as f32;
            let a = js_value_f64(&cmd_obj, "a").unwrap_or(1.0) as f32;
            let dur = js_value_f64(&cmd_obj, "dur").unwrap_or(0.0) as f32;
            let ease = js_value_string(&cmd_obj, "ease").unwrap_or_default();
            let tid = js_value_f64(&cmd_obj, "tid").unwrap_or(0.0) as u32;
            if let Some(entity) = (app.find_entity_by_id)(&app.world, eid as u32) {
                let Some(start) = crate::ecs::systems::tween_color_of(&app.world, entity) else { return true };
                app.tween_manager.add_tween_with_id(
                    tid,
                    // The real id: `tick_tweens` resolves by it, and a spawn handle
                    // is not one once hecs has reused a freed slot.
                    entity.id() as u64,
                    TweenTarget::SpriteColor,
                    start,
                    glam::Vec4::new(r, g, b, a),
                    dur,
                    EasingFunction::from_name(&ease),
                );
            }
        }

        "ct" => {
            let tid = js_value_f64(&cmd_obj, "tid").unwrap_or(0.0) as u32;
            app.tween_manager.cancel_tween(tid);
        }

        "cat" => {
            let eid = js_value_f64(&cmd_obj, "e").unwrap_or(0.0) as u64;
            // Tweens are keyed by the real id (see `"tp"`), so a handle is resolved.
            let eid = (app.find_entity_by_id)(&app.world, eid as u32).map_or(eid, |e| e.id() as u64);
            app.tween_manager.cancel_all(eid);
        }

        // Particle emitter control.
        "eb" => {
            let eid = js_value_f64(&cmd_obj, "e").unwrap_or(-1.0) as i64;
            let count = js_value_f64(&cmd_obj, "c").unwrap_or(0.0) as u32;
            if eid >= 0 {
                if let Some(entity) = (app.find_entity_by_id)(&app.world, eid as u32) {
                    let snap = app
                        .world
                        .get::<&ParticleEmitter>(entity)
                        .ok()
                        .map(|em| (*em).clone())
                        .zip(crate::particles::emitter_position(&app.world, entity));
                    if let Some((em, pos)) = snap {
                        app.particle_system.burst(eid as u64, &em, pos, count);
                    }
                }
            }
        }

        // Light edits (the web runtime renders 2D lights).
        "slr" => {
            use crate::ecs::components::{PointLight2D, SpotLight2D};
            let eid = js_value_f64(&cmd_obj, "e").unwrap_or(-1.0) as i64;
            let radius = js_value_f64(&cmd_obj, "r").unwrap_or(0.0) as f32;
            if eid < 0 {
                return true;
            }
            let target = eid as u32;
            // The spawn map first, then the world: `find_entity_by_id`'s rule.
            let entity = (app.find_entity_by_id)(&app.world, target);
            if let Some(e) = entity {
                if let Ok(mut l) = app.world.get::<&mut PointLight2D>(e) {
                    l.radius = radius;
                }
                if let Ok(mut l) = app.world.get::<&mut SpotLight2D>(e) {
                    l.radius = radius;
                }
            }
        }

        "psa" => {
            // `play_sound_at`: as "ps", at the volume and pan of where it
            // happens (`audio::spatial`).
            let clip = js_value_string(&cmd_obj, "c").unwrap_or_default();
            let x = js_value_f64(&cmd_obj, "x").unwrap_or(0.0) as f32;
            let y = js_value_f64(&cmd_obj, "y").unwrap_or(0.0) as f32;
            let vol = js_value_f64(&cmd_obj, "v").unwrap_or(1.0) as f32;
            let pitch = js_value_f64(&cmd_obj, "p").unwrap_or(1.0) as f32;
            let (gain, pan) = crate::audio::spatial::at_point(&app.world, x, y);
            let sid = js_value_f64(&cmd_obj, "sid").unwrap_or(0.0).max(0.0) as u64;
            if let Ok(id) = app.web_audio.play_sound_on_bus(
                &mut app.mixer,
                crate::audio::SFX_BUS,
                &clip,
                vol * gain,
                pitch,
                pan,
                false,
            ) {
                crate::audio::music::alias(sid, id);
            }
        }

        // Music is the director's (`audio::music`), driven once a frame.
        "pm" => {
            let clip = js_value_string(&cmd_obj, "c").unwrap_or_default();
            let vol = js_value_f64(&cmd_obj, "v").unwrap_or(1.0) as f32;
            let looping = js_sys::Reflect::get(&cmd_obj, &JsValue::from_str("l"))
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            crate::audio::music::request(crate::audio::music::Request::Play {
                layers: vec![(clip, vol)],
                looping,
                fade: 0.0,
            });
        }

        // A script's sound id names the sound it became (`music::alias`).
        "sts" => {
            let h = crate::audio::music::resolve(js_value_f64(&cmd_obj, "h").unwrap_or(0.0).max(0.0) as u64);
            app.web_audio.stop_id(h);
            app.mixer.unregister_track(h);
        }

        "ssv" => {
            let h = crate::audio::music::resolve(js_value_f64(&cmd_obj, "h").unwrap_or(0.0).max(0.0) as u64);
            let v = js_value_f64(&cmd_obj, "v").unwrap_or(1.0) as f32;
            if let Some(out) = app.mixer.set_track_volume(h, v) {
                app.web_audio.set_volume(h, out);
            }
        }

        "stm" => crate::audio::music::stop(0.0),

        "pam" => crate::audio::music::request(crate::audio::music::Request::Pause),

        "rem" => crate::audio::music::request(crate::audio::music::Request::Resume),

        // The `*_all` commands were aliased onto the music-only ones, so a
        // long one-shot played straight through `stop_all_sounds`.
        "ssbc" => {
            let clip = js_value_string(&cmd_obj, "c").unwrap_or_default();
            app.web_audio.stop_sound_by_clip(&clip);
        }

        "stal" => {
            app.web_audio.stop_all_oneshots();
            crate::audio::music::stop(0.0);
        }

        // Music voices are sounds on the music bus, so these reach them.
        "paal" => app.web_audio.pause_all_oneshots(),

        "real" => app.web_audio.resume_all_oneshots(),

        "smv" => {
            let v = js_value_f64(&cmd_obj, "v").unwrap_or(1.0) as f32;
            crate::audio::music::request(crate::audio::music::Request::Volume(v));
        }

        "smav" => {
            let v = js_value_f64(&cmd_obj, "v").unwrap_or(1.0) as f32;
            app.web_audio.set_master_volume(v);
        }

        "cup" => {
            let id = js_value_f64(&cmd_obj, "id").unwrap_or(-1.0) as i64;
            let anchor_str = js_value_string(&cmd_obj, "a").unwrap_or_default();
            let ox = js_value_f64(&cmd_obj, "ox").unwrap_or(0.0) as f32;
            let oy = js_value_f64(&cmd_obj, "oy").unwrap_or(0.0) as f32;
            let w = js_value_f64(&cmd_obj, "w").unwrap_or(64.0) as f32;
            let h = js_value_f64(&cmd_obj, "h").unwrap_or(64.0) as f32;
            // Stage 2.9m — optional bg color, defaults to translucent
            // black to preserve the previous viewport visual when
            // wrappers omit color args.
            let cr = js_value_f64(&cmd_obj, "cr").unwrap_or(0.0) as f32;
            let cg = js_value_f64(&cmd_obj, "cg").unwrap_or(0.0) as f32;
            let cb = js_value_f64(&cmd_obj, "cb").unwrap_or(0.0) as f32;
            let ca = js_value_f64(&cmd_obj, "ca").unwrap_or(0.5) as f32;
            if id < 0 {
                return true;
            }
            let entity = app.world.spawn((
                crate::ecs::components::Name(format!("UIPanel#{id}")),
                crate::ui::UIElement {
                    anchor: crate::ui::UIAnchor::from_name(&anchor_str),
                    offset: glam::Vec2::new(ox, oy),
                    size: glam::Vec2::new(w, h),
                    visible: true,
                    z_order: 99,
                    ..Default::default()
                },
                crate::ui::UIPanel {
                    bg_color: [cr, cg, cb, ca],
                },
            ));
            (app.note_spawn)(id as u32, entity);
        }

        // Stage 2.9g — UI overlay (HUD text/panels) commands. The
        // `id` returned by `cut`/`cup` is a script-allocated handle;
        // we spawn the ECS entity here and register the mapping in
        // `SCRIPT_ENTITY_MAP` so subsequent `sut`/`suv`/`sup` calls
        // resolve through `find_entity_by_id`.
        "cut" => {
            let id = js_value_f64(&cmd_obj, "id").unwrap_or(-1.0) as i64;
            let txt = js_value_string(&cmd_obj, "txt").unwrap_or_default();
            let anchor_str = js_value_string(&cmd_obj, "a").unwrap_or_default();
            let ox = js_value_f64(&cmd_obj, "ox").unwrap_or(0.0) as f32;
            let oy = js_value_f64(&cmd_obj, "oy").unwrap_or(0.0) as f32;
            // Stage 2.9m — optional styling: color/font_size/alignment.
            // Defaults match `UIText::default` (white / 16 / Left) so
            // pre-extension wrapper calls (which omit these fields)
            // produce identical visuals.
            let cr = js_value_f64(&cmd_obj, "cr").unwrap_or(1.0) as f32;
            let cg = js_value_f64(&cmd_obj, "cg").unwrap_or(1.0) as f32;
            let cb = js_value_f64(&cmd_obj, "cb").unwrap_or(1.0) as f32;
            let ca = js_value_f64(&cmd_obj, "ca").unwrap_or(1.0) as f32;
            let font_size = js_value_f64(&cmd_obj, "fs").unwrap_or(16.0) as f32;
            let align_str = js_value_string(&cmd_obj, "al")
                .unwrap_or_else(|| "left".to_string());
            let alignment = match align_str.to_ascii_lowercase().as_str() {
                "center" | "middle" => crate::ui::TextAlign::Center,
                "right" => crate::ui::TextAlign::Right,
                _ => crate::ui::TextAlign::Left,
            };
            if id < 0 {
                return true;
            }
            let entity = app.world.spawn((
                crate::ecs::components::Name(format!("UIText#{id}")),
                crate::ui::UIElement {
                    anchor: crate::ui::UIAnchor::from_name(&anchor_str),
                    offset: glam::Vec2::new(ox, oy),
                    size: glam::Vec2::new(256.0, 32.0),
                    visible: true,
                    z_order: 100,
                    ..Default::default()
                },
                crate::ui::UIText {
                    text: txt,
                    color: [cr, cg, cb, ca],
                    font_size,
                    alignment,
                    ..Default::default()
                },
            ));
            (app.note_spawn)(id as u32, entity);
        }

        // Scene fades + screen flash. Web builds ignored these entirely
        // before the overlay moved into jengine-core.
        "fdin" | "fdou" => {
            let d = js_value_f64(&cmd_obj, "d").unwrap_or(0.0) as f32;
            let color = [
                js_value_f64(&cmd_obj, "r").unwrap_or(0.0) as f32,
                js_value_f64(&cmd_obj, "g").unwrap_or(0.0) as f32,
                js_value_f64(&cmd_obj, "b").unwrap_or(0.0) as f32,
            ];
            *app.fade_active = Some(if kind == "fdin" {
                crate::fade_overlay::TransitionState::new_fade_in(d, color)
            } else {
                crate::fade_overlay::TransitionState::new_fade_out(d, color)
            });
        }

        // 2.4 audio busses. One tag, `op` discriminates.
        "ab" => {
            let name = js_value_string(&cmd_obj, "n").unwrap_or_default();
            let op = js_value_string(&cmd_obj, "op").unwrap_or_default();
            let flag = js_sys::Reflect::get(&cmd_obj, &JsValue::from_str("b"))
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            match op.as_str() {
                "create" => {
                    let parent = js_value_string(&cmd_obj, "p");
                    let volume = js_value_f64(&cmd_obj, "v").unwrap_or(1.0) as f32;
                    app.mixer.create_bus(&name, parent.as_deref(), volume);
                }
                "remove" => {
                    app.mixer.remove_bus(&name);
                }
                "vol" => {
                    let volume = js_value_f64(&cmd_obj, "v").unwrap_or(1.0) as f32;
                    app.mixer.set_volume(&name, volume);
                }
                // `Audio.set_bus_effect(bus, effect, value)`.
                "fx" => {
                    let effect = js_value_string(&cmd_obj, "e").unwrap_or_default();
                    let value = js_value_f64(&cmd_obj, "v").unwrap_or(0.0) as f32;
                    if let Err(e) = app.mixer.set_effect(&name, &effect, value) {
                        log::warn!("Audio.set_bus_effect: {e}");
                    }
                }
                "mute" => app.mixer.set_muted(&name, flag),
                "solo" => app.mixer.set_solo(&name, flag),
                other => {
                    app.unhandled_kinds.insert(format!("ab:{other}"));
                }
            }
        }

        // 2.5 input remapping. One tag, `op` discriminates - see
        // `preamble_command_tags`'s INTENTIONALLY_SHARED.
        "ib" => {
            let op = js_value_string(&cmd_obj, "op").unwrap_or_default();
            let action = js_value_string(&cmd_obj, "a").unwrap_or_default();
            match op.as_str() {
                "set" => {
                    let keys: Vec<String> =
                        js_sys::Reflect::get(&cmd_obj, &JsValue::from_str("k"))
                            .ok()
                            .filter(|v| v.is_instance_of::<js_sys::Array>())
                            .map(|v| {
                                let arr: js_sys::Array = v.unchecked_into();
                                arr.iter().filter_map(|k| k.as_string()).collect()
                            })
                            .unwrap_or_default();
                    app.input.set_bindings(&action, keys);
                }
                "add" => {
                    if let Some(key) = js_value_string(&cmd_obj, "key") {
                        app.input.add_binding(&action, &key);
                    }
                }
                "remove" => {
                    if let Some(key) = js_value_string(&cmd_obj, "key") {
                        app.input.remove_binding(&action, &key);
                    }
                }
                "clear" => app.input.clear_bindings(&action),
                "reset" => app.input.reset_bindings(),
                "dz" => {
                    let v = js_value_f64(&cmd_obj, "v").unwrap_or(0.0) as f32;
                    app.input.set_dead_zone(&action, v);
                }
                other => {
                    app.unhandled_kinds.insert(format!("ib:{other}"));
                }
            }
        }

        // 2.3 save system. Reads are served by the preamble's
        // `save_data` mirror; only writes come through here.
        "svst" => {
            let slot = js_value_string(&cmd_obj, "s").unwrap_or_default();
            let key = js_value_string(&cmd_obj, "k").unwrap_or_default();
            if !slot.is_empty() && !key.is_empty() {
                let value = js_sys::Reflect::get(&cmd_obj, &JsValue::from_str("v"))
                    .unwrap_or(JsValue::NULL);
                app.save_system.set(&slot, &key, jsvalue_to_json(&value));
            }
        }


        // Physics. Each was in both hosts, the same but for spelling: these
        // are the editor's, which ignore a missing entity id where the web
        // host's pushed entity 0. Tags are consumed with no physics world, so
        // a build without physics stays quiet. A script's handle is resolved
        // to its entity first - physics bodies are keyed by `entity.id()`,
        // and a spawned entity's handle is not its id (BUGS #43).
        "sv" => {
            let eid = js_value_f64(&cmd_obj, "e").unwrap_or(0.0) as u64;
            let vx = js_value_f64(&cmd_obj, "vx").unwrap_or(0.0) as f32;
            let vy = js_value_f64(&cmd_obj, "vy").unwrap_or(0.0) as f32;
            if let Some(entity) = (app.find_entity_by_id)(&app.world, eid as u32) {
                if let Ok(mut v) = app.world.get::<&mut crate::ecs::components::Velocity2D>(entity) {
                    v.x = vx;
                    v.y = vy;
                }
                #[cfg(feature = "physics")]
                if let Some(p) = app.physics.as_deref_mut() {
                    p.set_velocity(entity.id() as u64, vx, vy);
                }
            }
        }
        "af" | "ai" | "sav" => {
            let eid = js_value_f64(&cmd_obj, "e").unwrap_or(-1.0) as i64;
            let entity = if eid >= 0 { (app.find_entity_by_id)(&app.world, eid as u32) } else { None };
            #[cfg(feature = "physics")]
            if let Some(entity) = entity
                && let Some(p) = app.physics.as_deref_mut()
            {
                let id = entity.id() as u64;
                match kind {
                    "af" => p.apply_force(
                        id,
                        js_value_f64(&cmd_obj, "fx").unwrap_or(0.0) as f32,
                        js_value_f64(&cmd_obj, "fy").unwrap_or(0.0) as f32,
                    ),
                    "ai" => p.apply_impulse(
                        id,
                        js_value_f64(&cmd_obj, "ix").unwrap_or(0.0) as f32,
                        js_value_f64(&cmd_obj, "iy").unwrap_or(0.0) as f32,
                    ),
                    _ => p.set_angular_velocity(id, js_value_f64(&cmd_obj, "o").unwrap_or(0.0) as f32),
                }
            }
            #[cfg(not(feature = "physics"))]
            let _ = entity;
        }
        "sbt" => {
            use crate::ecs::components::BodyType;
            let eid = js_value_f64(&cmd_obj, "e").unwrap_or(-1.0) as i64;
            let bt = js_value_string(&cmd_obj, "bt").unwrap_or_default();
            // Both the component and the body (`set_entity_body_type`):
            // `ensure_physics_bodies` makes the body match the component
            // every frame, so writing either alone does nothing.
            let body_type = BodyType::from_script_name(&bt).unwrap_or_else(|| {
                (app.warn)(format!("set_body_type: unknown body type '{bt}' - using Dynamic"));
                BodyType::Dynamic
            });
            let entity = if eid >= 0 { (app.find_entity_by_id)(&app.world, eid as u32) } else { None };
            #[cfg(feature = "physics")]
            if let Some(entity) = entity
                && let Some(physics) = app.physics.as_deref_mut()
            {
                crate::ecs::systems::set_entity_body_type(&app.world, physics, entity, body_type);
            }
            #[cfg(not(feature = "physics"))]
            let _ = (entity, body_type);
        }
        "sg" => {
            // Both components default to 0.0: `set_gravity(0)` - gravity off,
            // the second argument forgotten - must not leave Earth gravity.
            let gx = js_value_f64(&cmd_obj, "gx").unwrap_or(0.0) as f32;
            let gy = js_value_f64(&cmd_obj, "gy").unwrap_or(0.0) as f32;
            #[cfg(feature = "physics")]
            if let Some(p) = app.physics.as_deref_mut() {
                p.set_gravity(gx, gy);
            }
            #[cfg(not(feature = "physics"))]
            let _ = (gx, gy);
        }
        // Joints and motors are addressed by entity *name*.
        "sms" | "rj" => {
            let en = js_value_string(&cmd_obj, "en").unwrap_or_default();
            #[cfg(feature = "physics")]
            if let Some(physics) = app.physics.as_deref_mut() {
                let found = app
                    .world
                    .query::<&crate::ecs::components::Name>()
                    .iter()
                    .find(|(_, n)| n.0 == en)
                    .map(|(e, _)| e.id() as u64);
                if let Some(id) = found {
                    if kind == "sms" {
                        physics.set_motor_speed(id, js_value_f64(&cmd_obj, "sp").unwrap_or(0.0) as f32);
                    } else {
                        physics.remove_joint(id);
                    }
                }
            }
            #[cfg(not(feature = "physics"))]
            let _ = en;
        }
        "aj" => {
            let en = js_value_string(&cmd_obj, "en").unwrap_or_default();
            let tn = js_value_string(&cmd_obj, "tn").unwrap_or_default();
            let jj = js_value_string(&cmd_obj, "jj").unwrap_or_default();
            #[cfg(feature = "physics")]
            if let Some(physics) = app.physics.as_deref_mut() {
                let mut entity_a = None;
                let mut entity_b = None;
                for (e, name) in app.world.query::<&crate::ecs::components::Name>().iter() {
                    if name.0 == en {
                        entity_a = Some(e.id() as u64);
                    }
                    if name.0 == tn {
                        entity_b = Some(e.id() as u64);
                    }
                }
                match (entity_a, entity_b) {
                    (Some(a), Some(b)) => match serde_json::from_str::<crate::ecs::components::Joint2D>(&jj) {
                        Ok(joint) => physics.add_joint(a, b, &joint),
                        Err(e) => (app.warn)(format!("add_joint: invalid joint JSON: {e}")),
                    },
                    _ => (app.warn)(format!("add_joint: entity '{en}' or target '{tn}' not found")),
                }
            }
            #[cfg(not(feature = "physics"))]
            let _ = (en, tn, jj);
        }

        // Debug shapes. A colour left out is white - the web host's rule; the
        // editor forwarded the raw object and its overlay drew `rgb(NaN, ..)`,
        // which is no stroke at all.
        "ddl" | "ddr" | "ddc" => {
            use crate::debug_draw::DebugDrawCommand;
            let f = |k: &str, d: f64| js_value_f64(&cmd_obj, k).unwrap_or(d) as f32;
            let (r, g, b) = (f("r", 1.0), f("g", 1.0), f("b", 1.0));
            app.debug_draws.push(match kind {
                "ddl" => DebugDrawCommand::Line {
                    x1: f("x1", 0.0),
                    y1: f("y1", 0.0),
                    x2: f("x2", 0.0),
                    y2: f("y2", 0.0),
                    r,
                    g,
                    b,
                },
                "ddr" => DebugDrawCommand::Rect { x: f("x", 0.0), y: f("y", 0.0), w: f("w", 0.0), h: f("h", 0.0), r, g, b },
                _ => DebugDrawCommand::Circle { x: f("x", 0.0), y: f("y", 0.0), radius: f("rad", 0.0), r, g, b },
            });
        }
        // Post-process effects, by name. Dropped until the host has a GPU.
        "sppe" => {
            let n = js_value_string(&cmd_obj, "n").unwrap_or_default();
            let v = js_sys::Reflect::get(&cmd_obj, &JsValue::from_str("v"))
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !n.is_empty()
                && let Some(pp) = app.post_process.as_deref_mut()
            {
                pp.set_effect_enabled(&n, v);
            }
        }
        "sppp" => {
            use crate::renderer::material::UniformValue;
            let n = js_value_string(&cmd_obj, "n").unwrap_or_default();
            let p = js_value_string(&cmd_obj, "p").unwrap_or_default();
            let utype = js_value_string(&cmd_obj, "ut").unwrap_or_default();
            let raw = js_sys::Reflect::get(&cmd_obj, &JsValue::from_str("v")).unwrap_or(JsValue::NULL);
            let value: Option<UniformValue> = match utype.as_str() {
                "f" => raw.as_f64().map(|v| UniformValue::Float(v as f32)),
                "i" => raw.as_f64().map(|v| UniformValue::Int(v as i32)),
                "v2" => raw.is_instance_of::<js_sys::Array>().then(|| {
                    let arr: js_sys::Array = raw.clone().unchecked_into();
                    UniformValue::Vec2([arr.get(0).as_f64().unwrap_or(0.0) as f32, arr.get(1).as_f64().unwrap_or(0.0) as f32])
                }),
                "v4" => raw.is_instance_of::<js_sys::Array>().then(|| {
                    let arr: js_sys::Array = raw.clone().unchecked_into();
                    UniformValue::Vec4([
                        arr.get(0).as_f64().unwrap_or(0.0) as f32,
                        arr.get(1).as_f64().unwrap_or(0.0) as f32,
                        arr.get(2).as_f64().unwrap_or(0.0) as f32,
                        arr.get(3).as_f64().unwrap_or(0.0) as f32,
                    ])
                }),
                _ => None,
            };
            if !n.is_empty()
                && !p.is_empty()
                && let (Some(v), Some(pp)) = (value, app.post_process.as_deref_mut())
            {
                pp.set_effect_param(&n, &p, v);
            }
        }

        // Swap a sprite to a named atlas region - the native
        // `SetSpriteRegion`: both `texture_id` and `uv_rect` from the loaded
        // atlas, a default sprite made on an entity with none, and one log
        // line per unknown atlas and region.
        "ssr" => {
            use crate::ecs::components::SpriteComponent;
            let eid = js_value_f64(&cmd_obj, "e").unwrap_or(-1.0) as i64;
            let atlas_id = js_value_string(&cmd_obj, "a").unwrap_or_default();
            let region = js_value_string(&cmd_obj, "r").unwrap_or_default();
            if eid >= 0 && !atlas_id.is_empty() && !region.is_empty() {
                let uv = app.assets.get_atlas_data(&atlas_id).and_then(|a| a.get_uv_rect(&region));
                if let Some(uv) = uv {
                    if let Some(entity) = (app.find_entity_by_id)(&app.world, eid as u32) {
                        if app.world.get::<&SpriteComponent>(entity).is_err() {
                            let _ = app.world.insert_one(entity, SpriteComponent::default());
                        }
                        if let Ok(mut sprite) = app.world.get::<&mut SpriteComponent>(entity) {
                            sprite.texture_id = atlas_id.as_str().into();
                            sprite.uv_rect = Some(uv);
                            // Resolved, so a prefab's pending region does not
                            // overwrite it (as native).
                            sprite.region = Some(region.clone());
                            sprite.region_resolved = true;
                        }
                    }
                } else if warn_once(format!("ssr:{atlas_id}:{region}")) {
                    (app.warn)(format!("[Script] set_sprite_region: atlas '{atlas_id}' region '{region}' not found"));
                }
            }
        }
        // `gui_on_click(entity, document, node, handler)`: the binding the
        // host's click dispatch hit-tests.
        "goc" => {
            let eid = js_value_f64(&cmd_obj, "e").unwrap_or(-1.0) as i64;
            let gid = js_value_string(&cmd_obj, "gid").unwrap_or_default();
            let nid = js_value_string(&cmd_obj, "nid").unwrap_or_default();
            let cb = js_value_string(&cmd_obj, "cb").unwrap_or_else(|| "on_gui_click".to_string());
            if eid >= 0 && !gid.is_empty() && !nid.is_empty() {
                app.gui_click_callbacks.insert((gid, nid), (eid as u32, cb));
            }
        }
        "svcm" => {
            let slot = js_value_string(&cmd_obj, "s").unwrap_or_default();
            if let Err(e) = app.save_system.commit(&slot) {
                (app.warn)(format!("save_commit('{slot}') failed: {e}"));
            }
        }
        "svcl" => {
            let slot = js_value_string(&cmd_obj, "s").unwrap_or_default();
            app.save_system.clear(&slot);
        }

        // Spawn an empty entity, known to the script by `idx`.
        "se" => {
            let name = js_value_string(&cmd_obj, "name").unwrap_or_else(|| "Entity".to_string());
            let idx = js_value_f64(&cmd_obj, "idx").unwrap_or(-1.0) as i64;
            if idx >= 0 {
                let entity = app.world.spawn((crate::ecs::components::Name(name), Transform::default()));
                (app.note_spawn)(idx as u32, entity);
            }
        }
        // Spawn a prefab: its component bundle through the registry (an
        // unknown key kept as custom data), then its parts (`kids`).
        "spf" => {
            let name = js_value_string(&cmd_obj, "name").unwrap_or_else(|| "Prefab".to_string());
            let idx = js_value_f64(&cmd_obj, "idx").unwrap_or(-1.0) as i64;
            let data_json = js_value_string(&cmd_obj, "data");
            if idx < 0 {
                return true;
            }
            let mut builder = hecs::EntityBuilder::new();
            builder.add(crate::ecs::components::Name(name.clone()));
            let mut had_transform = false;
            if let Some(json) = data_json.as_deref()
                && !json.is_empty()
            {
                match serde_json::from_str::<std::collections::HashMap<String, serde_json::Value>>(json) {
                    Ok(map) => {
                        had_transform = map.contains_key("transform");
                        let unknown = app.registry.deserialize_into_builder(&mut builder, &map);
                        if !unknown.is_empty() {
                            builder.add(crate::ecs::components::CustomComponents(unknown));
                        }
                    }
                    Err(e) => (app.warn)(format!("spawn_prefab '{name}' parse failed: {e}")),
                }
            }
            if !had_transform {
                builder.add(Transform::default());
            }
            let entity = app.world.spawn(builder.build());
            let kids = js_value_string(&cmd_obj, "kids");
            if let Err(e) = crate::scripting::bridge::adapters::js::publish::spawn_prefab_parts(
                app.world,
                app.registry,
                entity,
                kids.as_deref(),
            ) {
                (app.warn)(format!("spawn_prefab '{name}' parts did not parse: {e}"));
            }
            (app.note_spawn)(idx as u32, entity);
        }
        // Spawn a `.rig` from the host's assets, every bone an entity; the
        // script's handle names the root bone, as native's does.
        "srig" => {
            let rig_id = js_value_string(&cmd_obj, "r").unwrap_or_default();
            let idx = js_value_f64(&cmd_obj, "idx").unwrap_or(-1.0) as i64;
            if idx < 0 {
                return true;
            }
            let Some(rig) = app.assets.get_skeleton_rig(&rig_id).cloned() else {
                if warn_once(format!("srig:{rig_id}")) {
                    (app.warn)(format!("[Script] spawn_rig: rig '{rig_id}' not found"));
                }
                return true;
            };
            if let Some(root) = crate::animation::rig::spawn_rig_root(app.world, &rig) {
                (app.note_spawn)(idx as u32, root);
            }
        }
        // Camera shake, queued for the host's camera.
        "cmsh" => {
            let amplitude = js_value_f64(&cmd_obj, "a").unwrap_or(0.0) as f32;
            let duration = js_value_f64(&cmd_obj, "d").unwrap_or(0.0) as f32;
            let frequency = js_value_f64(&cmd_obj, "f").unwrap_or(0.0) as f32;
            app.pending_camera_shakes.push((amplitude, duration, frequency));
        }
        // `screen_flash(duration, r, g, b, a)`.
        "scrf" => {
            let d = js_value_f64(&cmd_obj, "d").unwrap_or(0.0) as f32;
            let color = [
                js_value_f64(&cmd_obj, "r").unwrap_or(1.0) as f32,
                js_value_f64(&cmd_obj, "g").unwrap_or(1.0) as f32,
                js_value_f64(&cmd_obj, "b").unwrap_or(1.0) as f32,
            ];
            let alpha = js_value_f64(&cmd_obj, "a").unwrap_or(1.0) as f32;
            *app.fade_active = Some(crate::fade_overlay::TransitionState::new_flash(d, color, alpha));
        }
        // `play_sound`: on the `sfx` bus, tracked by the mixer so a later bus
        // change reaches it, and the id the script holds aliased to it
        // (`audio::music::alias`). A failing clip warns once.
        "ps" => {
            let clip = js_value_string(&cmd_obj, "c").unwrap_or_default();
            let vol = js_value_f64(&cmd_obj, "v").unwrap_or(1.0) as f32;
            let pitch = js_value_f64(&cmd_obj, "p").unwrap_or(1.0) as f32;
            let pan = js_value_f64(&cmd_obj, "pan").unwrap_or(0.0) as f32;
            let sid = js_value_f64(&cmd_obj, "sid").unwrap_or(0.0).max(0.0) as u64;
            let looping = js_sys::Reflect::get(&cmd_obj, &JsValue::from_str("l"))
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let played = app.web_audio.play_sound_on_bus(
                app.mixer,
                crate::audio::SFX_BUS,
                &clip,
                vol,
                pitch,
                pan,
                looping,
            );
            match played {
                Ok(id) => crate::audio::music::alias(sid, id),
                Err(e) => {
                    if app.audio_warned.insert(clip.clone()) {
                        (app.warn)(format!("[Audio] play_sound('{clip}') - {e} (further occurrences suppressed)"));
                    }
                }
            }
        }
        // `gui_set(document, node, property, value)`. The value is tried as
        // JSON and else kept as a string - the native translator's coercion,
        // so numbers and colours do not arrive quoted.
        "gsp" => {
            let gid = js_value_string(&cmd_obj, "gid").unwrap_or_default();
            let nid = js_value_string(&cmd_obj, "nid").unwrap_or_default();
            let prop = js_value_string(&cmd_obj, "p").unwrap_or_default();
            let raw = js_value_string(&cmd_obj, "v").unwrap_or_default();
            if gid.is_empty() || nid.is_empty() || prop.is_empty() {
                return true;
            }
            let value = crate::ui::gui_document::gui_prop_value(&prop, raw);
            let Some(doc) = app.assets.get_gui_mut(&gid) else {
                (app.warn)(format!("gui_set: gui '{gid}' not loaded"));
                return true;
            };
            let Some(node) = doc.root.find_node_mut(&nid) else {
                (app.warn)(format!("gui_set: node '{nid}' not in '{gid}'"));
                return true;
            };
            if !crate::ui::gui_document::apply_gui_node_prop(node, prop.as_str(), &value) {
                (app.warn)(format!("gui_set: unknown property '{prop}'"));
            }
        }

        // `destroy_entity`: its `on_despawn` first, while it still exists,
        // then everything keyed by it - its script, its tweens (hecs hands
        // the id to the next spawn, and a tween left running would drive the
        // newcomer), its spawn handle.
        "de" => {
            let Some(eid) = js_value_f64(&cmd_obj, "e") else { return true };
            if let Some(e) = (app.find_entity_by_id)(app.world, eid as u32) {
                super::scripts::call_handler(app.script_fns, e.id(), "on_despawn", app.warn);
                app.script_fns.remove(&e.id());
                app.spawn_notified.remove(&e.id());
                app.tween_manager.cancel_all(e.id() as u64);
                let _ = app.world.despawn(e);
                super::scripts::SCRIPT_ENTITY_MAP.with(|m| m.borrow_mut().retain(|_, v| *v != e));
            }
        }
        // `load_script(entity, path)`: attach a script at runtime, from the
        // seeded `prefab_scripts` (what `require` resolves against), compiled
        // once a path, and its `init` run at once - as a scene's are.
        "ls" => {
            let eid = js_value_f64(&cmd_obj, "e").unwrap_or(-1.0);
            let path = js_value_string(&cmd_obj, "p").unwrap_or_default();
            if eid < 0.0 || path.is_empty() {
                return true;
            }
            let entity_id = eid as u32;
            let fns = match app.script_cache.get(&path).cloned() {
                Some(f) => f,
                None => {
                    let source = super::mirror::play_namespace()
                        .and_then(|p| js_sys::Reflect::get(&p, &JsValue::from_str("prefab_scripts")).ok())
                        .filter(|v| v.is_object())
                        .and_then(|v| {
                            js_sys::Reflect::get(&v.unchecked_into::<js_sys::Object>(), &JsValue::from_str(&path)).ok()
                        })
                        .and_then(|v| v.as_string());
                    let Some(src) = source else {
                        (app.warn)(format!("load_script: no script source for '{path}'"));
                        return true;
                    };
                    let language = if path.to_ascii_lowercase().ends_with(".lua") { "Lua" } else { "JavaScript" };
                    match super::scripts::compile_browser_script(&path, &src, language) {
                        Ok(f) => {
                            app.script_cache.insert(path.clone(), f.clone());
                            f
                        }
                        Err(e) => {
                            (app.warn)(format!("load_script '{path}' failed to compile: {e}"));
                            return true;
                        }
                    }
                }
            };
            if let Err(e) = super::scripts::call_script_fn(&fns.init, entity_id, 0.0) {
                (app.warn)(format!("load_script '{path}' init failed: {e}"));
            }
            app.script_fns.insert(entity_id, super::scripts::ScriptInstance { fns, label: path });
        }
        // `call(caller, target, method, ...)`: a function on another entity's
        // script, called with the caller first. Both compile paths expose
        // `lookup(name)`, so one dispatch serves Lua and JS scripts.
        "csm" => {
            let target = js_value_f64(&cmd_obj, "tgt").unwrap_or(-1.0);
            let caller = js_value_f64(&cmd_obj, "c").unwrap_or(-1.0);
            let method = js_value_string(&cmd_obj, "m").unwrap_or_default();
            if target < 0.0 || method.is_empty() {
                return true;
            }
            let Some(inst) = app.script_fns.get(&(target as u32)) else {
                (app.warn)(format!("call: entity {target} has no script"));
                return true;
            };
            let Some(f) = super::scripts::handler(inst, &method) else {
                (app.warn)(format!("call: '{method}' not found on '{}'", inst.label));
                return true;
            };
            let args = js_sys::Array::new();
            args.push(&JsValue::from_f64(caller));
            if let Ok(extra) = js_sys::Reflect::get(&cmd_obj, &JsValue::from_str("a"))
                && extra.is_instance_of::<js_sys::Array>()
            {
                let extra: js_sys::Array = extra.unchecked_into();
                for v in extra.iter() {
                    args.push(&v);
                }
            }
            if let Err(e) = f.apply(&JsValue::NULL, &args) {
                let msg = e.as_string().unwrap_or_else(|| format!("{e:?}"));
                (app.warn)(format!("call '{method}' on '{}' failed: {msg}", inst.label));
            }
        }
        // `send_event` / `send_event_to`: the listeners, then - for a
        // broadcast - behaviour graphs' `OnEvent`.
        "evt" => {
            let name = js_value_string(&cmd_obj, "n").unwrap_or_default();
            let sender = js_value_f64(&cmd_obj, "e").unwrap_or(-1.0) as i64;
            let data_str = js_value_string(&cmd_obj, "d");
            if name.is_empty() {
                return true;
            }
            let data_js = match data_str.as_deref() {
                Some(s) => JsValue::from_str(s),
                None => JsValue::NULL,
            };
            let target = js_value_f64(&cmd_obj, "tgt").map(|v| v as u32);
            super::scripts::dispatch_event(app.script_fns, &name, sender as u32, &data_js, target, app.warn);
            if target.is_none() {
                let data: Option<serde_json::Value> = data_str.as_deref().and_then(|s| serde_json::from_str(s).ok());
                app.behavior_system.deliver_script_event(&name, data.as_ref(), app.world);
            }
        }
        _ => return false,
    }
    true
}
