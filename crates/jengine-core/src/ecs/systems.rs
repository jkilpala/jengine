//! Built-in ECS systems.
//!
//! Phase 8 cut every gl-era render function (`render_sprites*`,
//! `render_shapes`, `render_shadows`, `render_text`, `render_particles`,
//! `render_ui`, `render_gui*`, `render_tilemaps`). Their wgpu twins live in
//! [`super::wgpu_systems`]. What's left here is purely simulation: input,
//! camera follow, animation ticking, UI button hit-tests, tween application,
//! the `ScriptSystem` dispatcher, and physics / audio command appliers.

use crate::detmath::DetMath;
use glam::Vec2;
use hecs::World;

use super::components::{Camera2DComponent, CameraFollow, SpriteComponent, Transform, WorldTransform};
use super::hierarchy::{world_position, world_transform_of};
use super::components::{BounceArea, Velocity2D};
use super::components::{CustomComponents, DespawnOnDeath, Health, HealthBar, HealthBarView, Hitbox, NameTagView, Obstacle, Separation, TextComponent, TriggerOverlaps};
#[cfg(feature = "native")]
use super::components::SpotLight2D;
#[cfg(feature = "native")]
use super::components::{Name, ScriptComponent, ScriptLanguage, Tags};
// Only `apply_script_commands_impl` reaches these, and everything from
// `ScriptSystem` down is `native`-only - so without the gate a wasm build
// (`default-features = false`) warns about every one of them.
#[cfg(feature = "native")]
use super::components::{AmbientLight, MaterialComponent, PointLight2D};
use crate::animation::SpriteAnimator;
use crate::animation::{AnimStateMachine, TriggeredEvent};
use crate::animation::tween::{TweenManager, TweenTarget};
use crate::renderer::camera::Camera2D;
// Same story: used by `ScriptSystem` and the script-command appliers, all
// of which are `native`-only. The handful of non-native references spell
// these out by full path.
#[cfg(feature = "native")]
use crate::asset::AssetManager;
#[cfg(feature = "native")]
use crate::particles::ParticleEmitter;
#[cfg(feature = "native")]
use crate::tilemap::Tilemap;
use crate::ui::{
    UIButton, UIElement, UIImage, UIPanel, UIProgressBar, UISlider, UIText, UITextInput,
    UIToggle,
};
use crate::scripting::api::ScriptCommand;

// Restored behavior systems (lifetime, motion, input, follow/seek, tweens,
// patrol, bounds, rotation, flicker/shake, batch dispatcher). See
// `super::behaviors` for full implementations.
pub use super::behaviors::*;

// ── Combat: trigger-hitbox damage, i-frames, death, health mirror ──────
//
// A data-driven combat layer that runs on any platform without a physics
// backend. A `Hitbox` pairs with a sibling `TriggerArea2D` (which supplies
// the shape + `match_tags` target filter); `update_trigger_areas` must run
// earlier in the tick to populate `TriggerOverlaps`. Call order each frame:
// `tick_invuln` → `apply_hitbox_damage` → `despawn_dead` →
// `mirror_health_to_custom`.

/// Count down per-entity invulnerability (i-frame) timers on `Health`.
pub fn tick_invuln(world: &mut World, dt: f32) {
    for (_e, health) in world.query::<&mut Health>().iter() {
        if health.invuln_timer > 0.0 {
            health.invuln_timer = (health.invuln_timer - dt).max(0.0);
        }
    }
}

/// Apply damage from every active [`Hitbox`] to the [`Health`] of the
/// entities its sibling [`TriggerArea2D`] currently overlaps
/// (`TriggerOverlaps::staying`). Respects the hitbox's per-victim
/// `retrigger` cooldown and the victim's i-frames, and applies positional
/// knockback away from the attacker. Pure ECS; no physics backend needed.
pub fn apply_hitbox_damage(world: &mut World, dt: f32) {
    struct Swing {
        attacker: hecs::Entity,
        victims: Vec<hecs::Entity>,
        damage: f32,
        knockback: f32,
        despawn_on_hit: bool,
    }
    let mut swings: Vec<Swing> = Vec::new();

    // Phase 1: tick cooldowns and collect candidates (mutably borrows Hitbox only).
    for (attacker, (hitbox, overlaps)) in
        world.query::<(&mut Hitbox, &TriggerOverlaps)>().iter()
    {
        if !hitbox.cooldowns.is_empty() {
            hitbox.cooldowns.retain(|_, t| {
                *t -= dt;
                *t > 0.0
            });
        }
        if !hitbox.active || hitbox.damage <= 0.0 {
            continue;
        }
        let victims: Vec<hecs::Entity> = overlaps
            .staying
            .iter()
            .filter(|bits| !hitbox.cooldowns.contains_key(bits))
            .filter_map(|bits| hecs::Entity::from_bits(*bits))
            .collect();
        if victims.is_empty() {
            continue;
        }
        // A projectile's cooldown is moot - it is gone after its hit - so
        // only a lingering hitbox records one per victim.
        if hitbox.retrigger > 0.0 && !hitbox.despawn_on_hit {
            for v in &victims {
                hitbox.cooldowns.insert(v.to_bits().get(), hitbox.retrigger);
            }
        }
        swings.push(Swing {
            attacker,
            victims,
            damage: hitbox.damage,
            knockback: hitbox.knockback,
            despawn_on_hit: hitbox.despawn_on_hit,
        });
    }

    // Phase 2: apply damage / i-frames / knockback (query borrow released).
    let hittable = |world: &World, victim: hecs::Entity| {
        world
            .get::<&Health>(victim)
            .is_ok_and(|h| h.invuln_timer <= 0.0 && h.current > 0.0)
    };
    let mut spent: Vec<hecs::Entity> = Vec::new();
    for swing in swings {
        // World positions: a hitbox on a parented limb swings from where the
        // limb is, not from its local offset.
        let from = world_transform_of(world, swing.attacker)
            .map(|t| t.position)
            .unwrap_or(Vec2::ZERO);

        let targets: Vec<hecs::Entity> = if swing.despawn_on_hit {
            // A projectile hits one target and is consumed: the nearest one it
            // can actually damage. It used to damage *every* entity it
            // overlapped on the frame it landed, so a bullet into a crowd hit
            // the whole crowd. Piercing, when it exists, is an explicit count
            // on top of this - not an accident of overlap.
            let distance = |e: &hecs::Entity| {
                world_transform_of(world, *e)
                    .map(|t| t.position.distance_squared(from))
                    .unwrap_or(f32::MAX)
            };
            swing
                .victims
                .iter()
                .copied()
                .filter(|v| hittable(world, *v))
                .min_by(|a, b| distance(a).total_cmp(&distance(b)))
                .into_iter()
                .collect()
        } else {
            swing.victims.iter().copied().filter(|v| hittable(world, *v)).collect()
        };

        for victim in &targets {
            if let Ok(mut h) = world.get::<&mut Health>(*victim) {
                h.current -= swing.damage;
                if h.invuln_time > 0.0 {
                    h.invuln_timer = h.invuln_time;
                }
            }
            if swing.knockback > 0.0
                && let Some(at) = world_transform_of(world, *victim).map(|t| t.position)
                && let Ok(mut tr) = world.get::<&mut Transform>(*victim)
            {
                let dir = at - from;
                let len = dir.length();
                if len > 1e-4 {
                    tr.position += dir / len * swing.knockback;
                }
            }
        }
        if swing.despawn_on_hit && !targets.is_empty() {
            spent.push(swing.attacker);
        }
    }
    // Projectiles (despawn_on_hit) are consumed once they land a hit.
    for e in spent {
        let _ = world.despawn(e);
    }
}

/// Give each nine-slice sprite the pixel size of what it samples, so its
/// borders slice the texture rather than the drawn quad (BUGS #96). Cheap
/// once resolved: a sprite is looked at again only while its texture is not
/// loaded yet.
fn resolve_nine_slice_source(sprite: &mut SpriteComponent, assets: &crate::asset::AssetManager) {
    use super::components::SpriteFill;
    let uv = sprite.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
    let tex = crate::asset::resolve_texture_id(&sprite.texture_id);
    let Some(SpriteFill::NineSlice { source_px, .. }) = sprite.fill.as_mut() else {
        return;
    };
    if source_px.is_some() {
        return;
    }
    let dims = assets
        .get_texture_bytes(tex)
        .map(|t| (t.width as f32, t.height as f32));
    #[cfg(feature = "gpu-wgpu")]
    let dims = dims.or_else(|| assets.get_wgpu_texture(tex).map(|t| (t.width as f32, t.height as f32)));
    let Some((w, h)) = dims else { return };
    *source_px = Some([w * (uv[2] - uv[0]).abs(), h * (uv[3] - uv[1]).abs()]);
}

/// Measure the texture of every sprite that has no explicit `size`, so it
/// draws at the texture's own size ([`SpriteComponent::drawn_size`]).
///
/// Only sprites whose texture changed since they were last measured pay for a
/// lookup; the rest cost one string comparison. The built-in white texture is
/// never measured: it is one pixel, and a sprite drawn with it is a coloured
/// quad that keeps [`SpriteComponent::FALLBACK_SIZE`].
fn resolve_texture_size(sprite: &mut SpriteComponent, assets: &crate::asset::AssetManager) {
    if sprite.size.is_some() || sprite.texture_px_for == sprite.texture_id {
        return;
    }
    let tex = crate::asset::resolve_texture_id(&sprite.texture_id);
    if tex == crate::asset::WHITE_TEXTURE_ID {
        sprite.texture_px = None;
        sprite.texture_px_for = sprite.texture_id;
        return;
    }
    let dims = assets.get_texture_bytes(tex).map(|t| [t.width as f32, t.height as f32]);
    #[cfg(feature = "gpu-wgpu")]
    let dims = dims.or_else(|| assets.get_wgpu_texture(tex).map(|t| [t.width as f32, t.height as f32]));
    // Not loaded yet: stay unmeasured and try again next frame.
    let Some(dims) = dims else { return };
    sprite.texture_px = Some(dims);
    sprite.texture_px_for = sprite.texture_id;
}

/// Despawn entities carrying [`DespawnOnDeath`] once their `Health` hits 0.
/// Entities without the marker (e.g. the player) persist at 0 HP so scripts
/// can react to death.
pub fn despawn_dead(world: &mut World) {
    let dead: Vec<hecs::Entity> = world
        .query::<(&Health, &DespawnOnDeath)>()
        .iter()
        .filter(|(_, (h, _))| h.current <= 0.0)
        .map(|(e, _)| e)
        .collect();
    for e in dead {
        let _ = world.despawn(e);
    }
}

/// Mirror each `Health` into a `Health` custom component so scripts can
/// read `current` / `max` / `invuln` via the existing `get_custom` bridge
/// (scripts never write it — `Health` is authoritative for native combat).
pub fn mirror_health_to_custom(world: &mut World) {
    use serde_json::json;
    // Entities with a `Health` and no bag yet get one; the query below
    // needs both to be present. Almost always empty after the first frame.
    let bagless: Vec<hecs::Entity> = world
        .query::<hecs::Without<&Health, &CustomComponents>>()
        .iter()
        .map(|(e, _)| e)
        .collect();
    for e in bagless {
        let _ = world.insert_one(e, CustomComponents(Default::default()));
    }
    // One query, in place. This used to collect every `Health` into a
    // `Vec`, then per entity per frame allocate the four key `String`s and
    // re-insert them - 22 script call sites read it, so it stays, but a
    // number that did not change costs nothing now.
    for (_, (h, custom)) in world.query_mut::<(&Health, &mut CustomComponents)>() {
        if !custom.0.contains_key("Health") {
            custom.0.insert("Health".to_string(), json!({}));
        }
        let Some(serde_json::Value::Object(map)) = custom.0.get_mut("Health") else { continue };
        for (key, value) in [("current", h.current), ("max", h.max), ("invuln", h.invuln_timer)] {
            match map.get_mut(key) {
                Some(slot) => {
                    if slot.as_f64() != Some(value as f64) {
                        *slot = json!(value);
                    }
                }
                None => {
                    map.insert(key.to_string(), json!(value));
                }
            }
        }
    }
}

/// Maintain floating [`HealthBar`] sprites above every owner that carries a
/// [`Health`]. For each owner, two runtime-only child sprites (a dark
/// backing + a colored fill that drains rightward as HP drops) are spawned
/// on demand, repositioned above the owner each frame, and despawned once
/// the owner dies or is removed. The bar sprites carry a [`HealthBarView`]
/// back-link and are never serialized. Runs after `mirror_health_to_custom`
/// and reads the authoritative `Health`.
pub fn sync_health_bars(world: &mut World) {
    use std::collections::HashMap;
    // Nothing to do in a world with no bars and no bar views - which is
    // most worlds - rather than two maps and a query per frame.
    if world.query::<&HealthBar>().iter().next().is_none()
        && world.query::<&HealthBarView>().iter().next().is_none()
    {
        return;
    }

    // Above sprites (player z=10) but below HUD text (z=100).
    const BAR_Z: i32 = 50;
    // The engine's built-in white pixel. This named `sprites/white.png`, a
    // file only one game happened to ship - in any other project the
    // renderer found no such texture and discarded the bars, so scene 36's
    // health bars never drew.
    const WHITE_TEX: &str = crate::asset::WHITE_TEXTURE_ID;

    // Index existing bar sprites by owner id; collect orphans (owner gone or
    // dead) for removal.
    let mut bg_of: HashMap<hecs::Entity, hecs::Entity> = HashMap::new();
    let mut fill_of: HashMap<hecs::Entity, hecs::Entity> = HashMap::new();
    let mut orphans: Vec<hecs::Entity> = Vec::new();
    for (bar, view) in world.query::<&HealthBarView>().iter() {
        let owner_alive = world
            .get::<&Health>(view.owner)
            .map(|h| h.current > 0.0)
            .unwrap_or(false);
        // An owner that dropped its `HealthBar` wants no bar. Without this the
        // bar stayed, frozen where it was last positioned.
        let wants_bar = world.satisfies::<&HealthBar>(view.owner).unwrap_or(false);
        if !owner_alive || !wants_bar {
            orphans.push(bar);
        } else if view.is_fill {
            fill_of.insert(view.owner, bar);
        } else {
            bg_of.insert(view.owner, bar);
        }
    }
    for bar in orphans {
        let _ = world.despawn(bar);
    }

    // Snapshot owners that should show a bar (release the query borrow before
    // spawning / mutating sprite entities).
    struct BarState {
        owner: hecs::Entity,
        pos: Vec2,
        ratio: f32,
        cfg: HealthBar,
    }
    let mut states: Vec<BarState> = Vec::new();
    for (owner, (tr, w, hp, bar)) in
        world.query::<(&Transform, Option<&WorldTransform>, &Health, &HealthBar)>().iter()
    {
        if hp.current <= 0.0 {
            continue;
        }
        let ratio = (hp.current / hp.max.max(1e-4)).clamp(0.0, 1.0);
        states.push(BarState { owner, pos: world_position(tr, w), ratio, cfg: bar.clone() });
    }

    for st in states {
        let key = st.owner;
        let hidden = st.cfg.hide_when_full && st.ratio >= 0.999;
        let w = st.cfg.width;
        let h = st.cfg.height;
        let cx = st.pos.x;
        let cy = st.pos.y + st.cfg.y_offset;

        // Background (centered).
        let bg_alpha = if hidden { 0.0 } else { 0.85 };
        let bg = *bg_of.entry(key).or_insert_with(|| {
            world.spawn((
                Transform::default(),
                SpriteComponent {
                    texture_id: WHITE_TEX.into(),
                    z_order: BAR_Z,
                    ..Default::default()
                },
                HealthBarView { owner: st.owner, is_fill: false },
            ))
        });
        if let Ok(mut tr) = world.get::<&mut Transform>(bg) {
            tr.position = Vec2::new(cx, cy);
        }
        if let Ok(mut sp) = world.get::<&mut SpriteComponent>(bg) {
            sp.size = Some([w, h]);
            sp.anchor = [0.5, 0.5];
            sp.color = [0.06, 0.06, 0.09, bg_alpha];
        }

        // Fill (anchored to the left edge so it drains rightward). Green at
        // full HP, red when low.
        let ir = 1.0 - st.ratio;
        let fill_alpha = if hidden { 0.0 } else { 1.0 };
        let fill = *fill_of.entry(key).or_insert_with(|| {
            world.spawn((
                Transform::default(),
                SpriteComponent {
                    texture_id: WHITE_TEX.into(),
                    z_order: BAR_Z + 1,
                    ..Default::default()
                },
                HealthBarView { owner: st.owner, is_fill: true },
            ))
        });
        if let Ok(mut tr) = world.get::<&mut Transform>(fill) {
            tr.position = Vec2::new(cx - w * 0.5, cy);
        }
        if let Ok(mut sp) = world.get::<&mut SpriteComponent>(fill) {
            sp.size = Some([(w * st.ratio).max(0.0), h]);
            sp.anchor = [0.0, 0.5];
            sp.color = [
                0.85 * ir + 0.30 * st.ratio,
                0.25 * ir + 0.85 * st.ratio,
                0.25 * ir + 0.35 * st.ratio,
                fill_alpha,
            ];
        }
    }
}

/// Maintain a floating name-tag text entity above every owner whose
/// `CustomComponents` carries a non-empty `NameTag.text` string. One
/// transient [`TextComponent`] follower per owner is spawned on demand,
/// repositioned each frame, and despawned once the owner is gone or clears
/// its tag (e.g. a player-toggled label off). Scripts author the label with
/// the existing `set_custom(e, "NameTag", "text", ...)` call — no dedicated
/// API. Runs after `sync_health_bars`.
pub fn sync_name_tags(world: &mut World) {
    use std::collections::HashMap;

    // Above health bars (z 50/51) but below the HUD text (z 100).
    const TAG_Z: i32 = 60;
    const TAG_Y: f32 = 34.0;

    // Owners that currently want a tag: id -> (owner, position, text).
    let mut wanted: HashMap<u64, (hecs::Entity, Vec2, String)> = HashMap::new();
    for (owner, (tr, w, custom)) in
        world.query::<(&Transform, Option<&WorldTransform>, &CustomComponents)>().iter()
    {
        if let Some(text) = custom
            .0
            .get("NameTag")
            .and_then(|v| v.get("text"))
            .and_then(|v| v.as_str())
            && !text.is_empty()
        {
            wanted.insert(owner.id() as u64, (owner, world_position(tr, w), text.to_string()));
        }
    }

    // Index existing followers; despawn orphans (owner gone or tag cleared).
    let mut tag_of: HashMap<u64, hecs::Entity> = HashMap::new();
    let mut orphans: Vec<hecs::Entity> = Vec::new();
    for (tag_e, view) in world.query::<&NameTagView>().iter() {
        let key = view.owner.id() as u64;
        match wanted.get(&key) {
            Some((owner, _, _)) if *owner == view.owner => {
                tag_of.insert(key, tag_e);
            }
            _ => orphans.push(tag_e),
        }
    }
    for e in orphans {
        let _ = world.despawn(e);
    }

    let items: Vec<(u64, hecs::Entity, Vec2, String)> = wanted
        .into_iter()
        .map(|(k, (o, p, t))| (k, o, p, t))
        .collect();
    for (key, owner, pos, text) in items {
        let tag = match tag_of.get(&key) {
            Some(&e) => e,
            None => world.spawn((
                Transform::default(),
                TextComponent {
                    text: text.clone(),
                    font_scale: 0.85,
                    color: [1.0, 1.0, 1.0, 0.92],
                    z_order: TAG_Z,
                    align: super::components::TextAlign::Center,
                    ..Default::default()
                },
                NameTagView { owner },
            )),
        };
        // Centred by the text's own `align`, so the tag sits over its owner
        // at any length (this used to guess 3 units per character).
        if let Ok(mut tr) = world.get::<&mut Transform>(tag) {
            tr.position = Vec2::new(pos.x, pos.y + TAG_Y);
        }
        if let Ok(mut tc) = world.get::<&mut TextComponent>(tag)
            && tc.text != text
        {
            tc.text = text;
        }
    }
}

/// Resolve movement collision against static [`Obstacle`] circles: push every
/// moving entity (anything with a [`Health`]) out to each obstacle's `radius`.
/// A cheap circle-push with no physics backend; run after all movement
/// (scripts, behaviors, knockback) and before the health-bar / name-tag
/// followers so they track the corrected positions.
pub fn resolve_obstacle_collisions(world: &mut World) {
    let obstacles: Vec<(hecs::Entity, Vec2, f32)> = world
        .query::<(&Transform, &Obstacle)>()
        .iter()
        .map(|(e, (t, o))| (e, t.position, o.radius))
        .collect();
    if obstacles.is_empty() {
        return;
    }
    // Who is a mover: anything with `Health` (the combat bodies this was
    // written for), and anything with a `Velocity2D` the motion systems move
    // - seekers, wanderers, a flock - unless rapier owns it (it would fight
    // the solver) or it is a projectile the cover check below consumes
    // (pushed to the rim first, it would never be inside to be consumed).
    // Only `Health` used to count, so scene 42's flock, documented as kept
    // out of the rocks, walked straight through them (BUGS #155).
    let mut movers = world.query::<(
        &mut Transform,
        Option<&Health>,
        Option<&Velocity2D>,
        Option<&crate::ecs::components::RigidBody2D>,
        Option<&Hitbox>,
    )>();
    for (e, (tr, health, vel, body, hitbox)) in movers.iter() {
        let projectile = hitbox.is_some_and(|h| h.despawn_on_hit);
        let mover = health.is_some() || (vel.is_some() && body.is_none() && !projectile);
        if !mover {
            continue;
        }
        for (obstacle, opos, r) in &obstacles {
            // A destructible obstacle (one with `Health`) sits at distance
            // zero from itself, which read as "inside" and pushed it `r` away
            // every frame - it flew off.
            if *obstacle == e {
                continue;
            }
            let d = tr.position - *opos;
            let len = d.length();
            if len <= 1e-4 {
                tr.position = *opos + Vec2::new(0.0, *r);
            } else if len < *r {
                tr.position = *opos + d / len * *r;
            }
        }
    }
    drop(movers);
    // Line-of-sight cover: a projectile (a `despawn_on_hit` hitbox) is
    // consumed when it enters an obstacle, so obstacles block arrows/bullets.
    let mut blocked: Vec<hecs::Entity> = Vec::new();
    for (e, (tr, hb)) in world.query::<(&Transform, &Hitbox)>().iter() {
        if !hb.despawn_on_hit {
            continue;
        }
        for (obstacle, opos, r) in &obstacles {
            if *obstacle == e {
                continue;
            }
            if (tr.position - *opos).length_squared() < r * r {
                blocked.push(e);
                break;
            }
        }
    }
    for e in blocked {
        let _ = world.despawn(e);
    }
}

/// Soft crowd separation: gently push overlapping [`Separation`] entities
/// (e.g. enemies) apart so packs spread out instead of stacking. Only entities
/// carrying `Separation` take part; the player / props don't, so they're
/// untouched. Run alongside `resolve_obstacle_collisions` (which runs after it
/// so nothing ends up inside an obstacle).
///
/// Broadphased through [`crate::ecs::spatial::UniformGrid`]. It used to compare
/// every body to every other, which measured 53 ms/frame at 8,000 bodies —
/// fine for the ten enemies it was written for, unusable for a crowd.
pub fn apply_separation(world: &mut World) {
    let bodies: Vec<(hecs::Entity, Vec2, f32)> = world
        .query::<(&Transform, &Separation)>()
        .iter()
        .map(|(e, (t, s))| (e, t.position, s.radius))
        .collect();
    let n = bodies.len();
    if n < 2 {
        return;
    }
    // Broadphase. Two bodies can only interact within the sum of their radii,
    // so cells the size of the largest possible interaction put every
    // candidate in the 3x3 block around each body.
    let max_radius = bodies.iter().fold(0.0f32, |m, (_, _, r)| m.max(*r));
    let positions: Vec<Vec2> = bodies.iter().map(|(_, p, _)| *p).collect();
    let grid = crate::ecs::spatial::UniformGrid::build(&positions, (max_radius * 2.0).max(1e-3));

    let mut push = vec![Vec2::ZERO; n];
    for i in 0..n {
        let (pi, ri) = (bodies[i].1, bodies[i].2);
        grid.for_each_near(pi, |j| {
            let j = j as usize;
            if j == i {
                return true;
            }
            let d = pi - bodies[j].1;
            let min_d = ri + bodies[j].2;
            let len = d.length();
            if len > 1e-4 && len < min_d {
                // Soft push: each moves a quarter of the overlap per frame.
                //
                // This is the `i<j` loop rewritten per body: `shove(j,i)` is
                // exactly `-shove(i,j)`, so accumulating only into `push[i]`
                // over every ordered pair gives the same totals the paired
                // `+=` / `-=` did.
                push[i] += d / len * ((min_d - len) * 0.25);
            } else if len <= 1e-4 {
                // Coincident. The original seeded the escape direction from
                // the *lower* index of the pair and negated it for the higher
                // one; seeding from `i` here would send both the same way and
                // they would never separate.
                let seed = i.min(j);
                let a = seed as f32 * 2.399_963;
                let nudge = Vec2::new(a.dcos(), a.dsin()) * (min_d * 0.25);
                push[i] += if i < j { nudge } else { -nudge };
            }
            true
        });
    }
    for (idx, (e, _, _)) in bodies.iter().enumerate() {
        if push[idx] != Vec2::ZERO
            && let Ok(mut tr) = world.get::<&mut Transform>(*e)
        {
            tr.position += push[idx];
        }
    }
}

/// Integrate velocity into position, apply gravity, and bounce off bounding areas.
///
/// For every entity with `Transform` + `Velocity2D`, applies gravity and moves
/// the entity. If the entity also has a `BounceArea`, clamps position and
/// reflects velocity at the boundaries.
///
/// A **dynamic** rigid body is the physics' to move, and is skipped: its
/// `Velocity2D` is what rapier says it is going (written back each step by
/// `PhysicsWorld::sync_transforms_to_ecs`). Until Oct 2026 this moved it too,
/// and the physics frame read that as a script's `set_position` and teleported
/// the body there - so a body authored with a velocity travelled at twice it,
/// for as long as it lived. A kinematic body is still moved here; that is how
/// a moving platform is authored.
pub fn velocity_system(world: &mut World, dt: f32) {
    let world_dt = dt;
    for (_, (transform, vel, bounce, body, own)) in world.query_mut::<(
        &mut Transform,
        &mut Velocity2D,
        Option<&BounceArea>,
        Option<&crate::ecs::components::RigidBody2D>,
        Option<&crate::ecs::time_scale::TimeScale>,
    )>() {
        if body.is_some_and(|b| b.body_type == crate::ecs::components::BodyType::Dynamic) {
            continue;
        }
        // Its own pace (`time_scale`), if it keeps one.
        let dt = world_dt * crate::ecs::time_scale::factor(own);
        // Apply gravity (downward = subtract from y)
        vel.y -= vel.gravity * dt;

        // Integrate velocity into position
        transform.position.x += vel.x * dt;
        transform.position.y += vel.y * dt;

        // Bounce off bounding area
        if let Some(area) = bounce {
            if transform.position.x < area.min_x {
                transform.position.x = area.min_x;
                vel.x = vel.x.abs() * area.damping;
            } else if transform.position.x > area.max_x {
                transform.position.x = area.max_x;
                vel.x = -(vel.x.abs() * area.damping);
            }

            if transform.position.y < area.min_y {
                transform.position.y = area.min_y;
                vel.y = vel.y.abs() * area.damping;
            } else if transform.position.y > area.max_y {
                transform.position.y = area.max_y;
                vel.y = -(vel.y.abs() * area.damping);
            }
        }
    }
}
/// Feed this frame's mouse wheel into whichever `Overflow::Scroll` GUI
/// container the cursor is over.
///
/// Every runtime calls this before its GUI click hit-test, so a wheel notch
/// and the click that follows agree on where the rows are. The actual work
/// lives in [`crate::ui::layout::apply_gui_scroll`]; this wrapper is just the
/// world/asset plumbing (which documents are on screen, which one is under
/// the cursor).
///
/// Returns the number of documents whose offset changed.
pub fn apply_gui_wheel_scroll(
    world: &World,
    assets: &mut crate::asset::AssetManager,
    mouse: (f32, f32),
    wheel_delta: f32,
    screen: (f32, f32),
) -> usize {
    if wheel_delta == 0.0 {
        return 0;
    }
    let visible: Vec<String> = {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        world
            .query::<&crate::ecs::components::GuiComponent>()
            .iter()
            .filter(|(_, g)| g.visible)
            .map(|(_, g)| g.gui_id.clone())
            .filter(|id| seen.insert(id.clone()))
            .collect()
    };
    let mut changed = 0;
    for gui_id in visible {
        let Some(doc) = assets.get_gui_mut(&gui_id) else {
            continue;
        };
        if crate::ui::layout::apply_gui_scroll(
            doc,
            mouse.0,
            mouse.1,
            wheel_delta,
            screen.0,
            screen.1,
        ) {
            changed += 1;
        }
    }
    changed
}

/// Advance pointer-driven scrolling — drag, flick and elastic ends — for every
/// visible GUI document.
///
/// The companion to [`apply_gui_wheel_scroll`], and here for the same reason:
/// all three runtimes hit-test GUI documents themselves, and a copy per runtime
/// is exactly how a GUI feature ends up working in one of them. Web already
/// maps touch events onto the mouse fields, so this is the touch path too.
///
/// Returns how many documents moved. Call once a frame, after input is
/// gathered; `state` is one long-lived [`crate::ui::scroll::GuiScrollState`]
/// per runtime.
pub fn update_gui_scroll_drag(
    world: &World,
    assets: &mut crate::asset::AssetManager,
    state: &mut crate::ui::scroll::GuiScrollState,
    pointer: crate::ui::scroll::PointerInput,
    screen: (f32, f32),
    dt: f32,
) -> usize {
    let visible: Vec<String> = {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        world
            .query::<&crate::ecs::components::GuiComponent>()
            .iter()
            .filter(|(_, g)| g.visible)
            .map(|(_, g)| g.gui_id.clone())
            .filter(|id| seen.insert(id.clone()))
            .collect()
    };
    let mut changed = 0;
    for gui_id in visible {
        let Some(doc) = assets.get_gui_mut(&gui_id) else {
            continue;
        };
        if state.update(doc, &gui_id, pointer, screen, dt) {
            changed += 1;
        }
    }
    changed
}

