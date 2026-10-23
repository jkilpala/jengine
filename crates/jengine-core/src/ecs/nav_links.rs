//! **Navigation links** (Oct 2026): ways across that are not walkable
//! ground - a jump between rooftops, a ladder, a zipline, a door that is
//! sometimes open - joining two points of one region or two regions.
//!
//! A [`NavLink`] on an entity runs from the entity (plus `from`) to the world
//! point `to`, both ways unless `one_way`. When the world has any, an agent's
//! plan is [`plan`]: a search over the start, the goal and every link's ends,
//! the walking legs between them found by each region's own `find_path`, so
//! a path may hop from one region to another; with none, planning is what it
//! always was. An agent reaching a link's start crosses it in a straight line
//! over `duration` seconds (0: at its walking speed), ignoring the ground -
//! `navigation::tick_navigation` does the walking - and while it crosses its
//! custom `NavCrossing` fields say which link (`kind`) and how far (`t`, 0 to
//! 1), so a script can show a jump; `on_nav_link` reaches its script as it
//! sets off. `enabled` false takes a link out of every plan (a drawbridge
//! raised).

use std::sync::Arc;

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Transform, WorldTransform};
use crate::navigation::{NavGraph, graph_at};

/// A way across from this entity to `to`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NavLink {
    /// Where it starts, world units from the entity.
    pub from: [f32; 2],
    /// Where it ends, a world point.
    pub to: [f32; 2],
    /// Only from `from` to `to`.
    pub one_way: bool,
    /// How dear it is: its length is multiplied by this in a plan.
    pub cost: f32,
    /// Seconds to cross (0: at the agent's walking speed).
    pub duration: f32,
    /// What it is, for the script (`"jump"`, `"ladder"`).
    pub kind: String,
    /// Off, no plan uses it.
    pub enabled: bool,
}

impl Default for NavLink {
    fn default() -> Self {
        Self { from: [0.0, 0.0], to: [0.0, 0.0], one_way: false, cost: 1.0, duration: 0.0, kind: "link".into(), enabled: true }
    }
}

/// A link as a plan sees it: its ends in world space.
#[derive(Debug, Clone)]
pub struct Link {
    pub a: Vec2,
    pub b: Vec2,
    pub one_way: bool,
    pub cost: f32,
    pub duration: f32,
    pub kind: String,
}

/// Every enabled link in the world.
pub fn links(world: &World) -> Vec<Link> {
    world
        .query::<(&NavLink, &Transform, Option<&WorldTransform>)>()
        .iter()
        .filter(|(_, (l, _, _))| l.enabled)
        .map(|(_, (l, t, w))| {
            let at = w.map_or(t.position, |w| w.position);
            Link {
                a: at + Vec2::from(l.from),
                b: Vec2::from(l.to),
                one_way: l.one_way,
                cost: l.cost.max(0.0),
                duration: l.duration.max(0.0),
                kind: l.kind.clone(),
            }
        })
        .collect()
}

/// A plan's way to each corner: walked, or across link `i`.
pub const WALK: u32 = u32::MAX;

/// A link an agent is crossing: from where to where, how far (0 to 1), in
/// how long (0: at its walking speed), and what kind of link.
#[derive(Debug, Clone, Default)]
pub struct Crossing {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub t: f32,
    pub duration: f32,
    pub kind: String,
}

/// What the links are, as a number: part of a plan's key, so moving,
/// adding or switching off a link replans every agent.
pub fn key(links: &[Link]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut put = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    put(links.len() as u64);
    for l in links {
        for v in [l.a.x, l.a.y, l.b.x, l.b.y, l.cost, l.duration] {
            put(v.to_bits() as u64);
        }
        put(l.one_way as u64);
    }
    h
}

/// An agent's custom `NavCrossing` fields: the link's `kind` and how far
/// across (`t`) while it crosses, an empty kind once it has landed.
pub fn publish(world: &mut World, e: Entity, now: Option<(String, f32)>) {
    use crate::ecs::components::CustomComponents;
    let value = match now {
        Some((kind, t)) => serde_json::json!({ "kind": kind, "t": t }),
        None => serde_json::json!({ "kind": "", "t": 0.0 }),
    };
    let missing = match world.get::<&mut CustomComponents>(e) {
        Ok(mut c) => {
            c.0.insert("NavCrossing".into(), value);
            None
        }
        Err(_) => Some(value),
    };
    if let Some(value) = missing {
        let mut map = std::collections::HashMap::new();
        map.insert("NavCrossing".into(), value);
        let _ = world.insert_one(e, CustomComponents(map));
    }
}

