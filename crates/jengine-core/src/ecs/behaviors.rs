//! Built-in behavior systems restored after the systems.rs purge.
//!
//! These were previously inlined in `ecs/systems.rs`. They live here as a
//! separate module so the file can stay small and focused; `systems.rs`
//! re-exports every public item via `pub use behaviors::*;` so existing
//! call-sites (`jengine_core::ecs::systems::update_lifetimes`, …) keep
//! working unchanged.

use crate::detmath::DetMath;
#[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host", feature = "native"))]
use std::collections::HashMap;

use glam::Vec2;
use hecs::World;

use super::components::{
    Acceleration2D, AngularVelocity, AxisDrive, ClampBounds, DespawnOutOfBounds, EntityRef, ExpireAction,
    FaceVelocity, Flicker, FollowTarget, Gravity2D, InputActionImpulse, InputAxis2D, Lifetime, MaxSpeed,
    Name, Oscillator, PatrolMode, PatrolPathRef, SeekTarget, Shake, SpriteComponent, Tags,
    Transform, TriggerArea2D, TriggerOverlaps, TriggerShape, Tween, TweenComplete,
    TweenTarget as ComponentTweenTarget, Velocity2D, WorldTransform, WrapBounds,
};
use super::hierarchy::world_position;

// ── Lifetime / despawn ──────────────────────────────────────────────────

/// Tick `Lifetime.remaining -= dt` and return the entities that should be
/// despawned this frame (those whose `on_expire == Despawn`). The caller
/// is responsible for actually calling `world.despawn(entity)` — keeping
/// this pure avoids the borrow-checker dance of mutating the world while
/// iterating it.
pub fn update_lifetimes(world: &mut World, dt: f32) -> Vec<hecs::Entity> {
    let _span = crate::profiling::trace::span("lifetimes");
    let mut expired = Vec::new();
    for (e, life) in world.query_mut::<&mut Lifetime>() {
        life.remaining -= dt;
        if life.remaining <= 0.0 && matches!(life.on_expire, ExpireAction::Despawn) {
            expired.push(e);
        }
    }
    expired
}

/// Has the camera left `offset` behind? Only along an axis it is moving on,
/// and only on the side it is moving away from.
///
/// This was one dot product with the camera's whole travel, and a still
/// camera counted everything outside as behind. Both deleted what was still
/// to come: an endless runner spawns its next chunk ahead of the view, so a
/// still frame (the ready screen, a pause) removed it, and a camera rising
/// with a jump could outweigh a far-ahead tile's horizontal lead. An endless
/// runner's hero ran on ground its package still collided with and nobody drew.
fn behind(offset: glam::Vec2, reach: glam::Vec2, travel: glam::Vec2) -> bool {
    const MOVING: f32 = 1e-3;
    let axis = |o: f32, r: f32, t: f32| o.abs() > r && t.abs() > MOVING && o * t < 0.0;
    axis(offset.x, reach.x, travel.x) || axis(offset.y, reach.y, travel.y)
}

thread_local! {
    /// What `despawn_offscreen` last saw of the floating origin, and the jump
    /// it owes the next frame's travel.
    static OFFSCREEN_ORIGIN: std::cell::Cell<(Option<(f64, f64)>, glam::Vec2)> =
        const { std::cell::Cell::new((None, glam::Vec2::ZERO)) };
}

/// Entities carrying [`DespawnOffscreen`] that have left the view.
///
/// `view` is the camera's centre and its half-extents - what
/// `Camera2D::half_extents` returns, so a turned camera is handled by
/// asking for a box big enough to hold its corners. `travel` is how far the
/// camera moved since the last frame, which is what `behind_only` means by
/// "behind" (see [`behind`]): a camera that is not moving has no behind, so
/// a `behind_only` entity waits for it to move - and nor does one that jumped
/// further than the view in one frame, which has cut rather than travelled.
/// And a `behind_only` entity is only ever behind once it has been in view:
/// the pass records that on the component, which is why it takes the world
/// mutably.
///
/// Returns the entities to despawn; the caller despawns them, as it does
/// for `update_lifetimes`.
pub fn despawn_offscreen(
    world: &mut World,
    view: (glam::Vec2, glam::Vec2),
    travel: glam::Vec2,
) -> Vec<hecs::Entity> {
    use crate::ecs::components::DespawnOffscreen;
    let (centre, half) = view;
    // The view is the renderer's camera, which follows a floating origin
    // only in `update_camera`, after this: on the frame the world is moved
    // back it is still where the world was, and everything by the car would
    // read as far behind it. Look where it is about to be - and give the
    // next frame's travel the jump back, which it will otherwise read as the
    // camera backing up.
    let (by, owed) = OFFSCREEN_ORIGIN.with(|o| {
        let (mut seen, owed) = o.get();
        let by = crate::ecs::floating_origin::follow(&mut seen);
        o.set((seen, by));
        (by, owed)
    });
    let centre = centre - by;
    let travel = travel + owed;
    // A camera that moved further than the view is wide in one frame has cut,
    // not travelled: a restart, a respawn, a level change. Nothing it jumped
    // away from is "behind" it in the sense of passed - and a runner restarting
    // at the beginning of its track spawns the new run's next chunk just off
    // screen on the very frame it cuts back, so reading the cut as travel
    // deleted that chunk (an endless runner's invisible ground at 25 m).
    let cut = travel.x.abs() > 2.0 * half.x || travel.y.abs() > 2.0 * half.y;
    let travel = if cut { glam::Vec2::ZERO } else { travel };
    let mut gone = Vec::new();
    for (e, (rule, transform)) in world.query_mut::<(&mut DespawnOffscreen, &Transform)>() {
        let offset = transform.position - centre;
        let outside = offset.x.abs() > half.x + rule.margin || offset.y.abs() > half.y + rule.margin;
        if !outside {
            rule.seen = true;
            continue;
        }
        // Only what the camera has reached can be behind it (see `seen`).
        if rule.behind_only && (!rule.seen || !behind(offset, half + glam::Vec2::splat(rule.margin), travel)) {
            continue;
        }
        gone.push(e);
    }
    gone
}

// ── Step 2D motion behaviors ────────────────────────────────────────────