/// Point the active camera's zoom at `zoom`, reporting whether one took it.
///
/// [`update_camera`] copies this field into the renderer's camera every frame,
/// so the engine could always zoom - nothing could *ask* it to. `camera_shake`
/// was the only camera control a script had, which left a scene wanting to
/// zoom no option but to move every entity instead.
///
/// The value is clamped rather than ignored: a zero or negative zoom collapses
/// the view matrix and blanks the screen, which reads as a crash rather than
/// as a bad argument.
pub fn set_active_camera_zoom(world: &mut World, zoom: f32) -> bool {
    let zoom = zoom.clamp(0.01, 100.0);
    let Some(main) = crate::ecs::camera_rig::main_camera(world) else { return false };
    world.get::<&mut Camera2DComponent>(main).map(|mut cam| cam.zoom = zoom).is_ok()
}

/// `Camera.set_pixel_size`: the active camera's world units per art pixel
/// (0 draws at the screen's resolution). False when there is no active
/// camera entity to set it on.
pub fn set_active_camera_pixel_size(world: &mut World, size: f32) -> bool {
    let size = if size.is_finite() { size.max(0.0) } else { 0.0 };
    let Some(main) = crate::ecs::camera_rig::main_camera(world) else { return false };
    world.get::<&mut Camera2DComponent>(main).map(|mut cam| cam.pixel_size = size).is_ok()
}

/// The centre and size of the box that contains every entity tagged `tag`,
/// plus `padding` on each side. `None` when nothing carries the tag.
///
/// Free-standing and pure, so the framing arithmetic is testable without a
/// renderer - which matters, because "the camera frames both ships" is the
/// sort of claim that is otherwise only checkable by looking at it.
pub fn frame_of(world: &World, tag: &str, padding: f32) -> Option<(Vec2, Vec2)> {
    use super::components::Tags;
    let mut min = Vec2::new(f32::MAX, f32::MAX);
    let mut max = Vec2::new(f32::MIN, f32::MIN);
    let mut found = false;
    for (_, (t, w, tags)) in world.query::<(&Transform, Option<&WorldTransform>, &Tags)>().iter() {
        if !tags.0.contains(tag) {
            continue;
        }
        found = true;
        let p = world_position(t, w);
        min = min.min(p);
        max = max.max(p);
    }
    if !found {
        return None;
    }
    let centre = (min + max) * 0.5;
    // A single target has a zero-size box, which would ask the camera to
    // zoom in infinitely - the padding is what stops that, and is why it
    // defaults to something rather than nothing.
    let size = (max - min) + Vec2::splat(padding * 2.0);
    Some((centre, size))
}

/// Drive the 2D camera from the world:
///
/// 1. The first entity with an active `Camera2DComponent` sets the camera's
///    zoom and viewport size.
/// 2. The camera smoothly follows a `CameraFollow`, clamped to its bounds:
///    the active camera entity's own if it has one, otherwise the first found.
/// 3. Otherwise the camera snaps to the active camera entity's position.
pub fn update_camera(world: &World, camera: &mut Camera2D, dt: f32) {
    let _span = crate::profiling::trace::span("camera");
    // The world was moved back under the camera (`ecs::floating_origin`):
    // so is the camera, or its follow would sweep across the jump.
    let by = crate::ecs::floating_origin::follow(&mut camera.origin_seen);
    if by != Vec2::ZERO {
        camera.position -= by;
        if let Some(p) = camera.follow_prev.as_mut() {
            *p -= by;
        }
        if let Some(b) = camera.blend.as_mut() {
            b.from_position -= by;
            b.live_position -= by;
        }
    }
    // The main camera: the active, untargeted one of the highest priority
    // (`ecs::camera_rig`). When another takes over and asks for a blend, the
    // view eases from where it is; the new camera starts on its target.
    let main = crate::ecs::camera_rig::main_camera(world);
    let mut snap = false;
    if main != camera.main_camera {
        let blend = main.and_then(|e| world.get::<&Camera2DComponent>(e).ok().map(|c| c.blend)).unwrap_or(0.0);
        camera.blend = None;
        if camera.main_camera.is_some() && main.is_some() && blend > 0.0 {
            camera.blend = Some(crate::renderer::camera::CameraBlend {
                from_position: camera.position,
                from_zoom: camera.zoom,
                from_rotation: camera.rotation,
                live_position: camera.position,
                live_zoom: camera.zoom,
                elapsed: 0.0,
                duration: blend,
            });
            snap = true;
        }
        camera.main_camera = main;
    }
    // While blending, the new camera moves its own view; the blend is laid
    // over it at the end.
    if let Some(b) = camera.blend {
        camera.position = b.live_position;
        camera.zoom = b.live_zoom;
    }
    let from = camera.position;
    let mut confine: Vec<[f32; 2]> = Vec::new();
    let mut cam_transform_pos = None;
    let mut active_camera = None;
    // The authored (or `set_camera_zoom`) zoom. Applied below only when
    // nothing is being framed: framing eases the renderer's zoom from one
    // frame to the next, and writing this back first every frame restarted
    // the ease from 1.0 - a framing camera hovered just under its authored
    // zoom and never pulled back (test-lab scene 93, BUGS #163).
    let mut base_zoom = None;
    let mut main_query = main.and_then(|e| world.query_one::<(&Transform, Option<&WorldTransform>, &Camera2DComponent)>(e).ok());
    if let (Some(entity), Some((transform, w, cam))) = (main, main_query.as_mut().and_then(|q| q.get())) {
        base_zoom = Some(cam.zoom);
        confine = cam.confine.clone();
        // The host sized the camera to the surface just before this, so its
        // current size carries the surface's shape (and fitting a fitted
        // size again is a no-op, should a host not).
        let view = cam.view_on(Vec2::new(camera.width(), camera.height()));
        camera.resize(view.x, view.y);
        // The camera entity's own rotation turns the view, so a script
        // turns the camera with `set_rotation` like anything else.
        camera.rotation = w.map_or(transform.rotation, |w| w.rotation);
        camera.pixel_size = cam.pixel_size.max(0.0);
        camera.integer_scale = cam.integer_scale;
        camera.cull_mask = cam.cull_mask;
        cam_transform_pos = Some(world_position(transform, w));
        active_camera = Some(entity);
    }
    drop(main_query);

    // The CameraFollow to use: the active camera's own, if it has one - a
    // scene with a second camera, or a follow left on a disabled one, used to
    // get whichever the query happened to yield first.
    // Followed at its world position: a follow on a child of something
    // (a rider, a turret) tracks where it is drawn.
    let own_follow = active_camera.and_then(|e| {
        let mut q = world.query_one::<(&Transform, Option<&WorldTransform>, &CameraFollow)>(e).ok()?;
        q.get().map(|(t, w, f)| (world_position(t, w), f.clone()))
    });
    let follow = own_follow.or_else(|| {
        world
            .query::<(&Transform, Option<&WorldTransform>, &CameraFollow)>()
            .iter()
            .next()
            .map(|(_, (t, w, f))| (world_position(t, w), f.clone()))
    });
    if let Some((target_pos, follow)) = follow {
        // Framing a set, or following one thing. `frame_of` returns `None`
        // when the tag matches nothing, which falls back to following - so a
        // scene does not go blank the moment the things it was framing are
        // destroyed.
        let framed = follow
            .frame_tag
            .as_deref()
            .and_then(|tag| frame_of(world, tag, follow.frame_padding));
        let mut target = match framed {
            Some((centre, _)) => centre + follow.offset,
            None => target_pos + follow.offset,
        };
        // A camera taking over with a blend starts on its target.
        let t = if snap { 1.0 } else { 1.0 - (1.0 - follow.smoothing).dpowf(dt * 60.0) };
        if framed.is_none() {
            // Look ahead: lead by the target's velocity, eased so a turn
            // swings the view round rather than throwing it.
            if follow.look_ahead > 0.0 && dt > 0.0 {
                let velocity = camera.follow_prev.map_or(Vec2::ZERO, |p| (target_pos - p) / dt);
                let want = (velocity * follow.look_ahead).clamp_length_max(follow.look_ahead_max.max(0.0));
                let k = 1.0 - (1.0 - follow.look_ahead_smoothing.clamp(0.0, 1.0)).dpowf(dt * 60.0);
                camera.look_ahead += (want - camera.look_ahead) * k;
            } else {
                camera.look_ahead = Vec2::ZERO;
            }
            target += camera.look_ahead;
            // Dead zone: the camera holds while the target is inside the
            // box round its middle, and is dragged by the edge past it.
            let half = Vec2::from(follow.dead_zone).max(Vec2::ZERO) * 0.5;
            if half != Vec2::ZERO && !snap {
                let d = target - camera.position;
                let pull = Vec2::new(
                    if d.x.abs() > half.x { d.x - half.x * d.x.signum() } else { 0.0 },
                    if d.y.abs() > half.y { d.y - half.y * d.y.signum() } else { 0.0 },
                );
                target = camera.position + pull;
            }
        }
        camera.follow_prev = Some(target_pos);
        let new_pos = Vec2::lerp(camera.position, target, t.clamp(0.0, 1.0));
        camera.position = if let Some([min_x, min_y, max_x, max_y]) = follow.bounds {
            Vec2::new(new_pos.x.clamp(min_x, max_x), new_pos.y.clamp(min_y, max_y))
        } else {
            new_pos
        };

        // Zoom to fit, eased at the same rate as the move. A zoom that
        // snapped while the position eased would read as two different
        // cameras fighting.
        if let Some((_, size)) = framed {
            let (vw, vh) = camera.viewport_size();
            let (vw, vh) = (vw.max(1.0), vh.max(1.0));
            // The renderer's zoom is "how close in", so fitting a box wider
            // than the viewport means a zoom below one.
            let fit = (vw / size.x.max(1.0)).min(vh / size.y.max(1.0));
            // With no range, never closer than the authored zoom - which is
            // the component's, not the renderer's: that one is last frame's
            // eased value, and a cap that follows it only ever goes down.
            let cap = base_zoom.unwrap_or(1.0).max(0.25);
            let [lo, hi] = follow.zoom_range.unwrap_or([0.25, cap]);
            let want = fit.clamp(lo.min(hi), hi.max(lo));
            camera.zoom += (want - camera.zoom) * t.clamp(0.0, 1.0);
        } else if let Some(z) = base_zoom {
            camera.zoom = z;
        }
    } else {
        if let Some(pos) = cam_transform_pos {
            // No CameraFollow — snap to active camera entity's transform
            camera.position = pos;
        }
        if let Some(z) = base_zoom {
            camera.zoom = z;
        }
    }

    // The camera's shape: its whole view stays inside, sliding along the
    // walls from where it was.
    if !confine.is_empty() {
        camera.position = crate::ecs::camera_rig::confine(&confine, camera.position, camera.half_extents(), Some(from));
    }

    if let Some(mut b) = camera.blend {
        b.live_position = camera.position;
        b.live_zoom = camera.zoom;
        b.elapsed += dt;
        if b.elapsed >= b.duration {
            camera.blend = None;
        } else {
            let k = b.weight();
            camera.position = b.from_position.lerp(b.live_position, k);
            // Zoom eases by ratio, so 1 to 4 passes 2 halfway - in steps
            // that look even.
            camera.zoom = if b.from_zoom > 0.0 && b.live_zoom > 0.0 {
                b.from_zoom * (b.live_zoom / b.from_zoom).dpowf(k)
            } else {
                b.live_zoom
            };
            let turn = (camera.rotation - b.from_rotation + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            camera.rotation = b.from_rotation + turn * k;
            camera.blend = Some(b);
        }
    }
}

/// Fill in the `uv_rect` of every sprite that names an atlas `region`.
///
/// Each sprite resolves once, on the first frame its atlas is loaded - the
/// editor and web builds load atlases asynchronously, so an unresolved sprite
/// is retried rather than left wrong. A region the atlas does not have is
/// reported once and the sprite keeps its authored rect.
///
/// Run before rendering. Sprites without a `region` cost one `Option` check.
pub fn resolve_sprite_regions(world: &mut World, assets: &crate::asset::AssetManager) {
    // Swapped parts first (`ecs::sprite_library`), so a new region resolves
    // in this same pass.
    crate::feature!("sprite_library", { crate::ecs::sprite_library::resolve(world) });
    // One walk over the sprites for all three resolutions. They were three
    // separate passes, each a full `query_mut` a frame. Region first, so a
    // nine-slice measured on the same frame sees the resolved rect.
    for (_, sprite) in world.query_mut::<&mut SpriteComponent>() {
        resolve_region(sprite, assets);
        resolve_nine_slice_source(sprite, assets);
        resolve_texture_size(sprite, assets);
    }
    // A mesh names a region the same way, and is re-pointed the same way:
    // clear `uv_rect` and `region_resolved` together.
    for (_, mesh) in world.query_mut::<&mut crate::ecs::components::Mesh2D>() {
        if mesh.region_resolved {
            continue;
        }
        let Some(region) = mesh.region.as_deref() else { continue };
        let Some(atlas) = assets.get_atlas_data(&mesh.texture_id) else { continue };
        match atlas.get_uv_rect(region) {
            Some(uv) => mesh.uv_rect = Some(uv),
            None => log::warn!("mesh region '{region}': atlas '{}' has no such region", mesh.texture_id),
        }
        mesh.region_resolved = true;
    }
    // And an entity-UI image: an icon cut from a sheet.
    for (_, image) in world.query_mut::<&mut UIImage>() {
        if !image.region_resolved
            && let Some(region) = image.region.as_deref()
            && let Some(atlas) = assets.get_atlas_data(&image.texture_id)
        {
            match atlas.get_uv_rect(region) {
                Some(uv) => image.uv_rect = Some(uv),
                None => log::warn!("ui image region '{region}': atlas '{}' has no such region", image.texture_id),
            }
            image.region_resolved = true;
        }
        resolve_ui_nine_slice_source(image, assets);
    }
}

/// [`resolve_nine_slice_source`] for a nine-slice UI image: the pixel size
/// of what it samples, once its region (if it names one) is known.
fn resolve_ui_nine_slice_source(image: &mut UIImage, assets: &crate::asset::AssetManager) {
    if image.nine_slice.is_none() || image.source_px.is_some() || (image.region.is_some() && !image.region_resolved) {
        return;
    }
    let tex = crate::asset::resolve_texture_id(&image.texture_id);
    let dims = assets.get_texture_bytes(tex).map(|t| (t.width as f32, t.height as f32));
    #[cfg(feature = "gpu-wgpu")]
    let dims = dims.or_else(|| assets.get_wgpu_texture(tex).map(|t| (t.width as f32, t.height as f32)));
    let Some((w, h)) = dims else { return };
    let uv = image.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
    image.source_px = Some([w * (uv[2] - uv[0]).abs(), h * (uv[3] - uv[1]).abs()]);
}

fn resolve_region(sprite: &mut SpriteComponent, assets: &crate::asset::AssetManager) {
    if sprite.region_resolved {
        return;
    }
    let Some(region) = sprite.region.as_deref() else {
        return;
    };
    let Some(atlas) = assets.get_atlas_data(&sprite.texture_id) else {
        return; // atlas not loaded yet
    };
    match atlas.get_uv_rect(region) {
        Some(uv) => sprite.uv_rect = Some(uv),
        None => log::warn!(
            "sprite region '{region}': atlas '{}' has no such region",
            sprite.texture_id
        ),
    }
    sprite.region_resolved = true;
}

/// Advance every [`SpriteAnimator`] and apply the frame it lands on: UV rect
/// and texture, property tracks, and frame triggers (returned).
///
/// Runs over **every** animator, whatever else the entity carries, and writes
/// only to the components that are there - a sprite's UV and colour, a
/// transform, or any of the UI widgets a track can drive. It used to be two
/// systems, and both were wrong for UI:
///
/// * this one required `Transform` + `SpriteComponent`, so an animator on a UI
///   panel or text never ticked and its clip sat on frame 0;
/// * `tick_ui_animations` re-sampled the tracks for every animator with a
///   current clip on every frame, **playing or not**, so once a UI fade ended
///   no script could change that property again - the finished clip wrote its
///   last value back each frame. It was also seven copies of the same block.
///
/// Values are applied only on ticks where the animator produced a frame, so a
/// stopped clip leaves its target alone.
///
/// `assets` supplies the atlases that frames naming a `region` resolve
/// against; each animator resolves once, on the first tick its atlas is loaded.
pub fn tick_sprite_animations(
    world: &mut World,
    assets: &crate::asset::AssetManager,
    dt: f32,
) -> Vec<TriggeredEvent> {
    let mut events = Vec::new();
    for (entity, (animator, transform, sprite, ui, own)) in world.query_mut::<(
        &mut SpriteAnimator,
        Option<&mut Transform>,
        Option<&mut SpriteComponent>,
        AnimatedUi,
        Option<&crate::ecs::time_scale::TimeScale>,
    )>() {
        if !animator.regions_resolved {
            let texture = sprite.as_deref().map(|s| s.texture_id.as_str());
            animator.regions_resolved = animator.resolve_frame_regions(texture, assets);
        }
        animator.apply_random_start(entity.to_bits().get());
        let Some(result) = animator.tick(dt * crate::ecs::time_scale::factor(own)) else {
            continue;
        };
        let p = &result.properties;

        if let Some(sprite) = sprite {
            sprite.uv_rect = Some(result.uv_rect);
            if let Some(tex) = result.texture_id {
                sprite.texture_id = tex.into();
            }
            set_rgba(&mut sprite.color, [p.color_r, p.color_g, p.color_b, p.color_a]);
        }
        if let Some(transform) = transform {
            set(&mut transform.position.x, p.position_x);
            set(&mut transform.position.y, p.position_y);
            set(&mut transform.rotation, p.rotation);
            set(&mut transform.scale.x, p.scale_x);
            set(&mut transform.scale.y, p.scale_y);
        }

        let (element, text, image, panel, button, progress, slider) = ui;
        if let Some(el) = element {
            set(&mut el.offset.x, p.ui_offset_x);
            set(&mut el.offset.y, p.ui_offset_y);
            set(&mut el.size.x, p.ui_size_x);
            set(&mut el.size.y, p.ui_size_y);
        }
        if let Some(text) = text {
            set(&mut text.font_size, p.ui_text_font_size);
            set_rgba(
                &mut text.color,
                [p.ui_text_color_r, p.ui_text_color_g, p.ui_text_color_b, p.ui_text_color_a],
            );
        }
        if let Some(img) = image {
            set_rgba(
                &mut img.color,
                [p.ui_image_color_r, p.ui_image_color_g, p.ui_image_color_b, p.ui_image_color_a],
            );
        }
        if let Some(panel) = panel {
            set_rgba(
                &mut panel.bg_color,
                [p.ui_panel_color_r, p.ui_panel_color_g, p.ui_panel_color_b, p.ui_panel_color_a],
            );
        }
        if let Some(btn) = button {
            set_rgba(
                &mut btn.normal_color,
                [p.ui_button_color_r, p.ui_button_color_g, p.ui_button_color_b, p.ui_button_color_a],
            );
        }
        if let Some(bar) = progress {
            set(&mut bar.value, p.ui_progress_value);
            set_rgba(
                &mut bar.fg_color,
                [p.ui_progress_fg_r, p.ui_progress_fg_g, p.ui_progress_fg_b, p.ui_progress_fg_a],
            );
        }
        if let Some(slider) = slider {
            set(&mut slider.value, p.ui_slider_value);
        }

        for trigger in result.triggered {
            events.push(TriggeredEvent {
                entity_id: entity.id() as u64,
                trigger,
            });
        }
    }
    events
}

/// The UI widgets a property track can drive, each optional.
type AnimatedUi<'a> = (
    Option<&'a mut UIElement>,
    Option<&'a mut UIText>,
    Option<&'a mut UIImage>,
    Option<&'a mut UIPanel>,
    Option<&'a mut UIButton>,
    Option<&'a mut UIProgressBar>,
    Option<&'a mut UISlider>,
);

/// Write a sampled track value if the clip has a track for it.
#[inline]
fn set(target: &mut f32, value: Option<f32>) {
    if let Some(v) = value {
        *target = v;
    }
}

/// [`set`] for each channel of a colour.
#[inline]
fn set_rgba(target: &mut [f32; 4], values: [Option<f32>; 4]) {
    for (channel, value) in target.iter_mut().zip(values) {
        set(channel, value);
    }
}

/// Edit the focused GUI `TextInput` this frame - the caret, selection,
/// words, the clipboard, Enter and Escape (`ui::text_edit`).
///
/// Focus lives on `GuiComponent::focused_node_id` (set by `gui_focus`, by
/// clicking the field - `track_gui_pointer` - or by navigation), so exactly
/// one node per document takes the keyboard, and while it does a game's own
/// key bindings do not fire. Escape lets the field go. A field newly
/// focused starts with its caret at the end.
///
/// Returns the number of documents whose text changed, so a caller can skip
/// re-uploading unchanged GUI state.
pub fn apply_text_input(
    world: &mut World,
    assets: &mut crate::asset::AssetManager,
    input: &mut crate::input::InputState,
) -> usize {
    use crate::ecs::components::GuiComponent;
    use crate::ui::gui_document::GuiNodeType;
    use crate::ui::text_edit;

    // Collect first: the world borrow has to end before the assets borrow.
    let mut focused: Vec<(hecs::Entity, String, String, Option<(f32, bool)>, Option<(f32, f32)>, bool)> = Vec::new();
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut docs: Vec<String> = Vec::new();
    for (entity, g) in world.query_mut::<&mut GuiComponent>() {
        let click = g.text_click.take();
        if !docs.contains(&g.gui_id) {
            docs.push(g.gui_id.clone());
        }
        if !g.visible {
            continue;
        }
        if let Some(n) = g.focused_node_id.clone()
            && seen.insert((g.gui_id.clone(), n.clone()))
        {
            focused.push((entity, g.gui_id.clone(), n, click, g.pointer_ui, g.pointer_down));
        }
    }
    // Every document's fields: a field not focused this frame forgets its
    // selection, and is entered at its end next time.
    let now = crate::profiling::unix_time();
    let mut changed = 0;
    let mut cancel: Vec<hecs::Entity> = Vec::new();
    for (entity, gui_id, node_id, click, pointer, down) in focused {
        let Some(doc) = assets.get_gui_mut(&gui_id) else {
            continue;
        };
        let Some(node) = doc.root.find_node_mut(&node_id) else {
            continue;
        };
        let GuiNodeType::TextInput { text, edit, max_length, .. } = &mut node.node_type else {
            // Focus can sit on any focusable node — a button reached by Tab,
            // say. Typing at one is simply not text entry.
            continue;
        };
        let key = text_edit::gui_key(&gui_id, &node_id);
        let view = text_edit::view(key);
        if !edit.dragging && click.is_none() && edit.moved_at == 0.0 {
            edit.focus_end(text);
        }
        if let Some((x, shift)) = click {
            let at = view.as_ref().map_or(text.chars().count(), |v| v.index_at(x));
            edit.click(text, at, shift, now);
        } else if down {
            if let (Some((x, _)), Some(v)) = (pointer, view.as_ref()) {
                edit.drag(text, v.index_at(x));
            }
        } else {
            edit.dragging = false;
        }
        let out = edit.update(text, input, *max_length, now);
        if out.changed {
            changed += 1;
        }
        if out.cancelled {
            cancel.push(entity);
            edit.moved_at = 0.0;
            edit.anchor = None;
        }
        if let Some(v) = view {
            crate::input::text_entry::report(crate::input::text_entry::Active { caret: v.caret_rect(edit.caret) });
        }
    }
    for e in cancel {
        if let Ok(mut g) = world.get::<&mut GuiComponent>(e) {
            g.focused_node_id = None;
        }
    }
    // A field not focused this frame was not submitted this frame, as an
    // entity-UI field already knew. Only `update` cleared the flag, and it
    // runs only on the focused field - so a chat that let its field go after
    // sending read `submitted` as true for good, and the next T (focus, then
    // the stale submit letting go again in the same `update`) never opened it.
    fn unsubmit(node: &mut crate::ui::gui_document::GuiNode, gui_id: &str, seen: &std::collections::HashSet<(String, String)>) {
        if let GuiNodeType::TextInput { edit, .. } = &mut node.node_type
            && edit.submitted
            && !seen.contains(&(gui_id.to_string(), node.id.clone()))
        {
            edit.submitted = false;
        }
        for child in &mut node.children {
            unsubmit(child, gui_id, seen);
        }
    }
    for gui_id in docs {
        if let Some(doc) = assets.get_gui_mut(&gui_id) {
            unsubmit(&mut doc.root, &gui_id, &seen);
        }
    }
    changed
}

/// The world posed for drawing, after everything that moves things this
/// frame: skeletal clips played, IK solved on top of them, dynamic bodies
/// blended between their last two physics steps (scripts and packages read
/// the stepped pose; the screen gets the smooth one), world transforms
/// resolved, then what records where things ended up - trails, cloth - and
/// the holes queued in carvable sprites cut from their pixels. Native and the
/// web ran these seven in this order by hand; the editor viewport splits them
/// between its Play tick and its render, since an edited scene animates
/// nothing but must still resolve and draw.
pub fn pose_world_for_draw(
    world: &mut World,
    assets: &mut crate::asset::AssetManager,
    physics: Option<&mut crate::physics::PhysicsWorld>,
    dt: f32,
) {
    crate::animation::skeletal::update_skeletal_players(world, assets.skeletal_clips(), dt);
    crate::feature!("ik", { crate::animation::ik::solve_ik(world) });
    // The physics' bodies between their last two physics steps. A host with
    // fixed-step gameplay passes no physics: it has blended everything a
    // step moved already (`FixedStep::blend_for_draw`), bodies included.
    if let Some(physics) = physics {
        physics.blend_transforms_for_render(world);
    }
    crate::ecs::hierarchy::resolve_world_transforms(world);
    crate::feature!("trails", { crate::ecs::trails::tick_trails(world, dt) });
    crate::feature!("cloth", { crate::ecs::cloth::tick_cloth(world, dt) });
    crate::ecs::carve::apply_carves(world, assets);
}

/// The gameplay systems every host runs each frame, in the one order they
/// have always run in: invulnerability, hitboxes, death, health mirrored to
/// scripts, separation, obstacles, health bars, name tags - then atlas regions
/// resolved, sprite animations stepped and their state machines. Returns the
/// frames' triggers (`animation::split_triggers` makes them sounds and
/// events). Each host carried the same eleven calls until Oct 2026.
pub fn tick_combat_and_animation(world: &mut World, assets: &crate::asset::AssetManager, dt: f32) -> Vec<TriggeredEvent> {
    let _span = crate::profiling::trace::span("animation.sprites");
    tick_invuln(world, dt);
    apply_hitbox_damage(world, dt);
    despawn_dead(world);
    mirror_health_to_custom(world);
    apply_separation(world);
    resolve_obstacle_collisions(world);
    sync_health_bars(world);
    sync_name_tags(world);
    resolve_sprite_regions(world, assets);
    let events = tick_sprite_animations(world, assets, dt);
    tick_animation_state_machines(world);
    events
}

/// Tick animation state machines — evaluates transitions and switches clips.
///
/// Call before `tick_sprite_animations` each frame.
pub fn tick_animation_state_machines(world: &mut World) {
    for (_, (animator, fsm)) in
        world.query_mut::<(&mut SpriteAnimator, &mut AnimStateMachine)>()
    {
        fsm.tick(animator);
    }
}

/// The event an entity-UI button click reaches scripts as, with the payload
/// `{ button = <entity id> }`. Listen with `on_event("ui_click", entity)` and
/// handle it in `on_ui_click(entity, data)`.
pub const UI_CLICK_EVENT: &str = "ui_click";

/// The `ui_click` payload for one clicked button.
pub fn ui_click_payload(button: u64) -> serde_json::Value {
    serde_json::json!({ "button": button })
}

