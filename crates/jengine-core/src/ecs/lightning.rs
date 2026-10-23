//! **Lightning** (Oct 2026): a branching bolt made afresh at every strike,
//! that flashes, flickers, lights the scene and is gone - a storm over the
//! sea, a wizard's chain lightning, a tesla coil arcing.
//!
//! A [`Lightning`] on an entity strikes from the entity (plus `start`) to a
//! point: `Lightning.strike(e, x, y)`, or by itself every `interval` seconds
//! somewhere within `spread` of `end`. Each strike's shape is grown from a
//! seed ([`bolt`]): the line cut in half again and again, each middle pushed
//! sideways by `jag`, with `branches` forks that do the same, thinner - so
//! no two strikes match, and every runtime draws the same one. It lives for
//! `duration` seconds, flickering `restrikes` times as it fades
//! ([`Lightning::brightness`]), and while it does it lights the world: point
//! lights along the bolt and a lift in the ambient light (`flash`), both
//! read by `gather_lights`, so the GPU and `light_at` agree. Drawn as soft
//! additive ribbons ([`mesh`]), a glow and a core, by `render_world_items`;
//! ticked in `tick_motion`; `on_strike` reaches the entity's script through
//! `ecs::callbacks`, and the custom `Lightning` fields (`strikes`, `x`, `y`)
//! say where the last one landed - so no host was wired.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{CustomComponents, Transform, WorldTransform};

/// A bolt's strike: where it is drawn and how bright it is now.
#[derive(Debug, Clone, Default)]
pub struct Strike {
    /// The branches, the main channel first: points from root to tip, and
    /// each one's width as a share of `width`.
    pub branches: Vec<(Vec<Vec2>, f32)>,
    /// Seconds since it struck.
    pub age: f32,
    /// The seed it was grown from.
    pub seed: u32,
}

/// Lightning that strikes from this entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Lightning {
    /// Where bolts leave from, world units from the entity.
    pub start: [f32; 2],
    /// Where a bolt it strikes by itself lands, world units.
    pub end: [f32; 2],
    /// How far either side of `end` (along x) a bolt it strikes by itself
    /// may land.
    pub spread: f32,
    /// Seconds between strikes it makes by itself, at least and at most
    /// (`[0, 0]`: only when told).
    pub interval: [f32; 2],
    /// How far a middle point is pushed sideways, as a share of its segment.
    pub jag: f32,
    /// How many times the line is halved (more: finer crackle).
    pub detail: u32,
    /// Forks off the main channel.
    pub branches: u32,
    /// A fork's length, as a share of the main channel.
    pub branch_length: f32,
    /// The core's width, world units; the glow is `glow` times it.
    pub width: f32,
    pub glow: f32,
    /// The core's colour and the glow's, linear (alpha scales them).
    pub color: [f32; 4],
    pub glow_color: [f32; 4],
    /// Seconds a strike lasts, and how many times it flickers back.
    pub duration: f32,
    pub restrikes: u32,
    /// The light it gives: point lights along it of this colour,
    /// intensity and radius (0: none), and `flash` added to the ambient
    /// light at its brightest.
    pub light_color: [f32; 3],
    pub light_intensity: f32,
    pub light_radius: f32,
    pub flash: f32,
    pub z_order: i32,
    /// The strike being shown (never saved).
    #[serde(skip)]
    pub strike: Option<Strike>,
    /// How many strikes so far, and seconds to the next by itself (never
    /// saved).
    #[serde(skip)]
    pub count: u32,
    #[serde(skip)]
    pub next_in: Option<f32>,
    /// A strike asked for (`Lightning.strike`), landing there (never saved).
    #[serde(skip)]
    pub asked: Option<Vec2>,
}

