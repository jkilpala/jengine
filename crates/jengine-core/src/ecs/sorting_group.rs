//! Sorting groups (Oct 2026; Unity's `SortingGroup`, Godot's
//! `CanvasGroup` ordering): a parent whose whole hierarchy sorts as **one**
//! drawable.
//!
//! A character made of parts - a body, a head, an arm in front and an arm
//! behind, a shadow - orders those parts among themselves by their own
//! `z_order`, and that is right inside the character. Among *other*
//! characters it is wrong: with one z band for everyone, the arm in front of
//! one character draws in front of the next character's body. A
//! `sorting_group` on the character's root sorts everything under it at the
//! group's `z_order` (and, with `y_sort`, by where the root stands), the
//! parts in their own order inside it - so each character is a card in the
//! deck rather than a handful of loose ones.
//!
//! The sprite pass reads it through [`SortGroups`]: a drawable under a
//! group sorts as if it were one sprite at the group's z and y, then by its
//! own key. Everything not under a group sorts exactly as before. A group
//! inside a group is part of the outer one: the outermost decides.

use std::collections::HashMap;

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Parent, Transform, WorldTransform};

/// `sorting_group`: everything under this entity sorts as one drawable.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SortingGroup {
    /// Where the whole group sorts, as a sprite's `z_order`.
    #[serde(default)]
    pub z_order: i32,
    /// A named sort layer, as on a sprite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_layer: Option<String>,
    /// Sort the group against the other y-sorted things of its z by where
    /// its root stands - lower on screen in front - as a sprite's `y_sort`.
    #[serde(default)]
    pub y_sort: bool,
}

/// Where a drawable's group puts it: `id` 0 for no group.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SortGroup {
    /// The group's entity id plus one; 0 when the drawable is in none.
    pub id: u32,
    pub z: i32,
    /// The group's y-sort key, as `SpriteComponent::y_sort_key` gives one.
    pub y: i32,
}

impl SortGroup {
    pub const NONE: SortGroup = SortGroup { id: 0, z: 0, y: 0 };
}

/// The y-sort key of a world height - `SpriteComponent::y_sort_key`'s rule
/// for a sprite that opted in: lower draws later, in sixteenths of a unit.
pub fn y_key(world_y: f32) -> i32 {
    if world_y.is_nan() {
        return i32::MAX;
    }
    (-world_y * 16.0).clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

/// The frame's sorting groups, gathered once per pass.
#[derive(Debug, Default)]
pub struct SortGroups {
    groups: HashMap<Entity, SortGroup>,
}

impl SortGroups {
    /// Every `sorting_group` in the world, its z resolved by `resolve` (the
    /// pass's sort layers: `(layer, z_order) -> z`).
    pub fn gather(world: &World, resolve: impl Fn(Option<&str>, i32) -> i32) -> Self {
        let mut groups = HashMap::new();
        for (e, (g, t, w)) in world.query::<(&SortingGroup, &Transform, Option<&WorldTransform>)>().iter() {
            let y = w.map_or(t.position.y, |w| w.position.y);
            groups.insert(e, SortGroup {
                id: e.id() + 1,
                z: resolve(g.sort_layer.as_deref(), g.z_order),
                y: if g.y_sort { y_key(y) } else { 0 },
            });
        }
        Self { groups }
    }

    /// Whether there are any - with none, every drawable is ungrouped and
    /// [`Self::of`] costs nothing.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// The group `entity` draws in: its outermost ancestor (or itself)
    /// carrying a `sorting_group`.
    pub fn of(&self, world: &World, entity: Entity) -> SortGroup {
        if self.groups.is_empty() {
            return SortGroup::NONE;
        }
        let mut found = self.groups.get(&entity).copied();
        let mut e = entity;
        for _ in 0..64 {
            let Ok(p) = world.get::<&Parent>(e).map(|p| p.0) else { break };
            if let Some(g) = self.groups.get(&p) {
                found = Some(*g);
            }
            e = p;
        }
        found.unwrap_or(SortGroup::NONE)
    }
}

/// The order key of a drawable whose own key is `(z, kind, y)`, in group
/// `g`: outside a group, its own key; inside one, the group's place - as a
/// sprite (kind 1) at the group's z and y - then the group, so its members
/// stay together, then their own keys break the tie.
pub fn outer_key(own: (i32, u8, i32), g: SortGroup) -> (i32, u8, i32, u32) {
    if g.id == 0 { (own.0, own.1, own.2, 0) } else { (g.z, 1, g.y, g.id) }
}