/// Drive every interactive entity-UI widget from this frame's pointer and
/// keyboard.
///
/// Entity UI is the second UI system - one entity per widget, placed by
/// `UIElement` - and until Sep 2026 none of it was interactive. The button
/// half existed as `update_ui_buttons` and **no runtime ever called it**, so
/// `pressed` and `hovered` were always false; the slider, toggle and text
/// input had no interaction code at all and could only be moved by a script
/// (BUGS #61).
///
/// `pointer` is in **UI space** - virtual pixels, top-left origin, the space
/// the widgets are laid out and drawn in (see [`crate::ui::scaling`]). The
/// rest of the input state is raw, because nothing else about a keystroke or
/// a mouse button depends on where the UI is drawn.
///
/// Topmost wins. Widgets are hit-tested in reverse draw order, so a button
/// under a panel is not clickable through it - the renderer draws ascending
/// `z_order`, so the highest is on top and is asked first.
///
/// `clicked_buttons` collects the buttons **clicked** this frame, and every
/// host hands them to scripts as a `ui_click` event ([`UI_CLICK_EVENT`]).
/// A click is a release over the button the press began on - what `.gui`
/// buttons and every desktop toolkit do, and what lets a player back out of
/// a click by dragging off. Hosts used to collect this list and drop it, so
/// the only way for a script to act on a button was to poll `pressed`, which
/// is true for **every frame the button is held**: a "buy" button polled
/// that way bought sixty times a second, and a script that ticks slower than
/// the frame could miss a quick click altogether.
pub fn update_ui_widgets(
    world: &mut World,
    input: &mut crate::input::InputState,
    pointer: (f32, f32),
    screen_w: f32,
    screen_h: f32,
    clicked_buttons: &mut Vec<u64>,
) {
    let _span = crate::profiling::trace::span("ui.widgets");
    // Cameras drawing onto part of the screen (`screen_rect`) keep their
    // images placed against this frame's UI size.
    crate::ecs::views::sync_screen_rects(world, screen_w, screen_h);

    // The pointer arrives in the usual top-left-origin screen space every host
    // reports it in; the layout is Y-up. Comparing the two directly - as this
    // did - hit-tests a widget against its mirror image across the middle of
    // the screen, so a button in the top half could only be clicked by
    // pointing at the bottom half.
    let mouse = Vec2::new(pointer.0, screen_h - pointer.1);
    let down = input.mouse_left;
    let just_pressed = input.mouse_left_just_pressed;
    let just_released = input.mouse_left_just_released;

    // Radial menus (`ui::radial_menu`): laid out and pointed at.
    crate::feature!("radial_menu", { crate::ui::radial_menu::update_radial_menus(world, input, mouse, screen_w, screen_h) });
    // Speech bubbles (`ui::speech_bubble`): sized, placed over their
    // speakers, kept apart.
    crate::feature!("speech_bubble", { crate::ui::speech_bubble::update_speech_bubbles(world, screen_w, screen_h) });
    // Toasts (`ui::toast`): sliding in from a corner, stacking, leaving.
    crate::feature!("toast", { crate::ui::toast::update_toasts(world, screen_w, screen_h) });
    // Drag and drop (`ui::drag_drop`): an item held by the pointer owns it,
    // as a slider mid-drag does.
    #[cfg(feature = "drag_drop")]
    if crate::ui::drag_drop::update_drag_drop(world, input, mouse, screen_w, screen_h) {
        update_text_inputs(world, input, mouse, screen_h, None, false, down);
        return;
    }

    // A drag belongs to the slider it began on, wherever the pointer has since
    // gone - letting go the moment the cursor leaves the track would make the
    // two ends, the values people reach for most, the hardest to hit.
    if !down {
        for (_, slider) in world.query_mut::<&mut UISlider>() {
            slider.dragging = false;
        }
    }
    let mut dragged: Option<(hecs::Entity, UIElement)> = None;
    for (entity, (ui_elem, slider)) in world.query::<(&UIElement, &UISlider)>().iter() {
        if slider.dragging {
            dragged = Some((entity, ui_elem.clone()));
            break;
        }
    }
    if let Some((entity, ui_elem)) = dragged {
        set_slider_from_pointer(world, entity, &ui_elem, mouse, screen_w, screen_h);
        // A slider mid-drag owns the pointer; nothing underneath may also
        // react to it. A focused field still takes the keyboard.
        update_text_inputs(world, input, mouse, screen_h, None, false, down);
        return;
    }

    // Draw order, topmost first. `visible` is the same gate the renderer uses:
    // an invisible widget is not in the way of the pointer either.
    // The rect is computed in the query rather than cloning every visible
    // element per frame: an `UIElement` owns strings.
    let mut candidates: Vec<(i32, hecs::Entity, Vec2, Vec2)> = world
        .query::<&UIElement>()
        .iter()
        .filter(|(_, e)| e.visible)
        .map(|(entity, e)| {
            let (min, max) = e.screen_rect(screen_w, screen_h);
            (e.z_order, entity, min, max)
        })
        .collect();
    candidates.sort_by_key(|(z, _, _, _)| -*z);

    let hit = candidates.iter().find(|(_, entity, min, max)| {
        let inside =
            mouse.x >= min.x && mouse.x <= max.x && mouse.y >= min.y && mouse.y <= max.y;
        // Only a widget that takes the pointer can block one: a label or a
        // panel drawn over a button is decoration, not a shield.
        inside && is_interactive(world, *entity)
    });
    let hit_entity = hit.map(|(_, entity, _, _)| *entity);

    // --- buttons ----------------------------------------------------------
    let buttons: Vec<hecs::Entity> = world
        .query::<&UIButton>()
        .iter()
        .map(|(entity, _)| entity)
        .collect();
    for entity in buttons {
        let Ok(mut btn) = world.get::<&mut UIButton>(entity) else {
            continue;
        };
        if btn.disabled {
            btn.hovered = false;
            btn.pressed = false;
            btn.armed = false;
            continue;
        }
        let over = hit_entity == Some(entity);
        btn.hovered = over;
        btn.pressed = over && down;
        if over && just_pressed {
            btn.armed = true;
        }
        if just_released {
            if btn.armed && over {
                clicked_buttons.push(entity.id() as u64);
            }
            btn.armed = false;
        }
    }

    let Some(hit_entity) = hit_entity else {
        // A click on empty space drops keyboard focus, the way clicking off a
        // field does everywhere else. Typing still reaches a focused field -
        // it used to only while the pointer happened to be over a widget.
        update_text_inputs(world, input, mouse, screen_h, None, just_pressed, down);
        return;
    };
    // One clone, for the widget that was hit, rather than one per candidate.
    let hit_elem = world
        .get::<&UIElement>(hit_entity)
        .map(|e| (*e).clone())
        .expect("hit entity has an element");

    // --- toggles ----------------------------------------------------------
    if just_pressed
        && let Ok(mut toggle) = world.get::<&mut UIToggle>(hit_entity)
    {
        toggle.checked = !toggle.checked;
    }

    // --- sliders ----------------------------------------------------------
    if just_pressed && world.get::<&UISlider>(hit_entity).is_ok() {
        if let Ok(mut slider) = world.get::<&mut UISlider>(hit_entity) {
            slider.dragging = true;
        }
        // Jump to where the track was clicked, rather than waiting for the
        // pointer to move: clicking a track is how you set a value coarsely.
        set_slider_from_pointer(world, hit_entity, &hit_elem, mouse, screen_w, screen_h);
    }

    // --- text inputs ------------------------------------------------------
    update_text_inputs(world, input, mouse, screen_h, Some(hit_entity), just_pressed, down);
}

/// Entity UI's text fields for one frame: a click focuses the one it hit
/// and puts the caret where it landed (anything else lets go), a drag
/// selects, and the focused field takes the keyboard (`ui::text_edit`).
/// `mouse` is Y-up, as the hit test's.
fn update_text_inputs(
    world: &mut World,
    input: &mut crate::input::InputState,
    mouse: Vec2,
    screen_h: f32,
    hit: Option<hecs::Entity>,
    just_pressed: bool,
    down: bool,
) {
    use crate::input::KeyCode;
    use crate::ui::text_edit;
    let now = crate::profiling::unix_time();
    let shift = input.is_pressed(KeyCode::ShiftLeft) || input.is_pressed(KeyCode::ShiftRight);
    let fields: Vec<hecs::Entity> = world.query::<&UITextInput>().iter().map(|(e, _)| e).collect();
    for entity in fields {
        let Ok(mut field) = world.get::<&mut UITextInput>(entity) else {
            continue;
        };
        let field = &mut *field;
        let view = text_edit::view(text_edit::entity_key(entity));
        if just_pressed {
            if hit == Some(entity) {
                let at = view.as_ref().map_or(field.text.chars().count(), |v| v.index_at(mouse.x));
                field.edit.click(&field.text, at, shift, now);
                field.focused = true;
            } else {
                field.focused = false;
                field.edit.anchor = None;
                field.edit.dragging = false;
            }
        } else if down && field.focused {
            if let Some(v) = view.as_ref() {
                field.edit.drag(&field.text, v.index_at(mouse.x));
            }
        } else {
            field.edit.dragging = false;
        }
        if !field.focused {
            field.edit.submitted = false;
            continue;
        }
        let out = field.edit.update(&mut field.text, input, field.max_length, now);
        if out.cancelled {
            field.focused = false;
            field.edit.anchor = None;
        }
        if let Some(v) = view {
            // The view is Y-up like the hit test; a host wants the top left.
            let [x, y, w, h] = v.caret_rect(field.edit.caret);
            crate::input::text_entry::report(crate::input::text_entry::Active { caret: [x, screen_h - y - h, w, h] });
        }
    }
}

/// Whether this entity carries a widget that answers the pointer.
///
/// Panels, labels, images and progress bars are drawn, not operated, so they
/// neither react nor block.
fn is_interactive(world: &World, entity: hecs::Entity) -> bool {
    world.get::<&UIButton>(entity).is_ok()
        || world.get::<&UISlider>(entity).is_ok()
        || world.get::<&UIToggle>(entity).is_ok()
        || world.get::<&UITextInput>(entity).is_ok()
}

/// Put a slider's value where the pointer is along its track.
///
/// The inverse of what the renderer draws: the thumb centre sits at
/// `left + width * fraction`, so the fraction is how far along the track the
/// pointer is, clamped to the ends.
fn set_slider_from_pointer(
    world: &mut World,
    entity: hecs::Entity,
    ui_elem: &UIElement,
    mouse: Vec2,
    screen_w: f32,
    screen_h: f32,
) {
    let (min, max) = ui_elem.screen_rect(screen_w, screen_h);
    let width = max.x - min.x;
    let Ok(mut slider) = world.get::<&mut UISlider>(entity) else {
        return;
    };
    let fraction = if width > 0.0 {
        ((mouse.x - min.x) / width).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let range = slider.max - slider.min;
    slider.value = slider.min + fraction * range;
}

/// What [`navigate_ui`] pressed this frame, for the host to deliver the way it
/// delivers a click: `.gui` nodes as `(gui_id, node_id)` through its GUI click
/// dispatch, entity-UI buttons as `ui_click` events.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct NavOutcome {
    pub gui_activations: Vec<(String, String)>,
    pub ui_clicks: Vec<u64>,
}

/// Menus without a pointer: the arrow keys, a D-pad or a stick move focus, A
/// (or Enter, or Space) presses what is focused, and Tab still walks a
/// document's own order. Every host calls this once a frame and delivers
/// the [`NavOutcome`] as it would a click - so a controller press and a tap
/// on the same button reach the script the same way.
///
/// Only one UI takes the input: the visible `.gui` document with `focusable`
/// nodes (the one already holding focus, if several), else the entity-UI
/// elements marked `focusable`. Nothing unmarked takes part, so a HUD's
/// buttons never start answering the D-pad the player is moving with.
///
/// Focus appears on the first direction or Tab - never on Enter, Space or A,
/// which a game may be using for itself - and that first press only shows
/// where focus is. Only focus navigation put there is pressed by A / Enter /
/// Space. It moves to the nearest item the way pressed
/// (`ui::navigation::pick_in_direction`), a row moved to inside a scrolling
/// list is scrolled into view, and a slider focused in entity UI takes left
/// and right as its value. A press of the pointer drops entity-UI focus, as
/// it does a text field's.
///
/// Before Sep 2026 Tab was the whole of it: `update_gui_focus_from_input`,
/// which moved `.gui` focus and nothing else, so no menu could be used from
/// a controller at all.
pub fn navigate_ui(
    world: &mut World,
    assets: &mut crate::asset::AssetManager,
    input: &mut crate::input::InputState,
    ui_scale: &crate::ui::scaling::UiScale,
    dt: f32,
) -> NavOutcome {
    let _span = crate::profiling::trace::span("ui.navigate");
    // A screen reader's "focus this" / "press this" (`ui::accessibility`)
    // is carried out as navigation's own focus and press, delivered as
    // every other activation is - and the tree it reads is gathered after
    // this frame's navigation, so no host does either.
    #[cfg(feature = "accessibility")]
    let (activations, clicks) = crate::ui::accessibility::apply_actions(world, assets);
    #[allow(unused_mut)]
    let mut outcome = navigate_ui_inner(world, assets, input, ui_scale, dt);
    #[cfg(feature = "accessibility")]
    {
        outcome.gui_activations.extend(activations);
        outcome.ui_clicks.extend(clicks);
        crate::ui::accessibility::gather(world, assets, ui_scale, input.screen_size);
    }
    outcome
}

fn navigate_ui_inner(
    world: &mut World,
    assets: &mut crate::asset::AssetManager,
    input: &mut crate::input::InputState,
    ui_scale: &crate::ui::scaling::UiScale,
    dt: f32,
) -> NavOutcome {
    use crate::ui::navigation::read_intent_with;

    // A focused text field has the keyboard: its arrows and Enter are its own.
    let typing = text_field_has_keys(world, assets);
    let intent = read_intent_with(input, dt, !typing);
    track_gui_pointer(world, assets, input, ui_scale, dt);
    apply_slider_drags(world, assets);
    if input.mouse_left_just_pressed {
        // Back to the pointer: navigation's focus is not the player's any more.
        for (_, e) in world.query_mut::<&mut UIElement>() {
            e.focused = false;
        }
        for (_, g) in world.query_mut::<&mut super::components::GuiComponent>() {
            g.nav_focus = false;
        }
    }
    if intent.is_empty() {
        return NavOutcome::default();
    }
    let (pw, ph) = input.screen_size;
    let (sw, sh) = ui_scale.virtual_size(pw, ph);
    if sw <= 0.0 || sh <= 0.0 {
        return NavOutcome::default();
    }
    if let Some(outcome) = navigate_gui_document(world, assets, intent, (sw, sh)) {
        return outcome;
    }
    navigate_entity_ui(world, intent, (sw, sh))
}

/// A text field holds the keyboard this frame: a visible `.gui` document's
/// focused node is a `TextInput`, or an entity-UI `ui_text_input` is
/// focused. Navigation then leaves the arrows, Enter and Space to it.
fn text_field_has_keys(world: &World, assets: &crate::asset::AssetManager) -> bool {
    use super::components::GuiComponent;
    use crate::ui::gui_document::GuiNodeType;
    let gui = world.query::<&GuiComponent>().iter().any(|(_, g)| {
        g.visible
            && g.focused_node_id.as_ref().is_some_and(|id| {
                assets
                    .get_gui(&g.gui_id)
                    .and_then(|d| d.root.find_node(id))
                    .is_some_and(|n| matches!(n.node_type, GuiNodeType::TextInput { .. }))
            })
    });
    gui || world.query::<&crate::ui::UITextInput>().iter().any(|(_, t)| t.focused)
}

/// Which `.gui` button is under the pointer, and whether the pointer is
/// held: what draws a button's `hover_color` and `pressed_color`. Part of
/// [`navigate_ui`], which every runtime calls once a frame, so no host
/// tracks it itself. The hit-test is `process_gui_clicks`' - the same
/// virtual pixels, the same layout, the topmost button winning.
pub fn track_gui_pointer(
    world: &mut World,
    assets: &crate::asset::AssetManager,
    input: &crate::input::InputState,
    ui_scale: &crate::ui::scaling::UiScale,
    dt: f32,
) {
    use super::components::{GUI_HOVER_SECONDS, GuiComponent};
    use crate::ui::gui_document::GuiNodeType;
    let (pw, ph) = input.screen_size;
    let pointer = (pw > 0.0 && ph > 0.0).then(|| ui_scale.to_ui(input.mouse_position.0, input.mouse_position.1, pw, ph));
    let (sw, sh) = ui_scale.virtual_size(pw, ph);
    for (_, g) in world.query_mut::<&mut GuiComponent>() {
        let mut hovered = None;
        g.pointer_ui = pointer;
        if g.visible
            && let Some((mx, my)) = pointer
            && let Some(doc) = assets.get_gui(&g.gui_id)
        {
            let layout = crate::ui::layout::compute_layout(doc, sw, sh);
            // A click on a text field focuses it and puts its caret where
            // the click was (`apply_text_input` reads `text_click`); a
            // click anywhere else lets a focused field go.
            if input.mouse_left_just_pressed {
                let field = layout.iter().rev().find_map(|e| {
                    let node = doc.root.find_node(&e.id)?;
                    (e.contains(mx, my) && e.opacity > 0.0 && node.style.visible)
                        .then_some((e.id.clone(), matches!(node.node_type, GuiNodeType::TextInput { .. })))
                });
                match field {
                    Some((id, true)) => {
                        let shift = input.is_pressed(crate::input::KeyCode::ShiftLeft)
                            || input.is_pressed(crate::input::KeyCode::ShiftRight);
                        g.focused_node_id = Some(id);
                        g.text_click = Some((mx, shift));
                    }
                    _ => {
                        let on_field = g.focused_node_id.as_ref().and_then(|f| doc.root.find_node(f)).is_some_and(
                            |n| matches!(n.node_type, GuiNodeType::TextInput { .. }),
                        );
                        if on_field {
                            g.focused_node_id = None;
                        }
                    }
                }
            }
            // A slider follows the pointer from the press that took it until
            // the button is let go, wherever the pointer wanders - as an
            // entity-UI slider does. The value is the thumb's arithmetic in
            // `render_gui_wgpu` turned round: `rect.x + fraction * width`.
            if input.mouse_left_just_pressed {
                g.slider_drag = layout.iter().rev().find_map(|e| {
                    let node = doc.root.find_node(&e.id)?;
                    if !(e.contains(mx, my) && e.opacity > 0.0 && node.style.visible) {
                        return None;
                    }
                    Some(matches!(node.node_type, GuiNodeType::Slider { .. }).then(|| (e.id.clone(), 0.0)))
                })
                .flatten();
            }
            if let Some((id, value)) = g.slider_drag.as_mut()
                && let (Some(e), Some(node)) = (layout.iter().find(|e| &e.id == id), doc.root.find_node(id))
                && let GuiNodeType::Slider { min, max, .. } = node.node_type
            {
                let t = ((mx - e.rect.x) / e.rect.width.max(f32::EPSILON)).clamp(0.0, 1.0);
                *value = min + (max - min) * t;
            }
            // Later entries draw over earlier ones.
            hovered = layout.iter().rev().find_map(|e| {
                let node = doc.root.find_node(&e.id)?;
                (e.contains(mx, my)
                    && e.opacity > 0.0
                    && node.style.visible
                    && matches!(node.node_type, GuiNodeType::Button { .. }))
                .then(|| e.id.clone())
            });
        }
        // Easing in: on the same button a little further, on a new one
        // from the start.
        g.hover_t = if hovered.is_some() && hovered == g.hovered_node_id {
            (g.hover_t + dt / GUI_HOVER_SECONDS).min(1.0)
        } else {
            0.0
        };
        g.hovered_node_id = hovered;
        g.pointer_down = input.mouse_left;
        if !input.mouse_left || !g.visible {
            g.slider_drag = None;
        }
    }
}

/// Write the value each held `.gui` slider's pointer is at (`slider_drag`,
/// set by [`track_gui_pointer`]) into its document, where `GUI.get(doc,
/// node, "value")` and the drawing read it.
fn apply_slider_drags(world: &mut World, assets: &mut crate::asset::AssetManager) {
    use super::components::GuiComponent;
    use crate::ui::gui_document::GuiNodeType;
    for (_, g) in world.query_mut::<&GuiComponent>() {
        if let Some((id, v)) = &g.slider_drag
            && let Some(node) = assets.get_gui_mut(&g.gui_id).and_then(|d| d.root.find_node_mut(id))
            && let GuiNodeType::Slider { value, .. } = &mut node.node_type
        {
            *value = *v;
        }
    }
}

/// [`navigate_ui`] for `.gui` documents. `None` when no visible document has
/// anything focusable, so entity UI gets its turn.
fn navigate_gui_document(
    world: &mut World,
    assets: &mut crate::asset::AssetManager,
    intent: crate::ui::navigation::NavIntent,
    (sw, sh): (f32, f32),
) -> Option<NavOutcome> {
    use super::components::GuiComponent;
    use crate::ui::navigation::{NavDirection, NavRect, pick_in_direction};

    // The document to drive: the one holding focus, else the first with
    // something to focus.
    let mut target: Option<(hecs::Entity, String, Option<String>, bool)> = None;
    for (entity, g) in world.query::<&GuiComponent>().iter() {
        if !g.visible {
            continue;
        }
        let Some(doc) = assets.get_gui(&g.gui_id) else { continue };
        if !doc.navigable || doc.root.collect_focusable().is_empty() {
            continue;
        }
        let holds = g.focused_node_id.is_some();
        if target.is_none() || (holds && target.as_ref().is_some_and(|t| t.2.is_none())) {
            target = Some((entity, g.gui_id.clone(), g.focused_node_id.clone(), g.nav_focus));
        }
    }
    let (entity, gui_id, focused, nav_focus) = target?;
    let doc = assets.get_gui(&gui_id)?;
    let order = doc.root.collect_focusable();
    let layout = crate::ui::layout::compute_layout(doc, sw, sh);
    let rect_of = |id: &str| {
        layout
            .iter()
            .find(|e| e.id == id)
            .map(|e| NavRect { x: e.rect.x, y: e.rect.y, w: e.rect.width, h: e.rect.height })
    };
    let current = focused.filter(|id| order.contains(id));
    // A focused slider takes left and right as its value, as in entity UI.
    let on_slider = current.as_deref().and_then(|c| doc.root.find_node(c)).is_some_and(|n| {
        matches!(n.node_type, crate::ui::gui_document::GuiNodeType::Slider { .. })
    });
    let slide = match intent.direction {
        Some(NavDirection::Left) if on_slider => Some(-1.0_f32),
        Some(NavDirection::Right) if on_slider => Some(1.0_f32),
        _ => None,
    };

    let mut outcome = NavOutcome::default();
    // Enter / Space / A alone never create focus: they may be the game's.
    if current.is_none() && intent.direction.is_none() && intent.tab.is_none() {
        return Some(outcome);
    }
    let next = match &current {
        None => {
            // First sight of focus: show it, press nothing.
            let backwards = intent.tab == Some(true);
            Some(if backwards { order.last() } else { order.first() }?.clone())
        }
        Some(cur) => {
            if let Some(backwards) = intent.tab {
                let i = order.iter().position(|n| n == cur).unwrap_or(0);
                let n = order.len();
                Some(order[if backwards { (i + n - 1) % n } else { (i + 1) % n }].clone())
            } else if slide.is_some() {
                None
            } else if let (Some(dir), Some(from)) = (intent.direction, rect_of(cur)) {
                let others: Vec<(&String, NavRect)> = order
                    .iter()
                    .filter(|n| *n != cur)
                    .filter_map(|n| rect_of(n).map(|r| (n, r)))
                    .collect();
                let rects: Vec<NavRect> = others.iter().map(|(_, r)| *r).collect();
                pick_in_direction(from, &rects, dir).map(|i| others[i].0.clone())
            } else {
                None
            }
        }
    };
    if intent.activate
        && nav_focus
        && let Some(cur) = &current
        && !doc.refuses_clicks(cur)
    {
        outcome.gui_activations.push((gui_id.clone(), cur.clone()));
    }
    if let (Some(sign), Some(cur)) = (slide, &current)
        && let Some(node) = assets.get_gui_mut(&gui_id).and_then(|d| d.root.find_node_mut(cur))
        && let crate::ui::gui_document::GuiNodeType::Slider { value, min, max, .. } = &mut node.node_type
    {
        // A tenth of the range a press.
        *value = (*value + (*max - *min) * 0.1 * sign).clamp(min.min(*max), max.max(*min));
    }
    let moved = next.as_ref().is_some_and(|n| current.as_ref() != Some(n));
    if moved {
        scroll_into_view(assets, &gui_id, &layout, next.as_deref().unwrap_or_default());
    }
    if let Ok(mut g) = world.get::<&mut GuiComponent>(entity) {
        if moved {
            g.focused_node_id = next;
        }
        // A direction or Tab makes whatever is focused navigation's, even a
        // node a click focused: the player has picked the D-pad up.
        if intent.direction.is_some() || intent.tab.is_some() {
            g.nav_focus = true;
        }
    }
    Some(outcome)
}

/// Scroll the list `node_id` sits in just far enough to show it whole - the
/// row focus just moved to would otherwise be below the fold.
fn scroll_into_view(
    assets: &mut crate::asset::AssetManager,
    gui_id: &str,
    layout: &crate::ui::layout::LayoutResult,
    node_id: &str,
) {
    let Some(entry) = layout.iter().find(|e| e.id == node_id) else { return };
    let Some(clip) = entry.clip else { return };
    let Some(doc) = assets.get_gui_mut(gui_id) else { return };
    let Some(scroller_id) = doc.scroll_container_of(node_id) else { return };
    let max = layout
        .iter()
        .find(|e| e.id == scroller_id)
        .and_then(|e| e.scroll_max)
        .unwrap_or((0.0, 0.0));
    let r = entry.rect;
    // Past the far edge: bring the far edge in. Past the near edge (or taller
    // than the view): bring the near edge in, which wins.
    let shift = |lo: f32, len: f32, view_lo: f32, view_len: f32| {
        let mut d = 0.0;
        if lo + len > view_lo + view_len {
            d = lo + len - (view_lo + view_len);
        }
        if lo - d < view_lo {
            d = lo - view_lo;
        }
        d
    };
    let dy = shift(r.y, r.height, clip.y, clip.height);
    let dx = shift(r.x, r.width, clip.x, clip.width);
    if let Some(node) = doc.root.find_node_mut(&scroller_id) {
        node.style.scroll_y = (node.style.scroll_y + dy).clamp(0.0, max.1);
        node.style.scroll_x = (node.style.scroll_x + dx).clamp(0.0, max.0);
    }
}

/// [`navigate_ui`] for entity UI: the visible elements marked `focusable`.
fn navigate_entity_ui(
    world: &mut World,
    intent: crate::ui::navigation::NavIntent,
    (sw, sh): (f32, f32),
) -> NavOutcome {
    use crate::ui::navigation::{NavDirection, NavRect, pick_in_direction};
    use crate::ui::{UISlider, UIToggle};

    // Reading order, in the y-down space `pick_in_direction` works in.
    let mut items: Vec<(hecs::Entity, NavRect, bool)> = world
        .query::<&UIElement>()
        .iter()
        .filter(|(_, e)| e.visible && e.focusable)
        .map(|(entity, e)| {
            let (min, max) = e.screen_rect(sw, sh);
            (entity, NavRect { x: min.x, y: sh - max.y, w: max.x - min.x, h: max.y - min.y }, e.focused)
        })
        .collect();
    let mut outcome = NavOutcome::default();
    if items.is_empty() {
        return outcome;
    }
    items.sort_by(|a, b| a.1.y.total_cmp(&b.1.y).then(a.1.x.total_cmp(&b.1.x)));
    let current = items.iter().position(|(_, _, f)| *f);
    // Enter / Space / A alone never create focus: they may be the game's.
    if current.is_none() && intent.direction.is_none() && intent.tab.is_none() {
        return outcome;
    }

    let next = match current {
        None => Some(if intent.tab == Some(true) { items.len() - 1 } else { 0 }),
        Some(i) => {
            let entity = items[i].0;
            let slider = world.get::<&UISlider>(entity).is_ok();
            if let Some(backwards) = intent.tab {
                let n = items.len();
                Some(if backwards { (i + n - 1) % n } else { (i + 1) % n })
            } else if let Some(dir) = intent.direction.filter(|d| {
                !(slider && matches!(d, NavDirection::Left | NavDirection::Right))
            }) {
                let others: Vec<usize> = (0..items.len()).filter(|j| *j != i).collect();
                let rects: Vec<NavRect> = others.iter().map(|j| items[*j].1).collect();
                pick_in_direction(items[i].1, &rects, dir).map(|k| others[k])
            } else {
                // A focused slider takes left and right as its value: a
                // tenth of the range a press.
                if let (true, Some(dir)) = (slider, intent.direction)
                    && let Ok(mut s) = world.get::<&mut UISlider>(entity)
                {
                    let step = (s.max - s.min) * 0.1 * if dir == NavDirection::Left { -1.0 } else { 1.0 };
                    s.value = (s.value + step).clamp(s.min.min(s.max), s.max.max(s.min));
                }
                None
            }
        }
    };
    if intent.activate
        && let Some(i) = current
    {
        let entity = items[i].0;
        if world.get::<&UIButton>(entity).is_ok_and(|b| !b.disabled) {
            outcome.ui_clicks.push(entity.id() as u64);
        }
        if let Ok(mut t) = world.get::<&mut UIToggle>(entity) {
            t.checked = !t.checked;
        }
    }
    if let Some(n) = next
        && current != Some(n)
    {
        for (j, (entity, _, _)) in items.iter().enumerate() {
            if let Ok(mut e) = world.get::<&mut UIElement>(*entity) {
                e.focused = j == n;
            }
        }
    }
    outcome
}

/// The colour `tween_color` starts from: the entity's sprite's, or its
/// text's when it has text and no sprite. Every runtime starts a colour
/// tween from here, and `tick_tweens` writes back to the same place.
pub fn tween_color_of(world: &World, entity: hecs::Entity) -> Option<glam::Vec4> {
    if let Ok(sprite) = world.get::<&SpriteComponent>(entity) {
        return Some(glam::Vec4::from_array(sprite.color));
    }
    world.get::<&TextComponent>(entity).ok().map(|t| glam::Vec4::from_array(t.color))
}

/// Tick all active tweens and apply results to ECS components.
///
/// Call after scripts but before rendering each frame.
///
/// Builds an `id -> Entity` index once per call when any tween is active.
/// A host that already holds one calls [`tick_tweens_indexed`]; the native
/// script system does. Each result used to scan every `Transform` in the
/// world until the id matched - `O(tweens x entities)` a frame - and a
/// colour tween walked the whole world.
pub fn tick_tweens(world: &mut World, tween_manager: &mut TweenManager, dt: f32) {
    let results = tween_manager.tick(dt);
    if results.is_empty() {
        return;
    }
    let index: std::collections::HashMap<u64, hecs::Entity> =
        world.iter().map(|e| (e.entity().id() as u64, e.entity())).collect();
    apply_tween_results(world, &results, &index);
}

/// [`tick_tweens`] resolving entities through an index the caller built.
pub fn tick_tweens_indexed(
    world: &mut World,
    tween_manager: &mut TweenManager,
    dt: f32,
    index: &std::collections::HashMap<u64, hecs::Entity>,
) {
    let results = tween_manager.tick(dt);
    apply_tween_results(world, &results, index);
}

fn apply_tween_results(
    world: &mut World,
    results: &[crate::animation::tween::TweenResult],
    index: &std::collections::HashMap<u64, hecs::Entity>,
) {
    for result in results {
        let Some(&entity) = index.get(&result.entity_id) else { continue };
        match result.target {
            TweenTarget::Position => {
                if let Ok(mut transform) = world.get::<&mut Transform>(entity) {
                    transform.position.x = result.value.x;
                    transform.position.y = result.value.y;
                }
            }
            TweenTarget::Rotation => {
                if let Ok(mut transform) = world.get::<&mut Transform>(entity) {
                    transform.rotation = result.value.x;
                }
            }
            TweenTarget::Scale => {
                if let Ok(mut transform) = world.get::<&mut Transform>(entity) {
                    transform.scale.x = result.value.x;
                    transform.scale.y = result.value.y;
                }
            }
            TweenTarget::SpriteColor => {
                // The sprite's colour, or the text's on an entity that has
                // text and no sprite - a label fading out.
                let color = [result.value.x, result.value.y, result.value.z, result.value.w];
                if let Ok(mut sprite) = world.get::<&mut SpriteComponent>(entity) {
                    sprite.color = color;
                } else if let Ok(mut text) = world.get::<&mut TextComponent>(entity) {
                    text.color = color;
                }
            }
            TweenTarget::SpriteAlpha => {
                if let Ok(mut sprite) = world.get::<&mut SpriteComponent>(entity) {
                    sprite.color[3] = result.value.x;
                }
            }
        }
    }
}

/// Script system state — owns the per-language [`ScriptBridgeRunner`]s and
/// dispatches every `ScriptComponent` entity through them each frame.
///
/// P7.5d: legacy `LuaScriptEngine` / `JsScriptEngine` engines, the
/// `BridgeScript` shadow-mode component, and the `set_bridge_runner`
/// installer have all been removed. Production native builds always run via
/// the bridge.
#[cfg(feature = "native")]
/// Phase C — maximum depth of nested `send_event_to` / `call(entity, ...)`
/// dispatches per frame. Exceeding this drops the remaining queued targeted
/// commands and logs an error, preventing a runaway A→B→A loop from hanging
/// the frame.
const MAX_SCRIPT_CALL_DEPTH: u32 = 8;

