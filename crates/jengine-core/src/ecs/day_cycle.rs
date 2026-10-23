//! A day that turns (Oct 2026): a clock, and everything the time of day
//! changes - the sky, the light that is everywhere, the sun and the moon -
//! read from a few keys through the day and eased between them.
//!
//! A [`DayCycle`] keeps `hour` (0 to 24), advanced `hours_per_second`. Its
//! `keys` say, at given hours, the sky's colour at the top and at the
//! horizon, the ambient light, and the sun's colour and strength; between
//! two keys every number is eased, and the last key wraps to the first
//! across midnight. Each frame it writes
//!
//! - the world's `ambient_light` (the first one found),
//! - the `directional_light` on the entity named `sun`: its colour and
//!   strength, and its `direction` - the sun rises at `sunrise` in the east
//!   (+x), stands overhead half way to `sunset`, and sets in the west - with
//!   no light below the horizon,
//! - and, with `sky`, a gradient behind everything as the entity's own
//!   `mesh`, kept over the main camera's view so it is always the sky,
//! - the entities named in `bodies` (a sun disc, a moon) placed on arcs
//!   across that view, rising and setting at their hours, and those in
//!   `night` (stars) faded in by how dark it is.
//!
//! It tells its script `on_dawn` at `sunrise` and `on_dusk` at `sunset`
//! (`ecs::callbacks`), and keeps the hour in its custom `DayCycle.hour` for a
//! script to read with `Entity.get_custom`. Ticked in `tick_motion`, so no
//! host was wired.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{AmbientLight, Camera2DComponent, CustomComponents, DirectionalLight2D, Mesh2D, MeshVertex, Name, SpriteComponent, Transform};

/// What the day is like at one hour.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DayKey {
    pub hour: f32,
    /// The sky at the top of the view and at the horizon (linear).
    pub sky_top: [f32; 3],
    pub sky_horizon: [f32; 3],
    pub ambient: [f32; 3],
    pub ambient_intensity: f32,
    pub sun_color: [f32; 3],
    pub sun_intensity: f32,
}

impl Default for DayKey {
    fn default() -> Self {
        Self {
            hour: 12.0,
            sky_top: [0.2, 0.4, 0.9],
            sky_horizon: [0.6, 0.75, 1.0],
            ambient: [1.0, 1.0, 1.0],
            ambient_intensity: 0.6,
            sun_color: [1.0, 0.95, 0.85],
            sun_intensity: 1.0,
        }
    }
}

/// Something that crosses the sky: a sun disc, the moon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SkyBody {
    /// The entity's name.
    pub name: String,
    pub rise: f32,
    pub set: f32,
    /// How high the arc goes, as a share of the view's half height.
    pub height: f32,
}

impl Default for SkyBody {
    fn default() -> Self {
        Self { name: String::new(), rise: 6.0, set: 18.0, height: 0.8 }
    }
}

/// `day_cycle`: the time of day, and what it changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DayCycle {
    pub hour: f32,
    pub hours_per_second: f32,
    pub keys: Vec<DayKey>,
    /// The name of the entity carrying the sun's `directional_light`.
    pub sun: String,
    pub sunrise: f32,
    pub sunset: f32,
    /// Draw the sky gradient behind everything, as this entity's mesh.
    pub sky: bool,
    pub sky_z: i32,
    pub bodies: Vec<SkyBody>,
    /// Names of entities faded in at night (their sprite's alpha).
    pub night: Vec<String>,
    /// Runtime state - never saved.
    #[serde(skip)]
    pub last_hour: Option<f32>,
}

impl Default for DayCycle {
    fn default() -> Self {
        Self {
            hour: 12.0,
            hours_per_second: 0.5,
            keys: vec![DayKey::default()],
            sun: String::new(),
            sunrise: 6.0,
            sunset: 18.0,
            sky: true,
            sky_z: -1000,
            bodies: Vec::new(),
            night: Vec::new(),
            last_hour: None,
        }
    }
}

