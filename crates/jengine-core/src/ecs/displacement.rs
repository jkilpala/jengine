//! A field things move through (Oct 2026): grass, reeds and flowers that part
//! around whoever walks through them and spring back behind.
//!
//! A [`DisplacementField`] is a grid laid over an area of the world, each
//! cell a push - a direction and how hard. A [`Displacer`] (a walker, a cart,
//! a dog) presses the cells round it outward from its centre, harder nearer
//! it; a cell keeps the strongest push it has had and lets it go over
//! `recover` seconds, so a path stays parted for a moment behind whoever made
//! it. A [`FieldBend`] on a sprite reads the field where it stands and leans
//! away from the push about its anchor, squashing a little - on a damped
//! spring, so a blade sways back past upright and settles, and with `wind`
//! it sways on its own. Everything is ticked in `tick_motion` and written to
//! the transforms, so no host was wired and every runtime draws it the same.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Name, Transform, WorldTransform};

/// A grid of pushes over an area centred on its entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplacementField {
    /// The area it covers, world units, centred on the entity.
    pub size: [f32; 2],
    /// World units a cell.
    pub cell: f32,
    /// Seconds a push takes to fall to half.
    pub recover: f32,
    /// The pushes, row by row from the bottom; never saved.
    #[serde(skip)]
    pub grid: Vec<Vec2>,
    #[serde(skip)]
    pub dims: (usize, usize),
}

impl Default for DisplacementField {
    fn default() -> Self {
        Self { size: [1024.0, 512.0], cell: 12.0, recover: 0.8, grid: Vec::new(), dims: (0, 0) }
    }
}

/// Something that parts a field as it moves through it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Displacer {
    /// How far round it the cells are pushed, world units.
    pub radius: f32,
    /// How hard: 1 lays a blade at its fullest lean beside it.
    pub strength: f32,
    /// The field it presses, by entity name; empty presses every field.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub field: String,
}

impl Default for Displacer {
    fn default() -> Self {
        Self { radius: 48.0, strength: 1.0, field: String::new() }
    }
}

/// A sprite that leans away from the field's push where it stands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FieldBend {
    /// The field it reads, by entity name; empty reads whichever covers it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub field: String,
    /// The most it leans, radians.
    pub max_angle: f32,
    /// How much shorter it gets under a full push (0 none).
    pub squash: f32,
    /// How quickly it swings toward its lean (a spring's rate, per second).
    pub stiffness: f32,
    /// How much of its swing it loses a second (0 never stops).
    pub damping: f32,
    /// A sway of its own, radians, out of step with its neighbours.
    pub wind: f32,
    /// How fast the wind sways it, radians a second.
    pub wind_speed: f32,
    /// What it was before it bent, and its swing: never saved.
    #[serde(skip)]
    pub state: BendState,
}