#[cfg(feature = "native")]
pub struct ScriptSystem {
    pub api: crate::scripting::api::ScriptState,
    /// Scripts that failed to read from disk (avoid retrying every frame).
    failed_scripts: std::collections::HashSet<hecs::Entity>,
    /// Bridge runner for `ScriptLanguage::Lua` entities (Luau adapter).
    pub lua_bridge: crate::scripting::bridge::ScriptBridgeRunner,
    /// Bridge runner for `ScriptLanguage::JavaScript` entities (host
    /// QuickJS adapter). Only present when `js-adapter-host` is enabled;
    /// otherwise JS entities are skipped with a warning.
    #[cfg(feature = "js-adapter-host")]
    pub js_bridge: crate::scripting::bridge::ScriptBridgeRunner,
    /// Per-entity language map so timer / event / GUI dispatch can pick
    /// the correct runner.
    entity_languages: std::collections::HashMap<u64, ScriptLanguage>,
    /// The frame's `id -> Entity` index between frames, so it is one
    /// allocation for the session rather than one per frame. Lent to the
    /// bridge snapshot for the frame (`snapshot_world_borrow_into`) and taken
    /// back at the end of `run`.
    entity_handles_pool: std::collections::HashMap<u64, hecs::Entity>,
    /// Where a `ScriptComponent::source_path` is read from, when the host
    /// set one: the project's assets directory. Without it the read is
    /// relative to the process directory, which is only the project root
    /// when the runtime happens to be launched there.
    script_root: Option<std::path::PathBuf>,
    /// Optional timing accumulator for run_via_bridge sub-phases. Logged
    /// every `JENGINE_SCRIPT_PROFILE` frames (default 0 = disabled). Set the
    /// env var to e.g. `600` to print average µs spent in snapshot / run /
    /// route / apply / dispatch every ~10 seconds at 60 fps.
    profile_window: u32,
    profile_frames: u32,
    profile_us_legacy_snap: u128,
    profile_us_bridge_snap: u128,
    profile_us_subsystem_snap: u128,
    profile_us_collect_loads: u128,
    profile_us_run_lua: u128,
    profile_us_run_js: u128,
    profile_us_route: u128,
    profile_us_apply: u128,
    profile_us_post: u128,
    profile_total_cmds: u64,
    /// Optional viewport center (world-space) used for distance-based
    /// script LOD / culling. Set per-frame from the active camera.
    viewport_center: Option<glam::Vec2>,
    /// Aggregated script timings captured during the most recent
    /// `run()` call. Drained by `take_frame_metrics()`.
    last_script_timings: crate::profiling::ScriptTimings,
    /// Aggregated frame counters captured during the most recent
    /// `run()` call. Drained by `take_frame_metrics()`.
    last_frame_counters: crate::profiling::FrameCounters,
    /// Physics contact events queued by the runtime after the most
    /// recent `physics.step()`. Drained inside `run_via_bridge` on the
    /// next tick to dispatch `on_collision_enter` / `on_collision_exit`
    /// to user scripts. Each tuple is `(entity_a_bits, entity_b_bits,
    /// started)` where `started == true` means the contact began this
    /// step (Enter) and `false` means it ended (Exit).
    pending_contact_events: Vec<(u64, u64, bool, f32, f32, f32)>,
    /// Animation frame events waiting to reach `on_animation_event`:
    /// `(entity id, event name, data)`.
    pending_animation_events: Vec<(u64, String, Option<serde_json::Value>)>,
    /// Handlers owed to entities' own scripts (`ecs::callbacks`):
    /// `on_screen_entered`, `on_path_end` ...
    pending_entity_callbacks: Vec<(u64, &'static str)>,
    /// Events a late callback sent (`on_collision_enter`, `on_animation_event`,
    /// `on_screen_entered` ...): its commands are applied after the frame's
    /// listener dispatch, so its `send_event`s go out on the next `run`.
    pending_late_events: Vec<crate::scripting::api::GameEvent>,
    /// Behaviour graphs' `EmitEvent`s from the last barrier, waiting for the
    /// next `run` to hand them to `on_<name>` listeners. A frame late, as
    /// contacts and triggers are: graphs run after scripts.
    pending_graph_events: Vec<crate::behavior::EmitEventOutcome>,
    /// Events the engine itself raises for scripts - an entity-UI click -
    /// delivered with the graphs' on the next `run`.
    pending_engine_events: Vec<crate::scripting::api::GameEvent>,
    /// The events *scripts* sent in the last `run`, for the runner to hand
    /// to graphs - see [`Self::take_events_for_graphs`].
    events_for_graphs: Vec<crate::scripting::api::GameEvent>,
    /// World gravity as last reported by the host, for the `get_gravity`
    /// script global. The script system does not own the physics world, so
    /// the runner hands this over each frame (see `set_world_gravity`).
    world_gravity: (f32, f32),

    /// Borrowed for the length of one `run` so `raycast` can be answered.
    /// See [`ScriptSystem::set_physics_ptr`]; it is `None` at every other
    /// moment, deliberately.
    #[cfg(feature = "physics")]
    physics_ptr: Option<*const crate::physics::PhysicsWorld>,
    /// Active camera view as last reported by the host. See
    /// `BridgeReadSnapshot::camera_view`.
    camera_view: (f32, f32, f32, f32, f32),
    /// The active camera's rotation, for the same conversion.
    camera_rotation: f32,
}

#[cfg(feature = "native")]
impl ScriptSystem {
    pub fn new() -> anyhow::Result<Self> {
        let lua_adapter = crate::scripting::bridge::LuauAdapter::new(
            crate::scripting::bridge::Capabilities::empty(),
        )
        .map_err(|e| anyhow::anyhow!("luau adapter init failed: {e}"))?;
        Ok(Self {
            api: crate::scripting::api::ScriptState::new(),
            failed_scripts: std::collections::HashSet::new(),
            lua_bridge: crate::scripting::bridge::ScriptBridgeRunner::new(
                Box::new(lua_adapter),
                crate::scripting::bridge::Capabilities::empty(),
            ),
            #[cfg(feature = "js-adapter-host")]
            js_bridge: crate::scripting::bridge::ScriptBridgeRunner::new(
                Box::new(crate::scripting::bridge::JsAdapter::new()),
                crate::scripting::bridge::Capabilities::empty(),
            ),
            entity_languages: std::collections::HashMap::new(),
            entity_handles_pool: std::collections::HashMap::new(),
            script_root: None,
            profile_window: std::env::var("JENGINE_SCRIPT_PROFILE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            profile_frames: 0,
            profile_us_legacy_snap: 0,
            profile_us_bridge_snap: 0,
            profile_us_subsystem_snap: 0,
            profile_us_collect_loads: 0,
            profile_us_run_lua: 0,
            profile_us_run_js: 0,
            profile_us_route: 0,
            profile_us_apply: 0,
            profile_us_post: 0,
            profile_total_cmds: 0,
            viewport_center: None,
            last_script_timings: crate::profiling::ScriptTimings::default(),
            last_frame_counters: crate::profiling::FrameCounters::default(),
            pending_contact_events: Vec::new(),
            pending_animation_events: Vec::new(),
            pending_entity_callbacks: Vec::new(),
            pending_late_events: Vec::new(),
            pending_graph_events: Vec::new(),
            pending_engine_events: Vec::new(),
            events_for_graphs: Vec::new(),
            world_gravity: (0.0, 0.0),
            #[cfg(feature = "physics")]
            physics_ptr: None,
            camera_view: (0.0, 0.0, 0.0, 0.0, 0.0),
            camera_rotation: 0.0,
        })
    }

    /// Stash physics contact events emitted by the most recent
    /// `PhysicsWorld::step()` so they can be dispatched as
    /// `on_collision_enter` / `on_collision_exit` to user scripts on
    /// the next `run()` call. Call this from the runtime immediately
    /// after the physics step.
    ///
    /// Same-frame semantics aren't possible here because native runtimes
    /// step physics AFTER `script_system.run`; the 1-frame delay matches
    /// the existing trigger-overlap dispatch (whose `TriggerOverlaps`
    /// state is also written in `tick_world` after scripts run).
    /// Tell the script system what gravity the physics world is running with.
    /// Call once per frame (or whenever it changes) before `run`.
    pub fn set_world_gravity(&mut self, gravity: (f32, f32)) {
        self.world_gravity = gravity;
    }

    /// Lend the script system the physics world for the duration of one
    /// `run`, so `raycast` can be answered.
    ///
    /// This is the only script read that cannot come from a snapshot: the
    /// ray's origin and direction come from the script, so there is nothing
    /// to precompute, and both alternatives are worse than borrowing -
    /// copying the collider set every frame is not cheap, and
    /// reimplementing ray-vs-shape would drift from the rapier everything
    /// else collides with.
    ///
    /// Unlike [`Self::set_world_gravity`], this is **not** a value that can
    /// sit here between frames. A scene load replaces the runtime's
    /// `PhysicsWorld`, which would leave a stale pointer, so the contract
    /// is: set it immediately before `run` and clear it immediately after.
    /// [`Self::clear_physics_ptr`] is the other half.
    ///
    /// SAFETY: the caller must own the physics world for the whole of the
    /// following `run` and must not move, replace or mutate it during it.
    #[cfg(feature = "physics")]
    pub fn set_physics_ptr(&mut self, physics: &crate::physics::PhysicsWorld) {
        self.physics_ptr = Some(physics as *const _);
    }

    /// Drop the borrowed physics pointer. Call straight after `run`.
    #[cfg(feature = "physics")]
    pub fn clear_physics_ptr(&mut self) {
        self.physics_ptr = None;
    }

    /// Tell the script system where the active camera is looking, as
    /// `(centre_x, centre_y, viewport_w, viewport_h, zoom)`. Call once per
    /// frame before `run`; without it `get_mouse_world_position` treats
    /// screen space as world space.
    pub fn set_camera_view(&mut self, view: (f32, f32, f32, f32, f32)) {
        self.camera_view = view;
    }

    /// The active camera's rotation, for `get_mouse_world_position`.
    pub fn set_camera_rotation(&mut self, rotation: f32) {
        self.camera_rotation = rotation;
    }

    /// Queue the graphs' events from this frame's barrier for the scripts'
    /// `on_<name>` listeners, delivered on the next `run`.
    ///
    /// Each runtime calls this with `flush_commands_and_route_events`'s
    /// `emit_event_requests`. Nothing did before Sep 2026, so a graph could
    /// tell other graphs things and never the game - an enemy authored as a
    /// graph had no way to say it had died.
    pub fn queue_graph_events(&mut self, events: &[crate::behavior::EmitEventOutcome]) {
        self.pending_graph_events.extend_from_slice(events);
    }

    /// Queue an event the engine raises - `ui_click` - for the scripts'
    /// `on_<name>` listeners, delivered on the next `run` like a graph's.
    pub fn queue_engine_event(&mut self, name: &str, data: serde_json::Value) {
        self.pending_engine_events.push(crate::scripting::api::GameEvent {
            name: name.to_string(),
            sender: 0,
            data: Some(data),
        });
    }

    /// The events **scripts** sent during the last `run`, for the runner to
    /// deliver to graphs with `BehaviorGraphSystem::deliver_script_event`.
    ///
    /// Deliberately excludes the graph events [`Self::queue_graph_events`]
    /// fed in: those reached every graph at the barrier already, and handing
    /// them back would deliver each one to graphs twice.
    pub fn take_events_for_graphs(&mut self) -> Vec<crate::scripting::api::GameEvent> {
        std::mem::take(&mut self.events_for_graphs)
    }

    pub fn queue_contact_events(
        &mut self,
        events: &[crate::physics::ContactEvent],
    ) {
        for ev in events {
            let started =
                matches!(ev.event_type, crate::physics::ContactType::Started);
            self.pending_contact_events
                .push((ev.entity_a, ev.entity_b, started, ev.impulse, ev.point.0, ev.point.1));
        }
    }

    /// Queue the handlers `ecs::callbacks` says are owed - pass it
    /// `callbacks::take()` once a frame - for dispatch to the entities' own
    /// scripts on the next `run`.
    pub fn queue_entity_callbacks(&mut self, owed: &[crate::ecs::callbacks::EntityCallback]) {
        self.pending_entity_callbacks
            .extend(owed.iter().map(|c| (c.entity.id() as u64, c.event())));
    }

    /// Queue the events an animation's frames fired, for dispatch to
    /// `on_animation_event(entity, { name, data })` on the next `run`.
    ///
    /// The runtime calls this with what `tick_sprite_animations` returned -
    /// which every runtime used to discard, so a frame that emitted an event
    /// reached nothing and the handler name was dead.
    pub fn queue_animation_events(&mut self, events: &[crate::animation::TriggeredEvent]) {
        let (_, fired) = crate::animation::split_triggers(events);
        self.pending_animation_events.extend(fired);
    }

    /// Update the viewport center used for distance-based script LOD.
    /// Call once per frame from the runtime with the active camera's
    /// world-space position.
    pub fn set_viewport_center(&mut self, center: Option<glam::Vec2>) {
        self.viewport_center = center;
    }

    /// Apply a script execution budget to both bridge runners. Limits
    /// per-frame instruction counts / wall time for unsafe scripts.
    pub fn set_script_budget(&mut self, cfg: crate::scripting::bridge::BudgetConfig) {
        self.lua_bridge.set_budget(cfg);
        #[cfg(feature = "js-adapter-host")]
        {
            self.js_bridge.set_budget(cfg);
        }
    }

    /// Install a project-defined script API registry into both bridge
    /// runners (so user-registered globals are visible from Lua + JS).
    pub fn install_script_api(&mut self, api: &crate::package::script_api::ScriptApiRegistry) {
        self.lua_bridge.install_script_api(api);
        #[cfg(feature = "js-adapter-host")]
        {
            self.js_bridge.install_script_api(api);
        }
    }

    /// Install the package-contributed script globals.
    ///
    /// Separate from [`Self::install_script_api`] because these reach every
    /// runtime: the global only queues a `PackageCall`, and the handler runs
    /// at apply time through the same dispatcher the browser hosts use — so
    /// the same package function works in the editor and a web build, which
    /// `install_script_api` handlers never did.
    pub fn install_package_commands(
        &mut self,
        commands: &crate::package::script_commands::PackageCommandRegistry,
    ) {
        for info in commands.commands() {
            self.lua_bridge
                .install_package_command(&info.name, &info.tag, &info.args);
            #[cfg(feature = "js-adapter-host")]
            {
                self.js_bridge
                    .install_package_command(&info.name, &info.tag, &info.args);
            }
        }
        // `Realm.talk` beside `realm_talk`. After the loop: a namespace
        // table points at globals, so they have to exist first.
        self.lua_bridge
            .install_namespaces(&crate::scripting::namespaces::package_entries(commands.commands()));
    }

    /// Drain the most recently aggregated script timings + frame
    /// counters. Both internal accumulators are reset to default after
    /// the call so the next frame starts clean.
    pub fn take_frame_metrics(
        &mut self,
    ) -> (crate::profiling::ScriptTimings, crate::profiling::FrameCounters) {
        (
            std::mem::take(&mut self.last_script_timings),
            std::mem::take(&mut self.last_frame_counters),
        )
    }

    /// Phase D — Wire a `require()` resolver into both the Lua and JS bridge
    /// runners. Typically called once at startup with
    /// `bridge::fs_resolver(asset_root.join("scripts"))` so script authors can
    /// `require("lib/util")` from anywhere under `assets/scripts/`.
    /// The directory a `ScriptComponent::source_path` is resolved against
    /// on its first sighting - the project's `assets/`. A host that knows
    /// its project root sets this; without it the read falls back to
    /// `assets/<path>` and then `<path>` relative to the process directory,
    /// which is only the project root when the runtime is launched there.
    pub fn set_script_root(&mut self, root: impl Into<std::path::PathBuf>) {
        self.script_root = Some(root.into());
    }

    pub fn set_module_resolver(
        &mut self,
        resolver: crate::scripting::bridge::ModuleResolver,
    ) {
        self.lua_bridge.set_module_resolver(resolver.clone());
        #[cfg(feature = "js-adapter-host")]
        {
            self.js_bridge.set_module_resolver(resolver);
        }
        #[cfg(not(feature = "js-adapter-host"))]
        {
            let _ = resolver;
        }
    }

    /// Data-oriented design — install the shared registry of loaded `.data`
    /// tables into both bridge runners so the `get_data` script global can
    /// read them. Typically called once at startup after loading the project's
    /// `.data` / `.dataschema` assets.
    pub fn set_data_registry(&mut self, registry: std::rc::Rc<crate::data::DataRegistry>) {
        self.lua_bridge.set_data_registry(registry.clone());
        #[cfg(feature = "js-adapter-host")]
        {
            self.js_bridge.set_data_registry(registry);
        }
        #[cfg(not(feature = "js-adapter-host"))]
        {
            let _ = registry;
        }
    }

    /// Phase D — Drop every cached `require()` module export across both
    /// bridge runners. Asset-watcher integration should call this when a
    /// `.lua` / `.luau` / `.js` file under `assets/scripts/` changes so
    /// the next `require(name)` re-runs the module body via the installed
    /// resolver. Errors are logged inside each adapter and swallowed.
    pub fn invalidate_module_cache(&mut self) {
        self.lua_bridge.invalidate_module_cache();
        #[cfg(feature = "js-adapter-host")]
        {
            self.js_bridge.invalidate_module_cache();
        }
    }

    /// P1.5 — hot-reload entry point. Forget every cached load for the
    /// given script `path` (across both bridge runners) so the next
    /// `run()` re-attaches and re-initialises any entity whose
    /// `ScriptComponent.source_path == path`.
    ///
    /// `path` must be the same id the `ScriptComponent` stores (e.g.
    /// `"scripts/player.lua"`); callers translating absolute filesystem
    /// paths from a watcher should normalise to forward slashes first.
    /// Returns `true` if any cached state was found and dropped.
    pub fn reload_script_path(&mut self, path: &str) -> bool {
        #[allow(unused_mut)]
        let mut hit = self.lua_bridge.unload_path(path);
        #[cfg(feature = "js-adapter-host")]
        {
            hit |= self.js_bridge.unload_path(path);
        }
        hit
    }

    /// Run scripts for all entities with `ScriptComponent`.
    /// Call this once per frame.
    pub fn run(
        &mut self,
        world: &mut World,
        dt: f32,
        input: &crate::input::InputState,
        scene_manager: Option<&mut crate::scene::manager::SceneManager>,
        assets: Option<&mut AssetManager>,
    ) {
        let _span = crate::profiling::trace::span("scripts");
        self.run_via_bridge(world, dt, input, scene_manager, assets);
    }

    /// Whether any loaded script defines `fixed_update` - the host runs the
    /// physics step by step, calling [`Self::run_fixed`] before each, only
    /// when one does.
    pub fn has_fixed_update(&self) -> bool {
        !self.lua_bridge.entities_defining("fixed_update").is_empty()
    }

    /// `fixed_update(entity, dt)` on every script that defines it, with the
    /// physics step's `dt` (Oct 2026; Godot's `_physics_process`, Unity's
    /// `FixedUpdate`). The host calls this once before each fixed step - see
    /// `PhysicsWorld::begin_frame` - so gameplay steps with the physics, and
    /// a run replayed from the same inputs steps the same way whatever the
    /// frame rate. Each call reads the world as the last step left it.
    ///
    /// The commands are applied here as `update`'s are; the ones the world
    /// cannot carry out by itself (physics, audio) are returned for the host
    /// to apply before the step.
    pub fn run_fixed(
        &mut self,
        world: &mut World,
        dt: f32,
        input: &crate::input::InputState,
        mut scene_manager: Option<&mut crate::scene::manager::SceneManager>,
        mut assets: Option<&mut AssetManager>,
    ) -> Vec<ScriptCommand> {
        use crate::scripting::bridge;
        let ids = self.lua_bridge.entities_defining("fixed_update");
        if ids.is_empty() {
            return Vec::new();
        }
        let mut snapshot = bridge::snapshot_world_borrow_into(world, std::mem::take(&mut self.entity_handles_pool));
        bridge::snapshot_input(&mut snapshot, input);
        snapshot.gravity = self.world_gravity;
        #[cfg(feature = "physics")]
        {
            snapshot.physics_ptr = self.physics_ptr;
            // Velocities as rapier has them after the last step: what a
            // controller damps with. Without them `get_velocity` answered the
            // `Velocity2D` seed, and a damped hover rang itself apart.
            if let Some(p) = self.physics_ptr {
                // SAFETY: `set_physics_ptr`'s contract - the host owns the
                // physics world, unmoved and unmutated, for this call.
                let physics = unsafe { &*p };
                bridge::snapshot_velocities(&mut snapshot, world);
                bridge::snapshot_physics(&mut snapshot, world, physics);
            }
        }
        snapshot.camera_view = self.camera_view;
        snapshot.camera_rotation = self.camera_rotation;
        snapshot.assets_ptr = assets.as_deref().map(|a| a as *const _);
        let frame_commands = std::mem::take(&mut self.api.commands);
        for id in ids {
            let args = [crate::scripting::bridge::value::Value::Number(dt as f64)];
            let (cmds, err) = self.lua_bridge.dispatch_named_with_args(id, "fixed_update", &args, &snapshot, dt);
            if let Some(e) = err {
                log::error!("fixed_update on entity {id}: {e}");
            }
            let targeted = Self::route_bridge_commands(&mut self.api, cmds);
            if !targeted.is_empty() {
                log::warn!("fixed_update on entity {id}: Event.send_to and script calls run from update, not fixed_update");
            }
        }
        let commands = std::mem::replace(&mut self.api.commands, frame_commands);
        apply_script_commands_indexed(
            world,
            &commands,
            Some(&self.api.services.pending_script_loads),
            scene_manager.as_deref_mut(),
            assets.as_deref_mut(),
            &mut snapshot.entity_handles,
        );
        // A burst asked for in `fixed_update` (a crash judged with the
        // physics) used to go nowhere: the burst loop lived in `run` alone,
        // reading `api.commands`, which this swaps out (a lunar lander's
        // crash).
        Self::emit_bursts(&mut self.api.services.particle_system, &commands, world, &snapshot.entity_handles);
        self.entity_handles_pool = std::mem::take(&mut snapshot.entity_handles);
        commands
    }

    /// Particle bursts the scripts asked for, into the system the host ticks
    /// and draws. Called for `update`'s commands and for `fixed_update`'s.
    fn emit_bursts<S: std::hash::BuildHasher>(
        particles: &mut crate::particles::ParticleSystem,
        commands: &[ScriptCommand],
        world: &World,
        handles: &std::collections::HashMap<u64, hecs::Entity, S>,
    ) {
        for cmd in commands {
            if let ScriptCommand::EmitBurst { entity_id, count } = cmd
                && let Some(&entity) = handles.get(entity_id)
                && let Ok(emitter) = world.get::<&crate::particles::ParticleEmitter>(entity)
                && let Some(at) = crate::particles::emitter_position(world, entity)
            {
                particles.burst(*entity_id, &emitter, at, *count);
            }
        }
    }

    // ------------------------------------------------------------------
    // Bridge-driven dispatch path (P7.5c → became the only path in P7.5d).
    // ------------------------------------------------------------------

    /// Drive every `ScriptComponent` entity through the bridge runners
    /// (`lua_bridge` for Lua/Luau, `js_bridge` for JavaScript). Bridge-only
    /// Fire `on_despawn` on every entity the script bridges still hold but the
    /// world no longer contains, then forget them.
    ///
    /// Returns the commands those handlers emitted so the caller can route
    /// them with the rest of the frame's — a teardown handler that calls
    /// `stop_music()` or `save_set()` has to be honoured like any other script
    /// command, or the hook is decorative.
    fn notify_vanished_entities(
        &mut self,
        world: &World,
        snapshot: &crate::scripting::bridge::BridgeReadSnapshot,
        dt: f32,
    ) -> Vec<crate::scripting::bridge::BridgeCommand> {
        // The snapshot's index is this frame's live set; it used to be rebuilt
        // here as a third walk of the world.
        let _ = world;
        let live = &snapshot.entity_handles;
        let mut out = Vec::new();
        for eid in self.lua_bridge.loaded_entity_ids() {
            if !live.contains_key(&eid) {
                // Forget the language too, or a later entity on the reused
                // index is taken for a scripted one by every dispatch filter.
                self.entity_languages.remove(&eid);
                let (cmds, err) = self.lua_bridge.notify_despawn(eid, snapshot, dt);
                if let Some(e) = err {
                    log::error!("Bridge (Luau) on_despawn on entity {eid}: {e}");
                }
                out.extend(cmds);
            }
        }
        #[cfg(feature = "js-adapter-host")]
        for eid in self.js_bridge.loaded_entity_ids() {
            if !live.contains_key(&eid) {
                self.entity_languages.remove(&eid);
                let (cmds, err) = self.js_bridge.notify_despawn(eid, snapshot, dt);
                if let Some(e) = err {
                    log::error!("Bridge (JS) on_despawn on entity {eid}: {e}");
                }
                out.extend(cmds);
            }
        }
        out
    }

    /// Fire `on_despawn` for **every** scripted entity and apply whatever the
    /// handlers ask for. Call when the whole world is about to be replaced — a
    /// scene transition — before the entity ids are reused by the new scene.
    ///
    /// Without this, a scene swap is invisible to scripts: the per-frame diff
    /// above cannot see it, because by the time it runs the new scene has
    /// already claimed the same entity ids. Anything a script owns that
    /// outlives the world — looping audio above all, since the audio backend
    /// is not part of the ECS — would strand with nothing left to stop it.
    pub fn notify_scene_teardown(
        &mut self,
        world: &mut World,
        scene_manager: Option<&mut crate::scene::manager::SceneManager>,
        assets: Option<&mut AssetManager>,
        dt: f32,
    ) {
        let snapshot = crate::scripting::bridge::snapshot_world_borrow(world);
        let mut cmds = Vec::new();
        let mut ids: Vec<u64> = self.lua_bridge.loaded_entity_ids();
        #[cfg(feature = "js-adapter-host")]
        ids.extend(self.js_bridge.loaded_entity_ids());
        // Descending: entities spawned later tear down before their spawners.
        ids.sort_unstable_by(|a, b| b.cmp(a));
        for eid in ids {
            let (c, err) = self.lua_bridge.notify_despawn(eid, &snapshot, dt);
            if let Some(e) = err {
                log::error!("Bridge (Luau) on_despawn on entity {eid}: {e}");
            }
            cmds.extend(c);
            #[cfg(feature = "js-adapter-host")]
            {
                let (c, err) = self.js_bridge.notify_despawn(eid, &snapshot, dt);
                if let Some(e) = err {
                    log::error!("Bridge (JS) on_despawn on entity {eid}: {e}");
                }
                cmds.extend(c);
            }
        }
        drop(snapshot);
        if cmds.is_empty() {
            return;
        }
        let targeted = Self::route_bridge_commands(&mut self.api, cmds);
        if !targeted.is_empty() {
            log::debug!(
                "[Script] {} targeted command(s) from on_despawn dropped at teardown",
                targeted.len()
            );
        }
        apply_script_commands(world, &self.api.commands, scene_manager, assets);
        self.api.commands.clear();
    }

    /// commands (timers, listeners, GUI bindings) are routed into
    /// `EngineApi` storage where the tick / dispatch loops consume them.
    fn run_via_bridge(
        &mut self,
        world: &mut World,
        dt: f32,
        input: &crate::input::InputState,
        mut scene_manager: Option<&mut crate::scene::manager::SceneManager>,
        mut assets: Option<&mut AssetManager>,
    ) {
        use crate::scripting::bridge;

        let prof_on = self.profile_window > 0;
        let t0 = if prof_on { Some(std::time::Instant::now()) } else { None };

        // Set the world pointer for any legacy `ScriptState` getters that may
        // still consult it (the bridge path no longer reads through
        // `ScriptState::snapshot`). We deliberately skip the full
        // `snapshot_world` call here — it used to rebuild `entity_map`,
        // `velocities`, and `gui_snapshots` every frame, which was ~1.5 ms at
        // 10k entities, and none of those maps are read by the bridge runners
        // or `apply_script_commands`. Tests that call `snapshot_world`
        // directly are unaffected.
        self.api.snapshot.world_ptr = Some(world as *const _);
        self.api.input.snapshot_input(input);
        self.api.snapshot.delta_time = dt;
        self.api.clear();

        let t1 = if prof_on { Some(std::time::Instant::now()) } else { None };

        // Build bridge snapshot. Hot path: borrow positions/rotations/etc.
        // live from the world via `world_ptr` + `entity_handles` instead of
        // populating per-entity HashMaps every frame. The accessors on
        // `BridgeReadSnapshot` (used by both adapters) automatically take
        // the borrow path when `world_ptr.is_some()`. CustomComponents are
        // borrowed by the same mechanism via the existing helper.
        let mut snapshot = bridge::snapshot_world_borrow_into(
            world,
            std::mem::take(&mut self.entity_handles_pool),
        );
        bridge::snapshot_input(&mut snapshot, input);
        // Tilemaps are read live through `world_ptr` (see
        // `BridgeReadSnapshot::with_tilemap`); this used to deep-copy every
        // tilemap in the world into the snapshot each frame.
        // Wheel scrolling runs before the GUI snapshot (so `gui_get(...,
        // "scroll_y")` reads this frame's offset) and before the click
        // hit-test further down (so a click lands on the row on screen).
        if let Some(a) = assets.as_deref_mut() {
            apply_gui_wheel_scroll(
                world,
                a,
                input.mouse_position,
                input.mouse_wheel_delta,
                input.screen_size,
            );
        }
        // `gui_get` reads the loaded documents live through this, under the
        // `world_ptr` contract; it used to be answered from a per-frame copy
        // of every readable property of every GUI node.
        snapshot.assets_ptr = assets.as_deref().map(|a| a as *const _);
        // 2.3 — surface the current contents of every save slot so `save_get`
        // reads are O(1) for the duration of the frame. Cheap: saves are tiny
        // structured data, not bulk arrays.
        //
        // `ensure_discovered` is what makes a save written by a PREVIOUS run
        // readable: `snapshot_all` only clones slots already in memory, and a
        // fresh process has none until something touches them by name. It
        // scans storage once and is a no-op every frame after that.
        self.api.services.save_system.ensure_discovered();
        // Frame counters for the stats globals. Already gathered by the host
        // through `update_frame_stats`; this hands the same numbers to
        // scripts.
        snapshot.frame_stats = self.api.snapshot.frame_stats.clone();
        // `Particles.get_count`: nothing filled this, so native answered 0.
        snapshot.particle_count = self.api.services.particle_system.alive_count();
        snapshot.music_position = crate::audio::music_position().unwrap_or(0.0);
        // Read once a frame rather than per call, so every script in a frame
        // agrees about what time it is.
        snapshot.unix_time = crate::profiling::unix_time();
        snapshot.dt = dt;
        // Physics reads. `contacts` is the same buffer the `on_collision_*`
        // dispatch below drains, copied before it is taken so `get_contacts()`
        // and the callbacks see the same frame.
        // Each named as the script knows it - a spawned body by its handle,
        // so a script can find its own bomb in the list.
        snapshot.contacts = self.pending_contact_events.clone();
        for c in &mut snapshot.contacts {
            c.0 = crate::scripting::spawn_aliases::script_id(c.0);
            c.1 = crate::scripting::spawn_aliases::script_id(c.1);
        }
        snapshot.gravity = self.world_gravity;
        // The one read that borrows rather than copies. `None` unless the
        // host lent it for this call.
        #[cfg(feature = "physics")]
        {
            snapshot.physics_ptr = self.physics_ptr;
        }
        snapshot.camera_view = self.camera_view;
        snapshot.camera_rotation = self.camera_rotation;
        // Named atlas points, computed straight into the snapshot. They were
        // computed into the legacy `ScriptState` and then cloned here - two
        // nested maps of strings per frame for one reader.
        if let Some(ref a) = assets {
            snapshot.sprite_points = crate::scripting::api::compute_sprite_points(world, a);
        }
        snapshot.save_data = self.api.services.save_system.snapshot_all();
        // 2.4 — surface current bus volumes/mute/solo to scripts. Cheap:
        // the bus tree is small (handful of named entries).
        snapshot.audio_mixer = self.api.services.mixer.snapshot();

        let t2 = if prof_on { Some(std::time::Instant::now()) } else { None };

        // Collect scripted entities, partition by language. The paths are
        // borrowed from the world for the length of the runners' frame: this
        // used to clone every scripted entity's path twice per frame, which at
        // ten thousand entities was twenty thousand allocations to say
        // "nothing changed". The query borrow lasts until `script_query` is
        // dropped, below, before anything mutates the world.
        let mut script_query = world.query::<&ScriptComponent>();
        let entities: Vec<(hecs::Entity, ScriptLanguage, &str)> = script_query
            .iter()
            .map(|(e, s)| (e, s.language, s.source_path.as_str()))
            .collect();

        // The legacy `ScriptState` snapshot maps (`snapshot_animations`,
        // `snapshot_tilemaps`, `snapshot_particles`, `snapshot_sprite_points`)
        // are no longer filled here. Nothing on the bridge path read them,
        // and filling them deep-copied every tilemap and every emitter in the
        // world each frame. The particle tick below reads the world.

        let t3 = if prof_on { Some(std::time::Instant::now()) } else { None };

        let mut lua_loads: Vec<(u64, &str, String)> = Vec::new();
        let mut js_loads: Vec<(u64, &str, String)> = Vec::new();
        for &(entity, language, source_path) in &entities {
            if self.failed_scripts.contains(&entity) {
                continue;
            }
            let entity_id = entity.id() as u64;
            // Hot path: avoid the per-frame disk read when the runner has
            // already loaded (or permanently failed on) this script path.
            // Bridge runners cache by path, so the source string is only
            // consulted on the *first* sighting of a given path. Reading and
            // allocating a String for every scripted entity every frame was
            // the dominant cost at high entity counts (1000+ entities).
            let already_loaded = match language {
                ScriptLanguage::Lua => self.lua_bridge.is_path_loaded(source_path),
                #[cfg(feature = "js-adapter-host")]
                ScriptLanguage::JavaScript => self.js_bridge.is_path_loaded(source_path),
                #[cfg(not(feature = "js-adapter-host"))]
                ScriptLanguage::JavaScript => true,
            };
            let source = if already_loaded {
                String::new()
            } else {
                // The host's script root first (the project's `assets/`),
                // then the two process-relative spellings the read always
                // tried - a runtime launched from another directory used to
                // fail every script once and give up on it - then any
                // mounted asset pack (`find_in_roots`).
                let roots: Vec<std::path::PathBuf> = self
                    .script_root
                    .iter()
                    .cloned()
                    .chain([std::path::PathBuf::from("assets"), std::path::PathBuf::new()])
                    .collect();
                let read = crate::asset::find_in_roots(source_path, &roots)
                    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "not in the game or a mounted pack"))
                    .and_then(std::fs::read_to_string);
                match read {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!(
                            "Failed to read script '{}' (root {:?}): {}",
                            source_path,
                            self.script_root,
                            e
                        );
                        self.failed_scripts.insert(entity);
                        continue;
                    }
                }
            };
            self.entity_languages.entry(entity_id).or_insert(language);
            match language {
                ScriptLanguage::Lua => {
                    lua_loads.push((entity_id, source_path, source));
                }
                ScriptLanguage::JavaScript => {
                    js_loads.push((entity_id, source_path, source));
                }
            }
        }

