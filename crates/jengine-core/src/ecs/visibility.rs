//! Knowing when something is on screen (Oct 2026; Godot's
//! `VisibleOnScreenNotifier2D` / `VisibleOnScreenEnabler2D`, Unity's
//! `OnBecameVisible`).
//!
//! A [`VisibilityNotifier`] is a box about its entity - its sprite's quad
//! when `size` is left at zero - and [`tick_visibility`] says, once a frame,
//! which of them have come into the camera's view or left it. The host calls
//! the entity's own script for each: `on_screen_entered(entity)` and
//! `on_screen_exited(entity)`. With `pause_offscreen` the entity's sprite
//! animation, skeletal player and particle emitter also hold still while it
//! is out of view, and go on where they stopped when it comes back.
//!
//! Every host already had the view at hand for `despawn_offscreen`, so
//! [`tick_visibility`] is called beside it; the calls go through
//! `ecs::callbacks`, which every host drains.

use crate::detmath::DetMath;
use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::animation::SpriteAnimator;
use crate::animation::skeletal::SkeletalPlayer;
use crate::ecs::components::{SpriteComponent, Transform};
use crate::particles::ParticleEmitter;

/// `visibility_notifier`: tells its entity's script when it comes on screen
/// and when it leaves.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VisibilityNotifier {
    /// The box, in local units, centred on the entity (plus `offset`).
    /// Zero - the default - takes the sprite's quad, or a point when the
    /// entity has no sprite.
    #[serde(default)]
    pub size: [f32; 2],
    /// Where the box's centre sits from the entity's origin, in local units.
    #[serde(default)]
    pub offset: [f32; 2],
    /// World units added round the view: positive notices a thing a little
    /// before it is seen (so it can wake up in time), negative only once it
    /// is well inside.
    #[serde(default)]
    pub margin: f32,
    /// While off screen, hold the entity's sprite animation, skeletal
    /// player and particle emission (Godot's `VisibleOnScreenEnabler2D`).
    #[serde(default)]
    pub pause_offscreen: bool,
    /// Whether it was on screen last frame; `None` until first checked.
    #[serde(skip)]
    pub on_screen: Option<bool>,
    /// What `pause_offscreen` stopped, to start again: bit 1 the sprite
    /// animation, 2 the skeletal player, 4 the emitter.
    #[serde(skip)]
    pub held: u8,
}

/// One entity coming into view (`entered`) or leaving it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibilityChange {
    pub entity: Entity,
    pub entered: bool,
}

impl VisibilityChange {
    /// The script handler this change calls.
    pub fn handler(&self) -> &'static str {
        if self.entered { ENTERED_HANDLER } else { EXITED_HANDLER }
    }

    /// The event name a host dispatching `on_<name>` uses.
    pub fn event(&self) -> &'static str {
        &self.handler()["on_".len()..]
    }
}

/// The handler called when an entity comes on screen.
pub const ENTERED_HANDLER: &str = "on_screen_entered";
/// The handler called when an entity leaves the screen.
pub const EXITED_HANDLER: &str = "on_screen_exited";

/// The box's centre and half-size in world units.
fn world_box(world: &World, e: Entity, n: &VisibilityNotifier) -> (Vec2, Vec2) {
    let (pos, rot, scale) = match crate::ecs::hierarchy::world_transform_of(world, e) {
        Some(w) => (w.position, w.rotation, w.scale),
        None => match world.get::<&Transform>(e) {
            Ok(t) => (t.position, t.rotation, t.scale),
            Err(_) => (Vec2::ZERO, 0.0, Vec2::ONE),
        },
    };
    let (size, centre) = if n.size[0] > 0.0 || n.size[1] > 0.0 {
        (Vec2::from(n.size), Vec2::from(n.offset))
    } else if let Ok(s) = world.get::<&SpriteComponent>(e) {
        let size = Vec2::from(s.size.unwrap_or(SpriteComponent::FALLBACK_SIZE));
        // y is up and the anchor's y = 0 is the quad's foot.
        let centre = (Vec2::splat(0.5) - Vec2::from(s.anchor)) * size;
        (size, centre + Vec2::from(n.offset))
    } else {
        (Vec2::ZERO, Vec2::from(n.offset))
    };
    let (sin, cos) = rot.dsin_cos();
    let turn = |v: Vec2| Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
    let half = size * scale.abs() * 0.5;
    // The box turned with the entity, held in an upright box.
    let reach = Vec2::new(
        (half.x * cos).abs() + (half.y * sin).abs(),
        (half.x * sin).abs() + (half.y * cos).abs(),
    );
    (pos + turn(centre * scale), reach)
}