/// The motion step, in one place: every host calls this and nothing else.
///
/// It was three hand-copied sequences - native's `tick_world`, the editor
/// viewport's and the web runtime's - which happened to agree, and a fourth
/// component added to one of them would have been silently missing from the
/// others (`motion_reaches_every_runtime` checks each host calls this and
/// none calls the pieces). Runs after the frame's input and behaviour graphs
/// and before physics.
///
/// `batch_owns_motion`: the host runs batch scripts, which integrate their
/// own entities' velocity and position (`update(ctx, q)` writes
/// `Velocity2D` / `Position2D`). The integrators here then leave those
/// entities as the batch left them. Before this, native integrated them a
/// second time - Test Lab scene 58's drifters moved about twice as far
/// natively as in the editor, where batch scripts do not run at all.
pub fn tick_motion(
    world: &mut World,
    dt: f32,
    paths: Option<&crate::ecs::path::Path2DLibrary>,
    batch_owns_motion: bool,
) {
    use crate::profiling::trace::{span, time};
    let _span = span("motion");
    // The batch's word on its entities, put back after the integrators.
    let owned: Vec<(hecs::Entity, Vec2, Velocity2D)> = if batch_owns_motion {
        world
            .query::<(&super::components::BatchScriptComponent, &Transform, &Velocity2D)>()
            .iter()
            .map(|(e, (_, t, v))| (e, t.position, v.clone()))
            .collect()
    } else {
        Vec::new()
    };

    // Water running along a curve (`ecs::flow`) eases what floats in it
    // towards its velocity, before the velocities are integrated.
    crate::feature!("flow", { time("flow", || crate::ecs::flow::tick_flows(world, dt)) });
    time("motion.integrate", || {
        crate::ecs::systems::velocity_system(world, dt);
        update_gravity(world, dt);
        update_acceleration(world, dt);
        update_forces(world, dt);
        update_seek_targets(world, dt);
        update_max_speed(world);
    });

    for (e, pos, vel) in owned {
        if let Ok(mut t) = world.get::<&mut Transform>(e) {
            t.position = pos;
        }
        if let Ok(mut v) = world.get::<&mut Velocity2D>(e) {
            *v = vel;
        }
    }

    time("motion.follow_patrol", || {
        update_follow_targets(world, dt);
        update_patrol_paths(world, paths, dt);
    });
    // Behaviour trees (`ecs::behavior_tree`), before navigation so a
    // `move_to` steering a `nav_agent` is walked this frame.
    crate::feature!("behavior_tree", { time("behavior_trees", || crate::ecs::behavior_tree::tick_behavior_trees(world, dt)) });
    // Riders on drawn curves (`path_follow`).
    time("path_follow", || crate::ecs::path_follow::tick_path_follow(world, dt));
    crate::feature!("text_path", { time("text_path", || crate::ecs::text_path::tick_text_paths(world, dt)) });
    crate::feature!("beat_clock", { time("beat_clock", || crate::ecs::beat_clock::tick_beat_clocks(world, dt)) });
    crate::feature!("displacement", { time("displacement", || crate::ecs::displacement::tick_displacement(world, dt)) });
    crate::feature!("smooth_terrain", { time("smooth_terrain", || crate::ecs::smooth_terrain::tick_smooth_terrain(world)) });
    crate::feature!("fog_of_war", { time("fog_of_war", || crate::ecs::fog_of_war::tick_fog(world, dt)) });
    crate::feature!("dialogue", { time("dialogue", || crate::ecs::dialogue::tick_dialogue(world)) });
    crate::feature!("sprite_stack", { time("sprite_stack", || crate::ecs::sprite_stack::tick_stacks(world, dt)) });
    crate::feature!("sprite_tilt", { time("sprite_tilt", || crate::ecs::sprite_tilt::tick_tilts(world, dt)) });
    crate::feature!("afterimage", { time("afterimage", || crate::ecs::afterimage::tick_afterimages(world, dt)) });
    crate::feature!("portal", { time("portal", || crate::ecs::portal::tick_portals(world)) });
    crate::feature!("jelly", { time("jelly", || crate::ecs::jelly::tick_jellies(world, dt)) });
    crate::feature!("lightning", { time("lightning", || crate::ecs::lightning::tick_lightning(world, dt)) });
    crate::feature!("snow", { time("snow", || crate::ecs::snow::tick_snow(world, dt)) });
    crate::feature!("path_morph", { time("path_morph", || crate::ecs::path_morph::tick_morphs(world, dt)) });
    crate::feature!("offscreen_marker", { time("offscreen_marker", || crate::ecs::offscreen_marker::tick_markers(world)) });
    crate::feature!("mode7", { time("mode7", || crate::ecs::mode7::tick_mode7(world)) });
    // A 3D stage (`ecs::stage`): quads projected into their meshes,
    // billboards placed, both given their depth.
    crate::feature!("stage", { time("stage", || crate::ecs::stage::tick_stage(world, dt)) });
    // Navigation: regions rebaked when their colliders move, agents walked
    // (`navigation::tick_navigation`).
    crate::feature!("navigation", { time("navigation", || crate::navigation::tick_navigation(world, dt)) });
    time("motion.tweens", || {
        update_oscillators(world, dt);
        update_tweens(world, dt);
    });
    // Keyframe timelines (`ecs::timeline`), after the tweens so a track
    // has the last word on a field both touch.
    crate::feature!("timeline", { time("timelines", || crate::ecs::timeline::tick_timelines(world, dt)) });
    // Liquids (`ecs::fluid`): poured, flowing, cooling, setting.
    crate::feature!("fluid", { time("fluids", || crate::ecs::fluid::tick_fluids(world, dt)) });
    crate::feature!("water", { time("water", || crate::ecs::water::tick_water(world, dt)) });
    crate::feature!("pixel_sim", { time("pixel_sims", || crate::ecs::pixel_sim::tick_pixel_sims(world, dt)) });
    crate::feature!("smoke", { time("smoke", || crate::ecs::smoke::tick_smoke(world, dt)) });
    crate::feature!("rewind", { time("rewind", || crate::ecs::rewind::tick_rewind(world, dt)) });
    crate::feature!("fog", { time("fog", || crate::ecs::fog::tick_fog(world, dt)) });
    crate::feature!("wfc", { time("wfc", || crate::ecs::wfc::tick_wfc(world, dt)) });
    crate::feature!("vision", { time("vision", || crate::ecs::vision::tick_vision(world)) });
    crate::feature!("day_cycle", { time("day_cycle", || crate::ecs::day_cycle::tick_day_cycle(world, dt)) });
    // Last: what moved this frame is where it is, and the physics step
    // after this follows the shift (`PhysicsWorld::follow_origin`).
    time("floating_origin", || crate::ecs::floating_origin::tick_floating_origin(world));
    // Rich text's typewriters (`renderer::rich_text`).
    // Text with a `text_key`, in the current language (`locale`) - before
    // the typewriters, which restart when their text changes.
    time("text", || {
        crate::locale::apply_text_keys(world);
        crate::renderer::rich_text::tick_reveals(world, dt);
    });
    // Animated tiles' clocks (`tilemap::TilesetDef::animations`).
    crate::tilemap::tick_tile_animations(world, dt);

    let _bounds = span("motion.turn_and_bounds");
    update_face_velocity_by(world, dt);
    update_angular_velocity(world, dt);

    update_wrap_bounds(world);
    update_clamp_bounds(world);
    for e in update_despawn_out_of_bounds(world) {
        let _ = world.despawn(e);
    }

    let shake_expired = update_shake(world, dt);
    remove_expired_shakes(world, &shake_expired);
    let flicker_expired = update_flicker(world, dt);
    remove_expired_flickers(world, &flicker_expired);
}

