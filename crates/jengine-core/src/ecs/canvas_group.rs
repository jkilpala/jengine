//! Canvas groups (Oct 2026; Godot's `CanvasGroup`, Unity's `CanvasGroup`
//! alpha): a hierarchy faded, tinted and sorted as **one picture**.
//!
//! Fading a character made of parts by fading each part shows the parts
//! through each other - at half opacity the arm behind the body shows
//! through the body, and every overlap is darker than the rest. A
//! [`CanvasGroup`] draws its whole hierarchy into a texture of its own first
//! and then draws that texture once, at the group's `z_order`, with its
//! `opacity` and `tint` - so the group fades as a picture of itself fades.
//!
//! It rides on views (`ecs::views`): each group is a view through the main
//! camera that draws only the group's members ([`canvas_group_views`],
//! which every host appends to its views), into a transparent texture; the
//! main camera skips the members (`wgpu_systems::in_view_layers`) and the
//! sprite pass draws the texture as one quad over the camera's view
//! (`wgpu_systems` pushes it). So the members are lit, shadowed and
//! post-processed by the same world pass as everything else, and a host
//! needs only to add the group views and clear them to transparent.
//!
//! The members are drawn *in* the group, the group is drawn in the world:
//! a member's own `z_order` orders it inside the group, and the group sorts
//! among everything else by its own. A group inside a group belongs to the
//! outer one. Each group costs a world pass a frame (only its members are
//! drawn, but the lights and the post chain run), so groups are for the
//! things that fade - a ghost, a dissolving character, a fading UI-in-world
//! panel - not for organising a scene.

use std::cell::RefCell;
use std::collections::HashMap;

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::Parent;
use crate::renderer::camera::{Camera2D, CameraPass};

fn default_opacity() -> f32 {
    1.0
}
fn default_tint() -> [f32; 4] {
    [1.0; 4]
}

/// `canvas_group`: this entity and everything under it drawn as one picture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasGroup {
    /// The whole picture's opacity: 0 hides it (and skips drawing it).
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// Multiplies the picture (linear colour; alpha multiplies `opacity`).
    #[serde(default = "default_tint")]
    pub tint: [f32; 4],
    /// Where the picture sorts among everything else.
    #[serde(default)]
    pub z_order: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_layer: Option<String>,
    /// Above 0, the picture is cut where its alpha crosses this (0..1), and
    /// drawn in its own colour, un-premultiplied: soft blobs drawn into the
    /// group merge into one hard-edged surface - a liquid of particles, a
    /// slime, metaballs. 0 draws the picture as it is.
    #[serde(default)]
    pub threshold: f32,
    /// How soft that edge is, in alpha (on top of a screen pixel).
    #[serde(default)]
    pub threshold_soft: f32,
}

impl Default for CanvasGroup {
    fn default() -> Self {
        Self { opacity: 1.0, tint: [1.0; 4], z_order: 0, sort_layer: None, threshold: 0.0, threshold_soft: 0.0 }
    }
}

/// The texture a group draws into: a name nothing else uses.
pub fn target_id(group: Entity) -> String {
    format!("__canvas_group/{}", group.id())
}

thread_local! {
    /// This frame's members: entity -> its outermost group's id plus one.
    static MEMBERS: RefCell<HashMap<Entity, u32>> = RefCell::new(HashMap::new());
}

/// The group `entity` is drawn in this frame (its outermost ancestor - or
/// itself - with a `canvas_group`), as the id plus one; 0 for none. Read
/// from what [`canvas_group_views`] gathered.
pub fn member_of(entity: Entity) -> u32 {
    MEMBERS.with(|m| {
        let m = m.borrow();
        if m.is_empty() { 0 } else { m.get(&entity).copied().unwrap_or(0) }
    })
}

/// The group's id as [`member_of`] reports it.
pub fn group_key(group: Entity) -> u32 {
    group.id() + 1
}

fn outermost_group(world: &World, mut e: Entity) -> Option<Entity> {
    let mut found = world.get::<&CanvasGroup>(e).is_ok().then_some(e);
    for _ in 0..64 {
        let Ok(p) = world.get::<&Parent>(e).map(|p| p.0) else { break };
        if world.get::<&CanvasGroup>(p).is_ok() {
            found = Some(p);
        }
        e = p;
    }
    found
}

/// Gather this frame's groups: who belongs to which (for
/// `wgpu_systems::in_view_layers`), and a view per visible group through
/// `main` - the main camera - drawing into its texture, cleared to
/// transparent. Every host appends these to its views each frame.
pub fn canvas_group_views(world: &World, main: &Camera2D, surface: (u32, u32)) -> Vec<crate::ecs::views::ViewCamera> {
    let groups: Vec<Entity> = world.query::<&CanvasGroup>().iter().map(|(e, _)| e).collect();
    let mut members = HashMap::new();
    if !groups.is_empty() {
        // Everything under a group: walk down from each entity's parents
        // once (a world is flat lists, not trees, so this walks up).
        for (e, _) in world.query::<()>().iter() {
            if let Some(g) = outermost_group(world, e) {
                members.insert(e, group_key(g));
            }
        }
    }
    MEMBERS.with(|m| *m.borrow_mut() = members);
    let mut views: Vec<crate::ecs::views::ViewCamera> = groups
        .into_iter()
        .filter(|g| {
            // Only the outermost groups draw; an inner one is part of its outer.
            outermost_group(world, *g) == Some(*g)
                && world.get::<&CanvasGroup>(*g).is_ok_and(|c| c.opacity > 0.0)
        })
        .map(|g| {
            let mut camera = main.clone();
            camera.pass = CameraPass::CanvasGroup(group_key(g));
            crate::ecs::views::ViewCamera {
                target: target_id(g),
                size: (surface.0.max(1), surface.1.max(1)),
                camera,
                transparent: true,
            }
        })
        .collect();
    views.sort_by(|a, b| a.target.cmp(&b.target));
    views
}

/// Forget this frame's members (a test or a tool drawing without a host).
pub fn clear_members() {
    MEMBERS.with(|m| m.borrow_mut().clear());
}

/// `CanvasGroup.set(e, field, value)`: write any field by name - `opacity`
/// to fade the group, `tint` to colour it.
pub fn set_canvas_group_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let Ok(mut c) = world.get::<&mut CanvasGroup>(entity) else {
        return Err("no canvas_group on that entity".into());
    };
    let mut whole = serde_json::to_value(&*c).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) && field != "sort_layer" {
        return Err(format!("'{field}' is not a canvas_group field"));
    }
    let value = match (field, value) {
        ("z_order", serde_json::Value::Number(n)) => serde_json::Value::from(n.as_f64().unwrap_or(0.0) as i64),
        _ => value.clone(),
    };
    map.insert(field.to_string(), value);
    *c = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    Ok(())
}