impl Default for Lightning {
    fn default() -> Self {
        Self {
            start: [0.0, 0.0],
            end: [0.0, -300.0],
            spread: 0.0,
            interval: [0.0, 0.0],
            jag: 0.22,
            detail: 6,
            branches: 4,
            branch_length: 0.35,
            width: 3.0,
            glow: 6.0,
            color: [1.0, 1.0, 1.0, 1.0],
            glow_color: [0.55, 0.65, 1.0, 0.6],
            duration: 0.45,
            restrikes: 2,
            light_color: [0.75, 0.82, 1.0],
            light_intensity: 1.5,
            light_radius: 600.0,
            flash: 0.5,
            z_order: 20,
            strike: None,
            count: 0,
            next_in: None,
            asked: None,
        }
    }
}

/// A small deterministic generator, so a seed is the same bolt everywhere.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        // xorshift32
        let mut x = self.0.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    /// A value in `-1..1`.
    fn signed(&mut self) -> f32 {
        self.next() * 2.0 - 1.0
    }
}

/// A seed from a strike's number and its owner, mixed.
pub fn seed_for(owner: u32, count: u32) -> u32 {
    let mut h = owner.wrapping_mul(0x9E37_79B9) ^ count.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h.max(1)
}

/// A jagged line from `a` to `b`: halved `detail` times, each middle pushed
/// sideways by up to `jag` of its segment's length.
fn jagged(a: Vec2, b: Vec2, detail: u32, jag: f32, rng: &mut Rng) -> Vec<Vec2> {
    let mut pts = vec![a, b];
    for _ in 0..detail.min(10) {
        let mut next = Vec::with_capacity(pts.len() * 2 - 1);
        for w in pts.windows(2) {
            let (p, q) = (w[0], w[1]);
            let d = q - p;
            let side = Vec2::new(-d.y, d.x);
            next.push(p);
            next.push((p + q) * 0.5 + side * (rng.signed() * jag));
        }
        next.push(*pts.last().unwrap());
        pts = next;
    }
    pts
}

/// A bolt from `from` to `to` grown from `seed`: the main channel first,
/// then its forks, each with its width as a share of the main channel's.
pub fn bolt(l: &Lightning, from: Vec2, to: Vec2, seed: u32) -> Vec<(Vec<Vec2>, f32)> {
    let mut rng = Rng(seed);
    let main = jagged(from, to, l.detail, l.jag, &mut rng);
    let length = (to - from).length();
    let mut out = vec![(main.clone(), 1.0)];
    let n = main.len();
    for _ in 0..l.branches {
        if n < 4 {
            break;
        }
        // From somewhere in the channel's first two thirds, off at 20 to 45
        // degrees either way from its heading there, shortening as it goes
        // down.
        let k = 1 + (rng.next() * (n as f32 * 0.66)) as usize;
        let k = k.min(n - 2);
        let root = main[k];
        let heading = (main[(k + 2).min(n - 1)] - root).normalize_or(Vec2::NEG_Y);
        let turn = (20.0 + rng.next() * 25.0).to_radians() * if rng.next() < 0.5 { -1.0 } else { 1.0 };
        let dir = Vec2::from_angle(turn).rotate(heading);
        let left = 1.0 - k as f32 / n as f32;
        let reach = length * l.branch_length * (0.5 + 0.5 * rng.next()) * left.max(0.3);
        let fork = jagged(root, root + dir * reach, l.detail.saturating_sub(2), l.jag * 1.2, &mut rng);
        out.push((fork, 0.45));
    }
    out
}

impl Lightning {
    /// How bright the strike is at `age`, 0..1: a flash that fades, coming
    /// back `restrikes` times before `duration` is up.
    pub fn brightness(&self, age: f32) -> f32 {
        let d = self.duration.max(1e-3);
        if age < 0.0 || age >= d {
            return 0.0;
        }
        let p = age / d;
        let fade = (1.0 - p).powf(1.5);
        let flick = (p * std::f32::consts::PI * (2 * self.restrikes + 1) as f32).cos().max(0.0);
        fade * (0.3 + 0.7 * flick)
    }