        let mut all_cmds: Vec<bridge::BridgeCommand> = Vec::new();

        // `on_despawn` for anything that left the world since the last frame —
        // `despawn_entity`, an expired `Lifetime`, a bounds despawn, whatever.
        // Diffing what the bridges still hold against the live world catches
        // every route in one place, which is the point: a hook that only fires
        // for *some* ways of dying is worse than none, because scripts would
        // leak resources depending on how they happened to be removed.
        //
        // The runner tolerates being asked about unknown ids (one HashMap
        // miss), and its own contract says handlers here must not assume the
        // entity is still in the world.
        all_cmds.extend(self.notify_vanished_entities(world, &snapshot, dt));

        let t4 = if prof_on { Some(std::time::Instant::now()) } else { None };
        {
            let (cmds, errs) = self.lua_bridge.run_frame(&lua_loads, &snapshot, dt, &[]);
            for e in errs {
                log::error!("Bridge (Luau) error: {}", e);
            }
            all_cmds.extend(cmds);
        }
        let t5 = if prof_on { Some(std::time::Instant::now()) } else { None };
        #[cfg(feature = "js-adapter-host")]
        {
            let (cmds, errs) = self.js_bridge.run_frame(&js_loads, &snapshot, dt, &[]);
            for e in errs {
                log::error!("Bridge (JS) error: {}", e);
            }
            all_cmds.extend(cmds);
        }
        #[cfg(not(feature = "js-adapter-host"))]
        {
            if !js_loads.is_empty() {
                log::warn!(
                    "{} JavaScript scripts skipped — js-adapter-host feature disabled",
                    js_loads.len()
                );
            }
        }
        let t6 = if prof_on { Some(std::time::Instant::now()) } else { None };
        // The runners are done with the world's path strings; release the
        // query borrow before anything below mutates the world.
        drop(lua_loads);
        drop(js_loads);
        drop(entities);
        drop(script_query);
        let cmd_count = all_cmds.len() as u64;

        let targeted = Self::route_bridge_commands(&mut self.api, all_cmds);
        if !targeted.is_empty() {
            self.dispatch_targeted_bridge_commands(targeted, &snapshot, dt, 0);
        }

        let t7 = if prof_on { Some(std::time::Instant::now()) } else { None };

        // Tick timers and dispatch via bridge runners.
        self.api.services.tick_timers(dt);
        let fired: Vec<(u64, String)> = self.api.services.fired_timers.clone();
        for (eid, cb) in &fired {
            self.dispatch_bridge_named(*eid, cb, &snapshot, dt);
        }

        // Process GUI clicks and dispatch via bridge runners.
        if let Some(ref a) = assets {
            let visible = crate::ui::layout::visible_gui_ids(world);
            // P8.2d: explicit sub-struct borrows — services &mut independent of input/snapshot.
            self.api.services.process_gui_clicks(&self.api.input, a, &visible);
        }
        let gui_clicks: Vec<(u64, String, String)> =
            self.api.services.fired_gui_clicks.clone();
        for (eid, cb, node_id) in &gui_clicks {
            let args = [crate::scripting::bridge::value::Value::String(
                node_id.clone(),
            )];
            self.dispatch_bridge_named_with_args_collect(
                *eid, cb, &args, &snapshot, dt, 0,
            );
        }

        // An animation frame's `EmitEvent` is, as its name says, an event
        // scripts can listen for - so it goes through the ordinary event
        // bus as well as to the entity's own `on_animation_event`. Without
        // this it would only ever reach a script *on the animated entity*,
        // and a game whose animations are on its sprites while its logic is
        // on one controller entity - which is most of them - would hear
        // nothing at all.
        for (id, name, data) in &self.pending_animation_events {
            self.api.commands.push(ScriptCommand::SendEvent {
                name: name.clone(),
                sender: *id,
                data: data.clone(),
            });
        }

        // Collect the events this frame emitted; dispatched just below,
        // before the apply, so handler commands are applied with the rest.
        let events: Vec<_> = self
            .api
            .commands
            .iter()
            .filter_map(|cmd| {
                if let ScriptCommand::SendEvent { name, sender, data } = cmd {
                    Some(crate::scripting::api::GameEvent {
                        name: name.clone(),
                        sender: *sender,
                        data: data.clone(),
                    })
                } else {
                    None
                }
            })
            .collect();

        // What last frame's late callbacks sent, which no listener heard yet.
        let mut events = events;
        events.splice(0..0, std::mem::take(&mut self.pending_late_events));
        // Scripts' events (and animation frames') go on to graphs; the
        // graphs' own, fed in by `queue_graph_events`, only come *here*.
        self.events_for_graphs = events.clone();
        for ev in std::mem::take(&mut self.pending_graph_events) {
            events.push(crate::scripting::api::GameEvent {
                name: ev.name.clone(),
                sender: 0,
                data: Some(ev.script_payload()),
            });
        }
        events.append(&mut self.pending_engine_events);

        // Dispatch to listeners *before* the apply below. This used to sit
        // after it, which meant every command an `on_event` handler issued —
        // set_position, stop_music, add_tag, anything — was routed into
        // `api.commands` and then wiped by the next frame's `api.clear()`
        // without ever being applied. Handlers appeared to run (their `log`
        // output showed up, because logging is not a deferred command) while
        // doing nothing at all.
        for event in &events {
            let listeners: Vec<(u64, u32)> = self
                .api
                .services
                .event_listeners
                .get(&event.name)
                .cloned()
                .unwrap_or_default();
            for (eid, _) in listeners {
                // With the event's data. `send_event(name, data)` has always
                // accepted a payload and the dispatch dropped it, so every
                // listener saw `nil` - test-lab scene 48 has been printing
                // "(no payload)" for its own event since it was written.
                match &event.data {
                    Some(data) => {
                        let payload = crate::scripting::bridge::value::Value::from_json(data);
                        self.dispatch_bridge_event_with_data_collect(
                            eid, &event.name, &payload, &snapshot, dt, 0,
                        );
                    }
                    None => self.dispatch_bridge_event(eid, &event.name, event.sender, &snapshot, dt),
                }
            }
        }

        apply_script_commands_indexed(
            world,
            &self.api.commands,
            Some(&self.api.services.pending_script_loads),
            scene_manager.as_deref_mut(),
            assets.as_deref_mut(),
            &mut snapshot.entity_handles,
        );

        // Clear pending script loads after processing to avoid reapplying
        self.api.services.pending_script_loads.clear();
        // Everything applied so far. The callbacks below - trigger areas,
        // contacts, animation frame events - can only be dispatched after
        // the world has been stepped, so they run past this point and their
        // commands land at the end of `api.commands`; see the apply at the
        // bottom of this function.
        let applied_through = self.api.commands.len();

        // Tweens started this frame begin from the world as the frame's
        // commands left it; then every tween advances.
        self.start_pending_tweens(world, &snapshot.entity_handles);
        tick_tweens_indexed(world, &mut self.api.services.tween_manager, dt, &snapshot.entity_handles);
        // A tween on an entity that is gone is ticked for nothing until it
        // ends - and if the index is reused, drives the newcomer. Drop them
        // with the entity, the way `entity_languages` is now.
        {
            let live = &snapshot.entity_handles;
            self.api.services.tween_manager.tweens.retain(|t| live.contains_key(&t.entity_id));
        }

        let t8 = if prof_on { Some(std::time::Instant::now()) } else { None };

        // Particle bursts. The host ticks this same system once a frame
        // (`particles::tick_particles`) and draws it; this used to tick it
        // too, a second simulation of every emitter, while native drew a
        // different system and never showed a burst.
        Self::emit_bursts(
            &mut self.api.services.particle_system,
            &self.api.commands,
            world,
            &snapshot.entity_handles,
        );


        // Trigger-area callbacks: for every entity that owns both a
        // `ScriptComponent` and a freshly-updated `TriggerOverlaps`, dispatch
        // `on_trigger_enter(self, data)` / `on_trigger_exit(self, data)` once
        // per other entity that crossed the boundary this frame. Requires
        // `update_trigger_areas` to have run earlier in the tick.
        // Payload schema: `{ other = <entity_id> }`. Handlers that don't
        // exist on the script are silent no-ops (adapter convention).
        //
        // Phase B (symmetric): for each (zone, other) pair, also dispatch
        // `on_entered_trigger(other, { trigger = <zone_id> })` /
        // `on_exited_trigger(other, { trigger = <zone_id> })` on the OTHER
        // side if it owns a `ScriptComponent`. Lets "player picks up coin"
        // patterns react from the moving entity without scripting every zone.
        let mut trigger_dispatches: Vec<(u64, &'static str, &'static str, u64)> = Vec::new();
        {
            for (entity, overlaps) in world
                .query::<&super::components::TriggerOverlaps>()
                .iter()
            {
                let zone_eid = entity.to_bits().get();
                let zone_scripted = self.entity_languages.contains_key(&zone_eid);
                for &other in &overlaps.entered {
                    if zone_scripted {
                        trigger_dispatches.push((zone_eid, "trigger_enter", "other", other));
                    }
                    if self.entity_languages.contains_key(&other) {
                        trigger_dispatches.push((other, "entered_trigger", "trigger", zone_eid));
                    }
                }
                for &other in &overlaps.exited {
                    if zone_scripted {
                        trigger_dispatches.push((zone_eid, "trigger_exit", "other", other));
                    }
                    if self.entity_languages.contains_key(&other) {
                        trigger_dispatches.push((other, "exited_trigger", "trigger", zone_eid));
                    }
                }
            }
        }
        for (eid, ev, key, ref_eid) in trigger_dispatches {
            let mut map = std::collections::BTreeMap::new();
            map.insert(
                key.to_string(),
                crate::scripting::bridge::value::Value::EntityId(ref_eid),
            );
            let payload = crate::scripting::bridge::value::Value::Map(map);
            self.dispatch_bridge_event_with_data_collect(eid, ev, &payload, &snapshot, dt, 0);
        }

        // Physics-contact callbacks: drain events queued by the runtime
        // after the previous frame's `physics.step()` and dispatch
        // `on_collision_enter` / `on_collision_exit` to whichever side
        // of each contact pair owns a scripted entity. `ContactEvent`
        // uses `entity.id() as u64` (the 32-bit hecs index), and so does
        // `entity_languages` - the script loader keys it by `entity.id()`.
        // This used to translate both sides to `Entity::to_bits().get()`
        // on the strength of a comment that said the map was keyed that
        // way; it is not, and since `to_bits()` carries the generation in
        // its high bits it never matched, so `on_collision_enter` and
        // `on_collision_exit` were **never called at all** on native.
        // Payload schema: `{ other = <entity_id> }`, mirroring triggers.
        // Handlers that don't exist on the script are silent no-ops.
        let contact_events = std::mem::take(&mut self.pending_contact_events);
        if !contact_events.is_empty() {
            let mut collision_dispatches: Vec<(u64, &'static str, u64, f32, f32, f32)> = Vec::new();
            for (a_id, b_id, started, impulse, x, y) in contact_events {
                // No zero guard: `0` is the id of the first entity a world
                // spawns, not a sentinel, and skipping it dropped every
                // contact that entity was part of. An id that names nothing
                // is filtered by the `entity_languages` lookup below.
                let ev_name = if started {
                    "collision_enter"
                } else {
                    "collision_exit"
                };
                for (self_id, other_id) in [(a_id, b_id), (b_id, a_id)] {
                    if !self.entity_languages.contains_key(&self_id) {
                        continue;
                    }
                    collision_dispatches.push((self_id, ev_name, other_id, impulse, x, y));
                }
            }
            for (eid, ev, other, impulse, x, y) in collision_dispatches {
                use crate::scripting::bridge::value::Value;
                let mut map = std::collections::BTreeMap::new();
                map.insert("other".to_string(), Value::EntityId(other));
                // How hard, and where. A collision used to say only that it
                // happened.
                map.insert("impulse".to_string(), Value::Number(impulse as f64));
                map.insert("x".to_string(), Value::Number(x as f64));
                map.insert("y".to_string(), Value::Number(y as f64));
                let payload = crate::scripting::bridge::value::Value::Map(map);
                self.dispatch_bridge_event_with_data_collect(
                    eid, ev, &payload, &snapshot, dt, 0,
                );
            }
        }

        // Animation frame events, dispatched the same way: the entity whose
        // clip reached the frame gets `on_animation_event` with the event's
        // name and whatever data the frame carried.
        let animation_events = std::mem::take(&mut self.pending_animation_events);
        if !animation_events.is_empty() {
            // Keyed by `entity.id()`, which is what the script loader keys
            // `entity_languages` and the bridge's own map by - *not*
            // `to_bits()`, whatever the comment on the collision dispatch
            // above says.
            for (id, name, data) in animation_events {
                if !self.entity_languages.contains_key(&id) {
                    continue;
                }
                let mut map = std::collections::BTreeMap::new();
                map.insert("name".to_string(), crate::scripting::bridge::value::Value::String(name));
                if let Some(d) = data {
                    map.insert(
                        "data".to_string(),
                        crate::scripting::bridge::value::Value::from_json(&d),
                    );
                }
                let payload = crate::scripting::bridge::value::Value::Map(map);
                self.dispatch_bridge_event_with_data_collect(
                    id, "animation_event", &payload, &snapshot, dt, 0,
                );
            }
        }

        // What the engine owes entities' own scripts (`ecs::callbacks`):
        // `on_screen_entered(entity)`, `on_path_end(entity)` ...
        for (id, name) in std::mem::take(&mut self.pending_entity_callbacks) {
            if !self.entity_languages.contains_key(&id) {
                continue;
            }
            let payload = crate::scripting::bridge::value::Value::Map(Default::default());
            self.dispatch_bridge_event_with_data_collect(id, name, &payload, &snapshot, dt, 0);
        }

        // The late callbacks' commands. Until Sep 2026 these were routed
        // into `api.commands` after the apply above and then wiped by the
        // next frame's `api.clear()`, so an `on_collision_enter` that moved
        // something, an `on_entered_trigger` that spawned something, or an
        // animation frame's handler doing anything at all was silently a
        // no-op - the handler ran, its `log` output appeared, and nothing
        // happened. The same bug was found and fixed for `on_event`
        // listeners by moving them *before* the apply; these three cannot
        // move, because what they dispatch is only known once the world has
        // been stepped.
        if self.api.commands.len() > applied_through {
            let late: Vec<ScriptCommand> = self.api.commands[applied_through..].to_vec();
            // Their `send_event`s missed this frame's listener dispatch, and
            // the apply below ignores them - so an `on_collision_enter` or
            // `on_screen_entered` that told anyone anything was silent,
            // natively only (the browser hosts drain a handler's commands
            // with the frame's). They go out on the next run.
            for cmd in &late {
                if let ScriptCommand::SendEvent { name, sender, data } = cmd {
                    self.pending_late_events.push(crate::scripting::api::GameEvent {
                        name: name.clone(),
                        sender: *sender,
                        data: data.clone(),
                    });
                }
            }
            apply_script_commands_indexed(
                world,
                &late,
                None,
                scene_manager,
                assets,
                &mut snapshot.entity_handles,
            );
        }

        self.api.snapshot.world_ptr = None;
        // Keep the index's allocation for next frame.
        self.entity_handles_pool = std::mem::take(&mut snapshot.entity_handles);

        if prof_on {
            let t9 = std::time::Instant::now();
            let us = |a: std::time::Instant, b: std::time::Instant| {
                b.duration_since(a).as_micros()
            };
            self.profile_us_legacy_snap += us(t0.unwrap(), t1.unwrap());
            self.profile_us_bridge_snap += us(t1.unwrap(), t2.unwrap());
            self.profile_us_subsystem_snap += us(t2.unwrap(), t3.unwrap());
            self.profile_us_collect_loads += us(t3.unwrap(), t4.unwrap());
            self.profile_us_run_lua += us(t4.unwrap(), t5.unwrap());
            self.profile_us_run_js += us(t5.unwrap(), t6.unwrap());
            self.profile_us_route += us(t6.unwrap(), t7.unwrap());
            self.profile_us_apply += us(t7.unwrap(), t8.unwrap());
            self.profile_us_post += us(t8.unwrap(), t9);
            self.profile_total_cmds += cmd_count;
            self.profile_frames += 1;
            if self.profile_frames >= self.profile_window {
                let n = self.profile_frames as u128;
                log::info!(
                    "[Script][PROF] frames={} cmds_avg={} legacy_snap={}us bridge_snap={}us subsys_snap={}us collect={}us run_lua={}us run_js={}us route={}us apply={}us post={}us",
                    self.profile_frames,
                    self.profile_total_cmds / n as u64,
                    self.profile_us_legacy_snap / n,
                    self.profile_us_bridge_snap / n,
                    self.profile_us_subsystem_snap / n,
                    self.profile_us_collect_loads / n,
                    self.profile_us_run_lua / n,
                    self.profile_us_run_js / n,
                    self.profile_us_route / n,
                    self.profile_us_apply / n,
                    self.profile_us_post / n,
                );
                self.profile_frames = 0;
                self.profile_us_legacy_snap = 0;
                self.profile_us_bridge_snap = 0;
                self.profile_us_subsystem_snap = 0;
                self.profile_us_collect_loads = 0;
                self.profile_us_run_lua = 0;
                self.profile_us_run_js = 0;
                self.profile_us_route = 0;
                self.profile_us_apply = 0;
                self.profile_us_post = 0;
                self.profile_total_cmds = 0;
            }
        }
    }

    /// The real entity id behind a script handle: a `spawn_prefab` handle
    /// resolves through `scripting::spawn_aliases`, anything else is already
    /// real.
    fn real_entity_id(id: u64) -> u64 {
        if id < crate::scripting::spawn_aliases::FIRST_SPAWN_ID {
            return id;
        }
        crate::scripting::spawn_aliases::resolve(id).map_or(id, |e| e.id() as u64)
    }

    /// Give every tween started this frame its start value - the entity's
    /// property now - and hand it to the manager under the id the script
    /// already has.
    fn start_pending_tweens(
        &mut self,
        world: &World,
        index: &std::collections::HashMap<u64, hecs::Entity>,
    ) {
        use crate::animation::tween::TweenTarget;
        for t in std::mem::take(&mut self.api.services.pending_tweens) {
            let id = Self::real_entity_id(t.entity_id);
            let Some(&entity) = index.get(&id) else { continue };
            let start = match t.target {
                TweenTarget::Position => world.get::<&Transform>(entity).ok().map(|x| glam::Vec4::new(x.position.x, x.position.y, 0.0, 0.0)),
                TweenTarget::Rotation => world.get::<&Transform>(entity).ok().map(|x| glam::Vec4::new(x.rotation, 0.0, 0.0, 0.0)),
                TweenTarget::Scale => world.get::<&Transform>(entity).ok().map(|x| glam::Vec4::new(x.scale.x, x.scale.y, 0.0, 0.0)),
                TweenTarget::SpriteColor => tween_color_of(world, entity),
                TweenTarget::SpriteAlpha => world.get::<&SpriteComponent>(entity).ok().map(|s| glam::Vec4::new(s.color[3], 0.0, 0.0, 0.0)),
            };
            let Some(start) = start else { continue };
            self.api.services.tween_manager.add_tween_with_id(t.tween_id, id, t.target, start, t.end_value, t.duration, t.easing);
        }
    }

    /// Drain a queue of bridge commands: bridge-only variants (timers,
    /// listeners, GUI bindings) get applied to `EngineApi` directly; the
    /// rest get translated to `ScriptCommand` and pushed into
    /// `api.commands` for the existing apply path.
    /// Extracts bridge-only commands from `cmds`, applies them to `api`,
    /// translates the rest into legacy `ScriptCommand`s, and returns any
    /// targeted-dispatch commands (`SendEventTo`, `CallScriptMethod`) that the
    /// caller must dispatch via [`dispatch_targeted_bridge_commands`].
    fn route_bridge_commands(
        api: &mut crate::scripting::api::ScriptState,
        cmds: Vec<crate::scripting::bridge::BridgeCommand>,
    ) -> Vec<crate::scripting::bridge::BridgeCommand> {
        use crate::scripting::bridge::BridgeCommand;
        let mut translatable = Vec::with_capacity(cmds.len());
        let mut targeted: Vec<BridgeCommand> = Vec::new();
        for cmd in cmds {
            match cmd {
                // Phase C — collect targeted dispatches; the caller drains them
                // through dispatch_targeted_bridge_commands so we can enforce a
                // recursion-depth cap and reuse the entity→language map.
                BridgeCommand::SendEventTo { .. } | BridgeCommand::CallScriptMethod { .. } => {
                    targeted.push(cmd);
                }
                BridgeCommand::SetTimer {
                    entity_id,
                    delay,
                    callback_name,
                    repeating,
                    timer_id,
                } => {
                    api.services.timers.push(crate::scripting::api::Timer {
                        id: timer_id,
                        entity_id,
                        callback_name,
                        delay,
                        elapsed: 0.0,
                        repeating,
                    });
                }
                BridgeCommand::CancelTimer { timer_id } => {
                    api.services.cancel_timer(timer_id);
                }
                // Tweens. These used to be translated into `ScriptCommand`s
                // that `apply_script_commands` treats as no-ops, so on native
                // no script tween ever moved anything (BUGS #107). They wait
                // here for the frame's commands, then start from the world.
                BridgeCommand::StartTween { entity_id, target, end_value, duration, easing, tween_id } => {
                    let target = crate::scripting::bridge::translator::parse_tween_target(&target);
                    api.services.pending_tweens.push(crate::scripting::api::PendingTween {
                        tween_id,
                        entity_id,
                        target,
                        end_value: glam::Vec4::from_array(end_value),
                        duration,
                        easing: crate::animation::easing::EasingFunction::from_name(&easing),
                    });
                }
                BridgeCommand::CancelTween { tween_id } => {
                    api.services.pending_tweens.retain(|t| t.tween_id != tween_id);
                    api.services.tween_manager.cancel_tween(tween_id);
                }
                BridgeCommand::CancelAllTweens { entity_id } => {
                    api.services.pending_tweens.retain(|t| t.entity_id != entity_id);
                    let real = Self::real_entity_id(entity_id);
                    api.services.tween_manager.cancel_all(entity_id);
                    api.services.tween_manager.cancel_all(real);
                }
                BridgeCommand::RegisterEventListener {
                    event_name,
                    entity_id,
                    listener_id,
                } => {
                    api.services.event_listeners
                        .entry(event_name)
                        .or_default()
                        .push((entity_id, listener_id));
                }
                BridgeCommand::RemoveEventListener {
                    event_name,
                    entity_id,
                } => {
                    if let Some(v) = api.services.event_listeners.get_mut(&event_name) {
                        v.retain(|(eid, _)| *eid != entity_id);
                    }
                }
                BridgeCommand::GuiOnClick {
                    entity_id,
                    gui_id,
                    node_id,
                    callback_name,
                } => {
                    api.services.gui_on_click(entity_id, gui_id, node_id, callback_name);
                }
                // P7.8: Dynamic script loading — queue for processing in apply_script_commands.
                BridgeCommand::LoadScript { entity_id, path } => {
                    api.services.pending_script_loads.push((entity_id, path));
                }
                // 2.3 — save game system. Mutations go straight into the
                // long-lived `SaveSystem` on `ScriptServices`; `save_get`
                // reads see them on the next frame via the snapshot.
                BridgeCommand::SaveSet { slot, key, value } => {
                    api.services.save_system.set(&slot, &key, value);
                }
                BridgeCommand::SaveCommit { slot } => {
                    if let Err(e) = api.services.save_system.commit(&slot) {
                        log::warn!("save_commit({slot}) failed: {e}");
                    }
                }
                BridgeCommand::SaveClear { slot } => {
                    api.services.save_system.clear(&slot);
                }
                // 2.4 — audio mixer. Mutations land on the long-lived
                // `AudioMixer`; `apply_audio_commands` drains
                // `mixer.take_dirty()` after processing audio commands to
                // re-apply effective volume on in-flight tracks.
                BridgeCommand::AudioCreateBus { name, parent, volume } => {
                    api.services
                        .mixer
                        .create_bus(&name, parent.as_deref(), volume);
                }
                BridgeCommand::AudioRemoveBus { name } => {
                    api.services.mixer.remove_bus(&name);
                }
                BridgeCommand::AudioSetBusVolume { name, volume } => {
                    api.services.mixer.set_volume(&name, volume);
                }
                BridgeCommand::AudioSetBusEffect { name, effect, value } => {
                    if let Err(e) = api.services.mixer.set_effect(&name, &effect, value) {
                        log::warn!("Audio.set_bus_effect: {e}");
                    }
                }
                BridgeCommand::AudioSetBusMuted { name, muted } => {
                    api.services.mixer.set_muted(&name, muted);
                }
                BridgeCommand::AudioSetBusSolo { name, solo } => {
                    api.services.mixer.set_solo(&name, solo);
                }
                // 2.5 — input remapping. Mutations against the live
                // `InputState` happen in the runtime (which owns it),
                // so we just buffer the commands here and let the host
                // drain them via `crate::input::apply_input_remap`
                // after `ScriptSystem::run` returns.
                cmd @ (BridgeCommand::InputSetBindings { .. }
                | BridgeCommand::InputAddBinding { .. }
                | BridgeCommand::InputRemoveBinding { .. }
                | BridgeCommand::InputClearBindings { .. }
                | BridgeCommand::InputResetBindings
                | BridgeCommand::InputSetDeadZone { .. }) => {
                    api.services.pending_input_remaps.push(cmd);
                }
                // Camera shake — queue for the runtime to drain after
                // `ScriptSystem::run` and forward to `Camera2D::start_shake`.
                BridgeCommand::CameraShake { amplitude, duration, frequency } => {
                    api.services
                        .pending_camera_shakes
                        .push((amplitude, duration, frequency));
                }
                // spawn_prefab path: the translator can't see the prefab table
                // (lives on api.services), so resolve it here and emit a
                // ScriptCommand::SpawnEntity carrying the actual components.
                // Without this, prefab spawns produced bare Transform-only
                // entities (no Sprite, no Script) and nothing rendered or moved.
                BridgeCommand::SpawnEntity {
                    name,
                    prefab: Some(prefab_name),
                    spawn_id,
                } => {
                    // Decoded once per prefab, not once per spawn.
                    if let Some(prefab) = api.services.prepared_prefab(&prefab_name) {
                        let resolved_name = if name.is_empty() {
                            prefab.name.clone()
                        } else {
                            name
                        };
                        api.services
                            .spawned_entities
                            .push((resolved_name.clone(), spawn_id));
                        api.commands
                            .push(crate::scripting::api::ScriptCommand::SpawnEntity {
                                name: resolved_name,
                                components: Box::new(None),
                                spawn_id: Some(spawn_id),
                                children: Vec::new(),
                                prepared: Some(prefab),
                            });
                    } else {
                        log::warn!("Prefab '{}' not found (script spawn)", prefab_name);
                    }
                }
                other => translatable.push(other),
            }
        }
        let translated = crate::scripting::bridge::to_script_commands(translatable);
        api.commands.extend(translated);
        targeted
    }

