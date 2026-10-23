//! **Liquids** (`fluid`, Oct 2026): a pour of particles that behave as one
//! liquid - molten metal, water, honey. Each particle pushes its neighbours
//! apart when they crowd and pulls them in when they thin out (Clavet et
//! al.'s double density relaxation, which needs no pressure solve and no
//! grid of cells to stay stable), a viscosity term evens their speeds, they
//! fall, they slide off and pool against every non-sensor `collider` (the
//! outlines particles meet, `ecs::outlines`), and each one cools - so a
//! colour runs from its gradient's hot end to its cold one - and **sets**
//! below `freeze_below`: a frozen particle stays where it is and the liquid
//! piles against it, which is how a mould fills and becomes a casting.
//!
//! It is drawn as a soft dot a particle in the sprite pass (z-sorted with
//! everything else), so a `canvas_group` round it with a `threshold` merges
//! the dots into one surface. It runs inside `tick_motion`, so no host was
//! wired. The particles are runtime state, never saved.

use glam::Vec2;
use hecs::World;
use serde::{Deserialize, Serialize};

use super::components::{Transform, WorldTransform};

/// One particle of a [`Fluid`]. Never saved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FluidParticle {
    pub position: Vec2,
    pub velocity: Vec2,
    /// 1 is as hot as it pours, 0 cold.
    pub temperature: f32,
    /// Set solid ([`Fluid::freeze_below`]): it no longer moves.
    pub frozen: bool,
}

/// A light a hot fluid gives off ([`Fluid::lights`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FluidLights {
    /// At most this many lights, at the hottest moving particles.
    pub max: u32,
    pub radius: f32,
    /// At full temperature; a light scales with its particle's.
    pub intensity: f32,
    pub color: [f32; 3],
}

impl Default for FluidLights {
    fn default() -> Self {
        Self { max: 8, radius: 160.0, intensity: 1.5, color: [1.0, 0.55, 0.2] }
    }
}

/// The `fluid` component.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Fluid {
    /// How far a particle feels its neighbours, in world units.
    pub radius: f32,
    /// How many neighbours' worth of crowding it rests at.
    pub rest_density: f32,
    /// How hard a crowd pushes apart, as a share of `radius` a step.
    pub stiffness: f32,
    /// How hard particles too close push apart - what keeps them from
    /// clumping in pairs.
    pub near_stiffness: f32,
    /// How quickly neighbours' speeds even out, per second: 0 water, 20 honey.
    pub viscosity: f32,
    /// World units a second squared.
    pub gravity: [f32; 2],
    pub max_particles: usize,
    /// Particles poured a second (0: not pouring).
    pub pour_rate: f32,
    /// Where it pours from, in the entity's own frame.
    pub pour_at: [f32; 2],
    /// Which way it pours, in the entity's own frame (radians), and how fast.
    pub pour_angle: f32,
    pub pour_speed: f32,
    /// How wide the stream is where it leaves.
    pub pour_width: f32,
    /// Temperature lost a second (1 is all of it).
    pub cooling: f32,
    /// Below this temperature a particle sets solid (0: never).
    pub freeze_below: f32,
    /// How big each particle is drawn.
    pub size: f32,
    pub texture_id: String,
    /// Colour by temperature: `[t, r, g, b, a]` stops, 0 cold to 1 hot.
    pub gradient: Vec<[f32; 5]>,
    pub z_order: i32,
    pub sort_layer: Option<String>,
    pub blend_mode: crate::renderer::material::BlendMode,
    /// Light from the hottest particles (`None`: none).
    pub lights: Option<FluidLights>,
    #[serde(skip)]
    pub particles: Vec<FluidParticle>,
    #[serde(skip)]
    pub pour_carry: f32,
    /// The lights this frame, for `gather_lights`. Never saved.
    #[serde(skip)]
    pub lit: Vec<(Vec2, f32)>,
}

impl Default for Fluid {
    fn default() -> Self {
        Self {
            radius: 22.0,
            rest_density: 3.0,
            stiffness: 0.06,
            near_stiffness: 0.12,
            viscosity: 4.0,
            gravity: [0.0, -900.0],
            max_particles: 1200,
            pour_rate: 0.0,
            pour_at: [0.0, 0.0],
            pour_angle: -std::f32::consts::FRAC_PI_2,
            pour_speed: 120.0,
            pour_width: 10.0,
            cooling: 0.0,
            freeze_below: 0.0,
            size: 28.0,
            texture_id: String::new(),
            gradient: Vec::new(),
            z_order: 0,
            sort_layer: None,
            blend_mode: crate::renderer::material::BlendMode::Alpha,
            lights: None,
            particles: Vec::new(),
            pour_carry: 0.0,
            lit: Vec::new(),
        }
    }
}

