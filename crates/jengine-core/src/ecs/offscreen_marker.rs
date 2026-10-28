//! **Off-screen markers** (Oct 2026): a marker kept at the edge of the
//! screen pointing at something out of sight - a beacon, an objective, a
//! friend - that turns into an icon over it once it is in view.
//!
//! An [`OffscreenMarker`] on an entity with a sprite names its `target` by
//! entity name. Each frame ([`tick_markers`], in `tick_motion`) the marker
//! is placed from the main camera's view:
//!
//! - the target **in view** (inside the view less `margin`): the marker sits
//!   at the target plus `icon_offset`, upright, showing `icon_texture`;
//! - **out of view**: the marker slides to where the line from the view's
//!   middle to the target leaves the view less `margin`, turned to point at
//!   it (its picture pointing along +x), showing `arrow_texture`.
//!
//! A `label` names a text entity kept just inside the arrow, upright, saying
//! how far the target is (`units_per_meter`, `{d}` in `label_format`). The
//! view is the main camera's as it was last drawn (`note_main_view`, called
//! by `frame_draw` for the screen's camera), so an edge marker is one frame
//! behind a moving camera - well inside its margin - and no host was wired.
//! Whether the target is on screen is the custom `OffscreenMarker.visible`,
//! for a script.

use glam::{Mat2, Vec2};
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{CustomComponents, Name, SpriteComponent, TextComponent, Transform, WorldTransform};

/// A marker that points at its target from the screen's edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OffscreenMarker {
    /// The target's entity name.
    pub target: String,
    /// How far inside the view's edge the marker stays, world units.
    pub margin: f32,
    /// What the marker shows at the edge (pointing along +x); empty keeps
    /// the sprite's texture.
    pub arrow_texture: String,
    /// What it shows over the target in view; empty keeps the arrow.
    pub icon_texture: String,
    /// Where the icon sits from the target.
    pub icon_offset: [f32; 2],
    /// A text entity (by name) that says how far the target is.
    pub label: String,
    /// The label's words, `{d}` the distance.
    pub label_format: String,
    /// World units in a metre, for the label.
    pub units_per_meter: f32,
    /// How far inside the arrow the label sits.
    pub label_inset: f32,
    /// Off, the marker is left where it is.
    pub enabled: bool,
}

impl Default for OffscreenMarker {
    fn default() -> Self {
        Self {
            target: String::new(),
            margin: 48.0,
            arrow_texture: String::new(),
            icon_texture: String::new(),
            icon_offset: [0.0, 60.0],
            label: String::new(),
            label_format: "{d} m".into(),
            units_per_meter: 100.0,
            label_inset: 46.0,
            enabled: true,
        }
    }
}


pub use super::views::{MainView, main_view, note_main_view, set_main_view};

/// Where a marker goes for a target at `p` in `view`: its position, its
/// turn, and whether the target is in view.
pub fn place(view: &MainView, margin: f32, icon_offset: Vec2, p: Vec2) -> (Vec2, f32, bool) {
    let d = p - view.center;
    let local = Vec2::new(d.dot(view.axis_x), d.dot(view.axis_y));
    let inner = (view.half - Vec2::splat(margin)).max(Vec2::splat(1.0));
    if local.x.abs() <= inner.x && local.y.abs() <= inner.y {
        return (p + icon_offset, 0.0, true);
    }
    // Where the line from the middle leaves the inner rectangle.
    let tx = if local.x.abs() > 1e-6 { inner.x / local.x.abs() } else { f32::MAX };
    let ty = if local.y.abs() > 1e-6 { inner.y / local.y.abs() } else { f32::MAX };
    let edge = local * tx.min(ty);
    let at = view.center + view.axis_x * edge.x + view.axis_y * edge.y;
    let turn = d.y.atan2(d.x);
    (at, turn, false)
}

/// Place every marker. Called from `tick_motion`.
pub fn tick_markers(world: &mut World) {
    let Some(view) = main_view() else { return };
    let markers: Vec<(Entity, OffscreenMarker)> =
        world.query::<&OffscreenMarker>().iter().filter(|(_, m)| m.enabled && !m.target.is_empty()).map(|(e, m)| (e, m.clone())).collect();
    if markers.is_empty() {
        return;
    }
    let position_of = |world: &World, e: Entity| {
        world.get::<&WorldTransform>(e).map(|w| w.position).or_else(|_| world.get::<&Transform>(e).map(|t| t.position)).ok()
    };
    let find = |world: &World, name: &str| world.query::<&Name>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e);
    for (e, m) in markers {
        let Some(target) = find(world, &m.target) else { continue };
        let Some(p) = position_of(world, target) else { continue };
        let (at, turn, visible) = place(&view, m.margin, Vec2::from(m.icon_offset), p);
        if let Ok(mut t) = world.get::<&mut Transform>(e) {
            t.position = at;
            t.rotation = turn;
        }
        let texture = if visible && !m.icon_texture.is_empty() { &m.icon_texture } else { &m.arrow_texture };
        if !texture.is_empty()
            && let Ok(mut s) = world.get::<&mut SpriteComponent>(e)
            && s.texture_id.as_str() != texture.as_str()
        {
            s.texture_id = texture.as_str().into();
        }
        // What a script reads.
        let custom = serde_json::json!({ "visible": visible });
        let custom = match world.get::<&mut CustomComponents>(e) {
            Ok(mut c) => {
                c.0.insert("OffscreenMarker".into(), custom);
                None
            }
            Err(_) => Some(custom),
        };
        if let Some(custom) = custom {
            let mut map = std::collections::HashMap::new();
            map.insert("OffscreenMarker".into(), custom);
            let _ = world.insert_one(e, CustomComponents(map));
        }
        if m.label.is_empty() {
            continue;
        }
        let Some(label) = find(world, &m.label) else { continue };
        let dist = (p - view.center).length() / m.units_per_meter.max(1e-3);
        let words = m.label_format.replace("{d}", &format!("{}", dist.round() as i64));
        let spot = if visible { at + Vec2::new(0.0, -m.label_inset) } else { at - Vec2::from_angle(turn) * m.label_inset };
        if let Ok(mut t) = world.get::<&mut Transform>(label) {
            t.position = spot;
            t.rotation = 0.0;
        }
        if let Ok(mut text) = world.get::<&mut TextComponent>(label)
            && text.text != words
        {
            text.text = words;
        }
    }
}