    /// Phase C — Drain `SendEventTo` / `CallScriptMethod` commands by dispatching
    /// each to the target entity's bridge runner. Re-routes any commands the
    /// dispatched script emits, recursing when those produce further targeted
    /// dispatches. `depth` starts at 0 and is bumped per recursion level; if it
    /// reaches [`MAX_SCRIPT_CALL_DEPTH`] the remaining work is dropped with an
    /// error log to prevent A→B→A loops from hanging the frame.
    fn dispatch_targeted_bridge_commands(
        &mut self,
        cmds: Vec<crate::scripting::bridge::BridgeCommand>,
        snap: &crate::scripting::bridge::BridgeReadSnapshot,
        dt: f32,
        depth: u32,
    ) {
        use crate::scripting::bridge::BridgeCommand;
        if depth >= MAX_SCRIPT_CALL_DEPTH {
            log::error!(
                "Script call depth exceeded {} (dropping {} pending dispatch{}); check for send_event_to / call() loops",
                MAX_SCRIPT_CALL_DEPTH,
                cmds.len(),
                if cmds.len() == 1 { "" } else { "es" }
            );
            return;
        }
        for cmd in cmds {
            match cmd {
                BridgeCommand::SendEventTo { name, sender: _, target, data } => {
                    // `data` arrives as the text it travels as; the receiver's
                    // `on_<event>(entity, data)` gets the value it spells, as
                    // the browser hosts hand it (`event_handler_data`) - a
                    // table as a table, other text as a string, none as nil.
                    let payload = crate::scripting::bridge::value::Value::from_json(
                        &crate::scripting::host_commands::event_handler_data(data.as_deref()),
                    );
                    self.dispatch_bridge_event_with_data_collect(target, &name, &payload, snap, dt, depth);
                }
                BridgeCommand::CallScriptMethod { caller: _, target, method, args } => {
                    // Phase D — args now plumbed end-to-end. The receiver
                    // sees `function method(entity, ...args)`.
                    self.dispatch_bridge_named_with_args_collect(target, &method, &args, snap, dt, depth);
                }
                _ => {}
            }
        }
    }

    /// Phase D — args-bearing variant of [`Self::dispatch_bridge_named_collect`].
    /// Forwards `args` through the runner's `dispatch_named_with_args` so the
    /// receiver sees `function method(entity, ...args)`. Recurses through any
    /// further targeted dispatches the called script emits.
    fn dispatch_bridge_named_with_args_collect(
        &mut self,
        entity_id: u64,
        fn_name: &str,
        args: &[crate::scripting::bridge::value::Value],
        snap: &crate::scripting::bridge::BridgeReadSnapshot,
        dt: f32,
        depth: u32,
    ) {
        let Some(lang) = self.entity_languages.get(&entity_id).cloned() else {
            return;
        };
        let (cmds, err) = match lang {
            ScriptLanguage::Lua => self.lua_bridge.dispatch_named_with_args(entity_id, fn_name, args, snap, dt),
            ScriptLanguage::JavaScript => {
                #[cfg(feature = "js-adapter-host")]
                { self.js_bridge.dispatch_named_with_args(entity_id, fn_name, args, snap, dt) }
                #[cfg(not(feature = "js-adapter-host"))]
                { (Vec::new(), None) }
            }
        };
        if let Some(e) = err {
            log::error!("Bridge call '{}' on entity {}: {}", fn_name, entity_id, e);
        }
        let nested = Self::route_bridge_commands(&mut self.api, cmds);
        if !nested.is_empty() {
            self.dispatch_targeted_bridge_commands(nested, snap, dt, depth + 1);
        }
    }

    /// Phase D — payload-bearing variant of [`Self::dispatch_bridge_event_collect`].
    fn dispatch_bridge_event_with_data_collect(
        &mut self,
        entity_id: u64,
        event_name: &str,
        data: &crate::scripting::bridge::value::Value,
        snap: &crate::scripting::bridge::BridgeReadSnapshot,
        dt: f32,
        depth: u32,
    ) {
        let Some(lang) = self.entity_languages.get(&entity_id).cloned() else {
            return;
        };
        let (cmds, err) = match lang {
            ScriptLanguage::Lua => self.lua_bridge.dispatch_event_with_data(entity_id, event_name, data, snap, dt),
            ScriptLanguage::JavaScript => {
                #[cfg(feature = "js-adapter-host")]
                { self.js_bridge.dispatch_event_with_data(entity_id, event_name, data, snap, dt) }
                #[cfg(not(feature = "js-adapter-host"))]
                { (Vec::new(), None) }
            }
        };
        if let Some(e) = err {
            log::error!("Bridge send_event_to '{}' on entity {}: {}", event_name, entity_id, e);
        }
        let nested = Self::route_bridge_commands(&mut self.api, cmds);
        if !nested.is_empty() {
            self.dispatch_targeted_bridge_commands(nested, snap, dt, depth + 1);
        }
    }

    /// Dispatch a named callback (timer / GUI click) to whichever bridge
    /// runner owns the entity.
    fn dispatch_bridge_named(
        &mut self,
        entity_id: u64,
        fn_name: &str,
        snap: &crate::scripting::bridge::BridgeReadSnapshot,
        dt: f32,
    ) {
        let lang = match self.entity_languages.get(&entity_id) {
            Some(l) => *l,
            None => return,
        };
        let (cmds, err) = match lang {
            ScriptLanguage::Lua => self.lua_bridge.dispatch_named(entity_id, fn_name, snap, dt),
            ScriptLanguage::JavaScript => {
                #[cfg(feature = "js-adapter-host")]
                {
                    self.js_bridge.dispatch_named(entity_id, fn_name, snap, dt)
                }
                #[cfg(not(feature = "js-adapter-host"))]
                {
                    (Vec::new(), None)
                }
            }
        };
        if let Some(e) = err {
            log::error!(
                "Bridge callback '{}' on entity {}: {}",
                fn_name,
                entity_id,
                e
            );
        }
        let targeted = Self::route_bridge_commands(&mut self.api, cmds);
        if !targeted.is_empty() {
            self.dispatch_targeted_bridge_commands(targeted, snap, dt, 0);
        }
    }

    /// Dispatch an event to whichever bridge runner owns the entity.
    fn dispatch_bridge_event(
        &mut self,
        entity_id: u64,
        event_name: &str,
        sender: u64,
        snap: &crate::scripting::bridge::BridgeReadSnapshot,
        dt: f32,
    ) {
        let lang = match self.entity_languages.get(&entity_id) {
            Some(l) => *l,
            None => return,
        };
        let (cmds, err) = match lang {
            ScriptLanguage::Lua => self.lua_bridge.dispatch_event(entity_id, event_name, sender, snap, dt),
            ScriptLanguage::JavaScript => {
                #[cfg(feature = "js-adapter-host")]
                {
                    self.js_bridge.dispatch_event(entity_id, event_name, sender, snap, dt)
                }
                #[cfg(not(feature = "js-adapter-host"))]
                {
                    (Vec::new(), None)
                }
            }
        };
        if let Some(e) = err {
            log::error!(
                "Bridge event '{}' on entity {}: {}",
                event_name,
                entity_id,
                e
            );
        }
        let targeted = Self::route_bridge_commands(&mut self.api, cmds);
        if !targeted.is_empty() {
            self.dispatch_targeted_bridge_commands(targeted, snap, dt, 0);
        }
    }

    /// Feed profiling stats into the scripting API so scripts can read them.
    pub fn update_frame_stats(&mut self, stats: &crate::profiling::FrameStats) {
        self.api.snapshot.frame_stats = stats.clone();
    }

    /// Reset init tracking so all entities will call init() again on the next frame.
    /// Used after scene transitions (load_scene / pop_scene).
    pub fn reset_initialized(&mut self) {
        self.failed_scripts.clear();
        // Tweens name entities of the scene being replaced; the next scene
        // reuses those ids. The browser hosts drop theirs the same way.
        self.api.services.tween_manager = crate::animation::tween::TweenManager::new();
        self.api.services.pending_tweens.clear();
        // ...and so do timers, event listeners and GUI click handlers, which
        // were missed here for the life of the engine. Nothing noticed,
        // because every game in the repo loads a scene once and stays in it,
        // so a strand had nowhere to land. See
        // `ScriptServices::clear_scene_scoped` for what survives and why, and
        // `tests/scene_churn_does_not_strand_state.rs` for the measurement:
        // a hundred swaps used to leave 200 timers and 200 listeners pointing
        // at entities that no longer existed.
        self.api.services.clear_scene_scoped();
        self.lua_bridge.reset();
        self.entity_languages.clear();
        #[cfg(feature = "js-adapter-host")]
        self.js_bridge.reset();
    }
}

/// P7.5a — Direct entry point for applying a queue of [`BridgeCommand`] values
/// to the world. Today this delegates through
/// [`crate::scripting::bridge::to_script_commands`] so the existing
/// [`apply_script_commands`] machinery handles every variant uniformly. This
/// keeps the cutover (P7.5b/c) reversible: callers can switch between
/// [`apply_script_commands`] and [`apply_bridge_commands`] without changing
/// the apply path itself.
///
/// Bridge-only variants (`DrawDebug*`, `SetDebugOverlay`, `SetTimer`,
/// `CancelTimer`, `RegisterEventListener`, `RemoveEventListener`,
/// `GuiOnClick`) are dropped on translation today — they're handled by
/// `ScriptSystem` directly via `EngineApi`.
#[cfg(feature = "native")]
pub fn apply_bridge_commands(
    world: &mut World,
    commands: Vec<crate::scripting::bridge::BridgeCommand>,
    scene_manager: Option<&mut crate::scene::manager::SceneManager>,
    assets: Option<&mut AssetManager>,
) {
    // Phase 5.13: handle `SetComponentField` directly via the global
    // `ComponentRegistry` before translation drops it. Roundtrips the
    // component through serde_json to patch a single field on a
    // package-registered component.
    let registry = crate::package::global_component_registry();
    let mut remaining: Vec<crate::scripting::bridge::BridgeCommand> =
        Vec::with_capacity(commands.len());
    // Built on the first field write, not per command: this walked the whole
    // world once per `SetComponentField`.
    let mut index: Option<std::collections::HashMap<u64, hecs::Entity>> = None;
    for cmd in commands {
        if let crate::scripting::bridge::BridgeCommand::SetComponentField {
            entity_id,
            ref component_name,
            ref field_name,
            ref value,
        } = cmd
        {
            if let Some(reg) = registry.as_ref()
                && let Some(desc) = reg.get(component_name.as_ref())
            {
                let index = index.get_or_insert_with(|| {
                    world.iter().map(|e| (e.entity().id() as u64, e.entity())).collect()
                });
                let Some(&entity) = index.get(&entity_id) else { continue };
                let Some(any_component) = (desc.query_fn)(world, entity) else {
                    continue;
                };
                let mut json = (desc.serialize_fn)(any_component.as_ref());
                if let Some(obj) = json.as_object_mut() {
                    obj.insert(field_name.to_string(), value.clone());
                } else {
                    continue;
                }
                if let Some(new_any) = (desc.deserialize_fn)(&json) {
                    (desc.insert_fn)(world, entity, new_any);
                }
                continue;
            }
            continue;
        }
        remaining.push(cmd);
    }

    let translated = crate::scripting::bridge::to_script_commands(remaining);
    apply_script_commands(world, &translated, scene_manager, assets);
}

/// One entity of a script spawn: its name and every component a bundle
/// carries (a default `Transform` if it has none), through the component
/// registry - as the scene loader and the browser hosts spawn, so a package
/// component is a component here too (BUGS #109) and the rest custom data.
#[cfg(feature = "native")]
fn spawn_bundle(world: &mut World, name: &str, components: Option<&crate::scene::format::ComponentBundle>) -> hecs::Entity {
    let mut builder = hecs::EntityBuilder::new();
    builder.add(Name(name.to_string()));
    // Added first, so a bundle's own transform replaces it.
    builder.add(Transform::default());
    if let Some(bundle) = components {
        let custom = crate::package::component_registry::with_registry(|r| bundle.add_to_builder(&mut builder, r));
        if !custom.is_empty() {
            builder.add(CustomComponents(custom));
        }
    }
    world.spawn(builder.build())
}

/// Apply script commands to the ECS world./// Returns a list of scene-related commands that the game loop must handle.
#[cfg(feature = "native")]
pub fn apply_script_commands(
    world: &mut World,
    commands: &[ScriptCommand],
    scene_manager: Option<&mut crate::scene::manager::SceneManager>,
    assets: Option<&mut AssetManager>,
) {
    apply_script_commands_impl(world, commands, None, scene_manager, assets, None);
}

/// Internal implementation of apply_script_commands with optional pending script loads.
#[cfg(feature = "native")]
pub fn apply_script_commands_with_pending(
    world: &mut World,
    commands: &[ScriptCommand],
    pending_script_loads: &[(u64, String)],
    scene_manager: Option<&mut crate::scene::manager::SceneManager>,
    assets: Option<&mut AssetManager>,
) {
    apply_script_commands_impl(world, commands, Some(pending_script_loads), scene_manager, assets, None);
}

/// [`apply_script_commands_with_pending`] resolving entities through an
/// `id -> Entity` index the caller already holds - the script system's
/// per-frame one. Entities spawned by the commands are added to it, so the
/// index stays true for whatever runs after.
#[cfg(feature = "native")]
pub fn apply_script_commands_indexed(
    world: &mut World,
    commands: &[ScriptCommand],
    pending_script_loads: Option<&[(u64, String)]>,
    scene_manager: Option<&mut crate::scene::manager::SceneManager>,
    assets: Option<&mut AssetManager>,
    index: &mut std::collections::HashMap<u64, hecs::Entity>,
) {
    apply_script_commands_impl(world, commands, pending_script_loads, scene_manager, assets, Some(index));
}