/// Wind, force, impulse, drag and torque - the components that were batch
/// script vocabulary only until Sep 2026. They were registered, authorable
/// in the editor and documented as motion, and no native system read any of
/// them, so a `drag2d` on a sprite did nothing unless a batch script
/// happened to integrate it. Found by the arcade's scriptless physics toy.
///
/// Unit mass throughout: a force is an acceleration. Drag is
/// `vel *= 1 - drag * dt` per axis, clamped at zero so a large drag stops a
/// body rather than reversing it. An impulse is spent in one frame.
pub fn update_forces(world: &mut World, dt: f32) {
    use super::components::{Drag2D, Force2D, Impulse2D, Torque2D, Wind2D};
    for (_, (vel, w)) in world.query_mut::<(&mut Velocity2D, &Wind2D)>() {
        vel.x += w.x * dt;
        vel.y += w.y * dt;
    }
    for (_, (vel, f)) in world.query_mut::<(&mut Velocity2D, &Force2D)>() {
        vel.x += f.x * dt;
        vel.y += f.y * dt;
    }
    for (_, (vel, i)) in world.query_mut::<(&mut Velocity2D, &mut Impulse2D)>() {
        vel.x += i.x;
        vel.y += i.y;
        i.x = 0.0;
        i.y = 0.0;
    }
    for (_, (vel, d)) in world.query_mut::<(&mut Velocity2D, &Drag2D)>() {
        vel.x *= (1.0 - d.x * dt).max(0.0);
        vel.y *= (1.0 - d.y * dt).max(0.0);
    }
    for (_, (av, t)) in world.query_mut::<(&mut AngularVelocity, &Torque2D)>() {
        av.omega += t.x * dt;
    }
}

/// Apply per-entity `Acceleration2D` to `Velocity2D` (`vel += a * dt`).
pub fn update_acceleration(world: &mut World, dt: f32) {
    for (_, (vel, a)) in world.query_mut::<(&mut Velocity2D, &Acceleration2D)>() {
        vel.x += a.x * dt;
        vel.y += a.y * dt;
    }
}

/// One sound an `AudioSource` wants started this frame.
#[derive(Debug, Clone)]
pub struct AudioPlayRequest {
    pub clip_id: String,
    /// The source's volume, times its distance gain if it is spatial.
    pub volume: f32,
    pub pitch: f32,
    pub looping: bool,
    pub bus_id: Option<String>,
    /// Where it starts in the stereo field (spatial sources).
    pub pan: f32,
    /// The entity it belongs to, for `audio::spatial::SpatialAudio::started`.
    pub entity: hecs::Entity,
    /// It can be muffled by walls: it needs a low-pass of its own.
    pub filter: bool,
}

/// Collect the `AudioSource`s whose `auto_play` has not fired yet, marking
/// them started so each plays exactly once.
///
/// Returns requests rather than playing them because the backend differs per
/// runtime — kira natively, WebAudio in the editor and on the web — and core
/// has no handle to any of them. Each runtime feeds these to its own mixer.
///
/// `AudioSource` was previously read by nothing at all: it deserialized, it
/// showed up in the editor's component list, and it never made a sound.
pub fn collect_autoplay_audio(world: &mut World) -> Vec<AudioPlayRequest> {
    let mut out = Vec::new();
    for (entity, src) in world.query_mut::<&mut crate::ecs::components::AudioSource>() {
        if src.started || !src.auto_play || src.clip_id.is_empty() {
            continue;
        }
        src.started = true;
        out.push(AudioPlayRequest {
            clip_id: src.clip_id.clone(),
            volume: src.volume,
            pitch: src.pitch,
            looping: src.looping,
            bus_id: src.bus_id.clone(),
            pan: 0.0,
            entity,
            filter: src.spatial && src.muffle > 0.0,
        });
    }
    // A spatial source starts at the volume and pan of where it is, so its
    // first frame is not heard at full volume from across the map.
    for r in &mut out {
        let (gain, pan) = crate::audio::spatial::source_start(world, r.entity);
        r.volume *= gain;
        r.pan = pan;
    }
    out
}

/// Hand an `auto_play` source back to [`collect_autoplay_audio`] for the next
/// frame - for a backend whose clip was not ready yet (the browsers decode
/// asynchronously). `started` is set as a source is collected, so without
/// this a sound that could not start yet never would.
pub fn retry_autoplay(world: &mut World, entity: hecs::Entity) {
    if let Ok(mut src) = world.get::<&mut crate::ecs::components::AudioSource>(entity) {
        src.started = false;
    }
}

/// Apply per-entity `Gravity2D` to `Velocity2D` (`vel += g * dt`).
///
/// The only reader of `Gravity2D` used to be `parallel::update_gravity_par`,
/// which no runtime calls and which is rayon-gated to native — so the
/// component was authorable in the editor and inert everywhere. This is the
/// serial twin, sitting next to `update_acceleration` in every tick path.
///
/// Distinct from `Velocity2D.gravity`, which is a scalar downward pull baked
/// into the velocity integrator. `Gravity2D` is a vector and composes: an
/// entity can carry both, and a sideways "gravity" well is the reason to.
pub fn update_gravity(world: &mut World, dt: f32) {
    for (_, (vel, g)) in world.query_mut::<(&mut Velocity2D, &Gravity2D)>() {
        vel.x += g.x * dt;
        vel.y += g.y * dt;
    }
}

/// Clamp velocity magnitude to `MaxSpeed.value`. Run after gravity /
/// acceleration / seek but before integration.
pub fn update_max_speed(world: &mut World) {
    for (_, (vel, ms)) in world.query_mut::<(&mut Velocity2D, &MaxSpeed)>() {
        let speed_sq = vel.x * vel.x + vel.y * vel.y;
        let max_sq = ms.value * ms.value;
        if speed_sq > max_sq && speed_sq > 0.0 {
            let scale = ms.value / speed_sq.sqrt();
            vel.x *= scale;
            vel.y *= scale;
        }
    }
}

/// Drive `Transform.position` from an `Oscillator`. On the first tick,
/// captures the entity's current position as the oscillation center.
pub fn update_oscillators(world: &mut World, dt: f32) {
    use std::f32::consts::TAU;
    for (_, (transform, osc)) in world.query_mut::<(&mut Transform, &mut Oscillator)>() {
        let center = match osc.center {
            Some(c) => c,
            None => {
                let c = [transform.position.x, transform.position.y];
                osc.center = Some(c);
                c
            }
        };
        osc.elapsed += dt;
        let t = osc.elapsed;
        transform.position.x =
            center[0] + (TAU * osc.frequency[0] * t + osc.phase[0]).dsin() * osc.amplitude[0];
        transform.position.y =
            center[1] + (TAU * osc.frequency[1] * t + osc.phase[1]).dsin() * osc.amplitude[1];
    }
}

// ── Input-driven behaviors ──────────────────────────────────────────────

/// Read the global [`crate::input::InputState`] and update entity-level
/// `InputAxis2D.current` plus apply edge-triggered `InputActionImpulse`
/// writes into [`Velocity2D`].
pub fn update_input_actions(world: &mut World, input: &crate::input::InputState) {
    for (_, axis) in world.query_mut::<&mut InputAxis2D>() {
        let mut x = 0.0_f32;
        let mut y = 0.0_f32;
        if !axis.x_neg.is_empty() && input.is_action_pressed(&axis.x_neg) {
            x -= 1.0;
        }
        if !axis.x_pos.is_empty() && input.is_action_pressed(&axis.x_pos) {
            x += 1.0;
        }
        if !axis.y_neg.is_empty() && input.is_action_pressed(&axis.y_neg) {
            y -= 1.0;
        }
        if !axis.y_pos.is_empty() && input.is_action_pressed(&axis.y_pos) {
            y += 1.0;
        }
        let mut v = Vec2::new(x, y);
        if axis.normalize {
            let len_sq = v.length_squared();
            if len_sq > 1.0 {
                v /= len_sq.sqrt();
            }
        }
        axis.current = v * axis.scale;
    }
    // An axis can drive its own entity, so a character moves on arrow keys
    // with no script at all. Before `drive` existed `current` was computed
    // every frame and read by nothing but scripts.
    for (_, (axis, vel, acc)) in
        world.query_mut::<(&InputAxis2D, Option<&mut Velocity2D>, Option<&mut Acceleration2D>)>()
    {
        match axis.drive {
            AxisDrive::None => {}
            AxisDrive::Velocity => {
                if let Some(v) = vel {
                    v.x = axis.current.x;
                    v.y = axis.current.y;
                }
            }
            AxisDrive::Acceleration => {
                if let Some(a) = acc {
                    a.x = axis.current.x;
                    a.y = axis.current.y;
                }
            }
        }
    }

    for (_, (impulse, vel)) in world.query_mut::<(&InputActionImpulse, &mut Velocity2D)>() {
        if impulse.action.is_empty() {
            continue;
        }
        if input.is_action_just_pressed(&impulse.action) {
            if let Some(x) = impulse.impulse_x {
                vel.x = x;
            }
            if let Some(y) = impulse.impulse_y {
                vel.y = y;
            }
        }
    }
}