    /// The strike's brightness now (0 with none).
    pub fn now(&self) -> f32 {
        self.strike.as_ref().map_or(0.0, |s| self.brightness(s.age))
    }

    /// Where its lights stand: the tip, the middle and the root of the main
    /// channel.
    pub fn light_points(&self) -> Vec<Vec2> {
        let Some(s) = &self.strike else { return Vec::new() };
        let Some((main, _)) = s.branches.first() else { return Vec::new() };
        let n = main.len();
        if n == 0 {
            return Vec::new();
        }
        vec![main[n - 1], main[n / 2], main[0]]
    }
}

/// One frame of every lightning: strikes asked for or due, and the ageing
/// of those showing.
pub fn tick_lightning(world: &mut World, dt: f32) {
    let mut struck: Vec<(Entity, Vec2, u32)> = Vec::new();
    for (e, (l, t, w)) in world.query_mut::<(&mut Lightning, &Transform, Option<&WorldTransform>)>() {
        let at = w.map_or(t.position, |w| w.position);
        if let Some(s) = &mut l.strike {
            s.age += dt;
            if s.age >= l.duration {
                l.strike = None;
            }
        }
        let auto = l.interval[1] > 0.0;
        let mut target = l.asked.take();
        if target.is_none() && auto {
            let mut rng = Rng(seed_for(e.id(), l.count.wrapping_add(7919)));
            let next = l.next_in.get_or_insert_with(|| {
                let (lo, hi) = (l.interval[0].max(0.0), l.interval[1].max(l.interval[0]));
                lo + (hi - lo) * rng.next()
            });
            *next -= dt;
            if *next <= 0.0 {
                l.next_in = None;
                let x = l.end[0] + l.spread * rng.signed();
                target = Some(Vec2::new(x, l.end[1]));
            }
        }
        let Some(to) = target else { continue };
        l.count = l.count.wrapping_add(1);
        let seed = seed_for(e.id(), l.count);
        let from = at + Vec2::from(l.start);
        l.strike = Some(Strike { branches: bolt(l, from, to, seed), age: 0.0, seed });
        struck.push((e, to, l.count));
    }
    for (e, to, count) in struck {
        crate::ecs::callbacks::push(e, "on_strike");
        let custom = serde_json::json!({ "strikes": count, "x": to.x, "y": to.y });
        let missing = match world.get::<&mut CustomComponents>(e) {
            Ok(mut c) => {
                c.0.insert("Lightning".into(), custom);
                None
            }
            Err(_) => Some(custom),
        };
        if let Some(custom) = missing {
            let mut map = std::collections::HashMap::new();
            map.insert("Lightning".into(), custom);
            let _ = world.insert_one(e, CustomComponents(map));
        }
    }
}