#[cfg(feature = "native")]
fn apply_script_commands_impl(
    world: &mut World,
    commands: &[ScriptCommand],
    pending_script_loads: Option<&[(u64, String)]>,
    mut scene_manager: Option<&mut crate::scene::manager::SceneManager>,
    mut assets: Option<&mut AssetManager>,
    index: Option<&mut std::collections::HashMap<u64, hecs::Entity>>,
) {
    if commands.is_empty() && pending_script_loads.is_none_or(|p| p.is_empty()) {
        return;
    }
    let mut entities_to_destroy: Vec<hecs::Entity> = Vec::new();
    // Remap table: temp spawn IDs (>= 1_000_000) → actual hecs entity IDs.
    // Almost always empty (only populated when a script spawned an entity
    // earlier in the same frame), so the per-cmd resolve checks `is_empty`
    // first and bypasses the HashMap lookup entirely on the fast path.
    let mut spawn_remap: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
    // The `id -> Entity` index every arm resolves through. The script system
    // lends its per-frame one; a caller without one gets a fresh build. Every
    // arm uses it - 29 of them used to scan the world instead, one query per
    // command, with this map sitting right here.
    let mut owned_index: std::collections::HashMap<u64, hecs::Entity>;
    let entity_index: &mut std::collections::HashMap<u64, hecs::Entity> = match index {
        Some(index) => index,
        None => {
            owned_index = std::collections::HashMap::with_capacity(world.len() as usize);
            for entity in world.iter().map(|e| e.entity()) {
                owned_index.insert(entity.id() as u64, entity);
            }
            &mut owned_index
        }
    };
    // Handles from earlier frames: a script keeps what `spawn_prefab`
    // returned and uses it long after this drain (BUGS #105).
    for (sid, e) in crate::scripting::spawn_aliases::live(world) {
        spawn_remap.insert(sid, e.id() as u64);
    }

    /// Resolve an entity_id through the spawn remap table. Hot path: if
    /// no entities were spawned this frame, the remap is empty and we
    /// short-circuit without hashing the id.
    #[inline(always)]
    fn resolve_id(spawn_remap: &std::collections::HashMap<u64, u64>, id: u64) -> u64 {
        if spawn_remap.is_empty() {
            return id;
        }
        *spawn_remap.get(&id).unwrap_or(&id)
    }

    /// Lends the shared package commands the native world.
    ///
    /// The same `ScriptWorldHost` the browser hosts implement, so a package
    /// command runs the identical code here - which is the whole point of
    /// routing package globals through this path rather than through
    /// `ScriptApiRegistry`, which native alone could see.
    struct NativeCommandHost<'a> {
        world: &'a mut World,
        index: &'a std::collections::HashMap<u64, hecs::Entity>,
        spawn_remap: &'a std::collections::HashMap<u64, u64>,
    }

    impl crate::scripting::host_commands::ScriptWorldHost for NativeCommandHost<'_> {
        fn world(&mut self) -> &mut World {
            self.world
        }
        fn resolve_entity(&mut self, id: u32) -> Option<hecs::Entity> {
            let real = resolve_id(self.spawn_remap, id as u64);
            self.index.get(&real).copied()
        }
    }

    for cmd in commands {
        match cmd {
            ScriptCommand::PackageCall { tag, args } => {
                let mut host = NativeCommandHost {
                    world,
                    index: entity_index,
                    spawn_remap: &spawn_remap,
                };
                crate::package::script_commands::dispatch_global(
                    &mut host,
                    tag,
                    &crate::scripting::host_commands::JsonArgs(args.clone()),
                );
            }
            ScriptCommand::SetPosition { entity_id, x, y } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut transform) = world.get::<&mut Transform>(entity) {
                        transform.position.x = *x;
                        transform.position.y = *y;
                    }
            }
            ScriptCommand::SetRotation { entity_id, radians } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut transform) = world.get::<&mut Transform>(entity) {
                        transform.rotation = *radians;
                    }
            }
            ScriptCommand::SetScale { entity_id, x, y } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut transform) = world.get::<&mut Transform>(entity) {
                        transform.scale.x = *x;
                        transform.scale.y = *y;
                    }
            }
            ScriptCommand::SetHealth { entity_id, current, max } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut health) = world.get::<&mut Health>(entity) {
                        health.current = *current;
                        if *max > 0.0 {
                            health.max = *max;
                        }
                    }
            }
            ScriptCommand::SetHitbox { entity_id, damage, knockback, retrigger } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut hitbox) = world.get::<&mut Hitbox>(entity) {
                        hitbox.damage = *damage;
                        hitbox.knockback = *knockback;
                        hitbox.retrigger = *retrigger;
                    }
            }
            ScriptCommand::SetParent { entity_id, parent_id } => {
                let child_real = resolve_id(&spawn_remap, *entity_id);
                let Some(&child) = entity_index.get(&child_real) else { continue };
                match parent_id {
                    Some(pid) => {
                        let parent_real = resolve_id(&spawn_remap, *pid);
                        if let Some(&parent) = entity_index.get(&parent_real) {
                            if parent == child {
                                log::warn!("set_parent: ignoring self-parent on entity {child_real}");
                            } else {
                                let _ = world.insert_one(child, crate::ecs::components::Parent(parent));
                            }
                        }
                    }
                    None => {
                        let _ = world.remove_one::<crate::ecs::components::Parent>(child);
                    }
                }
            }
            ScriptCommand::SetSkeletalPlayer {
                entity_id,
                clip,
                time,
                speed,
                looping,
                playing,
            } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut player) = world
                        .get::<&mut crate::animation::skeletal::SkeletalPlayer>(entity)
                {
                    if let Some(c) = clip { player.clip = c.clone(); }
                    if let Some(t) = time { player.time = *t; }
                    if let Some(s) = speed { player.speed = *s; }
                    if let Some(l) = looping { player.looping = *l; }
                    if let Some(p) = playing { player.playing = *p; }
                }
            }
            ScriptCommand::SetSpriteColor {
                entity_id,
                r,
                g,
                b,
                a,
            } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id) {
                    // A sprite, else a mesh: `Sprite.set_color` tints either.
                    if let Ok(mut sprite) = world.get::<&mut SpriteComponent>(entity) {
                        sprite.color = [*r, *g, *b, *a];
                    } else if let Ok(mut mesh) = world.get::<&mut crate::ecs::components::Mesh2D>(entity) {
                        mesh.color = [*r, *g, *b, *a];
                    }
                }
            }
            ScriptCommand::SetCameraPixelSize { size } => {
                if !set_active_camera_pixel_size(world, *size) {
                    log::warn!(
                        "set_camera_pixel_size({size}) had no effect: no entity carries an active Camera2DComponent"
                    );
                }
            }
            ScriptCommand::SetCameraZoom { zoom } => {
                if !set_active_camera_zoom(world, *zoom) {
                    log::warn!(
                        "set_camera_zoom({zoom}) had no effect: no entity carries an active Camera2DComponent"
                    );
                }
            }
            ScriptCommand::MoveCharacter { entity_id, vx, vy, drop_through } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut cc) = world.get::<&mut crate::ecs::components::CharacterController2D>(entity)
                {
                    cc.velocity = [*vx, *vy];
                    if let Some(d) = drop_through {
                        cc.drop_through = *d;
                    }
                }
            }
            ScriptCommand::SetTextColor { entity_id, r, g, b, a } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut text) = world.get::<&mut TextComponent>(entity)
                {
                    text.color = [*r, *g, *b, *a];
                }
            }
            ScriptCommand::SetMesh { entity_id, vertices, indices } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id) {
                    crate::scripting::host_commands::set_mesh(world, entity, vertices, indices);
                }
            }
            ScriptCommand::SetTrailEmitting { entity_id, on } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut trail) = world.get::<&mut crate::ecs::components::Trail>(entity)
                {
                    trail.emitting = *on;
                }
            }
            ScriptCommand::ClearTrail { entity_id } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut trail) = world.get::<&mut crate::ecs::components::Trail>(entity)
                {
                    trail.points.clear();
                }
            }
            ScriptCommand::PaintSprite { entity_id, x, y, size, color, shape, rotation, depth, blend } => {
                let _ = (entity_id, x, y, size, color, shape, rotation, depth, blend);
                #[cfg(not(feature = "paint"))]
                crate::features::missing("paint", "Sprite.paint");
                #[cfg(feature = "paint")]
                if let Some(&entity) = entity_index.get(&resolve_id(&spawn_remap, *entity_id)) {
                    let stamp = crate::ecs::paint::Stamp {
                        x: *x,
                        y: *y,
                        size: *size,
                        color: *color,
                        shape: crate::ecs::paint::StampShape::named(shape),
                        rotation: *rotation,
                        depth: *depth,
                        blend: crate::ecs::paint::PaintBlend::named(blend),
                    };
                    crate::ecs::paint::paint(world, entity, stamp);
                }
            }
            ScriptCommand::CarveSprite { entity_id, x, y, radius } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut c) = world.get::<&mut crate::ecs::carve::Carvable>(entity)
                {
                    c.pending.push(crate::ecs::carve::Carve { x: *x, y: *y, radius: *radius });
                }
            }
            ScriptCommand::SetZOrder { entity_id, z } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id) {
                    if let Ok(mut sprite) = world.get::<&mut SpriteComponent>(entity) {
                        sprite.z_order = *z;
                    }
                    if let Ok(mut text) = world.get::<&mut TextComponent>(entity) {
                        text.z_order = *z;
                    }
                    if let Ok(mut mesh) = world.get::<&mut crate::ecs::components::Mesh2D>(entity) {
                        mesh.z_order = *z;
                    }
                }
            }
            ScriptCommand::SetSpriteSize { entity_id, w, h } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut sprite) = world.get::<&mut SpriteComponent>(entity)
                {
                    sprite.size = Some([w.max(0.0), h.max(0.0)]);
                }
            }
            ScriptCommand::SetSpriteAnchor { entity_id, x, y } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut sprite) = world.get::<&mut SpriteComponent>(entity) {
                        sprite.anchor = [*x, *y];
                    }
            }
            ScriptCommand::SetSpriteLabel { entity_id, label, category } => {
                let _ = (entity_id, label, category);
                crate::feature!("sprite_library", "Sprite.set_label", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::sprite_library::set_label(world, entity, label, category.as_deref())
                    {
                        log::warn!("Sprite.set_label: {e}");
                    }
                });
            }
            ScriptCommand::SetSpriteRegion { entity_id, atlas_id, region_name } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id) {
                    // Look up the UV rect from the loaded AtlasData. Silent
                    // no-op if the atlas/region is unknown — keeps script
                    // hot paths cheap and matches the established pattern
                    // for missing entities elsewhere in this apply loop.
                    let uv_rect = assets
                        .as_deref()
                        .and_then(|a| a.get_atlas_data(atlas_id))
                        .and_then(|a| a.get_uv_rect(region_name));
                    if let Some(uv) = uv_rect {
                        // Runtime-spawned entities start with only
                        // Name+Transform. Materialize a default sprite so
                        // set_sprite_region works on them (mirrors viewport).
                        if world.get::<&SpriteComponent>(entity).is_err() {
                            let _ = world.insert_one(entity, SpriteComponent::default());
                        }
                        if let Ok(mut sprite) = world.get::<&mut SpriteComponent>(entity) {
                            sprite.texture_id = atlas_id.as_str().into();
                            sprite.uv_rect = Some(uv);
                            // The sprite's own region is this one now, and
                            // resolved: a prefab's pending region would
                            // otherwise overwrite it when it resolves.
                            sprite.region = Some(region_name.clone());
                            sprite.region_resolved = true;
                        }
                    } else {
                        log::warn!(
                            "[Script] set_sprite_region: atlas '{atlas_id}' region '{region_name}' not found"
                        );
                    }
                }
            }
            ScriptCommand::SetSpriteCorners { entity_id, corners } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id) {
                    // Materialize a default sprite for runtime-spawned
                    // entities so set_sprite_corners works (mirrors viewport).
                    if world.get::<&SpriteComponent>(entity).is_err() {
                        let _ = world.insert_one(entity, SpriteComponent::default());
                    }
                    if let Ok(mut sprite) = world.get::<&mut SpriteComponent>(entity) {
                        sprite.corners = *corners;
                    }
                }
            }
            ScriptCommand::Log { message } => {
                log::info!("[Script] {}", message);
            }
            ScriptCommand::DestroyEntity { entity_id } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id) {
                    entities_to_destroy.push(entity);
                }
            }
            ScriptCommand::SpawnEntity { name, components, spawn_id, children, prepared } => {
                let entity = match prepared {
                    // A loaded prefab: its decoded components cloned, its
                    // parts under it.
                    Some(prefab) => prefab.spawn(world, name),
                    None => {
                        let entity = spawn_bundle(world, name, components.as_ref().as_ref());
                        // A prefab's parts, each spawned the same way under it.
                        if !children.is_empty() {
                            crate::scene::prefab::spawn_children(world, entity, children, &mut |w, data| {
                                spawn_bundle(w, &data.name, Some(&data.components))
                            });
                        }
                        entity
                    }
                };
                if let Some(sid) = spawn_id {
                    spawn_remap.insert(*sid, entity.id() as u64);
                    crate::scripting::spawn_aliases::record(*sid, entity);
                }
                // A newly spawned entity might receive subsequent commands
                // in the same drain (e.g. SetPosition right after SpawnEntity);
                // insert it into the index so the next lookup finds it.
                entity_index.insert(entity.id() as u64, entity);
            }
            ScriptCommand::SpawnRig { rig_id, spawn_id } => {
                // Pull the rig out of the asset manager. Clone is cheap
                // (small Vec of BoneDefs) and keeps the immutable borrow
                // local so we can hand &mut world to spawn_rig.
                let rig = assets.as_deref().and_then(|a| a.get_skeleton_rig(rig_id).cloned());
                let Some(rig) = rig else {
                    log::warn!("[Script] spawn_rig: rig '{rig_id}' not found");
                    continue;
                };
                let map = crate::animation::rig::spawn_rig(world, &rig);
                // Root is the entity carrying SkeletonRoot. spawn_rig
                // already picked one; locate it so we can map spawn_id.
                let root = map
                    .values()
                    .copied()
                    .find(|&e| world.get::<&crate::ecs::components::SkeletonRoot>(e).is_ok());
                if let Some(root) = root {
                    spawn_remap.insert(*spawn_id, root.id() as u64);
                    crate::scripting::spawn_aliases::record(*spawn_id, root);
                    // Make every freshly-spawned bone reachable through
                    // entity_index so a follow-up SetPosition/SetParent in
                    // the same drain works without an extra rebuild.
                    for &e in map.values() {
                        entity_index.insert(e.id() as u64, e);
                    }
                }
            }
            ScriptCommand::SetIkTarget { entity_id, x, y } => {
                let _ = (entity_id, x, y);
                crate::feature!("ik", "Skeleton.set_ik_target", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id) {
                        if let Ok(mut ik) = world.get::<&mut crate::animation::ik::IkTwoBone>(entity) {
                            ik.target = [*x, *y];
                        } else if let Ok(mut ik) = world.get::<&mut crate::animation::ik::IkLookAt>(entity) {
                            ik.target = [*x, *y];
                        } else if let Ok(mut ik) = world.get::<&mut crate::animation::ik::IkChain>(entity) {
                            ik.target = [*x, *y];
                        }
                    }
                });
            }
            ScriptCommand::SetCustomField {
                entity_id,
                component_name,
                field_name,
                value,
            } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut custom) = world.get::<&mut CustomComponents>(entity) {
                        let comp = custom
                            .0
                            .entry(component_name.clone())
                            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
                        if let serde_json::Value::Object(map) = comp {
                            map.insert(field_name.clone(), value.clone());
                        }
                    }
            }
            ScriptCommand::AddTag { entity_id, tag } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                let mut found = false;
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut tags) = world.get::<&mut Tags>(entity)
                {
                    tags.0.insert(tag.clone());
                    found = true;
                }
                // If entity doesn't have a Tags component yet, add one
                if !found
                    && let Some(&entity) = entity_index.get(&real_id)
                {
                    let mut tag_set = std::collections::HashSet::new();
                    tag_set.insert(tag.clone());
                    let _ = world.insert_one(entity, Tags(tag_set));
                }
            }
            ScriptCommand::RemoveTag { entity_id, tag } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut tags) = world.get::<&mut Tags>(entity)
                {
                    tags.0.remove(tag);
                }
            }
            ScriptCommand::SendEvent { name, sender, data } => {
                // Events are collected and dispatched by the ScriptSystem after all commands
                // are applied. We don't process them here — just log for debugging.
                log::trace!("[Script] Event '{}' from entity {}", name, sender);
                let _ = (name, sender, data); // suppress unused warning
            }
            ScriptCommand::LoadScene { path } => {
                if let Some(ref mut mgr) = scene_manager {
                    mgr.queue_action(crate::scene::manager::SceneAction::Load(path.clone()));
                }
            }
            ScriptCommand::QuitGame => {
                if let Some(ref mut mgr) = scene_manager {
                    mgr.queue_action(crate::scene::manager::SceneAction::Quit);
                }
            }
            ScriptCommand::SetTimeScale { scale } => {
                if let Some(ref mut mgr) = scene_manager {
                    mgr.queue_action(
                        crate::scene::manager::SceneAction::SetTimeScale(*scale),
                    );
                }
            }
            ScriptCommand::PushScene { path } => {
                if let Some(ref mut mgr) = scene_manager {
                    mgr.queue_action(crate::scene::manager::SceneAction::Push(path.clone()));
                }
            }
            ScriptCommand::PopScene => {
                if let Some(ref mut mgr) = scene_manager {
                    mgr.queue_action(crate::scene::manager::SceneAction::Pop);
                }
            }
            ScriptCommand::FadeIn { duration_s, color } => {
                if let Some(ref mut mgr) = scene_manager {
                    mgr.queue_transition(
                        crate::scene::manager::TransitionRequest::FadeIn {
                            duration_s: *duration_s,
                            color: *color,
                        },
                    );
                }
            }
            ScriptCommand::FadeOut { duration_s, color } => {
                if let Some(ref mut mgr) = scene_manager {
                    mgr.queue_transition(
                        crate::scene::manager::TransitionRequest::FadeOut {
                            duration_s: *duration_s,
                            color: *color,
                        },
                    );
                }
            }
            ScriptCommand::ScreenFlash { duration_s, color } => {
                if let Some(ref mut mgr) = scene_manager {
                    mgr.queue_transition(
                        crate::scene::manager::TransitionRequest::Flash {
                            duration_s: *duration_s,
                            color: *color,
                        },
                    );
                }
            }
            // `play_animation` / `stop_animation` / `set_animation_speed`
            // drive a sprite animation, a skeletal one, or both - whatever
            // the entity has. Until Sep 2026 they reached only the sprite
            // animator in every runtime (native bound skeletal versions and
            // then overwrote them), so no script could start a skeletal clip.
            ScriptCommand::PlayAnimation {
                entity_id,
                clip_name,
            } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id) {
                    if let Ok(mut animator) = world.get::<&mut SpriteAnimator>(entity) {
                        animator.play(clip_name);
                    }
                    if let Ok(mut player) = world.get::<&mut crate::animation::skeletal::SkeletalPlayer>(entity) {
                        player.play(clip_name);
                    }
                }
            }
            ScriptCommand::StopAnimation { entity_id } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id) {
                    if let Ok(mut animator) = world.get::<&mut SpriteAnimator>(entity) {
                        animator.stop();
                    }
                    if let Ok(mut player) = world.get::<&mut crate::animation::skeletal::SkeletalPlayer>(entity) {
                        player.playing = false;
                    }
                }
            }
            ScriptCommand::SetAnimationSpeed { entity_id, speed } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id) {
                    if let Ok(mut animator) = world.get::<&mut SpriteAnimator>(entity) {
                        animator.speed = *speed;
                    }
                    if let Ok(mut player) = world.get::<&mut crate::animation::skeletal::SkeletalPlayer>(entity) {
                        player.speed = *speed;
                    }
                }
            }
            ScriptCommand::CrossfadeAnimation { entity_id, clip, seconds } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut player) = world.get::<&mut crate::animation::skeletal::SkeletalPlayer>(entity)
                {
                    player.crossfade(clip, *seconds);
                }
            }
            ScriptCommand::SetAnimationBlend { entity_id, clip, weight } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut player) = world.get::<&mut crate::animation::skeletal::SkeletalPlayer>(entity)
                {
                    player.set_blend(clip, *weight);
                }
            }
            ScriptCommand::SetConveyorSpeed { entity_id, speed } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut c) = world.get::<&mut crate::ecs::components::Conveyor>(entity)
                {
                    c.speed = *speed;
                }
            }
            ScriptCommand::AnimationLayer { entity_id, op, layer, clip, value, seconds, looping } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut player) = world.get::<&mut crate::animation::skeletal::SkeletalPlayer>(entity)
                {
                    player.layer_op(op, layer, clip, *value, *seconds, *looping);
                }
            }
            ScriptCommand::SetAnimationBlendPosition { entity_id, x, y } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut player) = world.get::<&mut crate::animation::skeletal::SkeletalPlayer>(entity)
                {
                    player.set_blend_position(*x, *y);
                }
            }
            ScriptCommand::SetIkWeight { entity_id, weight } => {
                let _ = (entity_id, weight);
                crate::feature!("ik", "Skeleton.set_ik_weight", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id) {
                        if let Ok(mut ik) = world.get::<&mut crate::animation::ik::IkTwoBone>(entity) {
                            ik.weight = weight.clamp(0.0, 1.0);
                        } else if let Ok(mut ik) = world.get::<&mut crate::animation::ik::IkChain>(entity) {
                            ik.weight = weight.clamp(0.0, 1.0);
                        }
                    }
                });
            }
            ScriptCommand::SetAnimBool { entity_id, param, value } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut fsm) = world.get::<&mut AnimStateMachine>(entity)
                {
                    fsm.set_bool(param, *value);
                }
            }
            ScriptCommand::SetAnimFloat { entity_id, param, value } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut fsm) = world.get::<&mut AnimStateMachine>(entity)
                {
                    fsm.set_float(param, *value);
                }
            }
            ScriptCommand::SetAnimTrigger { entity_id, param } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut fsm) = world.get::<&mut AnimStateMachine>(entity)
                {
                    fsm.set_trigger(param);
                }
            }
            ScriptCommand::StartStateMachine { entity_id, state_name } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok((animator, fsm)) =
                        world.query_one_mut::<(&mut SpriteAnimator, &mut AnimStateMachine)>(entity)
                {
                    fsm.start(state_name, animator);
                }
            }
            // Tween commands are handled by TweenManager directly from EngineApi methods,
            // so StartTween / CancelTween / CancelAllTweens are no-ops in apply_script_commands.
            ScriptCommand::StartTween { .. } => {}
            ScriptCommand::CancelTween { .. } => {}
            ScriptCommand::CancelAllTweens { .. } => {}
            // Phase 4D: Tilemap
            ScriptCommand::SetTile {
                entity_id,
                layer_name,
                x,
                y,
                tile_id,
            } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut tilemap) = world.get::<&mut Tilemap>(entity)
                    && let Some(layer) = tilemap.find_layer_mut(layer_name)
                {
                    layer.set_tile(*x, *y, *tile_id);
                }
            }
            // Phase 4F: Particles
            ScriptCommand::EmitBurst { entity_id, count } => {
                // Burst will be handled in the ScriptSystem::run after commands are applied.
                // We just log for tracing here.
                log::trace!("[Script] EmitBurst on entity {} count {}", entity_id, count);
                let _ = (entity_id, count);
            }
            ScriptCommand::SetEmitterActive { entity_id, active } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut emitter) = world.get::<&mut ParticleEmitter>(entity)
                {
                    emitter.active = *active;
                }
            }
            ScriptCommand::SetEmitterRate { entity_id, rate } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut emitter) = world.get::<&mut ParticleEmitter>(entity)
                {
                    emitter.emission_rate = *rate;
                }
            }
            ScriptCommand::SetEmitterField { entity_id, field, value } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id) {
                    // A CPU emitter or a GPU one (`particles::gpu`).
                    let result = if let Ok(mut emitter) = world.get::<&mut ParticleEmitter>(entity) {
                        crate::particles::set_emitter_field(&mut emitter, field, value)
                    } else if let Ok(mut g) = world.get::<&mut crate::particles::gpu::GpuParticles>(entity) {
                        crate::particles::gpu::set_field(&mut g, field, value)
                    } else {
                        Ok(())
                    };
                    if let Err(e) = result {
                        log::warn!("Particles.set: {e}");
                    }
                }
            }
            ScriptCommand::SetVideoField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("video", "Video.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::video::set_video_field(world, entity, field, value)
                    {
                        log::warn!("Video.set: {e}");
                    }
                });
            }
            ScriptCommand::SetRagdollField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("ragdoll", "Ragdoll.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::ragdoll::set_ragdoll_field(world, entity, field, value)
                    {
                        log::warn!("Ragdoll.set: {e}");
                    }
                });
            }
            ScriptCommand::SetBehaviorTreeField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("behavior_tree", "BehaviorTree.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::behavior_tree::set_behavior_tree_field(world, entity, field, value)
                    {
                        log::warn!("BehaviorTree.set: {e}");
                    }
                });
            }
            ScriptCommand::SetTimelineField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("timeline", "Timeline.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::timeline::set_timeline_field(world, entity, field, value)
                    {
                        log::warn!("Timeline.set: {e}");
                    }
                });
            }
            ScriptCommand::SetCanvasGroupField { entity_id, field, value } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Err(e) = crate::ecs::canvas_group::set_canvas_group_field(world, entity, field, value)
                {
                    log::warn!("CanvasGroup.set: {e}");
                }
            }
            ScriptCommand::SetFluidField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("fluid", "Fluid.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::fluid::set_fluid_field(world, entity, field, value)
                    {
                        log::warn!("Fluid.set: {e}");
                    }
                });
            }
            ScriptCommand::SmokePuff { entity_id, x, y, radius, amount, vx, vy } => {
                let _ = (entity_id, x, y, radius, amount, vx, vy);
                crate::feature!("smoke", "Smoke.puff", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id) {
                        let puff = crate::ecs::smoke::Puff {
                            at: Vec2::new(*x, *y),
                            radius: *radius,
                            amount: *amount,
                            velocity: Vec2::new(*vx, *vy),
                        };
                        if let Err(e) = crate::ecs::smoke::queue_puff(world, entity, puff) {
                            log::warn!("Smoke.puff: {e}");
                        }
                    }
                });
            }
            ScriptCommand::TerrainEdit { entity_id, x, y, radius, amount } => {
                let _ = (entity_id, x, y, radius, amount);
                crate::feature!("smooth_terrain", "Terrain.edit", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::smooth_terrain::edit(world, entity, *x, *y, *radius, *amount)
                    {
                        log::warn!("Terrain.edit: {e}");
                    }
                });
            }
            ScriptCommand::DialogueOp { entity_id, op, arg } => {
                let _ = (entity_id, op, arg);
                crate::feature!("dialogue", "Dialogue.*", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::dialogue::op(world, entity, op, arg)
                    {
                        log::warn!("Dialogue.{op}: {e}");
                    }
                });
            }
            ScriptCommand::UiDrop { entity_id, slot } => {
                let _ = (entity_id, slot);
                crate::feature!("drag_drop", "UI.drop", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id) {
                        crate::ui::drag_drop::drop_named(world, entity, slot);
                    }
                });
            }
            ScriptCommand::Mode7Place { entity_id, x, y, angle } => {
                let _ = (entity_id, x, y, angle);
                crate::feature!("mode7", "Mode7.place", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::mode7::place(world, entity, *x, *y, *angle)
                    {
                        log::warn!("Mode7.place: {e}");
                    }
                });
            }
            ScriptCommand::Mode7Set { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("mode7", "Mode7.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::mode7::set_field(world, entity, field, value)
                    {
                        log::warn!("Mode7.set: {e}");
                    }
                });
            }
            ScriptCommand::StagePlace { entity_id, x, y, z } => {
                let _ = (entity_id, x, y, z);
                crate::feature!("stage", "Stage.place", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::stage::place(world, entity, *x, *y, *z)
                    {
                        log::warn!("Stage.place: {e}");
                    }
                });
            }
            ScriptCommand::StageSet { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("stage", "Stage.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::stage::set_field(world, entity, field, value)
                    {
                        log::warn!("Stage.set: {e}");
                    }
                });
            }
            ScriptCommand::SpriteTiltSet { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("sprite_tilt", "SpriteTilt.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::sprite_tilt::set_field(world, entity, field, value)
                    {
                        log::warn!("SpriteTilt.set: {e}");
                    }
                });
            }
            ScriptCommand::AfterimageSet { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("afterimage", "Afterimage.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::afterimage::set_field(world, entity, field, value)
                    {
                        log::warn!("Afterimage.set: {e}");
                    }
                });
            }
            ScriptCommand::PhysicsSlice { x0, y0, x1, y1 } => {
                let _ = (x0, y0, x1, y1);
                crate::feature!("slice", "Physics.slice", { crate::ecs::slice::script_slice(world, *x0, *y0, *x1, *y1) });
            }
            ScriptCommand::PortalSet { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("portal", "Portal.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::portal::set_field(world, entity, field, value)
                    {
                        log::warn!("Portal.set: {e}");
                    }
                });
            }
            ScriptCommand::JellyPoke { entity_id, x, y, strength, radius } => {
                let _ = (entity_id, x, y, strength, radius);
                crate::feature!("jelly", "Jelly.poke", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::jelly::poke(world, entity, glam::Vec2::new(*x, *y), *strength, *radius)
                    {
                        log::warn!("Jelly.poke: {e}");
                    }
                });
            }
            ScriptCommand::JellySet { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("jelly", "Jelly.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::jelly::set_field(world, entity, field, value)
                    {
                        log::warn!("Jelly.set: {e}");
                    }
                });
            }
            ScriptCommand::MarkerSet { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("offscreen_marker", "Marker.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::offscreen_marker::set_field(world, entity, field, value)
                    {
                        log::warn!("Marker.set: {e}");
                    }
                });
            }
            ScriptCommand::RadialSet { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("radial_menu", "Radial.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ui::radial_menu::set_field(world, entity, field, value)
                    {
                        log::warn!("Radial.set: {e}");
                    }
                });
            }
            ScriptCommand::BubbleSay { entity_id, text, seconds } => {
                let _ = (entity_id, text, seconds);
                crate::feature!("speech_bubble", "Bubble.say", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ui::speech_bubble::say(world, entity, text, *seconds)
                    {
                        log::warn!("Bubble.say: {e}");
                    }
                });
            }
            ScriptCommand::BubbleSet { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("speech_bubble", "Bubble.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ui::speech_bubble::set_field(world, entity, field, value)
                    {
                        log::warn!("Bubble.set: {e}");
                    }
                });
            }
            ScriptCommand::OutlineSet { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("sprite_outline", "Outline.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::sprite_outline::set_field(world, entity, field, value)
                    {
                        log::warn!("Outline.set: {e}");
                    }
                });
            }
            ScriptCommand::LightningStrike { entity_id, x, y } => {
                let _ = (entity_id, x, y);
                crate::feature!("lightning", "Lightning.strike", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::lightning::strike(world, entity, *x, *y)
                    {
                        log::warn!("Lightning.strike: {e}");
                    }
                });
            }
            ScriptCommand::LightningSet { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("lightning", "Lightning.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::lightning::set_field(world, entity, field, value)
                    {
                        log::warn!("Lightning.set: {e}");
                    }
                });
            }
            ScriptCommand::FlowSet { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("flow", "Flow.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::flow::set_field(world, entity, field, value)
                    {
                        log::warn!("Flow.set: {e}");
                    }
                });
            }
            ScriptCommand::NavLinkSet { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("navigation", "NavLink.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::nav_links::set_field(world, entity, field, value)
                    {
                        log::warn!("NavLink.set: {e}");
                    }
                });
            }
            ScriptCommand::MorphTo { entity_id, target, seconds } => {
                let _ = (entity_id, target, seconds);
                crate::feature!("path_morph", "Morph.to", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::path_morph::morph_to(world, entity, target, *seconds)
                    {
                        log::warn!("Morph.to: {e}");
                    }
                });
            }
            ScriptCommand::MorphSet { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("path_morph", "Morph.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::path_morph::set_field(world, entity, field, value)
                    {
                        log::warn!("Morph.set: {e}");
                    }
                });
            }
            ScriptCommand::ToastShow { entity_id, title, text, icon, seconds } => {
                let _ = (entity_id, title, text, icon, seconds);
                crate::feature!("toast", "Toast.show", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ui::toast::show(world, entity, title, text, icon, *seconds)
                    {
                        log::warn!("Toast.show: {e}");
                    }
                });
            }
            ScriptCommand::ToastSet { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("toast", "Toast.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ui::toast::set_field(world, entity, field, value)
                    {
                        log::warn!("Toast.set: {e}");
                    }
                });
            }
            ScriptCommand::FogOfWarReset { entity_id } => {
                let _ = entity_id;
                crate::feature!("fog_of_war", "FogOfWar.reset", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::fog_of_war::reset_fog(world, entity)
                    {
                        log::warn!("FogOfWar.reset: {e}");
                    }
                });
            }
            ScriptCommand::FogOfWarSet { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("fog_of_war", "FogOfWar.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::fog_of_war::set_fog_of_war_field(world, entity, field, value)
                    {
                        log::warn!("FogOfWar.set: {e}");
                    }
                });
            }
            ScriptCommand::SnowSet { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("snow", "Snow.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::snow::set_field(world, entity, field, value)
                    {
                        log::warn!("Snow.set: {e}");
                    }
                });
            }
            ScriptCommand::SetBeatClockField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("beat_clock", "BeatClock.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::beat_clock::set_beat_clock_field(world, entity, field, value)
                    {
                        log::warn!("BeatClock.set: {e}");
                    }
                });
            }
            ScriptCommand::SetTextPathField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("text_path", "TextPath.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::text_path::set_text_path_field(world, entity, field, value)
                    {
                        log::warn!("TextPath.set: {e}");
                    }
                });
            }
            ScriptCommand::SetTrajectoryField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("trajectory", "Trajectory.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::trajectory::set_trajectory_field(world, entity, field, value)
                    {
                        log::warn!("Trajectory.set: {e}");
                    }
                });
            }
            ScriptCommand::SetTileLightField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("tile_light", "TileLight.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::tile_light::set_tile_light_field(world, entity, field, value)
                    {
                        log::warn!("TileLight.set: {e}");
                    }
                });
            }
            ScriptCommand::SetAutoNormalField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("auto_normal", "AutoNormal.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::auto_normal::set_auto_normal_field(world, entity, field, value)
                    {
                        log::warn!("AutoNormal.set: {e}");
                    }
                });
            }
            ScriptCommand::SetCastShadowField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("cast_shadow", "CastShadow.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::cast_shadow::set_shadow_field(world, entity, field, value)
                    {
                        log::warn!("CastShadow.set: {e}");
                    }
                });
            }
            ScriptCommand::SetTimeScaleField { entity_id, field, value } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Err(e) = crate::ecs::time_scale::set_time_scale_field(world, entity, field, value)
                {
                    log::warn!("TimeScale.set: {e}");
                }
            }
            ScriptCommand::SetOriginField { entity_id, field, value } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Err(e) = crate::ecs::floating_origin::set_origin_field(world, entity, field, value)
                {
                    log::warn!("FloatingOrigin.set: {e}");
                }
            }
            ScriptCommand::SetDayField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("day_cycle", "DayCycle.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::day_cycle::set_day_field(world, entity, field, value)
                    {
                        log::warn!("DayCycle.set: {e}");
                    }
                });
            }
            ScriptCommand::SetVisionField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("vision", "Vision.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::vision::set_vision_field(world, entity, field, value)
                    {
                        log::warn!("Vision.set: {e}");
                    }
                });
            }
            ScriptCommand::WfcGenerate { entity_id, x, y, w, h } => {
                let _ = (entity_id, x, y, w, h);
                crate::feature!("wfc", "Wfc.generate", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::wfc::queue_region(world, entity, *x, *y, *w, *h)
                    {
                        log::warn!("Wfc.generate: {e}");
                    }
                });
            }
            ScriptCommand::SetFogField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("fog", "Fog.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::fog::set_fog_field(world, entity, field, value)
                    {
                        log::warn!("Fog.set: {e}");
                    }
                });
            }
            ScriptCommand::SetRewindField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("rewind", "Rewind.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::rewind::set_rewind_field(world, entity, field, value)
                    {
                        log::warn!("Rewind.set: {e}");
                    }
                });
            }
            ScriptCommand::SetSmokeField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("smoke", "Smoke.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::smoke::set_smoke_field(world, entity, field, value)
                    {
                        log::warn!("Smoke.set: {e}");
                    }
                });
            }
            ScriptCommand::PixelPaint { entity_id, x, y, radius, material } => {
                let _ = (entity_id, x, y, radius, material);
                crate::feature!("pixel_sim", "PixelSim.paint", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::pixel_sim::queue_paint(world, entity, *x, *y, *radius, material)
                    {
                        log::warn!("PixelSim.paint: {e}");
                    }
                });
            }
            ScriptCommand::SetSpriteFill { entity_id, amount } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut sprite) = world.get::<&mut SpriteComponent>(entity)
                    && !sprite.set_fill_amount(*amount)
                {
                    log::warn!("Sprite.set_fill: the sprite has no bar or radial fill");
                }
            }
            ScriptCommand::SetWaterField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("water", "Water.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::water::set_water_field(world, entity, field, value)
                    {
                        log::warn!("Water.set: {e}");
                    }
                });
            }
            ScriptCommand::WaterSplash { entity_id, x, strength } => {
                let _ = (entity_id, x, strength);
                crate::feature!("water", "Water.splash", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::water::splash_world(world, entity, *x, *strength)
                    {
                        log::warn!("Water.splash: {e}");
                    }
                });
            }
            ScriptCommand::SetMirrorField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("mirror", "Mirror.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::mirror::set_mirror_field(world, entity, field, value)
                    {
                        log::warn!("Mirror.set: {e}");
                    }
                });
            }
            ScriptCommand::SetTrailField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("trails", "Trail.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = super::trails::set_trail_field(world, entity, field, value)
                    {
                        log::warn!("Trail.set: {e}");
                    }
                });
            }
            ScriptCommand::SetLineField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("line2d", "Line.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::renderer::line2d::set_line_field(world, entity, field, value)
                    {
                        log::warn!("Line.set: {e}");
                    }
                });
            }
            ScriptCommand::SetPathFollowField { entity_id, field, value } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Err(e) = crate::ecs::path_follow::set_path_follow_field(world, entity, field, value)
                {
                    log::warn!("PathFollow.set: {e}");
                }
            }
            ScriptCommand::SetClothField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("cloth", "Cloth.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::ecs::cloth::set_cloth_field(world, entity, field, value)
                    {
                        log::warn!("Cloth.set: {e}");
                    }
                });
            }
            ScriptCommand::SetAreaField { entity_id, field, value } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Err(e) = crate::ecs::area_effect::set_area_field(world, entity, field, value)
                {
                    log::warn!("Area.set: {e}");
                }
            }
            ScriptCommand::SetCameraFollow { entity_id, field, value } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Err(e) = crate::ecs::components::set_follow_field(world, entity, field, value)
                {
                    log::warn!("Camera.set_follow: {e}");
                }
            }
            ScriptCommand::SetCameraField { entity_id, field, value } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Err(e) = crate::ecs::camera_rig::set_camera_field(world, entity, field, value)
                {
                    log::warn!("Camera.set: {e}");
                }
            }
            ScriptCommand::SetCameraScreenRect { entity_id, rect } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id) {
                    crate::ecs::views::set_screen_rect(world, entity, *rect);
                }
            }
            ScriptCommand::SetNavTarget { entity_id, target } => {
                let _ = (entity_id, target);
                crate::feature!("navigation", "Nav.set_target", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::navigation::set_nav_target(world, entity, *target)
                    {
                        log::warn!("Nav.set_target: {e}");
                    }
                });
            }
            ScriptCommand::SetNavField { entity_id, field, value } => {
                let _ = (entity_id, field, value);
                crate::feature!("navigation", "Nav.set", {
                    let real_id = resolve_id(&spawn_remap, *entity_id);
                    if let Some(&entity) = entity_index.get(&real_id)
                        && let Err(e) = crate::navigation::set_nav_field(world, entity, field, value)
                    {
                        log::warn!("Nav.set: {e}");
                    }
                });
            }
            // Phase 4E: UI
            ScriptCommand::SetUiProp { entity_id, property, value } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && !crate::ui::apply_ui_prop(world, entity, property, value)
                {
                    log::warn!(
                        "set_ui_prop: entity {real_id} has no UI widget with property '{property}'"
                    );
                }
            }
            ScriptCommand::SetUIText { entity_id, text } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut ui_text) = world.get::<&mut UIText>(entity)
                {
                    ui_text.text = text.clone();
                }
            }
            ScriptCommand::SetText { entity_id, text } => {
                use super::components::TextComponent;
                let real_id = resolve_id(&spawn_remap, *entity_id);
                let mut updated = false;
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut tc) = world.get::<&mut TextComponent>(entity)
                {
                    tc.text = text.clone();
                    updated = true;
                }
                if !updated
                    && let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut ui_text) = world.get::<&mut UIText>(entity)
                {
                    ui_text.text = text.clone();
                }
            }
            ScriptCommand::SetUIVisible { entity_id, visible } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut ui_elem) = world.get::<&mut UIElement>(entity)
                {
                    ui_elem.visible = *visible;
                }
            }
            ScriptCommand::SetUIPosition { entity_id, anchor, offset_x, offset_y } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut ui_elem) = world.get::<&mut UIElement>(entity)
                {
                    ui_elem.anchor = *anchor;
                    ui_elem.offset = Vec2::new(*offset_x, *offset_y);
                }
            }
            ScriptCommand::CreateUIText { name, text, anchor, offset_x, offset_y, color, font_size, alignment } => {
                let entity = world.spawn((
                    Name(name.clone()),
                    UIElement {
                        anchor: *anchor,
                        offset: Vec2::new(*offset_x, *offset_y),
                        size: Vec2::new(256.0, 32.0),
                        visible: true,
                        z_order: 100,
                        ..Default::default()
                    },
                    UIText {
                        text: text.clone(),
                        color: *color,
                        font_size: *font_size,
                        alignment: *alignment,
                        ..Default::default()
                    },
                ));
                log::info!("[Script] Created UI text '{}' -> {:?}", name, entity);
            }
            ScriptCommand::CreateUIPanel { name, anchor, offset_x, offset_y, width, height, bg_color } => {
                let entity = world.spawn((
                    Name(name.clone()),
                    UIElement {
                        anchor: *anchor,
                        offset: Vec2::new(*offset_x, *offset_y),
                        size: Vec2::new(*width, *height),
                        visible: true,
                        z_order: 99,
                        ..Default::default()
                    },
                    UIPanel {
                        bg_color: *bg_color,
                    },
                ));
                log::info!("[Script] Created UI panel '{}' -> {:?}", name, entity);
            }
            // Phase 4E-GUI: GUI document commands
            ScriptCommand::GuiShow { gui_id } => {
                use super::components::GuiComponent;
                let mut found = false;
                for (_, gui_comp) in world.query_mut::<&mut GuiComponent>() {
                    if gui_comp.gui_id == *gui_id {
                        gui_comp.visible = true;
                        found = true;
                        break;
                    }
                }
                if !found {
                    log::warn!("[Script] gui_show: no entity with gui_id '{}'", gui_id);
                }
            }
            ScriptCommand::GuiHide { gui_id } => {
                use super::components::GuiComponent;
                for (_, gui_comp) in world.query_mut::<&mut GuiComponent>() {
                    if gui_comp.gui_id == *gui_id {
                        gui_comp.visible = false;
                        break;
                    }
                }
            }
            ScriptCommand::GuiSetNodeProp { gui_id, node_id, property, value } => {
                if let Some(assets) = assets.as_deref_mut() {
                    if let Some(doc) = assets.get_gui_mut(gui_id) {
                        if let Some(node) = doc.root.find_node_mut(node_id) {
                            // One shared dispatch for every runtime — see
                            // ui::gui_document::apply_gui_node_prop.
                            if !crate::ui::gui_document::apply_gui_node_prop(node, property, value) {
                                log::warn!("[Script] gui_set: unknown property '{}'", property);
                            }
                        } else {
                            log::warn!("[Script] gui_set: node '{}' not found in '{}'", node_id, gui_id);
                        }
                    } else {
                        log::warn!("[Script] gui_set: gui '{}' not loaded", gui_id);
                    }
                }
            }
            ScriptCommand::GuiFocus { gui_id, node_id } => {
                use super::components::GuiComponent;
                for (_, gui_comp) in world.query_mut::<&mut GuiComponent>() {
                    if gui_comp.gui_id == *gui_id {
                        gui_comp.focused_node_id = if node_id.is_empty() {
                            None
                        } else {
                            Some(node_id.clone())
                        };
                        break;
                    }
                }
            }
            // Phase 4B: Physics — most are handled by PhysicsWorld in the game loop.
            // SetVelocity also applies to simple Velocity2D components (non-Rapier).
            ScriptCommand::SetVelocity { entity_id, vx, vy } => {
                let real_id = resolve_id(&spawn_remap, *entity_id);
                if let Some(&entity) = entity_index.get(&real_id)
                    && let Ok(mut vel) = world.get::<&mut Velocity2D>(entity)
                {
                    vel.x = *vx;
                    vel.y = *vy;
                }
                // Physics package also handles this for Rapier bodies
            }
            ScriptCommand::ApplyForce { .. }
            | ScriptCommand::ApplyImpulse { .. }
            | ScriptCommand::SetAngularVelocity { .. }
            | ScriptCommand::SetGravity { .. }
            | ScriptCommand::SetBodyType { .. }
            | ScriptCommand::AddJoint { .. }
            | ScriptCommand::RemoveJoint { .. }
            | ScriptCommand::SetMotorSpeed { .. } => {}
            // Phase 4A: Audio — these are handled by apply_audio_commands in the game loop.
            ScriptCommand::PlaySound { .. }
            | ScriptCommand::PlayMusic { .. }
            | ScriptCommand::StopSound { .. }
            | ScriptCommand::StopSoundByClip { .. }
            | ScriptCommand::StopMusic
            | ScriptCommand::PauseMusic
            | ScriptCommand::ResumeMusic
            | ScriptCommand::SetSoundVolume { .. }
            | ScriptCommand::SetMusicVolume { .. }
            | ScriptCommand::SetMasterVolume { .. }
            | ScriptCommand::StopAllSounds
            | ScriptCommand::PauseAllSounds
            | ScriptCommand::ResumeAllSounds => {}
            // Material system
            ScriptCommand::SetMaterial { entity_id, material_id } => {
                let entity_id = resolve_id(&spawn_remap, *entity_id);
                let mat_id = material_id.clone();
                // Try to update existing MaterialComponent, or insert a new one
                let mut found = false;
                if let Some(&entity) = entity_index.get(&entity_id)
                    && let Ok(mut mat) = world.get::<&mut MaterialComponent>(entity)
                {
                    mat.material_id = mat_id.as_str().into();
                    found = true;
                }
                if !found {
                    let entity = hecs::Entity::from_bits(entity_id).unwrap();
                    let _ = world.insert_one(entity, MaterialComponent {
                        material_id: mat_id.into(),
                        ..Default::default()
                    });
                }
            }
            ScriptCommand::SetNormalMap { entity_id, normal_map_id } => {
                let entity_id = resolve_id(&spawn_remap, *entity_id);
                let id = crate::scripting::host_commands::normal_map_id(normal_map_id.as_deref());
                if let Some(&entity) = entity_index.get(&entity_id) {
                    let mut mat = world.remove_one::<MaterialComponent>(entity).unwrap_or_default();
                    mat.normal_map_id = id;
                    let _ = world.insert_one(entity, mat);
                }
            }
            ScriptCommand::SetMaterialUniform { entity_id, name, value } => {
                let entity_id = resolve_id(&spawn_remap, *entity_id);
                let name = name.clone();
                let value = value.clone();
                let mut found = false;
                if let Some(&entity) = entity_index.get(&entity_id)
                    && let Ok(mut mat) = world.get::<&mut MaterialComponent>(entity)
                {
                    mat.uniforms.insert(name.clone(), value.clone());
                    found = true;
                }
                if !found {
                    let entity = hecs::Entity::from_bits(entity_id).unwrap();
                    let mut mc = MaterialComponent::default();
                    mc.uniforms.insert(name, value);
                    let _ = world.insert_one(entity, mc);
                }
            }
            // Post-processing commands are handled by the runtime (needs PostProcessStack)
            ScriptCommand::SetPostProcessEnabled { .. }
            | ScriptCommand::SetPostProcessParam { .. } => {}
            // Lighting commands are handled by the runtime (needs direct ECS access + LightingPass)
            ScriptCommand::SetLightColor { entity_id, r, g, b } => {
                use crate::scripting::host_commands::{AnyLight, with_lights};
                let eid = resolve_id(&spawn_remap, *entity_id);
                let color = [*r, *g, *b];
                if let Some(&entity) = entity_index.get(&eid) {
                    with_lights(world, entity, |l| match l {
                        AnyLight::Point(l) => l.color = color,
                        AnyLight::Spot(l) => l.color = color,
                        AnyLight::Sun(l) => l.color = color,
                        #[cfg(feature = "freeform_light")]
                        AnyLight::Freeform(l) => l.color = color,
                    });
                }
            }
            ScriptCommand::SetLightIntensity { entity_id, intensity } => {
                use crate::scripting::host_commands::{AnyLight, with_lights};
                let eid = resolve_id(&spawn_remap, *entity_id);
                let val = *intensity;
                if let Some(&entity) = entity_index.get(&eid) {
                    with_lights(world, entity, |l| match l {
                        AnyLight::Point(l) => l.intensity = val,
                        AnyLight::Spot(l) => l.intensity = val,
                        AnyLight::Sun(l) => l.intensity = val,
                        #[cfg(feature = "freeform_light")]
                        AnyLight::Freeform(l) => l.intensity = val,
                    });
                }
            }
            ScriptCommand::SetColorLut { texture_id, strength } => {
                crate::scripting::host_commands::set_color_lut(world, texture_id, *strength);
            }
            ScriptCommand::SetLightMask { entity_id, mask } => {
                use crate::scripting::host_commands::{AnyLight, with_lights};
                let eid = resolve_id(&spawn_remap, *entity_id);
                let val = *mask;
                if let Some(&entity) = entity_index.get(&eid) {
                    with_lights(world, entity, |l| match l {
                        AnyLight::Point(l) => l.light_mask = val,
                        AnyLight::Spot(l) => l.light_mask = val,
                        AnyLight::Sun(l) => l.light_mask = val,
                        #[cfg(feature = "freeform_light")]
                        AnyLight::Freeform(_) => {}
                    });
                }
            }
            ScriptCommand::SetLightHeight { entity_id, height } => {
                use crate::scripting::host_commands::{AnyLight, with_lights};
                let eid = resolve_id(&spawn_remap, *entity_id);
                let val = *height;
                if let Some(&entity) = entity_index.get(&eid) {
                    with_lights(world, entity, |l| match l {
                        AnyLight::Point(l) => l.height = val,
                        AnyLight::Spot(l) => l.height = val,
                        AnyLight::Sun(l) => l.height = val,
                        #[cfg(feature = "freeform_light")]
                        AnyLight::Freeform(_) => {}
                    });
                }
            }
            ScriptCommand::SetLightRadius { entity_id, radius } => {
                let eid = resolve_id(&spawn_remap, *entity_id);
                let val = *radius;
                if let Some(&entity) = entity_index.get(&eid)
                    && let Ok(mut light) = world.get::<&mut PointLight2D>(entity)
                {
                    light.radius = val;
                }
                if let Some(&entity) = entity_index.get(&eid)
                    && let Ok(mut light) = world.get::<&mut SpotLight2D>(entity)
                {
                    light.radius = val;
                }
            }
            ScriptCommand::SetToonLighting { bands, softness, dither } => {
                crate::scripting::host_commands::set_toon_lighting(world, *bands, *softness, *dither);
            }
            ScriptCommand::SetAmbientOcclusion { radius, strength } => {
                crate::scripting::host_commands::set_ambient_occlusion(world, *radius, *strength);
            }
            ScriptCommand::SetGlobalIllumination { strength, scale } => {
                crate::scripting::host_commands::set_global_illumination(world, *strength, *scale);
            }
            ScriptCommand::SetAmbientLight { r, g, b, intensity } => {
                let color = [*r, *g, *b];
                let val = *intensity;
                // Update first AmbientLight found, or create on first entity that has one
                let mut found = false;
                if let Some((_, ambient)) = world.query_mut::<&mut AmbientLight>().into_iter().next() {
                    ambient.color = color;
                    ambient.intensity = val;
                    found = true;
                }
                if !found {
                    // Spawn a dedicated ambient light entity
                    world.spawn((AmbientLight { color, intensity: val },));
                }
            }
        }
    }

    // P7.8: Process pending script loads — attach scripts to entities dynamically.
    if let Some(loads) = pending_script_loads {
        for (entity_id, path) in loads {
            if let Some(&entity) = entity_index.get(entity_id) {
                // Determine language from file extension
                let language = if path.ends_with(".lua") {
                    ScriptLanguage::Lua
                } else if path.ends_with(".js") {
                    ScriptLanguage::JavaScript
                } else {
                    log::warn!("[Script] Unknown script extension in path: {}", path);
                    continue;
                };

                // Create ScriptComponent and attach to entity.
                // The engine will load the source on the next frame.
                let script = ScriptComponent {
                    language,
                    source_path: path.clone(),
                    cull_distance: None,
                };

                let _ = world.insert_one(entity, script);
                log::info!("[Script] Dynamically loaded script '{}' on entity with id {}", path, entity_id);
            }
        }
    }

    // Despawn entities after processing all commands (avoids iterator invalidation)
    for entity in entities_to_destroy {
        let _ = world.despawn(entity);
        log::info!("[Script] Destroyed entity {:?}", entity);
    }
}