/// The day at `hour`: the two keys either side, eased (wrapping midnight).
pub fn sample(keys: &[DayKey], hour: f32) -> DayKey {
    let Some(first) = keys.first() else { return DayKey::default() };
    if keys.len() == 1 {
        return *first;
    }
    let mut sorted: Vec<DayKey> = keys.to_vec();
    sorted.sort_by(|a, b| a.hour.total_cmp(&b.hour));
    let h = hour.rem_euclid(24.0);
    let n = sorted.len();
    // The key at or before h (wrapping to the last), and the next.
    let i = sorted.iter().rposition(|k| k.hour <= h).unwrap_or(n - 1);
    let (a, b) = (sorted[i], sorted[(i + 1) % n]);
    let span = (b.hour - a.hour).rem_euclid(24.0).max(1e-4);
    let t = ((h - a.hour).rem_euclid(24.0) / span).clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    let mix3 = |x: [f32; 3], y: [f32; 3]| std::array::from_fn(|k| x[k] + (y[k] - x[k]) * t);
    let mix = |x: f32, y: f32| x + (y - x) * t;
    DayKey {
        hour: h,
        sky_top: mix3(a.sky_top, b.sky_top),
        sky_horizon: mix3(a.sky_horizon, b.sky_horizon),
        ambient: mix3(a.ambient, b.ambient),
        ambient_intensity: mix(a.ambient_intensity, b.ambient_intensity),
        sun_color: mix3(a.sun_color, b.sun_color),
        sun_intensity: mix(a.sun_intensity, b.sun_intensity),
    }
}

/// How far a body is across its arc at `hour` (0 at rise, 1 at set), or
/// `None` below the horizon.
pub fn arc(rise: f32, set: f32, hour: f32) -> Option<f32> {
    let span = (set - rise).rem_euclid(24.0);
    let into = (hour - rise).rem_euclid(24.0);
    (span > 0.0 && into <= span).then(|| into / span)
}

/// Whether `hour` moved past `mark` going from `last` (forward, wrapping).
fn crossed(last: f32, now: f32, mark: f32) -> bool {
    let run = (now - last).rem_euclid(24.0);
    run > 0.0 && run < 12.0 && (mark - last).rem_euclid(24.0) <= run && (mark - last).rem_euclid(24.0) > 0.0
}

fn named(world: &World, name: &str) -> Option<Entity> {
    if name.is_empty() {
        return None;
    }
    world.query::<&Name>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e)
}

/// The main camera's centre and the half size of what it shows.
fn view(world: &World) -> (Vec2, Vec2) {
    let Some(cam) = crate::ecs::camera_rig::main_camera(world) else { return (Vec2::ZERO, Vec2::new(640.0, 360.0)) };
    let at = world.get::<&Transform>(cam).map(|t| t.position).unwrap_or(Vec2::ZERO);
    let half = world
        .get::<&Camera2DComponent>(cam)
        .map(|c| Vec2::new(c.viewport_width, c.viewport_height) * 0.5 / c.zoom.max(1e-3))
        .unwrap_or(Vec2::new(640.0, 360.0));
    (at, half)
}