fn length(path: &[Vec2]) -> f32 {
    path.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// The way from `from` to `to` over the regions' graphs and the links: its
/// corners after the start, and how each is reached ([`WALK`], or the index
/// of the link crossed to it). With no way across, the start region's own
/// nearest reach of `to`, as a region without links has always answered;
/// `None` when not even that.
pub fn plan(graphs: &[Arc<NavGraph>], links: &[Link], from: Vec2, to: Vec2) -> Option<(Vec<Vec2>, Vec<u32>)> {
    // The search's points: the start, the goal, then each link's two ends.
    let mut pts = vec![from, to];
    for l in links {
        pts.push(l.a);
        pts.push(l.b);
    }
    let n = pts.len();
    // A link's ends sit at a region's edge, often just past the area its
    // agents' radius leaves walkable: the nearest region is theirs, and
    // `find_path` snaps them in.
    let region = |p: Vec2| graph_at(graphs, p);
    // The walking leg between two points, when they share a region.
    let walk = |i: usize, j: usize| -> Option<Vec<Vec2>> {
        let (gi, gj) = (region(pts[i])?, region(pts[j])?);
        if !Arc::ptr_eq(gi, gj) {
            return None;
        }
        gi.find_path(pts[i], pts[j])
    };
    let mut cost = vec![f32::MAX; n];
    let mut came: Vec<Option<(usize, Option<usize>)>> = vec![None; n];
    let mut done = vec![false; n];
    cost[0] = 0.0;
    loop {
        let Some(i) = (0..n).filter(|&i| !done[i] && cost[i] < f32::MAX).min_by(|&a, &b| cost[a].total_cmp(&cost[b])) else { break };
        if i == 1 {
            break;
        }
        done[i] = true;
        // Walk to every other point in the same region.
        for j in 0..n {
            if j == i || done[j] {
                continue;
            }
            if let Some(path) = walk(i, j) {
                let c = cost[i] + length(&path);
                if c < cost[j] {
                    cost[j] = c;
                    came[j] = Some((i, None));
                }
            }
        }
        // Cross a link that starts here.
        if i >= 2 {
            let k = (i - 2) / 2;
            let l = &links[k];
            let at_a = (i - 2) % 2 == 0;
            if at_a || !l.one_way {
                let j = if at_a { i + 1 } else { i - 1 };
                let c = cost[i] + l.a.distance(l.b) * l.cost;
                if !done[j] && c < cost[j] {
                    cost[j] = c;
                    came[j] = Some((i, Some(k)));
                }
            }
        }
    }
    if came[1].is_none() {
        // No way there across links: what a region does alone - walk to the
        // nearest point of its own it can reach.
        let path = graph_at(graphs, from)?.find_path(from, to)?;
        let corners: Vec<Vec2> = path.into_iter().skip(1).collect();
        let ways = vec![WALK; corners.len()];
        return Some((corners, ways));
    }
    // Back from the goal: each step a walk (its corners) or a crossing.
    let mut steps: Vec<(usize, usize, Option<usize>)> = Vec::new();
    let mut j = 1;
    while j != 0 {
        let (i, link) = came[j]?;
        steps.push((i, j, link));
        j = i;
    }
    steps.reverse();
    let (mut corners, mut ways) = (Vec::new(), Vec::new());
    for (i, j, link) in steps {
        match link {
            Some(k) => {
                corners.push(pts[j]);
                ways.push(k as u32);
            }
            None => {
                let path = walk(i, j)?;
                for p in path.into_iter().skip(1) {
                    corners.push(p);
                    ways.push(WALK);
                }
            }
        }
    }
    Some((corners, ways))
}

/// `NavLink.set(e, field, value)`: any field by name.
pub fn set_field(world: &mut World, e: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut l = world.get::<&mut NavLink>(e).map_err(|_| "no nav_link on that entity".to_string())?;
    let mut v = serde_json::to_value(&*l).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("not an object")?;
    if !obj.contains_key(field) {
        return Err(format!("nav_link has no field '{field}'"));
    }
    obj.insert(field.to_string(), value.clone());
    *l = serde_json::from_value(v).map_err(|e| format!("nav_link.{field}: {e}"))?;
    Ok(())
}