/// The strike as triangles: for each branch a glow ribbon and a core
/// ribbon, each three points across - the edges clear, the middle lit - so
/// the soft edge is in the vertex colours and needs no texture. Positions,
/// texture coordinates (the white pixel's), colours and indices.
pub fn mesh(l: &Lightning) -> (Vec<Vec2>, Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) {
    let (mut pos, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    let b = l.now();
    let Some(s) = &l.strike else { return (pos, Vec::new(), col, idx) };
    if b <= 0.0 {
        return (pos, Vec::new(), col, idx);
    }
    let layers = [(l.width * l.glow.max(1.0), l.glow_color), (l.width, l.color)];
    for (points, share) in &s.branches {
        let n = points.len();
        if n < 2 {
            continue;
        }
        for &(w, c) in &layers {
            let base = pos.len() as u32;
            for i in 0..n {
                let prev = points[i.saturating_sub(1)];
                let next = points[(i + 1).min(n - 1)];
                let along = (next - prev).normalize_or(Vec2::Y);
                let side = Vec2::new(-along.y, along.x);
                // Thinning towards the tip.
                let half = 0.5 * w * share * (1.0 - 0.6 * i as f32 / (n - 1) as f32);
                let p = points[i];
                let lit = [c[0] * c[3] * b, c[1] * c[3] * b, c[2] * c[3] * b, 1.0];
                let clear = [0.0, 0.0, 0.0, 1.0];
                pos.extend_from_slice(&[p - side * half, p, p + side * half]);
                col.extend_from_slice(&[clear, lit, clear]);
            }
            for i in 0..(n - 1) as u32 {
                let (a, d) = (base + i * 3, base + (i + 1) * 3);
                for k in 0..2 {
                    let (p0, p1, q0, q1) = (a + k, a + k + 1, d + k, d + k + 1);
                    idx.extend_from_slice(&[p0, p1, q1, p0, q1, q0]);
                }
            }
        }
    }
    let uv = vec![[0.5, 0.5, 1.0]; pos.len()];
    (pos, uv, col, idx)
}

/// `Lightning.strike(e, x, y)`: strike now, landing at `(x, y)`.
pub fn strike(world: &mut World, e: Entity, x: f32, y: f32) -> Result<(), String> {
    let mut l = world.get::<&mut Lightning>(e).map_err(|_| "no lightning on that entity".to_string())?;
    l.asked = Some(Vec2::new(x, y));
    Ok(())
}

/// `Lightning.set(e, field, value)`: any field by name, keeping the strike
/// showing.
pub fn set_field(world: &mut World, e: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut l = world.get::<&mut Lightning>(e).map_err(|_| "no lightning on that entity".to_string())?;
    let mut v = serde_json::to_value(&*l).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("not an object")?;
    if !obj.contains_key(field) {
        return Err(format!("lightning has no field '{field}'"));
    }
    obj.insert(field.to_string(), value.clone());
    let mut next: Lightning = serde_json::from_value(v).map_err(|e| format!("lightning.{field}: {e}"))?;
    next.strike = l.strike.take();
    next.count = l.count;
    next.next_in = l.next_in.take();
    next.asked = l.asked.take();
    *l = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bolt_runs_from_end_to_end_and_forks() {
        let l = Lightning::default();
        let b = bolt(&l, Vec2::new(0.0, 300.0), Vec2::new(40.0, -300.0), 12345);
        let (main, w) = &b[0];
        assert_eq!(*w, 1.0);
        assert_eq!(main.len(), (1 << l.detail) + 1, "halved detail times");
        assert_eq!(main[0], Vec2::new(0.0, 300.0));
        assert_eq!(*main.last().unwrap(), Vec2::new(40.0, -300.0));
        assert_eq!(b.len(), 1 + l.branches as usize, "its forks");
        // Jagged, but never wandering off: each level's pushes are about
        // half the last's, so together they stay well inside its length.
        assert!(main.iter().all(|p| p.x.abs() < 400.0), "{main:?}");
        let bends = main.windows(3).filter(|w| (w[1] - w[0]).perp_dot(w[2] - w[1]).abs() > 1.0).count();
        assert!(bends > main.len() / 2, "crooked: {bends}");
        for (fork, share) in &b[1..] {
            assert!(*share < 1.0);
            assert!(main.contains(&fork[0]), "a fork leaves the channel");
        }
    }

    #[test]
    fn a_seed_is_one_bolt_and_another_seed_another() {
        let l = Lightning::default();
        let a = bolt(&l, Vec2::ZERO, Vec2::new(0.0, -400.0), 7);
        let again = bolt(&l, Vec2::ZERO, Vec2::new(0.0, -400.0), 7);
        let other = bolt(&l, Vec2::ZERO, Vec2::new(0.0, -400.0), 8);
        assert_eq!(a, again);
        assert_ne!(a, other);
    }

    #[test]
    fn it_flashes_flickers_and_is_gone() {
        let l = Lightning { duration: 0.5, restrikes: 2, ..Default::default() };
        assert!((l.brightness(0.0) - 1.0).abs() < 1e-6, "brightest as it strikes");
        assert_eq!(l.brightness(0.5), 0.0, "gone when done");
        // It dims between flickers and comes back.
        let samples: Vec<f32> = (0..50).map(|i| l.brightness(i as f32 * 0.01)).collect();
        let rises = samples.windows(2).filter(|w| w[1] > w[0] + 1e-4).count();
        assert!(rises >= 2, "flickers back: {samples:?}");
    }

    #[test]
    fn it_strikes_when_told_and_by_itself() {
        let mut w = World::new();
        let e = w.spawn((Transform { position: Vec2::new(0.0, 300.0), ..Default::default() }, Lightning::default()));
        tick_lightning(&mut w, 0.016);
        assert!(w.get::<&Lightning>(e).unwrap().strike.is_none(), "not until told");
        strike(&mut w, e, 50.0, -200.0).unwrap();
        tick_lightning(&mut w, 0.016);
        {
            let l = w.get::<&Lightning>(e).unwrap();
            let s = l.strike.as_ref().expect("struck");
            assert_eq!(*s.branches[0].0.last().unwrap(), Vec2::new(50.0, -200.0));
            assert_eq!(s.branches[0].0[0], Vec2::new(0.0, 300.0), "from the entity");
            assert!(l.now() > 0.9);
        }
        let c = w.get::<&CustomComponents>(e).unwrap().0.get("Lightning").cloned().unwrap();
        assert_eq!(c["strikes"], 1);
        assert_eq!(c["x"], 50.0);
        for _ in 0..60 {
            tick_lightning(&mut w, 0.016);
        }
        assert!(w.get::<&Lightning>(e).unwrap().strike.is_none(), "gone after its duration");
        // By itself, every half second or so.
        w.get::<&mut Lightning>(e).unwrap().interval = [0.4, 0.6];
        let mut strikes = 0;
        let mut last = 1;
        for _ in 0..200 {
            tick_lightning(&mut w, 0.016);
            let n = w.get::<&Lightning>(e).unwrap().count;
            if n != last {
                strikes += 1;
                last = n;
            }
        }
        assert!((5..=8).contains(&strikes), "about every half second over 3.2 s: {strikes}");
    }

    #[test]
    fn the_mesh_is_soft_edged_and_dark_once_gone() {
        let mut l = Lightning { branches: 0, detail: 2, ..Default::default() };
        l.strike = Some(Strike { branches: bolt(&l, Vec2::ZERO, Vec2::new(0.0, -100.0), 3), age: 0.0, seed: 3 });
        let (pos, uv, col, idx) = mesh(&l);
        assert_eq!(pos.len(), 5 * 3 * 2, "five points, three across, two layers");
        assert_eq!(uv.len(), pos.len());
        assert_eq!(idx.len(), 4 * 12 * 2);
        assert!(col[0][0] == 0.0 && col[1][0] > 0.0 && col[2][0] == 0.0, "clear edges, a lit middle");
        l.strike.as_mut().unwrap().age = 10.0;
        assert!(mesh(&l).0.is_empty(), "nothing once it has faded");
    }

    #[test]
    fn set_keeps_the_strike() {
        let mut w = World::new();
        let e = w.spawn((Transform::default(), Lightning::default()));
        strike(&mut w, e, 0.0, -100.0).unwrap();
        tick_lightning(&mut w, 0.0);
        set_field(&mut w, e, "glow_color", &serde_json::json!([1.0, 0.3, 0.9, 1.0])).unwrap();
        let l = w.get::<&Lightning>(e).unwrap();
        assert!(l.strike.is_some() && l.glow_color[1] == 0.3);
        drop(l);
        assert!(set_field(&mut w, e, "nope", &serde_json::json!(1)).is_err());
    }
}