/// Turn the day. Inside `tick_motion`.
pub fn tick_day_cycle(world: &mut World, dt: f32) {
    let Some((e, mut day)) = world.query::<&DayCycle>().iter().next().map(|(e, d)| (e, d.clone())) else { return };
    day.hour = (day.hour + day.hours_per_second * dt.max(0.0)).rem_euclid(24.0);
    let now = sample(&day.keys, day.hour);
    if let Some(last) = day.last_hour {
        if crossed(last, day.hour, day.sunrise) {
            crate::ecs::callbacks::push(e, "on_dawn");
        }
        if crossed(last, day.hour, day.sunset) {
            crate::ecs::callbacks::push(e, "on_dusk");
        }
    }
    day.last_hour = Some(day.hour);
    // The light that is everywhere.
    if let Some((_, a)) = world.query_mut::<&mut AmbientLight>().into_iter().next() {
        a.color = now.ambient;
        a.intensity = now.ambient_intensity;
    }
    // The sun: across the sky by day, nothing by night.
    if let Some(sun) = named(world, &day.sun)
        && let Ok(mut l) = world.get::<&mut DirectionalLight2D>(sun)
    {
        match arc(day.sunrise, day.sunset, day.hour) {
            Some(f) => {
                // From the east (+x) up and over to the west; the light
                // travels away from the sun.
                let up = f * std::f32::consts::PI;
                let from = Vec2::new(up.cos(), up.sin().max(0.05));
                l.direction = (-from).y.atan2((-from).x);
                // Low suns are weak: the light comes in at a slant.
                l.color = now.sun_color;
                l.intensity = now.sun_intensity * (up.sin() * 1.5).clamp(0.0, 1.0);
            }
            None => l.intensity = 0.0,
        }
    }
    let (centre, half) = view(world);
    // Bodies on their arcs across the view.
    for b in &day.bodies {
        let Some(be) = named(world, &b.name) else { continue };
        let pos = match arc(b.rise, b.set, day.hour) {
            Some(f) => {
                let a = f * std::f32::consts::PI;
                centre + Vec2::new(a.cos() * half.x * 0.85, -half.y * 0.3 + a.sin() * half.y * b.height)
            }
            None => centre + Vec2::new(0.0, -half.y * 3.0),
        };
        if let Ok(mut t) = world.get::<&mut Transform>(be) {
            t.position = pos;
        }
    }
    // Stars by how dark it is.
    let dark = 1.0 - (now.ambient_intensity / day.keys.iter().map(|k| k.ambient_intensity).fold(1e-3, f32::max)).clamp(0.0, 1.0);
    for name in &day.night {
        if let Some(n) = named(world, name)
            && let Ok(mut s) = world.get::<&mut SpriteComponent>(n)
        {
            s.color[3] = (dark * 1.6 - 0.4).clamp(0.0, 1.0);
        }
    }
    // The sky.
    if day.sky {
        let local = |p: Vec2| -> [f32; 2] {
            let at = world.get::<&Transform>(e).map(|t| t.position).unwrap_or(Vec2::ZERO);
            (p - at).to_array()
        };
        let (lo, hi) = (centre - half * 1.2, centre + half * 1.2);
        let top = [now.sky_top[0], now.sky_top[1], now.sky_top[2], 1.0];
        let bottom = [now.sky_horizon[0], now.sky_horizon[1], now.sky_horizon[2], 1.0];
        let v = |p: Vec2, c: [f32; 4]| MeshVertex { position: local(p), uv: [0.5, 0.5], color: c, q: 1.0 };
        let vertices = vec![
            v(Vec2::new(lo.x, hi.y), top),
            v(Vec2::new(hi.x, hi.y), top),
            v(Vec2::new(hi.x, lo.y), bottom),
            v(Vec2::new(lo.x, lo.y), bottom),
        ];
        let indices = vec![0, 1, 2, 0, 2, 3];
        let missing = match world.get::<&mut Mesh2D>(e) {
            Ok(mut m) => {
                m.vertices = vertices;
                m.indices = indices;
                m.z_order = day.sky_z;
                None
            }
            Err(_) => Some(Mesh2D { vertices, indices, z_order: day.sky_z, ..Default::default() }),
        };
        if let Some(mesh) = missing {
            let _ = world.insert_one(e, mesh);
        }
    }
    // The hour, for scripts.
    let custom = serde_json::json!({ "hour": day.hour });
    let custom = match world.get::<&mut CustomComponents>(e) {
        Ok(mut c) => {
            c.0.insert("DayCycle".into(), custom);
            None
        }
        Err(_) => Some(custom),
    };
    if let Some(custom) = custom {
        let mut map = std::collections::HashMap::new();
        map.insert("DayCycle".into(), custom);
        let _ = world.insert_one(e, CustomComponents(map));
    }
    if let Ok(mut d) = world.get::<&mut DayCycle>(e) {
        d.hour = day.hour;
        d.last_hour = day.last_hour;
    }
}