// ── Trigger areas ───────────────────────────────────────────────────────

/// Compute spatial overlap for every [`TriggerArea2D`] and write the
/// resulting enter/exit/staying lists into the sibling [`TriggerOverlaps`]
/// component (auto-inserted on first tick).
pub fn update_trigger_areas(world: &mut World) {
    let _span = crate::profiling::trace::span("triggers");
    struct Candidate {
        entity: hecs::Entity,
        bits: u64,
        pos: Vec2,
        /// Which of the triggers' filter tags this entity has, one bit each.
        ///
        /// This used to be a cloned `HashSet<String>` per candidate per frame.
        /// Measured at 8,000 tagged entities and a single tag-filtering
        /// trigger, that cloning was **2.4 ms a frame** against 0.24 ms for
        /// the same work without it — ten times the cost of everything else
        /// the system does.
        tag_mask: u64,
        /// Fallback for the rare case of more than 64 distinct filter tags,
        /// where a bitmask cannot represent the vocabulary.
        tags: Option<std::collections::HashSet<String>>,
    }
    // Nothing to test against means nothing to build. The candidate list is
    // over *every* entity with a Transform, so a scene with no triggers was
    // paying an allocation and a `Tags` lookup per entity per frame for a loop
    // that then ran zero times.
    //
    // `needs_tags` is the same idea one level down: cloning a
    // `HashSet<String>` per candidate is only worth it if some trigger
    // actually filters on tags.
    // The vocabulary of tags the triggers actually filter on - usually a
    // handful, often none. An entity's tags are then a bitmask over this list
    // instead of a cloned set.
    let mut any_trigger = false;
    let mut vocab: Vec<String> = Vec::new();
    // Where the triggers reach, as boxes (a circle by its square): an entity
    // outside every box is in no trigger, so it never becomes a candidate.
    let mut reach: Vec<(Vec2, Vec2)> = Vec::new();
    for (_, (tr, w, trigger)) in world.query::<(&Transform, Option<&WorldTransform>, &TriggerArea2D)>().iter() {
        any_trigger = true;
        let at = world_position(tr, w);
        let half = match trigger.shape {
            TriggerShape::Aabb { half_extents } => Vec2::new(half_extents[0], half_extents[1]),
            TriggerShape::Circle { radius } => Vec2::splat(radius),
        };
        reach.push((at - half, at + half));
        for t in &trigger.match_tags {
            if !vocab.iter().any(|v| v == t) {
                vocab.push(t.clone());
            }
        }
    }
    // Nothing to test against means nothing to build. The candidate list is
    // over *every* entity with a Transform, so a scene with no triggers was
    // paying an allocation and a `Tags` lookup per entity per frame for a loop
    // that then ran zero times.
    if !any_trigger {
        return;
    }
    let needs_tags = !vocab.is_empty();
    // A bitmask holds 64 distinct tags. Beyond that, fall back to the old
    // set comparison rather than silently matching the wrong things.
    let use_mask = vocab.len() <= 64;

    // An entity is a candidate only inside some trigger's box. Measured at
    // 8,000 entities and one trigger, building a candidate for every entity -
    // its tags, its position, a push - was ~265 us of a ~280 us call, and the
    // test against the trigger itself ~12 us (architecture-debt #5). With
    // many triggers, each entity is tested against their union instead.
    let union = reach.iter().fold((Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)), |(lo, hi), (a, b)| {
        (lo.min(*a), hi.max(*b))
    });
    let within = |p: Vec2, (lo, hi): (Vec2, Vec2)| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y;
    let near = |p: Vec2| within(p, union) && (reach.len() > 16 || reach.iter().any(|&r| within(p, r)));

    // `Option<&Tags>` in the same query, rather than a `world.get` per entity:
    // that was 8,000 random archetype lookups a frame on top of everything
    // else, and this way the tags arrive with the transform.
    // Positions are world positions - a candidate parented to something is
    // where its parent carries it, as the renderer draws it.
    let candidates: Vec<Candidate> = world
        .query::<(&Transform, Option<&WorldTransform>, Option<&Tags>)>()
        .iter()
        .filter_map(|(e, (t, w, tg))| {
            let pos = world_position(t, w);
            near(pos).then_some((e, pos, tg))
        })
        .map(|(e, pos, tg)| {
            let (tag_mask, tags) = if !needs_tags {
                (0, None)
            } else if use_mask {
                let mask = tg
                    .map(|tg| {
                        let mut m = 0u64;
                        for (i, v) in vocab.iter().enumerate() {
                            if tg.0.contains(v) {
                                m |= 1 << i;
                            }
                        }
                        m
                    })
                    .unwrap_or(0);
                (mask, None)
            } else {
                (0, tg.map(|tg| tg.0.clone()))
            };
            Candidate {
                entity: e,
                bits: e.to_bits().get(),
                pos,
                tag_mask,
                tags,
            }
        })
        .collect();

    let mut updates: Vec<(hecs::Entity, TriggerOverlaps)> = Vec::new();
    for (trigger_entity, (trigger_tr, trigger_w, trigger)) in
        world.query::<(&Transform, Option<&WorldTransform>, &TriggerArea2D)>().iter()
    {
        let trigger_pos = world_position(trigger_tr, trigger_w);
        let mut staying: Vec<u64> = Vec::new();
        for c in &candidates {
            if c.entity == trigger_entity {
                continue;
            }
            if !trigger.match_tags.is_empty() {
                let matched = if use_mask {
                    // Bit i is set when the entity has `vocab[i]`.
                    let want: u64 = trigger
                        .match_tags
                        .iter()
                        .filter_map(|t| vocab.iter().position(|v| v == t))
                        .fold(0u64, |m, i| m | (1 << i));
                    c.tag_mask & want != 0
                } else {
                    match &c.tags {
                        Some(tg) => trigger.match_tags.iter().any(|t| tg.contains(t)),
                        None => false,
                    }
                };
                if !matched {
                    continue;
                }
            }
            let delta = c.pos - trigger_pos;
            let inside = match trigger.shape {
                TriggerShape::Aabb { half_extents } => {
                    delta.x.abs() <= half_extents[0] && delta.y.abs() <= half_extents[1]
                }
                TriggerShape::Circle { radius } => delta.length_squared() <= radius * radius,
            };
            if inside {
                staying.push(c.bits);
            }
        }
        staying.sort_unstable();

        let prev: Vec<u64> = world
            .get::<&TriggerOverlaps>(trigger_entity)
            .ok()
            .map(|o| o.staying.clone())
            .unwrap_or_default();
        let mut entered = Vec::new();
        let mut exited = Vec::new();
        let (mut i, mut j) = (0_usize, 0_usize);
        while i < staying.len() && j < prev.len() {
            match staying[i].cmp(&prev[j]) {
                std::cmp::Ordering::Less => {
                    entered.push(staying[i]);
                    i += 1;
                }
                std::cmp::Ordering::Greater => {
                    exited.push(prev[j]);
                    j += 1;
                }
                std::cmp::Ordering::Equal => {
                    i += 1;
                    j += 1;
                }
            }
        }
        entered.extend_from_slice(&staying[i..]);
        exited.extend_from_slice(&prev[j..]);

        updates.push((
            trigger_entity,
            TriggerOverlaps {
                entered,
                staying,
                exited,
            },
        ));
    }

    for (e, overlaps) in updates {
        if let Ok(mut existing) = world.get::<&mut TriggerOverlaps>(e) {
            *existing = overlaps;
        } else {
            let _ = world.insert_one(e, overlaps);
        }
    }
}