impl Fluid {
    /// A particle's colour at `temperature` (0..1), from the gradient; white
    /// without one.
    pub fn color_at(&self, temperature: f32) -> [f32; 4] {
        let g = &self.gradient;
        if g.is_empty() {
            return [1.0; 4];
        }
        let t = temperature.clamp(0.0, 1.0);
        if t <= g[0][0] {
            return [g[0][1], g[0][2], g[0][3], g[0][4]];
        }
        for w in g.windows(2) {
            let (a, b) = (w[0], w[1]);
            if t <= b[0] {
                let k = if b[0] > a[0] { (t - a[0]) / (b[0] - a[0]) } else { 1.0 };
                return [1, 2, 3, 4].map(|i| a[i] + (b[i] - a[i]) * k);
            }
        }
        let l = g[g.len() - 1];
        [l[1], l[2], l[3], l[4]]
    }
}

/// The step: at most 1/60 s, a few to a frame.
const STEP: f32 = 1.0 / 60.0;

fn hash(x: u32) -> u32 {
    let mut h = x.wrapping_mul(0x9E37_79B1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85EB_CA77);
    h ^ (h >> 13)
}

/// Pour, move and cool every fluid. Called by `behaviors::tick_motion`.
pub fn tick_fluids(world: &mut World, dt: f32) {
    if world.query::<&Fluid>().iter().next().is_none() {
        return;
    }
    let outlines = super::outlines::ColliderOutlines::gather(world);
    for (_, (fluid, t, w)) in world.query_mut::<(&mut Fluid, &Transform, Option<&WorldTransform>)>() {
        let xf = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        let steps = ((dt / STEP).ceil() as usize).clamp(1, 4);
        let h = dt.min(STEP * 4.0) / steps as f32;
        for _ in 0..steps {
            pour(fluid, &xf, h);
            step(fluid, &outlines, h);
        }
        light(fluid);
    }
}

fn pour(f: &mut Fluid, xf: &WorldTransform, dt: f32) {
    if f.pour_rate <= 0.0 {
        f.pour_carry = 0.0;
        return;
    }
    f.pour_carry += f.pour_rate * dt;
    let at = xf.apply(Vec2::from(f.pour_at));
    let dir = Vec2::from_angle(f.pour_angle + xf.rotation);
    let across = dir.perp();
    while f.pour_carry >= 1.0 {
        f.pour_carry -= 1.0;
        if f.particles.len() >= f.max_particles {
            continue;
        }
        let n = f.particles.len() as u32;
        let jitter = (hash(n ^ 0x5bd1_e995) as f32 / u32::MAX as f32 - 0.5) * f.pour_width;
        f.particles.push(FluidParticle {
            position: at + across * jitter,
            velocity: dir * f.pour_speed,
            temperature: 1.0,
            frozen: false,
        });
    }
}

fn step(f: &mut Fluid, outlines: &super::outlines::ColliderOutlines, dt: f32) {
    let n = f.particles.len();
    if n == 0 {
        return;
    }
    let h = f.radius.max(1.0);
    let gravity = Vec2::from(f.gravity);
    // Gravity, and cooling.
    for p in f.particles.iter_mut() {
        if p.frozen {
            continue;
        }
        p.velocity += gravity * dt;
        p.temperature = (p.temperature - f.cooling * dt).max(0.0);
        if f.freeze_below > 0.0 && p.temperature < f.freeze_below {
            p.frozen = true;
            p.velocity = Vec2::ZERO;
        }
    }
    // Neighbours: a hash of cells a radius wide.
    let cell = |p: Vec2| ((p.x / h).floor() as i32, (p.y / h).floor() as i32);
    let mut grid: std::collections::HashMap<(i32, i32), Vec<u32>> = std::collections::HashMap::with_capacity(n);
    for (i, p) in f.particles.iter().enumerate() {
        grid.entry(cell(p.position)).or_default().push(i as u32);
    }
    let neighbours = |i: usize, parts: &[FluidParticle]| -> Vec<(usize, Vec2, f32)> {
        let pi = parts[i].position;
        let (cx, cy) = cell(pi);
        let mut out = Vec::new();
        for dy in -1..=1 {
            for dx in -1..=1 {
                if let Some(list) = grid.get(&(cx + dx, cy + dy)) {
                    for &j in list {
                        let j = j as usize;
                        if j == i {
                            continue;
                        }
                        let d = parts[j].position - pi;
                        let r = d.length();
                        if r < h && r > 1e-5 {
                            out.push((j, d / r, 1.0 - r / h));
                        }
                    }
                }
            }
        }
        out
    };
    // Viscosity: neighbours closing on each other share their speed.
    if f.viscosity > 0.0 {
        for i in 0..n {
            if f.particles[i].frozen {
                continue;
            }
            for (j, dir, q) in neighbours(i, &f.particles) {
                if j < i || f.particles[j].frozen {
                    continue;
                }
                let u = (f.particles[i].velocity - f.particles[j].velocity).dot(dir);
                if u > 0.0 {
                    let imp = dir * (dt * q * f.viscosity * u * 0.5).min(u * 0.5);
                    f.particles[i].velocity -= imp;
                    f.particles[j].velocity += imp;
                }
            }
        }
    }
    // Move, remembering where from.
    let prev: Vec<Vec2> = f.particles.iter().map(|p| p.position).collect();
    for p in f.particles.iter_mut() {
        if !p.frozen {
            p.position += p.velocity * dt;
        }
    }
    // Double density relaxation: crowding pushes apart, thinning draws in.
    // Stiffness is a share of the radius a step, so it means the same at any
    // scale and any step.
    let k = f.stiffness * h;
    let k_near = f.near_stiffness * h;
    for i in 0..n {
        let near = neighbours(i, &f.particles);
        let (mut rho, mut rho_near) = (0.0f32, 0.0f32);
        for &(_, _, q) in &near {
            rho += q * q;
            rho_near += q * q * q;
        }
        let pressure = k * (rho - f.rest_density);
        let pressure_near = k_near * rho_near;
        let mut moved = Vec2::ZERO;
        for (j, dir, q) in near {
            let d = dir * (pressure * q + pressure_near * q * q).min(h * 0.5);
            let (fi, fj) = (f.particles[i].frozen, f.particles[j].frozen);
            match (fi, fj) {
                (false, false) => {
                    f.particles[j].position += d * 0.5;
                    moved -= d * 0.5;
                }
                (false, true) => moved -= d,
                (true, false) => f.particles[j].position += d,
                (true, true) => {}
            }
        }
        if !f.particles[i].frozen {
            f.particles[i].position += moved;
        }
    }
    // Surfaces: a move that crossed one stops on its near side.
    for (i, p) in f.particles.iter_mut().enumerate() {
        if p.frozen {
            continue;
        }
        if let Some(hit) = outlines.first_hit(prev[i], p.position, u16::MAX) {
            p.position = hit.point + hit.normal * 0.5;
        }
        // The speed is what the step did.
        p.velocity = (p.position - prev[i]) / dt;
    }
}