/// `DayCycle.set(e, field, value)`: any field by name (`hour` jumps the
/// clock, `hours_per_second` speeds or stops it).
pub fn set_day_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut d = world.get::<&mut DayCycle>(entity).map_err(|_| "no day_cycle on that entity".to_string())?;
    let mut whole = serde_json::to_value(&*d).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) {
        return Err(format!("'{field}' is not a day_cycle field"));
    }
    map.insert(field.to_string(), value.clone());
    let mut new: DayCycle = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    // A jump of the clock is not a dawn passed.
    new.last_hour = if field == "hour" { None } else { d.last_hour };
    *d = new;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(hour: f32, amb: f32) -> DayKey {
        DayKey { hour, ambient_intensity: amb, ..Default::default() }
    }

    #[test]
    fn keys_ease_and_wrap_across_midnight() {
        let keys = [key(6.0, 0.4), key(12.0, 1.0), key(20.0, 0.1)];
        assert!((sample(&keys, 12.0).ambient_intensity - 1.0).abs() < 1e-5);
        assert!((sample(&keys, 9.0).ambient_intensity - 0.7).abs() < 1e-5, "half way, eased to half");
        // 20:00 -> 06:00 across midnight: 1:00 is half way.
        assert!((sample(&keys, 1.0).ambient_intensity - 0.25).abs() < 1e-5);
        assert!((sample(&keys, 23.0).ambient_intensity - sample(&keys, 23.0 - 24.0).ambient_intensity).abs() < 1e-6);
    }

    #[test]
    fn arcs_rise_and_set() {
        assert_eq!(arc(6.0, 18.0, 6.0), Some(0.0));
        assert_eq!(arc(6.0, 18.0, 12.0), Some(0.5));
        assert_eq!(arc(6.0, 18.0, 20.0), None);
        // A moon from 19:00 to 05:00.
        assert_eq!(arc(19.0, 5.0, 0.0), Some(0.5));
        assert_eq!(arc(19.0, 5.0, 12.0), None);
        assert!(crossed(5.9, 6.1, 6.0) && !crossed(6.1, 6.3, 6.0) && crossed(23.9, 0.2, 0.0));
    }

    #[test]
    fn the_day_writes_the_ambient_the_sun_and_says_dawn() {
        let mut world = World::new();
        world.spawn((AmbientLight { color: [1.0; 3], intensity: 1.0 },));
        let sun = world.spawn((Name("Sun".into()), DirectionalLight2D::default()));
        let day = world.spawn((
            Transform::default(),
            DayCycle {
                hour: 5.5,
                hours_per_second: 1.0,
                keys: vec![key(0.0, 0.1), key(12.0, 0.9)],
                sun: "Sun".into(),
                ..Default::default()
            },
        ));
        crate::ecs::callbacks::take();
        tick_day_cycle(&mut world, 0.0);
        assert_eq!(world.get::<&DirectionalLight2D>(sun).unwrap().intensity, 0.0, "before sunrise, no sun");
        tick_day_cycle(&mut world, 1.0);
        let calls: Vec<&str> = crate::ecs::callbacks::take().iter().map(|c| c.handler).collect();
        assert_eq!(calls, vec!["on_dawn"]);
        let amb = world.query::<&AmbientLight>().iter().next().unwrap().1.intensity;
        assert!(amb > 0.1 && amb < 0.9, "between the keys: {amb}");
        set_day_field(&world, day, "hour", &serde_json::json!(12.0)).unwrap();
        tick_day_cycle(&mut world, 0.0);
        let l = world.get::<&DirectionalLight2D>(sun).unwrap();
        assert!(l.intensity > 0.9, "noon: full sun");
        assert!((l.direction + std::f32::consts::FRAC_PI_2).abs() < 0.05, "straight down at noon: {}", l.direction);
        assert_eq!(world.get::<&CustomComponents>(day).unwrap().0["DayCycle"]["hour"], 12.0);
        assert_eq!(world.get::<&Mesh2D>(day).unwrap().vertices.len(), 4, "the sky is drawn");
    }
}