// ── Entity-ref helpers + follow / seek ─────────────────────────────────

/// The tag [`EntityRef::Player`] resolves by.
pub const PLAYER_TAG: &str = "player";

/// Resolve an `EntityRef` to a live `hecs::Entity`. `Runtime`/`SelfEntity`
/// are O(1); `Named` and `Player` are O(N) — fine for occasional lookups but
/// cache the result in `EntityRef::Runtime` if you query every frame.
///
/// `Player` is the first entity tagged `"player"`, the tag every project here
/// already puts on its player. It used to resolve to nothing, so a
/// `FollowTarget` or `SeekTarget` pointed at "Player" - offered in the
/// inspector - silently never moved.
pub fn resolve_entity_ref(
    world: &World,
    r: &EntityRef,
    self_entity: Option<hecs::Entity>,
) -> Option<hecs::Entity> {
    match r {
        EntityRef::Runtime(e) => Some(*e),
        EntityRef::SelfEntity => self_entity,
        EntityRef::Named(name) => world
            .query::<&Name>()
            .iter()
            .find_map(|(e, n)| if n.0 == *name { Some(e) } else { None }),
        EntityRef::Player => world
            .query::<&Tags>()
            .iter()
            .find_map(|(e, t)| t.0.contains(PLAYER_TAG).then_some(e)),
        EntityRef::SceneUuid(_) => None,
    }
}

/// Move entities with `FollowTarget` toward their target's position.
pub fn update_follow_targets(world: &mut World, dt: f32) {
    let mut moves: Vec<(hecs::Entity, Vec2)> = Vec::new();
    for (e, (transform, follow)) in world.query::<(&Transform, &FollowTarget)>().iter() {
        let target_e = match resolve_entity_ref(world, &follow.target, Some(e)) {
            Some(t) if t != e => t,
            _ => continue,
        };
        let Ok(target_tr) = world.get::<&Transform>(target_e) else {
            continue;
        };
        let goal = target_tr.position + Vec2::new(follow.offset[0], follow.offset[1]);
        let delta = goal - transform.position;
        let dist = delta.length();
        if dist <= follow.stop_distance {
            continue;
        }
        let step = (follow.max_speed * dt).min(dist - follow.stop_distance);
        let new_pos = transform.position + delta / dist * step;
        moves.push((e, new_pos));
    }
    for (e, p) in moves {
        if let Ok(mut tr) = world.get::<&mut Transform>(e) {
            tr.position = p;
        }
    }
}

/// Steer `Velocity2D` toward target by accelerating; clamp to `max_speed`,
/// honouring `slowing_radius` for arrival behaviour.
pub fn update_seek_targets(world: &mut World, dt: f32) {
    // (entity, velocity change, distance to target). The distance is kept so
    // the arrival clamp below needs no second lookup: resolving the target
    // twice per seeker made a `Named` target O(N) twice over, per frame.
    let mut adjustments: Vec<(hecs::Entity, Vec2, f32)> = Vec::new();
    for (e, (transform, seek, _vel)) in world
        .query::<(&Transform, &SeekTarget, &Velocity2D)>()
        .iter()
    {
        let target_e = match resolve_entity_ref(world, &seek.target, Some(e)) {
            Some(t) if t != e => t,
            _ => continue,
        };
        let Ok(target_tr) = world.get::<&Transform>(target_e) else {
            continue;
        };
        let to_target = target_tr.position - transform.position;
        let dist = to_target.length();
        if dist < 1e-4 {
            continue;
        }
        let dv = to_target / dist * (seek.accel * dt);
        adjustments.push((e, dv, dist));
    }
    for (e, dv, dist) in adjustments {
        if let Ok(mut vel) = world.get::<&mut Velocity2D>(e) {
            vel.x += dv.x;
            vel.y += dv.y;
            if let Ok(seek) = world.get::<&SeekTarget>(e) {
                let mut effective_max = seek.max_speed;
                if seek.slowing_radius > 0.0 && dist < seek.slowing_radius {
                    let t = (dist / seek.slowing_radius).clamp(0.0, 1.0);
                    effective_max = seek.max_speed * t;
                }
                let speed_sq = vel.x * vel.x + vel.y * vel.y;
                let max_sq = effective_max * effective_max;
                if speed_sq > max_sq && speed_sq > 0.0 {
                    let scale = effective_max / speed_sq.sqrt();
                    vel.x *= scale;
                    vel.y *= scale;
                }
            }
        }
    }
}

// ── Tweens ──────────────────────────────────────────────────────────────

/// Tick `Tween.elapsed`, write the eased value to the targeted property,
/// and handle `on_complete` (Stop / Loop / PingPong).
pub fn update_tweens(world: &mut World, dt: f32) {
    enum Apply {
        PositionX(f32),
        PositionY(f32),
        Rotation(f32),
        ScaleX(f32),
        ScaleY(f32),
        Opacity(f32),
    }
    let mut applies: Vec<(hecs::Entity, Apply)> = Vec::new();
    for (e, tween) in world.query_mut::<&mut Tween>() {
        if tween.duration <= 0.0 {
            continue;
        }
        tween.elapsed += dt;
        let t = (tween.elapsed / tween.duration).min(1.0);
        let completed = tween.elapsed >= tween.duration;
        let eased = tween.ease.apply(t);
        let value = tween.from + (tween.to - tween.from) * eased;

        if completed {
            match tween.on_complete {
                TweenComplete::Stop => {
                    tween.elapsed = tween.duration;
                }
                TweenComplete::Loop => {
                    tween.elapsed = 0.0;
                }
                TweenComplete::PingPong => {
                    std::mem::swap(&mut tween.from, &mut tween.to);
                    tween.elapsed = 0.0;
                }
            }
        }

        let apply = match tween.target {
            ComponentTweenTarget::PositionX => Apply::PositionX(value),
            ComponentTweenTarget::PositionY => Apply::PositionY(value),
            ComponentTweenTarget::Rotation => Apply::Rotation(value),
            ComponentTweenTarget::ScaleX => Apply::ScaleX(value),
            ComponentTweenTarget::ScaleY => Apply::ScaleY(value),
            ComponentTweenTarget::Opacity => Apply::Opacity(value),
        };
        applies.push((e, apply));
    }

    for (e, apply) in applies {
        match apply {
            Apply::Opacity(v) => {
                if let Ok(mut s) = world.get::<&mut SpriteComponent>(e) {
                    s.color[3] = v;
                }
            }
            other => {
                if let Ok(mut tr) = world.get::<&mut Transform>(e) {
                    match other {
                        Apply::PositionX(v) => tr.position.x = v,
                        Apply::PositionY(v) => tr.position.y = v,
                        Apply::Rotation(v) => tr.rotation = v,
                        Apply::ScaleX(v) => tr.scale.x = v,
                        Apply::ScaleY(v) => tr.scale.y = v,
                        Apply::Opacity(_) => unreachable!(),
                    }
                }
            }
        }
    }
}