/// `Marker.set(e, field, value)`: any field by name.
pub fn set_field(world: &mut World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut m = world.get::<&mut OffscreenMarker>(entity).map_err(|_| "no offscreen_marker on that entity".to_string())?;
    let mut v = serde_json::to_value(&*m).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("not an object")?;
    if !obj.contains_key(field) {
        return Err(format!("offscreen_marker has no field '{field}'"));
    }
    obj.insert(field.to_string(), value.clone());
    *m = serde_json::from_value(v).map_err(|e| format!("offscreen_marker.{field}: {e}"))?;
    Ok(())
}

/// The turn's matrix, for tests reading where an arrow points.
pub fn pointing(turn: f32) -> Vec2 {
    Mat2::from_angle(turn) * Vec2::X
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> MainView {
        // A 1280 x 720 view centred at (100, 50), upright.
        MainView { center: Vec2::new(100.0, 50.0), axis_x: Vec2::X, axis_y: Vec2::Y, half: Vec2::new(640.0, 360.0) }
    }

    #[test]
    fn in_view_it_sits_over_the_target() {
        let (at, turn, visible) = place(&view(), 48.0, Vec2::new(0.0, 60.0), Vec2::new(300.0, 100.0));
        assert!(visible && turn == 0.0);
        assert_eq!(at, Vec2::new(300.0, 160.0));
    }

    #[test]
    fn out_of_view_it_waits_at_the_edge_pointing_at_it() {
        // Far to the right: on the right edge less the margin, level with
        // the middle's line to it, pointing right.
        let (at, turn, visible) = place(&view(), 48.0, Vec2::ZERO, Vec2::new(5100.0, 50.0));
        assert!(!visible);
        assert!((at - Vec2::new(100.0 + 592.0, 50.0)).length() < 1e-3, "{at}");
        assert!(pointing(turn).distance(Vec2::X) < 1e-4);
        // Up and to the left, steeply: it meets the top edge first.
        let (at, turn, _) = place(&view(), 48.0, Vec2::ZERO, Vec2::new(-300.0, 2050.0));
        assert!((at.y - (50.0 + 312.0)).abs() < 1e-3 && at.x < 100.0 && at.x > 100.0 - 592.0, "{at}");
        let dir = pointing(turn);
        assert!(dir.y > 0.9 && dir.x < 0.0, "pointing up and a little left: {dir}");
    }

    #[test]
    fn a_turned_camera_keeps_it_inside_its_own_edges() {
        let c = [Vec2::new(-1.0, 1.0), Vec2::new(1.0, 1.0), Vec2::new(1.0, -1.0), Vec2::new(-1.0, -1.0)].map(|v| Mat2::from_angle(0.5) * (v * Vec2::new(640.0, 360.0)));
        let v = MainView::of_corners(c);
        let (at, _, visible) = place(&v, 48.0, Vec2::ZERO, Vec2::new(0.0, 4000.0));
        assert!(!visible);
        let d = at - v.center;
        let local = Vec2::new(d.dot(v.axis_x), d.dot(v.axis_y));
        assert!(local.x.abs() <= 592.0 + 1e-2 && local.y.abs() <= 312.0 + 1e-2, "on its own inner edge: {local}");
        assert!((local.x.abs() - 592.0).abs() < 1e-2 || (local.y.abs() - 312.0).abs() < 1e-2);
    }

    #[test]
    fn a_marker_follows_its_target_and_labels_the_distance() {
        let mut world = World::new();
        world.spawn((Name("Beacon".into()), Transform { position: Vec2::new(2100.0, 50.0), ..Default::default() }));
        let label = world.spawn((Name("BeaconLabel".into()), Transform::default(), TextComponent::default()));
        let m = world.spawn((
            Transform::default(),
            SpriteComponent { texture_id: "arrow.png".into(), ..Default::default() },
            OffscreenMarker { target: "Beacon".into(), icon_texture: "flag.png".into(), arrow_texture: "arrow.png".into(), label: "BeaconLabel".into(), ..Default::default() },
        ));
        set_main_view(Some(view()));
        tick_markers(&mut world);
        assert_eq!(world.get::<&TextComponent>(label).unwrap().text, "20 m");
        assert_eq!(world.get::<&SpriteComponent>(m).unwrap().texture_id.as_str(), "arrow.png");
        let t = world.get::<&Transform>(label).unwrap().position;
        assert!(t.x < 692.0 && (t.y - 50.0).abs() < 1e-3, "inside the arrow: {t}");
        // It comes into view.
        world.query_mut::<(&Name, &mut Transform)>().into_iter().find(|(_, (n, _))| n.0 == "Beacon").unwrap().1.1.position = Vec2::new(400.0, 0.0);
        tick_markers(&mut world);
        assert_eq!(world.get::<&SpriteComponent>(m).unwrap().texture_id.as_str(), "flag.png");
        assert_eq!(world.get::<&Transform>(m).unwrap().position, Vec2::new(400.0, 60.0));
        set_main_view(None);
    }
}