thread_local! {
    /// What `tick_visibility` last saw of the floating origin.
    static VISIBILITY_ORIGIN: std::cell::Cell<Option<(f64, f64)>> = const { std::cell::Cell::new(None) };
}

/// Check every [`VisibilityNotifier`] against the view - its centre and
/// half-extents, as `Camera2D::half_extents` gives them - and return what
/// changed since last frame - also owed to each entity's script through
/// `ecs::callbacks`. The first check of a notifier reports it only
/// if it starts on screen, so a scene's first frame says what is visible
/// and nothing about what is not.
pub fn tick_visibility(world: &mut World, view: (Vec2, Vec2)) -> Vec<VisibilityChange> {
    let _span = crate::profiling::trace::span("visibility");
    let notifiers: Vec<Entity> = world.query::<&VisibilityNotifier>().iter().map(|(e, _)| e).collect();
    let mut changes = Vec::new();
    let (centre, half) = view;
    // The renderer's camera follows a floating origin only later this
    // frame (`update_camera`): look where it is about to be, or a shift
    // would send everything off screen and back for a frame.
    let by = VISIBILITY_ORIGIN.with(|o| {
        let mut seen = o.get();
        let by = crate::ecs::floating_origin::follow(&mut seen);
        o.set(seen);
        by
    });
    let centre = centre - by;
    for e in notifiers {
        let Some(n) = world.get::<&VisibilityNotifier>(e).ok().map(|n| (*n).clone()) else { continue };
        let (at, reach) = world_box(world, e, &n);
        let room = half + reach + Vec2::splat(n.margin);
        let d = (at - centre).abs();
        let visible = d.x <= room.x && d.y <= room.y;
        if n.on_screen == Some(visible) {
            continue;
        }
        let first = n.on_screen.is_none();
        let held = if n.pause_offscreen { hold(world, e, visible, n.held) } else { 0 };
        if let Ok(mut n) = world.get::<&mut VisibilityNotifier>(e) {
            n.on_screen = Some(visible);
            n.held = held;
        }
        if !(first && !visible) {
            let change = VisibilityChange { entity: e, entered: visible };
            crate::ecs::callbacks::push(e, change.handler());
            changes.push(change);
        }
    }
    changes
}

/// Stop (`visible` false) or restart what `pause_offscreen` holds, returning
/// what is now held.
fn hold(world: &mut World, e: Entity, visible: bool, held: u8) -> u8 {
    if visible {
        if held & 1 != 0
            && let Ok(mut a) = world.get::<&mut SpriteAnimator>(e)
        {
            a.playing = true;
        }
        if held & 2 != 0
            && let Ok(mut p) = world.get::<&mut SkeletalPlayer>(e)
        {
            p.playing = true;
        }
        if held & 4 != 0
            && let Ok(mut p) = world.get::<&mut ParticleEmitter>(e)
        {
            p.active = true;
        }
        return 0;
    }
    let mut now = 0;
    if let Ok(mut a) = world.get::<&mut SpriteAnimator>(e)
        && a.playing
    {
        a.playing = false;
        now |= 1;
    }
    if let Ok(mut p) = world.get::<&mut SkeletalPlayer>(e)
        && p.playing
    {
        p.playing = false;
        now |= 2;
    }
    if let Ok(mut p) = world.get::<&mut ParticleEmitter>(e)
        && p.active
    {
        p.active = false;
        now |= 4;
    }
    now
}