// ── Patrol paths ────────────────────────────────────────────────────────

/// Walk entities with `PatrolPathRef` along their referenced `Path2D`.
/// Missing / empty paths are skipped silently so an asset typo can't
/// crash a game.
pub fn update_patrol_paths(
    world: &mut World,
    paths: Option<&crate::ecs::path::Path2DLibrary>,
    dt: f32,
) {
    let Some(lib) = paths else { return };
    for (_, (transform, patrol, face)) in
        world.query_mut::<(&mut Transform, &mut PatrolPathRef, Option<&FaceVelocity>)>()
    {
        let Some(path) = lib.get(&patrol.path_id) else {
            continue;
        };
        if path.is_empty() {
            continue;
        }
        if path.len() == 1 {
            if let Some(p) = path.point(0) {
                transform.position = p;
            }
            continue;
        }
        if patrol.index >= path.len() {
            patrol.index = path.len() - 1;
        }
        let Some(target) = path.point(patrol.index) else {
            continue;
        };
        let delta = target - transform.position;
        let dist = delta.length();
        let step = patrol.speed * dt;
        // A patroller with `face_velocity` turns to the way it walks. Patrol
        // writes no velocity, so it used to slide round every corner facing
        // wherever it was authored (Sep 2026).
        if let Some(face) = face
            && dist > 1e-4
        {
            transform.rotation = face.turn(transform.rotation, delta.y.datan2(delta.x), dt);
        }
        if step >= dist {
            transform.position = target;
            advance_patrol_index(patrol, path.len());
        } else if dist > 0.0 {
            transform.position += delta / dist * step;
        }
    }
}

fn advance_patrol_index(patrol: &mut PatrolPathRef, len: usize) {
    match patrol.mode {
        PatrolMode::Loop => {
            patrol.index = (patrol.index + 1) % len;
        }
        PatrolMode::Once => {
            if patrol.index + 1 < len {
                patrol.index += 1;
            }
        }
        PatrolMode::PingPong => {
            if patrol.forward {
                if patrol.index + 1 < len {
                    patrol.index += 1;
                } else {
                    patrol.forward = false;
                    if patrol.index > 0 {
                        patrol.index -= 1;
                    }
                }
            } else if patrol.index > 0 {
                patrol.index -= 1;
            } else {
                patrol.forward = true;
                if len > 1 {
                    patrol.index = 1;
                }
            }
        }
    }
}

// ── Bounds: wrap, clamp, despawn-out-of-bounds ──────────────────────────

/// Wrap `Transform.position` modulo a rectangular world-space box.
pub fn update_wrap_bounds(world: &mut World) {
    for (_, (tr, w)) in world.query_mut::<(&mut Transform, &WrapBounds)>() {
        let dx = w.max[0] - w.min[0];
        let dy = w.max[1] - w.min[1];
        if dx > 0.0 {
            let mut v = (tr.position.x - w.min[0]) % dx;
            if v < 0.0 {
                v += dx;
            }
            tr.position.x = v + w.min[0];
        }
        if dy > 0.0 {
            let mut v = (tr.position.y - w.min[1]) % dy;
            if v < 0.0 {
                v += dy;
            }
            tr.position.y = v + w.min[1];
        }
    }
}

/// Clamp `Transform.position` inside a rectangular world-space box.
pub fn update_clamp_bounds(world: &mut World) {
    for (_, (tr, vel, c)) in
        world.query_mut::<(&mut Transform, Option<&mut Velocity2D>, &ClampBounds)>()
    {
        let mut hit_x = false;
        let mut hit_y = false;
        if tr.position.x < c.min[0] {
            tr.position.x = c.min[0];
            hit_x = true;
        } else if tr.position.x > c.max[0] {
            tr.position.x = c.max[0];
            hit_x = true;
        }
        if tr.position.y < c.min[1] {
            tr.position.y = c.min[1];
            hit_y = true;
        } else if tr.position.y > c.max[1] {
            tr.position.y = c.max[1];
            hit_y = true;
        }
        if c.zero_velocity_on_hit && let Some(v) = vel {
            if hit_x {
                v.x = 0.0;
            }
            if hit_y {
                v.y = 0.0;
            }
        }
    }
}

/// Collect entities whose `Transform.position` has left the
/// `DespawnOutOfBounds` box (with `margin` outward slack).
pub fn update_despawn_out_of_bounds(world: &mut World) -> Vec<hecs::Entity> {
    let mut out = Vec::new();
    for (e, (tr, b)) in world.query::<(&Transform, &DespawnOutOfBounds)>().iter() {
        let min_x = b.min[0] - b.margin;
        let max_x = b.max[0] + b.margin;
        let min_y = b.min[1] - b.margin;
        let max_y = b.max[1] + b.margin;
        if tr.position.x < min_x
            || tr.position.x > max_x
            || tr.position.y < min_y
            || tr.position.y > max_y
        {
            out.push(e);
        }
    }
    out
}

// ── Rotation behaviors ──────────────────────────────────────────────────

/// Rotate `Transform.rotation` to face the entity's `Velocity2D`.
pub fn update_face_velocity(world: &mut World) {
    update_face_velocity_by(world, f32::INFINITY);
}

/// [`update_face_velocity`] over `dt` seconds, so a `turn_rate` turns
/// smoothly. What the motion step calls.
pub fn update_face_velocity_by(world: &mut World, dt: f32) {
    for (_, (tr, vel, fv)) in
        world.query_mut::<(&mut Transform, &Velocity2D, &FaceVelocity)>()
    {
        let speed_sq = vel.x * vel.x + vel.y * vel.y;
        let min_sq = fv.min_speed * fv.min_speed;
        if speed_sq < min_sq {
            continue;
        }
        tr.rotation = fv.turn(tr.rotation, vel.y.datan2(vel.x), dt);
    }
}

/// Apply constant angular velocity to `Transform.rotation`.
pub fn update_angular_velocity(world: &mut World, dt: f32) {
    for (_, (tr, av)) in world.query_mut::<(&mut Transform, &AngularVelocity)>() {
        tr.rotation += av.omega * dt;
    }
}

// ── Visual effects: flicker + shake ─────────────────────────────────────