/// A frame of physics, begun: a body made for every new collider, kinematic
/// bodies moved to their transforms, character controllers moved by `dt`.
/// Then either `physics.step(dt)`, or - when scripts step with the physics -
/// `begin_frame`, and for each step the scripts' `fixed_update` followed by
/// [`physics_fixed_step`], then `end_frame`. Then [`physics_frame_end`].
/// Each host spelled the same sequence out until Oct 2026.
#[cfg(feature = "physics")]
pub fn physics_frame_begin(world: &mut World, physics: &mut crate::physics::PhysicsWorld, dt: f32) {
    ensure_physics_bodies(world, physics);
    physics.sync_kinematic_from_ecs(world);
    physics.move_characters(world, dt);
}

/// One fixed step after the scripts' `fixed_update`: a body their call
/// spawned is built first, so a force aimed at it this step reaches it, then
/// the forces are flushed for this step alone, the world stepped and the
/// transforms written back for the next call to read.
#[cfg(feature = "physics")]
pub fn physics_fixed_step(world: &mut World, physics: &mut crate::physics::PhysicsWorld, fixed_dt: f32) {
    ensure_physics_bodies(world, physics);
    physics.sync_kinematic_from_ecs(world);
    physics.flush_forces(fixed_dt);
    physics.step_once();
    physics.sync_transforms_to_ecs(world);
}

/// A frame of physics, ended: the stepped transforms written back, and what
/// this frame's contacts broke (`ecs::fracture`) broken, with its dust.
#[cfg(feature = "physics")]
pub fn physics_frame_end(
    world: &mut World,
    physics: &mut crate::physics::PhysicsWorld,
    contacts: &[crate::physics::ContactEvent],
    particles: &mut crate::particles::ParticleSystem,
    dt: f32,
) {
    physics.sync_transforms_to_ecs(world);
    #[cfg(not(feature = "fracture"))]
    let _ = (contacts, particles, dt);
    crate::feature!("fracture", {
        let breaks = crate::ecs::fracture::break_on_impact(world, physics, contacts, dt);
        crate::ecs::fracture::burst_debris(world, particles, &breaks);
    });
    // Save states asked for this frame (`ecs::save_state`): written down, or
    // put back before anything predicts from the world.
    crate::feature!("save_state", { crate::ecs::save_state::sync(world, physics) });
    // Shots seen before they are taken (`ecs::trajectory`), from this
    // frame's settled world.
    crate::feature!("trajectory", { crate::ecs::trajectory::predict(world, physics) });
}

/// Ensure all entities with RigidBody2D + Transform have physics bodies registered.
/// Entities with Collider2D also get colliders attached.
/// Call this each frame before stepping physics.
#[cfg(feature = "physics")]
pub fn ensure_physics_bodies(world: &hecs::World, physics: &mut crate::physics::PhysicsWorld) {
    let _span = crate::profiling::trace::span("physics.sync_bodies");
    use crate::ecs::components::{Collider2D, Joint2D, Name, RigidBody2D, Velocity2D};

    // The world may have been moved back since the last step
    // (`ecs::floating_origin`): the bodies follow before anything is synced.
    physics.follow_origin();
    // Drop bodies whose entity is gone first. Nothing that despawns an entity
    // tells physics, and hecs gives the freed index to the next spawn - so
    // this has to run before the `has_body` check below, or a new entity on a
    // reused index would be handed the dead one's body instead of its own.
    physics.remove_stale_bodies(world);
    // Tilemaps with collision layers get (or lose, or rebuild) their body.
    physics.sync_tilemap_colliders(world);
    physics.sync_carved_colliders(world);
    crate::feature!("spline_shape", { physics.sync_spline_colliders(world) });
    crate::feature!("smooth_terrain", { physics.sync_smooth_terrain(world) });
    physics.sync_area_effects(world);
    // Skeletons gone limp get bodies, stood-up ones lose them (`ecs::ragdoll`).
    crate::feature!("ragdoll", { physics.sync_ragdolls(world) });

    // A collider in the shape of its sprite (`SpriteCollider`) is built once
    // the outline is traced, and rebuilt when it is traced again.
    for (entity, sc) in world.query::<&crate::ecs::components::SpriteCollider>().iter() {
        let eid = entity.id() as u64;
        if sc.key != 0 && physics.has_collider(eid) && physics.sprite_shape_key(eid) != Some(sc.key) {
            physics.remove_collider(eid);
        }
    }

    // Register bodies for entities that have RigidBody2D + Transform but no physics body yet.
    for (entity, (rb, transform)) in world.query::<(&RigidBody2D, &Transform)>().iter() {
        let eid = entity.id() as u64;
        // A sprite-shaped body waits for its shape rather than falling as a
        // placeholder for a frame.
        let sprite_key = world.get::<&crate::ecs::components::SpriteCollider>(entity).ok().map(|s| s.key);
        if sprite_key == Some(0) {
            continue;
        }
        if let Some(k) = sprite_key
            && !physics.has_collider(eid)
            && world.get::<&Collider2D>(entity).is_ok()
        {
            physics.set_sprite_shape_key(eid, k);
        }
        if !physics.has_body(eid) {
            physics.add_body_for_entity(
                entity,
                rb,
                transform.position.x,
                transform.position.y,
                transform.rotation,
            );
            // Also add collider if present
            if let Ok(collider) = world.get::<&Collider2D>(entity) {
                physics.add_collider(eid, &collider);
            }
            // Seed initial linear velocity from Velocity2D if present. This
            // lets scene files specify a starting "kick" for physics-driven
            // entities (e.g. pendulum bobs). Without this, Velocity2D only
            // writes to Transform via velocity_system, which physics then
            // overwrites from the body's pose each step.
            if let Ok(vel) = world.get::<&Velocity2D>(entity) {
                physics.set_velocity(eid, vel.x, vel.y);
            }
            // A velocity or impulse a script gave it before it had a body -
            // a prefab fired in the frame it was spawned.
            physics.apply_pending(eid);
        } else {
            if !physics.has_collider(eid)
                && let Ok(collider) = world.get::<&Collider2D>(entity)
            {
                // A collider added after the body existed - a script attaching
                // one, an editor edit - used to be skipped for good, because
                // this branch only ran when the body was new.
                physics.add_collider(eid, &collider);
            }
            // An edited body type reaches the simulation too: the inspector
            // switching a crate from static to dynamic used to change the
            // component and nothing else.
            if physics.body_type_of(eid).is_some_and(|t| t != rb.body_type) {
                physics.set_body_type(eid, rb.body_type);
            }
        }
    }

    // Conveyors slide what rests on them (`Conveyor`).
    physics.sync_conveyors(world);
    physics.sync_body_settings(world);

    // Create joints after all bodies are registered. The name index is built
    // once per call and only when some joint still needs resolving; a joint
    // whose target does not exist used to scan every `Name` in the world
    // every frame, forever, and say nothing about it.
    thread_local! {
        static WARNED_JOINTS: std::cell::RefCell<std::collections::HashSet<u64>> =
            std::cell::RefCell::new(std::collections::HashSet::new());
    }
    let mut names: Option<std::collections::HashMap<String, u64>> = None;
    for (entity, joint) in world.query::<&Joint2D>().iter() {
        let eid_a = entity.id() as u64;
        if physics.has_joint(eid_a) {
            continue;
        }
        // A target joint naming nothing pulls toward its fixed point.
        if let crate::ecs::components::JointType::Target { target, .. } = &joint.joint_type
            && joint.target_entity.is_empty()
        {
            physics.add_target_joint(eid_a, joint, None, *target);
            continue;
        }
        let names = names.get_or_insert_with(|| {
            world.query::<&Name>().iter().map(|(e, n)| (n.0.clone(), e.id() as u64)).collect()
        });
        // Still retried every frame - a target that is respawned later gets
        // its joint back - but warned about once.
        match names.get(joint.target_entity.as_str()) {
            Some(&eid_b) => {
                physics.add_joint(eid_a, eid_b, joint);
                WARNED_JOINTS.with(|w| w.borrow_mut().remove(&eid_a));
            }
            None => {
                if WARNED_JOINTS.with(|w| w.borrow_mut().insert(eid_a)) {
                    log::warn!(
                        "joint on entity {eid_a} names target '{}', which no entity is called; the joint waits for one",
                        joint.target_entity
                    );
                }
            }
        }
    }

    // Target joints pull toward the entity they follow, wherever it now is.
    let follows = physics.target_follows();
    if !follows.is_empty() {
        let at: std::collections::HashMap<u64, [f32; 2]> = world
            .query::<(&Transform, Option<&crate::ecs::components::WorldTransform>)>()
            .iter()
            .filter(|(e, _)| follows.contains(&(e.id() as u64)))
            .map(|(e, (t, w))| {
                let p = w.map_or(t.position, |w| w.position);
                (e.id() as u64, [p.x, p.y])
            })
            .collect();
        physics.sync_joint_targets(|id| at.get(&id).copied());
    } else {
        physics.sync_joint_targets(|_| None);
    }
}

/// Change an entity's body type - the `RigidBody2D` component **and** the
/// simulation's body. `set_body_type` in every runtime goes through here.
///
/// Both, because `ensure_physics_bodies` makes the body match the component
/// every frame (so an inspector edit reaches the simulation): a script that
/// changed only the body was put back the next frame, and a kinematic ball
/// switched to dynamic hung in the air. Before that reconciliation existed
/// the opposite half was the bug - the viewport wrote only the component.
pub fn set_entity_body_type(
    world: &hecs::World,
    physics: &mut crate::physics::PhysicsWorld,
    entity: hecs::Entity,
    body_type: crate::ecs::components::BodyType,
) {
    if let Ok(mut rb) = world.get::<&mut crate::ecs::components::RigidBody2D>(entity) {
        rb.body_type = body_type;
    }
    physics.set_body_type(entity.id() as u64, body_type);
}

/// Apply physics-related script commands to the PhysicsWorld.
/// Called from the game loop with access to the PhysicsWorld.
pub fn apply_physics_commands(
    commands: &[ScriptCommand],
    physics: &mut crate::physics::PhysicsWorld,
    world: &hecs::World,
) {
    use crate::ecs::components::{BodyType, Joint2D, Name};

    // A `spawn_prefab` handle names the entity it was recorded for. One
    // spawned this frame is not recorded yet - natively these commands run
    // before the spawn is applied - and keeps its handle, which the physics
    // world's pending pushes are keyed by until the body is built. Without
    // this every physics command on a spawned entity went to a body id that
    // did not exist, so a fired cannonball dropped at the muzzle.
    let real = |id: u64| {
        if id < crate::scripting::spawn_aliases::FIRST_SPAWN_ID {
            id
        } else {
            crate::scripting::spawn_aliases::resolve(id).map_or(id, |e| e.id() as u64)
        }
    };
    for cmd in commands {
        match cmd {
            ScriptCommand::ApplyForce { entity_id, fx, fy } => {
                physics.apply_force(real(*entity_id), *fx, *fy);
            }
            ScriptCommand::ApplyImpulse { entity_id, ix, iy } => {
                physics.apply_impulse(real(*entity_id), *ix, *iy);
            }
            ScriptCommand::SetVelocity { entity_id, vx, vy } => {
                physics.set_velocity(real(*entity_id), *vx, *vy);
            }
            ScriptCommand::SetAngularVelocity { entity_id, omega } => {
                physics.set_angular_velocity(real(*entity_id), *omega);
            }
            ScriptCommand::SetGravity { gx, gy } => {
                physics.set_gravity(*gx, *gy);
            }
            ScriptCommand::SetBodyType { entity_id, body_type } => {
                // One parser for all three runtimes; they used to disagree on
                // capitalisation. See `BodyType::from_script_name`.
                let bt = BodyType::from_script_name(body_type).unwrap_or_else(|| {
                    log::warn!(
                        "set_body_type: unknown body type '{body_type}' - using Dynamic"
                    );
                    BodyType::Dynamic
                });
                match physics.owner_of(*entity_id) {
                    Some(e) => set_entity_body_type(world, physics, e, bt),
                    None => physics.set_body_type(*entity_id, bt),
                }
            }
            ScriptCommand::AddJoint { entity_name, target_name, joint_json } => {
                // Resolve entity names to IDs
                let mut entity_a_id = None;
                let mut entity_b_id = None;
                for (e, name) in world.query::<&Name>().iter() {
                    if name.0 == *entity_name {
                        entity_a_id = Some(e.id() as u64);
                    }
                    if name.0 == *target_name {
                        entity_b_id = Some(e.id() as u64);
                    }
                }
                if let (Some(a), Some(b)) = (entity_a_id, entity_b_id) {
                    match serde_json::from_str::<Joint2D>(joint_json) {
                        Ok(joint) => physics.add_joint(a, b, &joint),
                        Err(e) => log::warn!("add_joint: invalid joint JSON: {}", e),
                    }
                } else {
                    log::warn!("add_joint: entity '{}' or target '{}' not found", entity_name, target_name);
                }
            }
            ScriptCommand::RemoveJoint { entity_name } => {
                for (e, name) in world.query::<&Name>().iter() {
                    if name.0 == *entity_name {
                        physics.remove_joint(e.id() as u64);
                        break;
                    }
                }
            }
            ScriptCommand::SetMotorSpeed { entity_name, speed } => {
                for (e, name) in world.query::<&Name>().iter() {
                    if name.0 == *entity_name {
                        physics.set_motor_speed(e.id() as u64, *speed);
                        break;
                    }
                }
            }
            _ => {}
        }
    }
}

/// Apply audio-related script commands to the audio backend.
/// Also loads clips from the asset manager on demand.
///
/// Commands that are not audio are ignored, so a host can pass its whole
/// command list. Filtering first is how native lost four commands: the
/// runner kept its own list of "audio variants", which omitted
/// `stop_sound_by_clip` and the three `*_all_sounds`, so those did nothing in a
/// native build.
///
/// Volumes follow the mixer model (see [`crate::audio::mixer`]): a sound plays
/// at its own volume times its bus. Call [`crate::audio::sync_mixer`] once a
/// frame as well - bus changes and finished sounds are handled there, because
/// this only runs on frames that issue an audio command.
pub fn apply_audio_commands(
    commands: &[ScriptCommand],
    audio: &mut dyn crate::audio::AudioBackend,
    assets: &crate::asset::AssetManager,
    api: &mut crate::scripting::api::ScriptState,
) {
    use crate::audio::{SFX_BUS, SoundHandle};

    for cmd in commands {
        let services = &mut api.services;
        match cmd {
            ScriptCommand::PlaySound { clip_id, volume, pitch, pan, sound_id, looping } => {
                let clip = PlayClip { clip_id, volume: *volume, pitch: *pitch, pan: *pan, looping: *looping, filter: false };
                if let Some(h) = play_clip(audio, assets, &mut services.mixer, SFX_BUS, clip) {
                    services.last_sound_handle = h;
                    // The id the script holds now reaches this sound.
                    crate::audio::music::alias(*sound_id, h);
                }
            }
            // Music is the director's (`audio::music`), which the runner
            // drives once a frame: one path for every runtime.
            ScriptCommand::PlayMusic { clip_id, volume, looping } => {
                crate::audio::music::request(crate::audio::music::Request::Play {
                    layers: vec![(clip_id.clone(), *volume)],
                    looping: *looping,
                    fade: 0.0,
                });
            }
            ScriptCommand::StopSoundByClip { clip_id } => {
                if let Some(clip) = audio.get_clip_by_name(clip_id) {
                    let n = audio.stop_by_clip(clip);
                    log::debug!("stop_sound_by_clip('{clip_id}') stopped {n}");
                } else {
                    log::warn!("stop_sound_by_clip: clip '{clip_id}' is not loaded");
                }
                // The mixer forgets them at the next `sync_mixer`.
            }
            ScriptCommand::StopSound { handle_id } => {
                let h = crate::audio::music::resolve(*handle_id);
                audio.stop(SoundHandle(h));
                services.mixer.unregister_track(h);
            }
            ScriptCommand::StopMusic => crate::audio::music::stop(0.0),
            ScriptCommand::PauseMusic => crate::audio::music::request(crate::audio::music::Request::Pause),
            ScriptCommand::ResumeMusic => crate::audio::music::request(crate::audio::music::Request::Resume),
            // Both setters change the sound's own volume and still play it
            // through its bus. They used to write the raw value, so
            // `set_music_volume(0.6)` un-muted a muted music bus.
            ScriptCommand::SetSoundVolume { handle_id, volume } => {
                let h = crate::audio::music::resolve(*handle_id);
                if let Some(out) = services.mixer.set_track_volume(h, *volume) {
                    audio.set_volume(SoundHandle(h), out);
                }
            }
            ScriptCommand::SetMusicVolume { volume } => {
                crate::audio::music::request(crate::audio::music::Request::Volume(*volume));
            }
            ScriptCommand::SetMasterVolume { volume } => {
                audio.set_global_volume(*volume);
            }
            ScriptCommand::StopAllSounds => {
                audio.stop_all();
                // Every backend sound is gone, the music's included.
                crate::audio::music::stop(0.0);
                services.mixer.clear_tracks();
            }
            ScriptCommand::PauseAllSounds => {
                audio.pause_all();
            }
            ScriptCommand::ResumeAllSounds => {
                audio.resume_all();
            }
            _ => {}
        }
    }
}

/// Start the `AudioSource { auto_play }` sounds that
/// [`crate::ecs::behaviors::collect_autoplay_audio`] returned, each on its
/// `bus_id` (default `sfx`).
///
/// A looping source plays as its own looping sound. These used to become
/// `play_music` commands, which stopped whatever music was playing - an ambient
/// loop on a torch ended the level's soundtrack - and ignored `bus_id` entirely.
pub fn play_autoplay_audio(
    requests: &[crate::ecs::behaviors::AudioPlayRequest],
    audio: &mut dyn crate::audio::AudioBackend,
    assets: &crate::asset::AssetManager,
    mixer: &mut crate::audio::AudioMixer,
    spatial: &mut crate::audio::spatial::SpatialAudio,
) {
    for r in requests {
        let bus = mixer.route(r.bus_id.as_deref(), crate::audio::SFX_BUS);
        let clip = PlayClip {
            clip_id: &r.clip_id,
            volume: r.volume,
            pitch: r.pitch,
            pan: r.pan,
            looping: r.looping,
            filter: r.filter,
        };
        if let Some(h) = play_clip(audio, assets, mixer, &bus, clip)
            && let Some(old) = spatial.started(r.entity, h, &r.clip_id)
        {
            audio.stop(crate::audio::SoundHandle(old));
            mixer.unregister_track(old);
        }
    }
}

/// Positional audio's frame (`audio::spatial`): every spatial
/// `AudioSource`'s sound set to the volume and pan of where it is now, and
/// the sounds whose entity has gone stopped. The native host's half; the
/// browsers' is `WebAudioBackend::apply_spatial`.
pub fn apply_spatial_audio(
    world: &World,
    audio: &mut dyn crate::audio::AudioBackend,
    mixer: &mut crate::audio::AudioMixer,
    spatial: &mut crate::audio::spatial::SpatialAudio,
) {
    // Zones first: they need no positional sound to be playing.
    crate::audio::zones::apply(world, mixer);
    if spatial.is_empty() {
        return;
    }
    let frame = spatial.update(world, |h| audio.is_playing(crate::audio::SoundHandle(h)));
    for h in frame.stop {
        audio.stop(crate::audio::SoundHandle(h));
        mixer.unregister_track(h);
    }
    for m in frame.mix {
        if let Some(out) = mixer.set_track_volume(m.handle, m.volume) {
            audio.set_volume(crate::audio::SoundHandle(m.handle), out);
        }
        audio.set_pan(crate::audio::SoundHandle(m.handle), m.pan);
        audio.set_lowpass(crate::audio::SoundHandle(m.handle), m.cutoff);
    }
}

/// Play a one-shot on the SFX bus. What a frame trigger's `PlaySound`
/// needs: the runtimes tick animations themselves and have to play what the
/// frames asked for, and [`play_clip`] is private to this module.
pub fn play_sfx(
    audio: &mut dyn crate::audio::AudioBackend,
    assets: &crate::asset::AssetManager,
    mixer: &mut crate::audio::AudioMixer,
    clip_id: &str,
    volume: f32,
) {
    let clip = PlayClip { clip_id, volume, pitch: 1.0, pan: 0.0, looping: false, filter: false };
    let _ = play_clip(audio, assets, mixer, crate::audio::SFX_BUS, clip);
}

/// What to play, for [`play_clip`].
struct PlayClip<'a> {
    clip_id: &'a str,
    volume: f32,
    pitch: f32,
    pan: f32,
    looping: bool,
    /// A low-pass of its own (`PlaySettings::filter`).
    filter: bool,
}

/// Load `clip` on first use, play it on `bus`, and register it with the mixer.
/// Returns the sound handle, or `None` (after logging) if it could not play.
fn play_clip(
    audio: &mut dyn crate::audio::AudioBackend,
    assets: &crate::asset::AssetManager,
    mixer: &mut crate::audio::AudioMixer,
    bus: &str,
    clip: PlayClip<'_>,
) -> Option<u64> {
    let id = clip.clip_id;
    if audio.get_clip_by_name(id).is_none() {
        let Some(data) = assets.get_audio_data(id) else {
            log::warn!("Audio clip '{id}' not found in assets");
            return None;
        };
        if let Err(e) = audio.load_clip(id, data) {
            log::warn!("Failed to load audio clip '{id}': {e}");
            return None;
        }
    }
    let handle = audio.get_clip_by_name(id)?;
    let settings = crate::audio::PlaySettings {
        volume: mixer.output_volume(bus, clip.volume),
        pitch: clip.pitch,
        looping: clip.looping,
        pan: clip.pan,
        bus: bus.to_string(),
        filter: clip.filter,
    };
    match audio.play(handle, settings) {
        Ok(sound) => {
            mixer.register_track(sound.0, bus, clip.volume);
            Some(sound.0)
        }
        Err(e) => {
            // Once per clip, as the browser hosts say it: past kira's sound
            // capacity every play of a busy frame failed with its own line
            // (World Table's instant AI played ~100 moves in one frame).
            thread_local! {
                static WARNED: std::cell::RefCell<std::collections::HashSet<String>> = Default::default();
            }
            if WARNED.with(|w| w.borrow_mut().insert(id.to_string())) {
                log::warn!("Failed to play audio clip '{id}': {e} (further occurrences suppressed)");
            }
            None
        }
    }
}