fn light(f: &mut Fluid) {
    f.lit.clear();
    let Some(l) = &f.lights else { return };
    if l.max == 0 || f.particles.is_empty() {
        return;
    }
    // Evenly through the hottest particles still moving.
    let mut hot: Vec<&FluidParticle> = f.particles.iter().filter(|p| !p.frozen && p.temperature > 0.2).collect();
    hot.sort_by(|a, b| b.temperature.total_cmp(&a.temperature));
    let stride = (hot.len() / l.max as usize).max(1);
    for p in hot.iter().step_by(stride).take(l.max as usize) {
        f.lit.push((p.position, p.temperature));
    }
}

/// A fluid's particles as a triangle list in world space - a quad a
/// particle - for the sprite pass: positions, uvq, colours and indices.
pub fn quads(f: &Fluid) -> (Vec<Vec2>, Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) {
    let n = f.particles.len();
    let mut pos = Vec::with_capacity(n * 4);
    let mut uvq = Vec::with_capacity(n * 4);
    let mut col = Vec::with_capacity(n * 4);
    let mut idx = Vec::with_capacity(n * 6);
    let r = f.size * 0.5;
    for p in &f.particles {
        let c = f.color_at(p.temperature);
        let base = pos.len() as u32;
        for (dx, dy, u, v) in [(-r, r, 0.0, 0.0), (r, r, 1.0, 0.0), (r, -r, 1.0, 1.0), (-r, -r, 0.0, 1.0)] {
            pos.push(p.position + Vec2::new(dx, dy));
            uvq.push([u, v, 1.0]);
            col.push(c);
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (pos, uvq, col, idx)
}

/// `Fluid.set(e, field, value)`: any field by name, through serde, keeping
/// the particles - `pour_rate` to start and stop a pour, `clear` (any value)
/// to empty it.
pub fn set_fluid_field(world: &World, entity: hecs::Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let Ok(mut f) = world.get::<&mut Fluid>(entity) else {
        return Err("no fluid on that entity".into());
    };
    if field == "clear" {
        f.particles.clear();
        f.lit.clear();
        return Ok(());
    }
    let mut whole = serde_json::to_value(&*f).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) && !matches!(field, "sort_layer" | "lights") {
        return Err(format!("'{field}' is not a fluid field"));
    }
    // A script's numbers are floats; a count stays a count.
    let value = match (field, value) {
        ("max_particles", serde_json::Value::Number(n)) => serde_json::Value::from(n.as_f64().unwrap_or(0.0).max(0.0) as u64),
        _ => value.clone(),
    };
    map.insert(field.to_string(), value);
    let mut next: Fluid = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    next.particles = std::mem::take(&mut f.particles);
    next.pour_carry = f.pour_carry;
    next.lit = std::mem::take(&mut f.lit);
    *f = next;
    Ok(())
}