/// Tick `Flicker` components, toggling sprite alpha. Returns expired
/// entities; caller should pair with [`remove_expired_flickers`].
pub fn update_flicker(world: &mut World, dt: f32) -> Vec<hecs::Entity> {
    let mut expired = Vec::new();
    for (e, (sprite, fl)) in world.query_mut::<(&mut SpriteComponent, &mut Flicker)>() {
        let orig = match fl.original_alpha {
            Some(a) => a,
            None => {
                let a = sprite.color[3];
                fl.original_alpha = Some(a);
                a
            }
        };
        fl.elapsed += dt;
        if fl.elapsed >= fl.duration {
            sprite.color[3] = orig;
            expired.push(e);
            continue;
        }
        let period = if fl.frequency > 0.0 {
            1.0 / fl.frequency
        } else {
            fl.duration.max(1e-3)
        };
        let phase = (fl.elapsed / period).floor() as i64;
        sprite.color[3] = if phase & 1 == 0 { orig } else { 0.0 };
    }
    expired
}

/// Remove expired `Flicker` components (companion to [`update_flicker`]).
pub fn remove_expired_flickers(world: &mut World, expired: &[hecs::Entity]) {
    for e in expired {
        let _ = world.remove_one::<Flicker>(*e);
    }
}

/// Tick `Shake` components, applying a transient additive jitter offset
/// to `Transform.position`. Returns expired entities; caller should pair
/// with [`remove_expired_shakes`].
///
/// Additive for real: last tick's offset comes off before the new one goes
/// on, so movement applied between ticks is kept.
pub fn update_shake(world: &mut World, dt: f32) -> Vec<hecs::Entity> {
    fn next_rand(state: &mut u32) -> f32 {
        *state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        ((*state >> 16) as f32 / 32767.5) - 1.0
    }

    let mut expired = Vec::new();
    for (e, (tr, sh)) in world.query_mut::<(&mut Transform, &mut Shake)>() {
        // Take last tick's jitter back off, leaving wherever the entity has
        // actually moved to.
        if let Some([ox, oy]) = sh.offset.take() {
            tr.position.x -= ox;
            tr.position.y -= oy;
        }
        if sh.seed.is_none() {
            let bits = e.to_bits().get();
            sh.seed = Some(((bits ^ (bits >> 32)) as u32).wrapping_add(0x9E37_79B9));
        }

        sh.elapsed += dt;
        if sh.elapsed >= sh.duration {
            expired.push(e);
            continue;
        }

        let decay = 1.0 - (sh.elapsed / sh.duration).clamp(0.0, 1.0);
        let amp = sh.intensity * decay;

        let freq = sh.frequency.max(1.0);
        let sample_t = sh.elapsed * freq;
        let sample_idx = sample_t.floor() as u32;
        let frac = sample_t - sample_idx as f32;

        let mut s_a = sh.seed.unwrap().wrapping_add(sample_idx.wrapping_mul(2));
        let mut s_b = sh
            .seed
            .unwrap()
            .wrapping_add(sample_idx.wrapping_add(1).wrapping_mul(2));
        let ax = next_rand(&mut s_a);
        let ay = next_rand(&mut s_a);
        let bx = next_rand(&mut s_b);
        let by = next_rand(&mut s_b);
        let ox = (ax + (bx - ax) * frac) * amp;
        let oy = (ay + (by - ay) * frac) * amp;
        tr.position.x += ox;
        tr.position.y += oy;
        sh.offset = Some([ox, oy]);
    }
    expired
}

/// Remove expired `Shake` components (companion to [`update_shake`]).
pub fn remove_expired_shakes(world: &mut World, expired: &[hecs::Entity]) {
    for e in expired {
        let _ = world.remove_one::<Shake>(*e);
    }
}

// ── Batch script dispatcher (native + js-adapter-host only) ─────────────