impl Default for FieldBend {
    fn default() -> Self {
        Self {
            field: String::new(),
            max_angle: 0.9,
            squash: 0.25,
            stiffness: 60.0,
            damping: 6.0,
            wind: 0.0,
            wind_speed: 1.5,
            state: BendState::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BendState {
    /// Its authored rotation and scale, taken the first frame.
    pub base: Option<(f32, Vec2)>,
    pub angle: f32,
    pub speed: f32,
    pub time: f32,
}

impl DisplacementField {
    fn ensure(&mut self) {
        let cell = self.cell.max(1.0);
        let dims = (((self.size[0] / cell).ceil() as usize).max(1), ((self.size[1] / cell).ceil() as usize).max(1));
        if self.dims != dims || self.grid.len() != dims.0 * dims.1 {
            self.dims = dims;
            self.grid = vec![Vec2::ZERO; dims.0 * dims.1];
        }
    }

    /// Whether `local` (a point relative to the entity) is inside it.
    pub fn covers(&self, local: Vec2) -> bool {
        local.x.abs() <= self.size[0] * 0.5 && local.y.abs() <= self.size[1] * 0.5
    }

    /// The push at `local`, between the four nearest cells.
    pub fn sample(&self, local: Vec2) -> Vec2 {
        let (w, h) = self.dims;
        if w == 0 || h == 0 {
            return Vec2::ZERO;
        }
        let cell = self.cell.max(1.0);
        let gx = (local.x + self.size[0] * 0.5) / cell - 0.5;
        let gy = (local.y + self.size[1] * 0.5) / cell - 0.5;
        let (x0, y0) = (gx.floor(), gy.floor());
        let (fx, fy) = (gx - x0, gy - y0);
        let at = |x: f32, y: f32| {
            let (x, y) = (x as isize, y as isize);
            if x < 0 || y < 0 || x >= w as isize || y >= h as isize {
                Vec2::ZERO
            } else {
                self.grid[y as usize * w + x as usize]
            }
        };
        let a = at(x0, y0).lerp(at(x0 + 1.0, y0), fx);
        let b = at(x0, y0 + 1.0).lerp(at(x0 + 1.0, y0 + 1.0), fx);
        a.lerp(b, fy)
    }

    /// Let every push go a frame's worth (`dt`), toward none.
    pub fn relax(&mut self, dt: f32) {
        let k = 0.5f32.powf(dt / self.recover.max(1e-3));
        for v in &mut self.grid {
            *v *= k;
            if v.length_squared() < 1e-6 {
                *v = Vec2::ZERO;
            }
        }
    }

    /// Press the cells round `local` outward from it; a cell keeps the
    /// stronger of what it had and this.
    pub fn press(&mut self, local: Vec2, radius: f32, strength: f32) {
        let (w, h) = self.dims;
        let cell = self.cell.max(1.0);
        let r = radius.max(1.0);
        let gx0 = ((local.x - r + self.size[0] * 0.5) / cell).floor().max(0.0) as usize;
        let gy0 = ((local.y - r + self.size[1] * 0.5) / cell).floor().max(0.0) as usize;
        let gx1 = (((local.x + r + self.size[0] * 0.5) / cell).ceil() as usize).min(w);
        let gy1 = (((local.y + r + self.size[1] * 0.5) / cell).ceil() as usize).min(h);
        for gy in gy0..gy1 {
            for gx in gx0..gx1 {
                let centre = Vec2::new((gx as f32 + 0.5) * cell - self.size[0] * 0.5, (gy as f32 + 0.5) * cell - self.size[1] * 0.5);
                let d = centre - local;
                let l = d.length();
                if l >= r {
                    continue;
                }
                // Outward, hardest close in; straight under it, sideways.
                let dir = if l > 1e-3 { d / l } else { Vec2::X };
                let push = dir * strength * (1.0 - l / r).sqrt();
                let v = &mut self.grid[gy * w + gx];
                if push.length_squared() > v.length_squared() {
                    *v = push;
                }
            }
        }
    }
}

fn place(t: &Transform, w: Option<&WorldTransform>) -> (Vec2, f32) {
    w.map_or((t.position, t.rotation), |w| (w.position, w.rotation))
}

/// One frame of every field: relax it, press it where the displacers are,
/// then lean every bender by it. Called from `tick_motion`.
pub fn tick_displacement(world: &mut World, dt: f32) {
    let fields: Vec<Entity> = world.query::<&DisplacementField>().iter().map(|(e, _)| e).collect();
    if fields.is_empty() {
        return;
    }
    let displacers: Vec<(Vec2, Displacer)> = world
        .query::<(&Displacer, &Transform, Option<&WorldTransform>)>()
        .iter()
        .map(|(_, (d, t, w))| (place(t, w).0, d.clone()))
        .collect();
    // Each field: where it is, its name, relaxed and pressed.
    let mut placed: Vec<(Entity, Vec2, f32, String)> = Vec::new();
    for e in fields {
        let (origin, rot, name) = {
            let t = world.get::<&Transform>(e).ok().map(|t| (*t).clone()).unwrap_or_default();
            let w = world.get::<&WorldTransform>(e).ok().map(|w| (*w).clone());
            let (o, r) = place(&t, w.as_ref());
            let n = world.get::<&Name>(e).map(|n| n.0.clone()).unwrap_or_default();
            (o, r, n)
        };
        if let Ok(mut f) = world.get::<&mut DisplacementField>(e) {
            f.ensure();
            f.relax(dt.max(0.0));
            let (sn, cs) = (-rot).sin_cos();
            for (p, d) in &displacers {
                if !d.field.is_empty() && d.field != name {
                    continue;
                }
                let v = *p - origin;
                f.press(Vec2::new(v.x * cs - v.y * sn, v.x * sn + v.y * cs), d.radius, d.strength);
            }
        }
        placed.push((e, origin, rot, name));
    }
    // Every bender: the push where it stands, as a lean on a spring.
    let benders: Vec<Entity> = world.query::<&FieldBend>().iter().map(|(e, _)| e).collect();
    for e in benders {
        let (pos, field_name) = {
            let t = world.get::<&Transform>(e).ok().map(|t| (*t).clone()).unwrap_or_default();
            let w = world.get::<&WorldTransform>(e).ok().map(|w| (*w).clone());
            let f = world.get::<&FieldBend>(e).map(|b| b.field.clone()).unwrap_or_default();
            (place(&t, w.as_ref()).0, f)
        };
        let mut push = Vec2::ZERO;
        for (fe, origin, rot, name) in &placed {
            if !field_name.is_empty() && field_name != *name {
                continue;
            }
            let Ok(f) = world.get::<&DisplacementField>(*fe) else { continue };
            let (sn, cs) = (-rot).sin_cos();
            let v = pos - *origin;
            let local = Vec2::new(v.x * cs - v.y * sn, v.x * sn + v.y * cs);
            if f.covers(local) {
                let p = f.sample(local);
                // Back into the world's frame.
                let (s2, c2) = rot.sin_cos();
                push = Vec2::new(p.x * c2 - p.y * s2, p.x * s2 + p.y * c2);
                break;
            }
        }
        let Ok(mut b) = world.get::<&mut FieldBend>(e) else { continue };
        let Ok(mut t) = world.get::<&mut Transform>(e) else { continue };
        let base = *b.state.base.get_or_insert((t.rotation, t.scale));
        let dt = dt.clamp(0.0, 0.1);
        b.state.time += dt;
        // A push to the right leans it right: clockwise, a negative turn.
        let phase = (pos.x * 0.031 + pos.y * 0.017).sin() * 3.0;
        let wind = b.wind * (b.state.time * b.wind_speed + phase).sin();
        let target = -push.x.clamp(-1.0, 1.0) * b.max_angle + wind;
        let accel = (target - b.state.angle) * b.stiffness - b.state.speed * b.damping;
        b.state.speed += accel * dt;
        b.state.angle += b.state.speed * dt;
        t.rotation = base.0 + b.state.angle;
        let squash = 1.0 - b.squash * push.length().min(1.0);
        t.scale = Vec2::new(base.1.x, base.1.y * squash);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_pushes_outward_and_fades() {
        let mut f = DisplacementField { size: [200.0, 200.0], cell: 10.0, recover: 0.5, ..Default::default() };
        f.ensure();
        f.press(Vec2::ZERO, 40.0, 1.0);
        let right = f.sample(Vec2::new(20.0, 0.0));
        let left = f.sample(Vec2::new(-20.0, 0.0));
        assert!(right.x > 0.3 && left.x < -0.3, "outward both ways: {right} {left}");
        assert_eq!(f.sample(Vec2::new(90.0, 90.0)), Vec2::ZERO, "nothing far off");
        f.relax(0.5);
        let half = f.sample(Vec2::new(20.0, 0.0));
        assert!((half.x - right.x * 0.5).abs() < 1e-3, "half gone in `recover`: {half} of {right}");
    }

    #[test]
    fn a_cell_keeps_the_stronger_push() {
        let mut f = DisplacementField { size: [200.0, 200.0], cell: 10.0, ..Default::default() };
        f.ensure();
        f.press(Vec2::ZERO, 40.0, 1.0);
        let strong = f.sample(Vec2::new(15.0, 0.0));
        f.press(Vec2::new(30.0, 0.0), 40.0, 0.2);
        assert_eq!(f.sample(Vec2::new(15.0, 0.0)), strong, "a weak press does not undo a strong one");
    }
}