/// Dispatch every [`super::components::BatchScriptComponent`] in the world
/// through the [`crate::scripting::worker_pool::BatchScriptPool`].
///
/// Native-only; on WASM the `BatchScriptComponent` is a no-op.
#[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host", feature = "native"))]
pub fn run_batch_scripts(
    world: &mut World,
    pool: &mut crate::scripting::worker_pool::BatchScriptPool,
    dt: f32,
    counters: &mut crate::profiling::FrameCounters,
) {
    use super::components::{
        Acceleration2D as Acc, BatchScriptComponent, Drag2D, Force2D, Impulse2D, ScriptLang,
        Torque2D, Wind2D,
    };
    use crate::scripting::worker_pool::{
        OwnedOptionalColumn, OwnedReadColumn, OwnedWriteColumn,
    };

    fn stable_path_hash(path: &str) -> u32 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut h);
        h.finish() as u32
    }

    counters.script_worker_count = pool.worker_count();
    counters.vm_memory_bytes_per_worker = pool.vm_memory_bytes(0);
    counters.batch_workers_active = pool.batch_workers_active();

    pool.advance_frame();

    let mut by_key: HashMap<(ScriptLang, String), Vec<hecs::Entity>> = HashMap::new();
    for (entity, batch) in world.query::<&BatchScriptComponent>().iter() {
        by_key
            .entry((batch.lang, batch.path.clone()))
            .or_default()
            .push(entity);
    }

    let gather = |w: &World, entity: hecs::Entity, name: &str| -> Option<(f32, f32)> {
        match name {
            "Position2D" => w
                .get::<&Transform>(entity)
                .ok()
                .map(|t| (t.position.x, t.position.y)),
            "Velocity2D" => w.get::<&Velocity2D>(entity).ok().map(|v| (v.x, v.y)),
            "Acceleration2D" => w.get::<&Acc>(entity).ok().map(|c| (c.x, c.y)),
            "Drag2D" => w.get::<&Drag2D>(entity).ok().map(|c| (c.x, c.y)),
            "Force2D" => w.get::<&Force2D>(entity).ok().map(|c| (c.x, c.y)),
            "Wind2D" => w.get::<&Wind2D>(entity).ok().map(|c| (c.x, c.y)),
            "Impulse2D" => w.get::<&Impulse2D>(entity).ok().map(|c| (c.x, c.y)),
            "Torque2D" => w.get::<&Torque2D>(entity).ok().map(|c| (c.x, c.y)),
            _ => None,
        }
    };
    let scatter = |w: &mut World, entity: hecs::Entity, name: &str, x: f32, y: f32| -> bool {
        match name {
            "Position2D" => {
                if let Ok(mut t) = w.get::<&mut Transform>(entity) {
                    t.position.x = x;
                    t.position.y = y;
                    true
                } else {
                    false
                }
            }
            "Velocity2D" => {
                if let Ok(mut v) = w.get::<&mut Velocity2D>(entity) {
                    v.x = x;
                    v.y = y;
                    true
                } else {
                    false
                }
            }
            "Acceleration2D" => w
                .get::<&mut Acc>(entity)
                .ok()
                .map(|mut c| {
                    c.x = x;
                    c.y = y;
                })
                .is_some(),
            "Drag2D" => w
                .get::<&mut Drag2D>(entity)
                .ok()
                .map(|mut c| {
                    c.x = x;
                    c.y = y;
                })
                .is_some(),
            "Force2D" => w
                .get::<&mut Force2D>(entity)
                .ok()
                .map(|mut c| {
                    c.x = x;
                    c.y = y;
                })
                .is_some(),
            "Wind2D" => w
                .get::<&mut Wind2D>(entity)
                .ok()
                .map(|mut c| {
                    c.x = x;
                    c.y = y;
                })
                .is_some(),
            "Impulse2D" => w
                .get::<&mut Impulse2D>(entity)
                .ok()
                .map(|mut c| {
                    c.x = x;
                    c.y = y;
                })
                .is_some(),
            "Torque2D" => w
                .get::<&mut Torque2D>(entity)
                .ok()
                .map(|mut c| {
                    c.x = x;
                    c.y = y;
                })
                .is_some(),
            _ => false,
        }
    };

    // The systems run in path order, not the map's, which is new every run.
    let mut by_key: Vec<_> = by_key.into_iter().collect();
    by_key.sort_by(|a, b| a.0.1.cmp(&b.0.1).then_with(|| format!("{:?}", a.0.0).cmp(&format!("{:?}", b.0.0))));
    for ((lang, path), entities) in by_key {
        if entities.is_empty() {
            continue;
        }
        let loaded = match lang {
            ScriptLang::Js => pool.is_js_loaded(&path),
            ScriptLang::Lua => pool.is_lua_loaded(&path),
        };
        if !loaded {
            continue;
        }

        let read_schema: Vec<String> = match lang {
            ScriptLang::Js => pool.js_read_schema(&path).to_vec(),
            ScriptLang::Lua => pool.lua_read_schema(&path).to_vec(),
        };
        let write_schema: Vec<String> = match lang {
            ScriptLang::Js => pool.js_write_schema(&path).to_vec(),
            ScriptLang::Lua => pool.lua_write_schema(&path).to_vec(),
        };
        let opt_schema: Vec<String> = match lang {
            ScriptLang::Js => pool.js_read_optional_schema(&path).to_vec(),
            ScriptLang::Lua => pool.lua_read_optional_schema(&path).to_vec(),
        };

        let read_schema = if read_schema.is_empty() && write_schema.is_empty() {
            vec!["Velocity2D".to_string()]
        } else {
            read_schema
        };
        let write_schema = if write_schema.is_empty() {
            vec!["Position2D".to_string()]
        } else {
            write_schema
        };

        debug_assert!(
            opt_schema.len() <= 8,
            "read_optional schema length capped by validator"
        );
        let mut buckets: HashMap<u8, Vec<hecs::Entity>> = HashMap::new();
        for entity in entities {
            let mut ok = true;
            for name in read_schema.iter().chain(write_schema.iter()) {
                if gather(world, entity, name).is_none() {
                    ok = false;
                    break;
                }
            }
            if !ok {
                continue;
            }
            let mut mask: u8 = 0;
            for (i, name) in opt_schema.iter().enumerate() {
                if gather(world, entity, name).is_some() {
                    mask |= 1 << i;
                }
            }
            buckets.entry(mask).or_default().push(entity);
        }

        for (mask, bucket) in buckets {
            if bucket.is_empty() {
                continue;
            }
            let n = bucket.len();
            let mut read_cols: Vec<(Vec<f32>, Vec<f32>)> = (0..read_schema.len())
                .map(|_| (Vec::with_capacity(n), Vec::with_capacity(n)))
                .collect();
            let mut write_cols: Vec<(Vec<f32>, Vec<f32>)> = (0..write_schema.len())
                .map(|_| (Vec::with_capacity(n), Vec::with_capacity(n)))
                .collect();
            let mut opt_cols: Vec<Option<(Vec<f32>, Vec<f32>)>> = (0..opt_schema.len())
                .map(|i| {
                    if mask & (1 << i) != 0 {
                        Some((Vec::with_capacity(n), Vec::with_capacity(n)))
                    } else {
                        None
                    }
                })
                .collect();
            let mut order: Vec<hecs::Entity> = Vec::with_capacity(n);
            for entity in bucket {
                let reads_ok: Option<Vec<(f32, f32)>> = read_schema
                    .iter()
                    .map(|name| gather(world, entity, name))
                    .collect();
                let Some(reads_v) = reads_ok else { continue };
                let writes_ok: Option<Vec<(f32, f32)>> = write_schema
                    .iter()
                    .map(|name| gather(world, entity, name))
                    .collect();
                let Some(writes_v) = writes_ok else { continue };
                let mut opt_v: Vec<Option<(f32, f32)>> = Vec::with_capacity(opt_schema.len());
                let mut opt_ok = true;
                for (i, name) in opt_schema.iter().enumerate() {
                    let v = gather(world, entity, name);
                    let want_present = mask & (1 << i) != 0;
                    if v.is_some() != want_present {
                        opt_ok = false;
                        break;
                    }
                    opt_v.push(v);
                }
                if !opt_ok {
                    continue;
                }
                for (col, (x, y)) in read_cols.iter_mut().zip(reads_v) {
                    col.0.push(x);
                    col.1.push(y);
                }
                for (col, (x, y)) in write_cols.iter_mut().zip(writes_v) {
                    col.0.push(x);
                    col.1.push(y);
                }
                for (slot, v) in opt_cols.iter_mut().zip(opt_v) {
                    if let (Some(buf), Some((x, y))) = (slot.as_mut(), v) {
                        buf.0.push(x);
                        buf.1.push(y);
                    }
                }
                order.push(entity);
            }

            if order.is_empty() {
                continue;
            }

            let owned_reads: Vec<OwnedReadColumn> = read_schema
                .iter()
                .zip(read_cols)
                .map(|(name, (x, y))| OwnedReadColumn {
                    name: name.clone(),
                    x,
                    y,
                })
                .collect();
            let owned_writes: Vec<OwnedWriteColumn> = write_schema
                .iter()
                .zip(write_cols)
                .map(|(name, (x, y))| OwnedWriteColumn {
                    name: name.clone(),
                    x,
                    y,
                })
                .collect();
            let owned_optionals: Vec<OwnedOptionalColumn> = opt_schema
                .iter()
                .zip(opt_cols)
                .map(|(name, value)| OwnedOptionalColumn {
                    name: name.clone(),
                    value,
                })
                .collect();

            let result = match lang {
                ScriptLang::Js => {
                    let system_id = stable_path_hash(&path);
                    pool.run_js_batch(
                        &path,
                        dt,
                        owned_reads,
                        owned_writes,
                        owned_optionals,
                        system_id,
                        counters,
                    )
                }
                ScriptLang::Lua => {
                    let system_id = stable_path_hash(&path);
                    pool.run_lua_batch(
                        &path,
                        dt,
                        owned_reads,
                        owned_writes,
                        owned_optionals,
                        system_id,
                        counters,
                    )
                }
            };

            match result {
                Ok(output) => {
                    counters.batch_systems_run += 1;
                    counters.batch_chunks += 1;
                    counters.batch_vm_calls += 1;
                    let crate::scripting::worker_pool::RunBatchOutput {
                        writes: out_writes,
                        write_dirty,
                        elapsed_us: _,
                        worker_memory_bytes_max: _,
                        worker_memory_bytes_total: _,
                    } = output;
                    for (col_idx, col) in out_writes.into_iter().enumerate() {
                        if !write_dirty[col_idx] {
                            counters.batch_writeback_scatters_skipped += order.len() as u64;
                            continue;
                        }
                        for (i, entity) in order.iter().enumerate() {
                            scatter(world, *entity, col.name.as_str(), col.x[i], col.y[i]);
                        }
                    }
                }
                Err(e) => {
                    log::warn!(
                        "[BatchScript] script '{}' ({:?}) failed: {}",
                        path,
                        lang,
                        e
                    );
                }
            }
        }
    }
}
