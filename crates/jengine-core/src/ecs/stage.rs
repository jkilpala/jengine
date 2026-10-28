//! **A 3D stage** (Oct 2026): a perspective camera looking down at a
//! diorama of flat pieces placed in three dimensions - floors, walls, roofs
//! and the sprites standing among them - which is how the HD-2D look is
//! built: pixel-art characters as upright cards in
//! a scene drawn in perspective, blurred by depth.
//!
//! Stage space is x east, y north (into the scene) and z up, as a Mode 7
//! ground has it with height added. A [`Stage`] is the camera: it looks at
//! `target` from `distance` away, turned `yaw` about the vertical (0 looks
//! north) and tipped `pitch` down from the horizon. At the target's depth
//! one stage unit is `zoom` world units, and the target falls on the stage
//! entity's own position - so the 2D camera can stay where it is and the
//! stage camera does the moving. The nearer the eye (`distance`), the
//! stronger the perspective.
//!
//! A [`StageQuad`] is a flat parallelogram: a corner (`origin`) and two
//! edges (`u` across, `v` up the picture) - so its front is `u x v`, and a
//! wall facing the camera has `u` east and `v` up, a floor `u` east and `v`
//! north. Each frame it is projected into the entity's own `mesh`
//! (`Mesh2D`: put the texture and material there), cut into a cell per
//! texture repeat - every sampler clamps, so a tiled floor is tiled in its
//! geometry - and clipped at the near plane, each vertex carrying its
//! perspective weight, so the sprite shader textures it in true perspective.
//! A back-facing quad draws nothing unless it is `two_sided`.
//!
//! A [`StageBillboard`] stands anything at a stage point: its `Transform`
//! is written to where the point falls on the screen and its scale to how
//! big it is there (`perspective` 1; 0 keeps its size), so a sprite
//! anchored at its feet stands on the floor. On a point or spot light it
//! also sets the light's radius (`light_radius`, stage units) and squashes
//! its pool into the floor's ellipse (`squash`, from the pitch).
//!
//! **A billboard can throw its shadow** (`shadow`): for each of the lights
//! nearest it - stage lights, billboards with a `light_radius` - its sprite's
//! current frame is projected from the light's 3D position onto the floor, a
//! flat dark silhouette from its feet to where the light puts its top: long
//! when the lamp is near and low, under it when the lamp is overhead, turning
//! as it walks past, foreshortened by the camera like the floor. The quads are
//! worked out here (`StageBillboard::shadow_quads`) and drawn by
//! `render_world_items` in the floor band, over the floors and under
//! everything standing. Screen-space shadows (`cast_shadow`) cannot do this:
//! they never knew where a light is in depth.
//!
//! **Solids throw shadows too** (`stage_caster`): a footprint (a convex
//! polygon on the floor) standing `height` tall - a house, a well, a wall.
//! From each light near it, its shadow is the convex hull of the footprint and
//! its top projected from the light onto the floor: one shape for the whole
//! solid, so its walls' shadows never darken each other, fading as the light
//! does with distance, under everything standing. A light standing inside the
//! footprint (a lamp in a room) throws nothing from it.
//!
//! **Order is ground depth.** Everything upright sorts by how far into the
//! scene its footprint lies (`z_base - depth * z_per_unit`, the target's
//! depth at `z_base`), the painter's order a diorama needs; a `ground` quad
//! draws in a band of its own under all of it (`ground_z`), so nothing
//! standing on a floor is ever drawn under it. Each also records that depth
//! per vertex (`view_z`), which the velocity pass hands `depth_of_field` -
//! so focus at `z_base` is focus on the target, and a floor blurs smoothly
//! into the distance rather than as one layer.
//!
//! **A `stage_group` is a frame** (a house, a cart, a market stall): the
//! stage coordinates of everything under it - a quad's `origin`, `u` and
//! `v`, a billboard's `position`, a caster's footprint - are in its frame,
//! which stands at its `position` turned `yaw` about the vertical, in the
//! frame of the group above it. Parents without one are passed through, and
//! a piece under no group is where its coordinates say. Each piece still
//! sorts by its own ground depth, so a traveller can stand between a house's
//! front wall and its roof. `follow` takes the place of the groups: the piece
//! is then an offset from its leader - any named stage object, found by its
//! whole name or, failing that, its part name (`Hero` for `Party/Hero`) -
//! wherever that leader is, through its own groups and follows.
//!
//! **A stage piece is left out of the 2D hierarchy**: the transform written
//! here is already where it lands on the screen, so
//! `hierarchy::resolve_world_transforms` takes it as a root - a parent's 2D
//! transform used to move the projected picture again, flat, over the scene.
//!
//! Stepped in `tick_motion`, so no host was wired. Scripts move things with
//! `Stage.place(e, x, y, z)` and change any field with `Stage.set(e, field,
//! value)` - a group's too, which moves everything under it. The stage writes
//! its objects' transforms every frame: move one through the stage, not with
//! `set_position`.

use std::collections::{HashMap, HashSet};

use glam::{Vec2, Vec3};
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Mesh2D, MeshVertex, Name, PointLight2D, SpotLight2D, SpriteComponent, TextComponent, Transform, WorldTransform};

/// The camera of a 3D stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Stage {
    /// The point looked at, in stage units (x east, y north, z up).
    pub target: [f32; 3],
    /// Turn about the vertical, radians: 0 looks north, positive turns the
    /// camera toward the east.
    pub yaw: f32,
    /// How far it looks down from the horizon, radians.
    pub pitch: f32,
    /// From the eye to the target, stage units: nearer is more perspective.
    pub distance: f32,
    /// World units per stage unit at the target's depth.
    pub zoom: f32,
    /// Nothing nearer the eye than this is drawn.
    pub near: f32,
    /// A named stage object the target follows (empty: none).
    pub follow: String,
    /// Added to the followed object's position.
    pub follow_offset: [f32; 3],
    /// How fast the target eases to it, per second (0 snaps).
    pub follow_ease: f32,
    /// The z of something upright at the target's depth.
    pub z_base: i32,
    /// z steps per stage unit of depth; nearer is higher.
    pub z_per_unit: f32,
    /// The band every `ground` quad draws in, under everything upright.
    pub ground_z: i32,
}

impl Default for Stage {
    fn default() -> Self {
        Self {
            target: [0.0; 3],
            yaw: 0.0,
            pitch: 0.6,
            distance: 40.0,
            zoom: 32.0,
            near: 0.5,
            follow: String::new(),
            follow_offset: [0.0; 3],
            follow_ease: 0.0,
            z_base: 0,
            z_per_unit: 10.0,
            ground_z: -20000,
        }
    }
}

/// A place and a turn in stage space: where a group's frame stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub offset: Vec3,
    /// Radians about the vertical, anticlockwise seen from above.
    pub yaw: f32,
}

impl Frame {
    pub const IDENTITY: Frame = Frame { offset: Vec3::ZERO, yaw: 0.0 };

    /// A frame moved and not turned.
    pub fn at(offset: Vec3) -> Self {
        Self { offset, yaw: 0.0 }
    }

    /// A direction in this frame, in the outer one.
    pub fn turn(&self, v: Vec3) -> Vec3 {
        if self.yaw == 0.0 {
            return v;
        }
        let (s, c) = self.yaw.sin_cos();
        Vec3::new(v.x * c - v.y * s, v.x * s + v.y * c, v.z)
    }

    /// A point in this frame, in the outer one.
    pub fn point(&self, p: Vec3) -> Vec3 {
        self.offset + self.turn(p)
    }

    /// The frame `local` describes inside this one.
    pub fn then(&self, local: Frame) -> Frame {
        Frame { offset: self.point(local.offset), yaw: self.yaw + local.yaw }
    }
}

/// A frame its children's stage coordinates are in: moved and turned, it
/// carries everything under it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StageGroup {
    /// Where the frame's origin stands, in the frame of the group above it.
    pub position: [f32; 3],
    /// Its turn about the vertical, radians, anticlockwise seen from above
    /// (a quarter turn takes east to north).
    pub yaw: f32,
    /// A named stage object it moves with, `position` then an offset from it.
    pub follow: String,
}

/// A flat piece of the stage, projected into its entity's `mesh`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StageQuad {
    /// The stage it is on, by name (empty: the first).
    pub stage: String,
    /// The picture's bottom-left corner, seen from the front.
    pub origin: [f32; 3],
    /// Its bottom edge (left to right) and its left edge (bottom to top).
    pub u: [f32; 3],
    pub v: [f32; 3],
    /// How many times the texture repeats along `u` and `v`.
    pub repeat: [f32; 2],
    /// A floor: drawn in the stage's ground band, under everything upright.
    pub ground: bool,
    /// Drawn from behind too (else a back-facing quad draws nothing).
    pub two_sided: bool,
    /// Stage units added to its sort depth (positive draws it as farther).
    pub sort_bias: f32,
    /// Added to the z it is given.
    pub z_bias: i32,
    /// A named stage object it moves with, `origin` then being an offset.
    pub follow: String,
    /// Each vertex's depth for `depth_of_field`, in z units (never saved).
    #[serde(skip)]
    pub view_z: Vec<f32>,
}

impl Default for StageQuad {
    fn default() -> Self {
        Self {
            stage: String::new(),
            origin: [0.0; 3],
            u: [1.0, 0.0, 0.0],
            v: [0.0, 0.0, 1.0],
            repeat: [1.0, 1.0],
            ground: false,
            two_sided: false,
            sort_bias: 0.0,
            z_bias: 0,
            follow: String::new(),
            view_z: Vec::new(),
        }
    }
}

/// Anything standing at a stage point: a sprite, a text, an emitter, a light.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StageBillboard {
    pub stage: String,
    /// Where it stands (a sprite's anchor - its feet, anchored `[0.5, 0]`).
    pub position: [f32; 3],
    /// 1 grows and shrinks with distance; 0 stays its own size; between.
    pub perspective: f32,
    /// Multiplied into the scale it is given (`-1` on x faces it the other way).
    pub scale: [f32; 2],
    /// Stage units added to its sort depth (positive draws it as farther).
    pub sort_bias: f32,
    pub z_bias: i32,
    /// A named stage object it moves with, `position` then being an offset.
    pub follow: String,
    /// On a point or spot light: its radius in stage units (0 leaves it).
    pub light_radius: f32,
    /// On a light: how much its pool is squashed into the floor (1 a
    /// circle). Absent: the sine of the pitch, a circle on the floor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub squash: Option<f32>,
    /// Its depth for `depth_of_field`, in z units (never saved).
    #[serde(skip)]
    pub view_z: f32,
    /// Throw its silhouette on the floor away from the lights near it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow: Option<StageShadow>,
    /// This frame's shadows, one a light (never saved).
    #[serde(skip)]
    pub shadow_quads: Vec<ShadowQuad>,
}

/// How a billboard throws its shadow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StageShadow {
    /// How dark at its feet, 0..1, under a light of strength 1.
    pub opacity: f32,
    /// How dark at its tip, as a share of the feet's.
    pub fade: f32,
    /// How many lights it casts for, the strongest at its feet first.
    pub lights: u32,
    /// The longest a shadow gets, in its own heights.
    pub max_length: f32,
    /// How tall it stands, stage units (0: its sprite's height at the
    /// target's depth, above its anchor).
    pub height: f32,
}

impl Default for StageShadow {
    fn default() -> Self {
        Self { opacity: 0.6, fade: 0.15, lights: 2, max_length: 3.0, height: 0.0 }
    }
}

/// One shadow on the floor: its corners on the screen (feet left, feet
/// right, tip right, tip left), `(u q, v q, q)` across the sprite's frame
/// (0..1, flipped as the sprite is), how dark each corner is, and its z.
#[derive(Debug, Clone, Copy)]
pub struct ShadowQuad {
    pub corners: [Vec2; 4],
    pub uvq: [[f32; 3]; 4],
    pub alpha: [f32; 4],
    pub z: i32,
}

/// A light standing on the stage, as a shadow sees it.
#[derive(Debug, Clone, Copy)]
pub struct StageLight {
    pub position: Vec3,
    /// Its reach on the floor, stage units.
    pub radius: f32,
    pub intensity: f32,
    pub falloff: f32,
}

impl StageLight {
    /// How strongly it lights a point on the floor.
    pub fn strength_at(&self, p: Vec2) -> f32 {
        let d = p.distance(self.position.truncate());
        if d >= self.radius {
            return 0.0;
        }
        self.intensity * (1.0 - d / self.radius).powf(self.falloff.max(0.01))
    }
}

/// The shadows a thing `width` wide and `height` tall standing at `foot`
/// throws from `lights` (the strongest `shadow.lights` of them that reach
/// it), projected through the camera. `flip` mirrors the frame, as the
/// sprite is.
pub fn shadow_quads(
    cam: &Camera,
    foot: Vec3,
    width: f32,
    height: f32,
    flip: bool,
    shadow: &StageShadow,
    lights: &[StageLight],
) -> Vec<ShadowQuad> {
    let mut lit: Vec<(f32, &StageLight)> =
        lights.iter().map(|l| (l.strength_at(foot.truncate()), l)).filter(|(s, _)| *s > 0.01).collect();
    lit.sort_by(|a, b| b.0.total_cmp(&a.0));
    let height = height.max(0.05);
    let mut out = Vec::new();
    // A light it carries, or stands right under, throws nothing worth drawing.
    lit.retain(|(_, l)| l.position.truncate().distance(foot.truncate()) > width * 0.75);
    for (strength, l) in lit.into_iter().take(shadow.lights as usize) {
        // The top, projected from the light onto the floor: P = L + (T - L) k
        // with k = lz / (lz - h). A lamp no higher than the thing would throw
        // it to the horizon: held just above its top, and clamped below.
        let lz = (l.position.z - foot.z).max(height * 1.05);
        let k = lz / (lz - height);
        let mut d = (foot.truncate() - l.position.truncate()) * (k - 1.0);
        let len = d.length();
        if len < height * 0.05 {
            continue;
        }
        let longest = shadow.max_length * height;
        if len > longest {
            d *= longest / len;
        }
        let dir = d.normalize();
        let perp = Vec2::new(-dir.y, dir.x);
        let half = width * 0.5;
        // The tip is wider, as a real projection's is.
        let tip_half = half * k.min(2.0);
        let f = foot.truncate();
        let ground = [f - perp * half, f + perp * half, f + d + perp * tip_half, f + d - perp * tip_half];
        let (u0, u1) = if flip { (1.0, 0.0) } else { (0.0, 1.0) };
        let uv = [[u0, 1.0], [u1, 1.0], [u1, 0.0], [u0, 0.0]];
        let a = (shadow.opacity * strength.min(1.0)).clamp(0.0, 1.0);
        let mut quad = ShadowQuad {
            corners: [Vec2::ZERO; 4],
            uvq: [[0.0; 3]; 4],
            alpha: [a, a, a * shadow.fade, a * shadow.fade],
            z: cam.ground_z.saturating_add(4),
        };
        let mut ok = true;
        for i in 0..4 {
            let p = ground[i].extend(foot.z + 0.005);
            match cam.project(p) {
                Some((s, _)) => {
                    let q = cam.distance / cam.depth(p).max(1e-4);
                    quad.corners[i] = s;
                    quad.uvq[i] = [uv[i][0] * q, uv[i][1] * q, q];
                }
                None => ok = false,
            }
        }
        if ok {
            out.push(quad);
        }
    }
    out
}

/// A solid that throws its shadow on the floor from the stage's lights.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StageCaster {
    pub stage: String,
    /// Its footprint on the floor, stage units - a convex polygon.
    pub footprint: Vec<[f32; 2]>,
    /// How high its base stands, and how tall it is above that.
    pub base: f32,
    pub height: f32,
    /// How dark under a light of strength 1, 0..1.
    pub opacity: f32,
    /// How many lights it casts for, the strongest at its middle first.
    pub lights: u32,
    /// The longest a shadow gets, in its own heights.
    pub max_length: f32,
    /// A named stage object it moves with, its footprint then an offset.
    pub follow: String,
    /// This frame's shadows (never saved).
    #[serde(skip)]
    pub shadow: CasterShadow,
}

impl Default for StageCaster {
    fn default() -> Self {
        Self {
            stage: String::new(),
            footprint: Vec::new(),
            base: 0.0,
            height: 1.0,
            opacity: 0.75,
            lights: 2,
            max_length: 3.0,
            follow: String::new(),
            shadow: CasterShadow::default(),
        }
    }
}

/// A caster's shadows for one frame: screen points, their colours (black at
/// the light's strength there), a triangle list, and the z they draw at.
#[derive(Debug, Clone, Default)]
pub struct CasterShadow {
    pub positions: Vec<Vec2>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    pub z: i32,
}

/// The convex hull of `points`, anticlockwise (Andrew's monotone chain).
pub fn convex_hull(mut points: Vec<Vec2>) -> Vec<Vec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup_by(|a, b| a.distance(*b) < 1e-5);
    if points.len() < 3 {
        return points;
    }
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    let mut lower: Vec<Vec2> = Vec::new();
    for &p in &points {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<Vec2> = Vec::new();
    for &p in points.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

fn inside_convex(poly: &[Vec2], p: Vec2) -> bool {
    let hull = convex_hull(poly.to_vec());
    hull.len() >= 3 && (0..hull.len()).all(|i| (hull[(i + 1) % hull.len()] - hull[i]).perp_dot(p - hull[i]) >= 0.0)
}

/// A solid's shadows from `lights`, projected through the camera; its
/// footprint and base are in `frame`.
pub fn caster_shadow(cam: &Camera, caster: &StageCaster, frame: Frame, lights: &[StageLight]) -> CasterShadow {
    let mut out = CasterShadow { z: cam.ground_z.saturating_add(3), ..Default::default() };
    let foot: Vec<Vec2> = caster.footprint.iter().map(|p| frame.point(Vec2::from(*p).extend(0.0)).truncate()).collect();
    if foot.len() < 3 || caster.height <= 0.0 {
        return out;
    }
    let base = caster.base + frame.offset.z;
    let middle = foot.iter().copied().sum::<Vec2>() / foot.len() as f32;
    let mut lit: Vec<(f32, &StageLight)> =
        lights.iter().map(|l| (l.strength_at(middle), l)).filter(|(s, _)| *s > 0.01).collect();
    lit.retain(|(_, l)| !inside_convex(&foot, l.position.truncate()));
    lit.sort_by(|a, b| b.0.total_cmp(&a.0));
    let h = caster.height;
    for (_, l) in lit.into_iter().take(caster.lights as usize) {
        let lz = (l.position.z - base).max(h * 1.05);
        let k = lz / (lz - h);
        let longest = caster.max_length * h;
        let mut points = foot.clone();
        for f in &foot {
            let mut d = (*f - l.position.truncate()) * (k - 1.0);
            if d.length() > longest {
                d *= longest / d.length();
            }
            points.push(*f + d);
        }
        let hull = convex_hull(points);
        if hull.len() < 3 {
            continue;
        }
        let projected: Option<Vec<Vec2>> =
            hull.iter().map(|p| cam.project(p.extend(base + 0.004)).map(|(s, _)| s)).collect();
        let Some(projected) = projected else { continue };
        let first = out.positions.len() as u32;
        for (p, s) in hull.iter().zip(projected) {
            let a = (caster.opacity * l.strength_at(*p).min(1.0)).clamp(0.0, 1.0);
            out.positions.push(s);
            out.colors.push([0.0, 0.0, 0.0, a]);
        }
        for i in 1..hull.len() as u32 - 1 {
            out.indices.extend_from_slice(&[first, first + i, first + i + 1]);
        }
    }
    out
}

impl Default for StageBillboard {
    fn default() -> Self {
        Self {
            stage: String::new(),
            position: [0.0; 3],
            perspective: 1.0,
            scale: [1.0, 1.0],
            sort_bias: 0.0,
            z_bias: 0,
            follow: String::new(),
            light_radius: 0.0,
            squash: None,
            view_z: 0.0,
            shadow: None,
            shadow_quads: Vec::new(),
        }
    }
}

/// How a stage's camera sees, worked out once a frame.
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub eye: Vec3,
    /// Forward, right and up of the eye.
    pub fwd: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    /// Forward along the ground.
    pub ground_fwd: Vec2,
    pub focal: f32,
    pub zoom: f32,
    pub distance: f32,
    pub near: f32,
    pub pitch: f32,
    /// Where the target lands, world units.
    pub centre: Vec2,
    /// The target's ground depth, which sorts at `z_base`.
    pub target_depth: f32,
    pub z_base: i32,
    pub z_per_unit: f32,
    pub ground_z: i32,
}

impl Stage {
    pub fn camera(&self, centre: Vec2) -> Camera {
        let pitch = self.pitch.clamp(-1.55, 1.55);
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = pitch.sin_cos();
        let ground_fwd = Vec2::new(sy, cy);
        let fwd = Vec3::new(sy * cp, cy * cp, -sp);
        let right = Vec3::new(cy, -sy, 0.0);
        let up = Vec3::new(sy * sp, cy * sp, cp);
        let distance = self.distance.max(0.01);
        let target = Vec3::from(self.target);
        let eye = target - fwd * distance;
        Camera {
            eye,
            fwd,
            right,
            up,
            ground_fwd,
            focal: self.zoom * distance,
            zoom: self.zoom.max(1e-4),
            distance,
            near: self.near.max(0.01),
            pitch,
            centre,
            target_depth: (target.truncate() - eye.truncate()).dot(ground_fwd),
            z_base: self.z_base,
            z_per_unit: self.z_per_unit,
            ground_z: self.ground_z,
        }
    }
}

impl Camera {
    /// How far a point is in front of the eye.
    pub fn depth(&self, p: Vec3) -> f32 {
        (p - self.eye).dot(self.fwd)
    }

    /// Where a stage point falls on the screen and how many world units a
    /// stage unit is there; `None` nearer than `near` (or behind).
    pub fn project(&self, p: Vec3) -> Option<(Vec2, f32)> {
        let d = p - self.eye;
        let depth = d.dot(self.fwd);
        if depth < self.near {
            return None;
        }
        let k = self.focal / depth;
        Some((self.centre + Vec2::new(d.dot(self.right), d.dot(self.up)) * k, k))
    }

    /// How far into the scene a point lies along the ground - the order
    /// things standing on a floor are painted in.
    pub fn ground_depth(&self, p: Vec3) -> f32 {
        (p.truncate() - self.eye.truncate()).dot(self.ground_fwd)
    }

    /// The z (continuous) of a ground depth: `z_base` at the target's.
    pub fn view_z(&self, ground_depth: f32) -> f32 {
        self.z_base as f32 - (ground_depth - self.target_depth) * self.z_per_unit
    }

    /// The z an upright thing at that ground depth sorts at.
    pub fn sort_z(&self, ground_depth: f32) -> i32 {
        let z = self.view_z(ground_depth).round();
        z.clamp(i32::MIN as f32 / 2.0, i32::MAX as f32 / 2.0) as i32
    }

    /// How much a light's pool on the floor is squashed: the floor's own
    /// foreshortening.
    pub fn floor_squash(&self) -> f32 {
        self.pitch.sin().abs().clamp(0.05, 1.0)
    }
}

/// A quad projected: its mesh about `anchor`, each vertex's view z, and the
/// z it sorts at. `None` when none of it is in front of the eye, or it faces
/// away and is not two-sided.
pub struct Projected {
    pub anchor: Vec2,
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
    pub view_z: Vec<f32>,
    pub z: i32,
}

/// A point of a cell before projection: where it is and its texture place.
#[derive(Clone, Copy)]
struct Corner {
    p: Vec3,
    uv: Vec2,
}

/// Keep what is at least `near` in front of the eye (Sutherland-Hodgman
/// against one plane), interpolating in stage space, where a texture
/// coordinate is linear.
fn clip_near(cam: &Camera, poly: &[Corner]) -> Vec<Corner> {
    let mut out = Vec::with_capacity(poly.len() + 2);
    let n = poly.len();
    // A hair in front of the plane, so a corner cut on it still projects.
    let plane = cam.near * 1.001 + 1e-4;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (da, db) = (cam.depth(a.p) - plane, cam.depth(b.p) - plane);
        if da >= 0.0 {
            out.push(a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            let t = da / (da - db);
            out.push(Corner { p: a.p + (b.p - a.p) * t, uv: a.uv + (b.uv - a.uv) * t });
        }
    }
    out
}

/// Project a quad whose coordinates are in `frame`: cut into a cell per
/// texture repeat, each clipped at the near plane and fanned.
pub fn project_quad(cam: &Camera, q: &StageQuad, frame: Frame) -> Option<Projected> {
    let origin = frame.point(Vec3::from(q.origin));
    let (u, v) = (frame.turn(Vec3::from(q.u)), frame.turn(Vec3::from(q.v)));
    let normal = u.cross(v);
    if !q.two_sided && normal.dot(cam.eye - origin) <= 0.0 {
        return None;
    }
    let reps = |r: f32| if r.is_finite() && r > 0.0 { r } else { 1.0 };
    let (ru, rv) = (reps(q.repeat[0]), reps(q.repeat[1]));
    let (nu, nv) = ((ru.ceil() as usize).clamp(1, 256), (rv.ceil() as usize).clamp(1, 256));
    // Each kept corner: where it is, its depth, where it lands, its texture place.
    let mut corners: Vec<(Vec3, f32, Vec2, Vec2)> = Vec::new();
    let mut indices = Vec::new();
    for j in 0..nv {
        let (s0, s1) = (j as f32 / rv, ((j + 1) as f32 / rv).min(1.0));
        for i in 0..nu {
            let (t0, t1) = (i as f32 / ru, ((i + 1) as f32 / ru).min(1.0));
            let (tu, tv) = ((t1 - t0) * ru, (s1 - s0) * rv);
            // Texture v runs down the picture: the bottom edge is v = 1.
            let cell = [
                Corner { p: origin + u * t0 + v * s0, uv: Vec2::new(0.0, 1.0) },
                Corner { p: origin + u * t1 + v * s0, uv: Vec2::new(tu, 1.0) },
                Corner { p: origin + u * t1 + v * s1, uv: Vec2::new(tu, 1.0 - tv) },
                Corner { p: origin + u * t0 + v * s1, uv: Vec2::new(0.0, 1.0 - tv) },
            ];
            let poly = clip_near(cam, &cell);
            if poly.len() < 3 {
                continue;
            }
            // A clipped corner can still miss the eye's plane by a rounding
            // error: keep the cell only if every corner projects.
            let Some(kept) = poly.iter().map(|c| cam.project(c.p).map(|(s, _)| (c.p, cam.depth(c.p), s, c.uv))).collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let base = corners.len() as u32;
            corners.extend(kept);
            for k in 1..poly.len() as u32 - 1 {
                indices.extend_from_slice(&[base, base + k, base + k + 1]);
            }
        }
    }
    if corners.is_empty() {
        return None;
    }
    let anchor = corners.iter().map(|c| c.2).sum::<Vec2>() / corners.len() as f32;
    let vertices = corners
        .iter()
        .map(|(_, depth, s, uv)| MeshVertex {
            position: (*s - anchor).into(),
            uv: (*uv).into(),
            color: [1.0; 4],
            // 1 at the target's depth, so nothing is far from 1.
            q: cam.distance / depth.max(1e-4),
        })
        .collect();
    let view_z = corners.iter().map(|(p, ..)| cam.view_z(cam.ground_depth(*p))).collect();
    let centroid = origin + (u + v) * 0.5;
    let z = if q.ground {
        cam.ground_z.saturating_add(q.z_bias)
    } else {
        cam.sort_z(cam.ground_depth(centroid) + q.sort_bias).saturating_add(q.z_bias)
    };
    Some(Projected { anchor, vertices, indices, view_z, z })
}

fn position_of(world: &World, e: Entity) -> Vec2 {
    world
        .get::<&WorldTransform>(e)
        .map(|w| w.position)
        .or_else(|_| world.get::<&Transform>(e).map(|t| t.position))
        .unwrap_or_default()
}

/// A stage object as the frames see it.
struct Node {
    follow: String,
    /// A group's own frame, in the frame above it.
    group: Option<Frame>,
    /// The point a follower is placed from, in its frame (a billboard's
    /// position, a quad's origin).
    point: Vec3,
}

/// Where every stage object's coordinates are: through its groups, or from
/// its leader. Worked out once a frame, each object once.
pub struct Frames {
    nodes: HashMap<Entity, Node>,
    parents: HashMap<Entity, Entity>,
    names: HashMap<String, Entity>,
    parts: HashMap<String, Entity>,
    base: HashMap<Entity, Frame>,
    busy: HashSet<Entity>,
}

impl Frames {
    /// Every group, billboard, quad and caster, their parents and names.
    pub fn new(world: &World) -> Self {
        let mut nodes = HashMap::new();
        for (e, c) in world.query::<&StageCaster>().iter() {
            nodes.insert(e, Node { follow: c.follow.clone(), group: None, point: Vec3::ZERO });
        }
        for (e, q) in world.query::<&StageQuad>().iter() {
            nodes.insert(e, Node { follow: q.follow.clone(), group: None, point: Vec3::from(q.origin) });
        }
        for (e, b) in world.query::<&StageBillboard>().iter() {
            nodes.insert(e, Node { follow: b.follow.clone(), group: None, point: Vec3::from(b.position) });
        }
        // A group last: on an entity that is also a piece, the piece is in
        // the group's frame, so it is the group that is followed and placed.
        for (e, g) in world.query::<&StageGroup>().iter() {
            let group = Some(Frame { offset: Vec3::from(g.position), yaw: g.yaw });
            nodes.insert(e, Node { follow: g.follow.clone(), group, point: Vec3::ZERO });
        }
        let parents = world.query::<&crate::ecs::components::Parent>().iter().map(|(e, p)| (e, p.0)).collect();
        // A billboard wins a name it shares with another stage object, as it
        // was the only thing that could be followed.
        let mut named: Vec<(bool, u32, Entity, String)> = world
            .query::<&Name>()
            .iter()
            .filter(|(e, _)| nodes.contains_key(e))
            .map(|(e, n)| (world.get::<&StageBillboard>(e).is_ok(), e.id(), e, n.0.clone()))
            .collect();
        named.sort_by_key(|(billboard, id, ..)| (!*billboard, *id));
        let (mut names, mut parts) = (HashMap::new(), HashMap::new());
        for (_, _, e, name) in named {
            parts.entry(crate::ecs::hierarchy::part_name(&name).to_string()).or_insert(e);
            names.entry(name).or_insert(e);
        }
        Self { nodes, parents, names, parts, base: HashMap::new(), busy: HashSet::new() }
    }

    /// The stage objects worked out.
    pub fn objects(&self) -> Vec<Entity> {
        self.nodes.keys().copied().collect()
    }

    /// The stage object a `follow` names: by its whole name, else its part name.
    pub fn leader(&self, name: &str) -> Option<Entity> {
        if name.is_empty() {
            return None;
        }
        self.names.get(name).or_else(|| self.parts.get(crate::ecs::hierarchy::part_name(name))).copied()
    }

    /// The frame an object's own coordinates are in: its leader's point if it
    /// follows one, else its nearest group's frame, else the stage's.
    pub fn base(&mut self, e: Entity) -> Frame {
        if let Some(f) = self.base.get(&e) {
            return *f;
        }
        // A loop of follows or groups: the stage's own frame breaks it.
        if !self.busy.insert(e) {
            return Frame::IDENTITY;
        }
        let follow = self.nodes.get(&e).map(|n| n.follow.clone()).unwrap_or_default();
        let frame = match self.leader(&follow) {
            Some(l) if l != e => Frame::at(self.point(l)),
            _ => self.groups_above(e),
        };
        self.busy.remove(&e);
        self.base.insert(e, frame);
        frame
    }

    /// The frame of the nearest group above `e`, through plain parents.
    fn groups_above(&mut self, e: Entity) -> Frame {
        let mut at = self.parents.get(&e).copied();
        for _ in 0..256 {
            let Some(a) = at else { break };
            if self.nodes.get(&a).is_some_and(|n| n.group.is_some()) {
                return self.frame(a);
            }
            at = self.parents.get(&a).copied();
        }
        Frame::IDENTITY
    }

    /// The frame inside a group, for what is under it.
    fn frame(&mut self, g: Entity) -> Frame {
        let base = self.base(g);
        match self.nodes.get(&g).and_then(|n| n.group) {
            Some(local) => base.then(local),
            None => base,
        }
    }

    /// The frame `e`'s own coordinates are in, for a piece; the frame inside
    /// it, for a group.
    pub fn frame_of(&mut self, e: Entity) -> Frame {
        if self.nodes.get(&e).is_some_and(|n| n.group.is_some()) { self.frame(e) } else { self.base(e) }
    }

    /// Where a stage object stands, as a follower sees it: a group's origin,
    /// a billboard's position, a quad's corner, a caster's frame.
    pub fn point(&mut self, e: Entity) -> Vec3 {
        let Some(node) = self.nodes.get(&e) else { return Vec3::ZERO };
        if node.group.is_some() {
            return self.frame(e).offset;
        }
        let local = node.point;
        self.base(e).point(local)
    }
}

/// Follow, then project every quad and place every billboard. Called from
/// `tick_motion`.
pub fn tick_stage(world: &mut World, dt: f32) {
    let stages: Vec<Entity> = world.query::<&Stage>().iter().map(|(e, _)| e).collect();
    if stages.is_empty() {
        return;
    }
    // Where every piece's coordinates are, through its groups and follows.
    let mut frames = Frames::new(world);
    let bases: HashMap<Entity, Frame> = frames.objects().into_iter().map(|e| (e, frames.base(e))).collect();
    let base_of = |e: Entity| bases.get(&e).copied().unwrap_or(Frame::IDENTITY);

    for &s in &stages {
        let follow = match world.get::<&Stage>(s) {
            Ok(st) => st.follow.clone(),
            Err(_) => continue,
        };
        let Some(leader) = frames.leader(&follow) else { continue };
        let at = frames.point(leader);
        let Ok(mut st) = world.get::<&mut Stage>(s) else { continue };
        let want = at + Vec3::from(st.follow_offset);
        let now = Vec3::from(st.target);
        let t = if st.follow_ease > 0.0 { 1.0 - (-st.follow_ease * dt.max(0.0)).exp() } else { 1.0 };
        st.target = (now + (want - now) * t).into();
    }
    let cameras: Vec<(String, Camera)> = stages
        .iter()
        .filter_map(|&s| {
            let cam = world.get::<&Stage>(s).ok()?.camera(position_of(world, s));
            let name = world.get::<&Name>(s).map(|n| n.0.clone()).unwrap_or_default();
            Some((name, cam))
        })
        .collect();
    let camera_for = |stage: &str| cameras.iter().find(|(n, _)| stage.is_empty() || n == stage).map(|(_, c)| *c);

    // The stage's lights, where they stand, for shadows.
    let mut lights: Vec<StageLight> = Vec::new();
    for (e, (b, point, spot)) in world.query::<(&StageBillboard, Option<&PointLight2D>, Option<&SpotLight2D>)>().iter() {
        if b.light_radius <= 0.0 {
            continue;
        }
        let (intensity, falloff) = match (point, spot) {
            (Some(l), _) if !l.subtract => (l.intensity, l.falloff),
            (_, Some(l)) if !l.subtract => (l.intensity, l.falloff),
            _ => continue,
        };
        let position = base_of(e).point(Vec3::from(b.position));
        lights.push(StageLight { position, radius: b.light_radius, intensity, falloff });
    }

    // Billboards.
    for (e, (b, t, sprite, text, emitter, point, spot)) in world.query_mut::<(
        &mut StageBillboard,
        &mut Transform,
        Option<&mut SpriteComponent>,
        Option<&mut TextComponent>,
        Option<&mut crate::particles::ParticleEmitter>,
        Option<&mut PointLight2D>,
        Option<&mut SpotLight2D>,
    )>() {
        let Some(cam) = camera_for(&b.stage) else { continue };
        let p = base_of(e).point(Vec3::from(b.position));
        t.rotation = 0.0;
        let Some((at, k)) = cam.project(p) else {
            t.scale = Vec2::ZERO;
            continue;
        };
        let size = (k / cam.zoom).powf(b.perspective.clamp(0.0, 1.0));
        t.position = at;
        t.scale = Vec2::from(b.scale) * size;
        let depth = cam.ground_depth(p) + b.sort_bias;
        b.view_z = cam.view_z(depth);
        let z = cam.sort_z(depth).saturating_add(b.z_bias);
        b.shadow_quads.clear();
        if let Some(s) = sprite {
            s.z_order = z;
            if let Some(shadow) = &b.shadow
                && let Some(size) = s.size
            {
                let width = size[0] * b.scale[0].abs() / cam.zoom;
                let height = if shadow.height > 0.0 {
                    shadow.height
                } else {
                    size[1] * b.scale[1].abs() * (1.0 - s.anchor[1]) / cam.zoom
                };
                b.shadow_quads = shadow_quads(&cam, p, width, height, b.scale[0] < 0.0, shadow, &lights);
            }
        }
        if let Some(tx) = text {
            tx.z_order = z;
        }
        if let Some(em) = emitter {
            em.z_order = z;
        }
        let squash = b.squash.unwrap_or_else(|| cam.floor_squash()).clamp(0.05, 1.0);
        if let Some(l) = point {
            if b.light_radius > 0.0 {
                l.radius = b.light_radius * k;
            }
            l.squash = squash;
        }
        if let Some(l) = spot {
            if b.light_radius > 0.0 {
                l.radius = b.light_radius * k;
            }
            l.squash = squash;
        }
    }

    // Solids' shadows.
    for (e, c) in world.query_mut::<&mut StageCaster>() {
        let Some(cam) = camera_for(&c.stage) else { continue };
        c.shadow = caster_shadow(&cam, c, base_of(e), &lights);
    }

    // Quads, into their meshes - one made for a quad that has none.
    let missing: Vec<Entity> =
        world.query::<&StageQuad>().without::<&Mesh2D>().iter().map(|(e, _)| e).collect();
    for e in missing {
        let _ = world.insert_one(e, Mesh2D::default());
    }
    for (e, (q, t, mesh)) in world.query_mut::<(&mut StageQuad, &mut Transform, &mut Mesh2D)>() {
        let Some(cam) = camera_for(&q.stage) else { continue };
        t.rotation = 0.0;
        t.scale = Vec2::ONE;
        match project_quad(&cam, q, base_of(e)) {
            Some(p) => {
                t.position = p.anchor;
                mesh.vertices = p.vertices;
                mesh.indices = p.indices;
                mesh.z_order = p.z;
                q.view_z = p.view_z;
            }
            None => {
                mesh.vertices.clear();
                mesh.indices.clear();
                q.view_z.clear();
            }
        }
    }
}

/// Whether the stage places `e` on the screen itself - a quad or a
/// billboard, whose transform it writes - so the 2D hierarchy must not move
/// it again (`hierarchy::resolve_world_transforms`).
pub fn places_on_screen(world: &World, e: Entity) -> bool {
    world.get::<&StageQuad>(e).is_ok() || world.get::<&StageBillboard>(e).is_ok()
}

const NOT_ON_A_STAGE: &str = "no stage, stage_group, stage_quad, stage_billboard or stage_caster on that entity";

/// `Stage.place(e, x, y, z)`: move a group (and everything under it), stand a
/// billboard, move a quad's corner, or point a stage's camera.
pub fn place(world: &mut World, e: Entity, x: f32, y: f32, z: f32) -> Result<(), String> {
    if let Ok(mut g) = world.get::<&mut StageGroup>(e) {
        g.position = [x, y, z];
        return Ok(());
    }
    if let Ok(mut b) = world.get::<&mut StageBillboard>(e) {
        b.position = [x, y, z];
        return Ok(());
    }
    if let Ok(mut q) = world.get::<&mut StageQuad>(e) {
        q.origin = [x, y, z];
        return Ok(());
    }
    if let Ok(mut s) = world.get::<&mut Stage>(e) {
        s.target = [x, y, z];
        return Ok(());
    }
    Err(NOT_ON_A_STAGE.into())
}

fn set_json<T: Serialize + serde::de::DeserializeOwned>(c: &mut T, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut v = serde_json::to_value(&*c).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("not an object")?;
    // `squash` and `shadow` are serialised only when set.
    if !obj.contains_key(field) && field != "squash" && field != "shadow" {
        return Err(format!("no field '{field}'"));
    }
    obj.insert(field.to_string(), value.clone());
    *c = serde_json::from_value(v).map_err(|e| format!("{field}: {e}"))?;
    Ok(())
}

/// `Stage.set(e, field, value)`: any field of the entity's group, else its
/// billboard, else its quad, else its caster, else its stage.
pub fn set_field(world: &mut World, e: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let only_billboards = field == "squash" || field == "shadow";
    if !only_billboards && let Ok(mut g) = world.get::<&mut StageGroup>(e) {
        return set_json(&mut *g, field, value);
    }
    if let Ok(mut b) = world.get::<&mut StageBillboard>(e) {
        return set_json(&mut *b, field, value);
    }
    if let Ok(mut q) = world.get::<&mut StageQuad>(e) {
        if field == "squash" || field == "shadow" {
            return Err(format!("no field '{field}'"));
        }
        return set_json(&mut *q, field, value);
    }
    if let Ok(mut c) = world.get::<&mut StageCaster>(e) {
        if only_billboards {
            return Err(format!("no field '{field}'"));
        }
        return set_json(&mut *c, field, value);
    }
    if let Ok(mut s) = world.get::<&mut Stage>(e) {
        if field == "squash" || field == "shadow" {
            return Err(format!("no field '{field}'"));
        }
        return set_json(&mut *s, field, value);
    }
    Err(NOT_ON_A_STAGE.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> Camera {
        Stage { target: [0.0, 0.0, 0.0], yaw: 0.0, pitch: 0.6, distance: 40.0, zoom: 32.0, ..Default::default() }
            .camera(Vec2::new(100.0, 50.0))
    }

    #[test]
    fn the_target_lands_on_the_stage_entity() {
        let cam = camera();
        let (at, k) = cam.project(Vec3::ZERO).unwrap();
        assert!(at.distance(Vec2::new(100.0, 50.0)) < 1e-3, "{at}");
        assert!((k - 32.0).abs() < 1e-3, "zoom at the target: {k}");
        // North is up the screen and smaller; up is up; east is right.
        let (north, kn) = cam.project(Vec3::new(0.0, 10.0, 0.0)).unwrap();
        assert!(north.y > at.y && kn < k);
        let (above, _) = cam.project(Vec3::new(0.0, 0.0, 3.0)).unwrap();
        assert!(above.y > at.y && (above.x - at.x).abs() < 1e-3);
        let (east, _) = cam.project(Vec3::new(4.0, 0.0, 0.0)).unwrap();
        assert!(east.x > at.x);
        // Behind the eye: nothing.
        assert!(cam.project(cam.eye - cam.fwd * 2.0).is_none());
    }

    #[test]
    fn turning_the_camera_swings_the_world() {
        // Yaw a quarter turn: the camera looks east, so east is up the screen.
        let cam = Stage { yaw: std::f32::consts::FRAC_PI_2, ..Default::default() }.camera(Vec2::ZERO);
        let (east, _) = cam.project(Vec3::new(6.0, 0.0, 0.0)).unwrap();
        let (north, _) = cam.project(Vec3::new(0.0, 6.0, 0.0)).unwrap();
        assert!(east.y > 1.0 && east.x.abs() < 1e-3, "{east}");
        assert!(north.x < -1.0, "north is to the left: {north}");
    }

    #[test]
    fn a_quad_is_cut_into_its_repeats_and_textured_in_perspective() {
        let cam = camera();
        let floor = StageQuad { origin: [-4.0, -2.0, 0.0], u: [8.0, 0.0, 0.0], v: [0.0, 6.0, 0.0], repeat: [4.0, 3.0], ground: true, ..Default::default() };
        let p = project_quad(&cam, &floor, Frame::IDENTITY).unwrap();
        assert_eq!(p.vertices.len(), 4 * 12, "a cell per repeat, four corners each");
        assert_eq!(p.indices.len(), 6 * 12);
        assert_eq!(p.z, cam.ground_z, "a floor draws in the ground band");
        // The far edge carries less weight: q is 1 / depth.
        let (qn, qf) = (p.vertices[0].q, p.vertices[4 * 11 + 2].q);
        assert!(qf < qn, "{qn} {qf}");
        // A point a third of the way up a cell, interpolated in screen space
        // as the rasteriser does and divided by q, is the stage point there.
        let cell = &p.vertices[0..4];
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let t = 0.33;
        // Down the cell's left edge: corner 0 (bottom) to corner 3 (top).
        let (b, top) = (&cell[0], &cell[3]);
        let (uq, vq, q) = (
            lerp(b.uv[0] * b.q, top.uv[0] * top.q, t),
            lerp(b.uv[1] * b.q, top.uv[1] * top.q, t),
            lerp(b.q, top.q, t),
        );
        let (u, v) = (uq / q, vq / q);
        // The screen point there, unprojected onto the floor's left edge.
        let s = Vec2::from(b.position).lerp(Vec2::from(top.position), t) + p.anchor;
        let along = (0..=1000)
            .map(|i| i as f32 / 1000.0 * 2.0)
            .min_by(|x, y| {
                let d = |yy: f32| cam.project(Vec3::new(-4.0, -2.0 + yy, 0.0)).unwrap().0.distance(s);
                d(*x).total_cmp(&d(*y))
            })
            .unwrap();
        assert!(u.abs() < 1e-4, "the left edge is u 0: {u}");
        assert!((v - (1.0 - along / 2.0)).abs() < 2e-3, "v in perspective: {v} against {}", 1.0 - along / 2.0);
        // The depth for focus runs with the floor: nearer is higher z.
        assert!(p.view_z[0] > p.view_z[4 * 11 + 2]);
    }

    #[test]
    fn a_back_face_draws_nothing_and_the_near_plane_cuts() {
        let cam = camera();
        let front = StageQuad { origin: [-1.0, 3.0, 0.0], u: [2.0, 0.0, 0.0], v: [0.0, 0.0, 2.0], ..Default::default() };
        assert!(project_quad(&cam, &front, Frame::IDENTITY).is_some(), "u east, v up faces south, toward the camera");
        let back = StageQuad { u: [-2.0, 0.0, 0.0], origin: [1.0, 3.0, 0.0], ..front.clone() };
        assert!(project_quad(&cam, &back, Frame::IDENTITY).is_none(), "facing north: culled");
        assert!(project_quad(&cam, &StageQuad { two_sided: true, ..back }, Frame::IDENTITY).is_some());
        // A floor running under the eye and past it is clipped, not dropped.
        let long = StageQuad { origin: [-2.0, -80.0, 0.0], u: [4.0, 0.0, 0.0], v: [0.0, 100.0, 0.0], ground: true, ..Default::default() };
        let p = project_quad(&cam, &long, Frame::IDENTITY).unwrap();
        assert!(p.vertices.iter().all(|v| v.q.is_finite() && v.q > 0.0));
        assert!(p.vertices.len() >= 4);
    }

    #[test]
    fn nearer_things_sort_in_front_and_floors_under_all() {
        let mut world = World::new();
        world.spawn((Name("Stage".into()), Transform::default(), Stage::default()));
        let sprite = |world: &mut World, y: f32| {
            world.spawn((
                Transform::default(),
                SpriteComponent { texture_id: "hero.png".into(), size: Some([16.0, 24.0]), anchor: [0.5, 0.0], ..Default::default() },
                StageBillboard { position: [0.0, y, 0.0], ..Default::default() },
            ))
        };
        let near = sprite(&mut world, -3.0);
        let far = sprite(&mut world, 8.0);
        let wall = world.spawn((
            Transform::default(),
            StageQuad { origin: [-2.0, 4.0, 0.0], u: [4.0, 0.0, 0.0], v: [0.0, 0.0, 3.0], ..Default::default() },
        ));
        let floor = world.spawn((
            Transform::default(),
            StageQuad { origin: [-5.0, -5.0, 0.0], u: [10.0, 0.0, 0.0], v: [0.0, 20.0, 0.0], ground: true, ..Default::default() },
        ));
        tick_stage(&mut world, 1.0 / 60.0);
        let z = |e: Entity| world.get::<&SpriteComponent>(e).unwrap().z_order;
        let mz = |e: Entity| world.get::<&Mesh2D>(e).unwrap().z_order;
        assert!(z(near) > mz(wall) && mz(wall) > z(far), "near {} wall {} far {}", z(near), mz(wall), z(far));
        assert!(mz(floor) < z(far), "the floor is under everything standing on it");
        assert!(!world.get::<&Mesh2D>(wall).unwrap().vertices.is_empty(), "a mesh was made for the wall");
        // Nearer is drawn bigger, and focus at z_base is the target's depth.
        let s = |e: Entity| world.get::<&Transform>(e).unwrap().scale.x;
        assert!(s(near) > 1.0 && s(far) < 1.0, "{} {}", s(near), s(far));
        let b = world.get::<&StageBillboard>(near).unwrap().view_z;
        assert!(b > 0.0);
    }

    #[test]
    fn lights_shrink_with_distance_and_squash_into_the_floor() {
        let mut world = World::new();
        world.spawn((Transform::default(), Stage { pitch: 0.5, ..Default::default() }));
        let lamp = world.spawn((
            Transform::default(),
            PointLight2D { radius: 1.0, ..Default::default() },
            StageBillboard { position: [0.0, 0.0, 0.0], light_radius: 4.0, ..Default::default() },
        ));
        tick_stage(&mut world, 0.0);
        let l = world.get::<&PointLight2D>(lamp).unwrap().clone();
        assert!((l.radius - 4.0 * 32.0).abs() < 1e-2, "4 units at the target: {}", l.radius);
        assert!((l.squash - 0.5f32.sin()).abs() < 1e-4);
    }

    #[test]
    fn a_lamp_throws_a_shadow_away_from_itself() {
        let cam = camera();
        let lamp = StageLight { position: Vec3::new(-3.0, 0.0, 3.0), radius: 10.0, intensity: 1.0, falloff: 1.0 };
        let shadow = StageShadow { lights: 2, ..Default::default() };
        let quads = shadow_quads(&cam, Vec3::ZERO, 0.8, 1.6, false, &shadow, &[lamp]);
        assert_eq!(quads.len(), 1);
        // From a lamp 3 west and 3 up, a thing 1.6 tall throws its top to
        // 3 * (3 / 1.4 - 1) = 3.43 east of its feet.
        let (tip, _) = cam.project(Vec3::new(3.0 * (3.0 / 1.4 - 1.0), 0.0, 0.005)).unwrap();
        // Half way between the tip's corners on the screen, which perspective
        // puts a fraction of a pixel from the projection of the half way point.
        let mid_tip = (quads[0].corners[2] + quads[0].corners[3]) * 0.5;
        assert!(mid_tip.distance(tip) < 0.5, "{mid_tip} {tip}");
        // Darker at the feet than the tip; under the standing things.
        assert!(quads[0].alpha[0] > quads[0].alpha[2]);
        assert_eq!(quads[0].z, cam.ground_z + 4);
        // A lamp overhead throws nothing to speak of; one out of reach, none.
        let over = StageLight { position: Vec3::new(0.0, 0.0, 4.0), ..lamp };
        assert!(shadow_quads(&cam, Vec3::ZERO, 0.8, 1.6, false, &shadow, &[over]).is_empty());
        let far = StageLight { position: Vec3::new(-30.0, 0.0, 3.0), ..lamp };
        assert!(shadow_quads(&cam, Vec3::ZERO, 0.8, 1.6, false, &shadow, &[far]).is_empty());
        // A low lamp's shadow is held to `max_length` heights.
        let low = StageLight { position: Vec3::new(-2.0, 0.0, 1.0), ..lamp };
        let q = shadow_quads(&cam, Vec3::ZERO, 0.8, 1.6, false, &shadow, &[low])[0];
        let (cap, _) = cam.project(Vec3::new(3.0 * 1.6, 0.0, 0.005)).unwrap();
        assert!(((q.corners[2] + q.corners[3]) * 0.5).distance(cap) < 0.5);
    }

    #[test]
    fn a_house_throws_one_shadow_away_from_the_lamp() {
        let cam = camera();
        let house = StageCaster {
            footprint: vec![[1.0, -1.0], [3.0, -1.0], [3.0, 1.0], [1.0, 1.0]],
            height: 2.0,
            opacity: 1.0,
            ..Default::default()
        };
        let lamp = StageLight { position: Vec3::new(-2.0, 0.0, 3.0), radius: 20.0, intensity: 2.0, falloff: 1.0 };
        let s = caster_shadow(&cam, &house, Frame::IDENTITY, &[lamp]);
        // The hull of a box's footprint and its projected top: six corners at
        // most, one fan, darker near the lamp's side.
        assert!(s.positions.len() >= 4 && s.positions.len() <= 8, "{}", s.positions.len());
        assert_eq!(s.indices.len(), (s.positions.len() - 2) * 3);
        assert_eq!(s.z, cam.ground_z + 3);
        // Its far edge: the top corner (3, 1), 2 tall, from a lamp 3 up and
        // 5 west of it, would land 10 further east - held to 3 heights (6
        // units), near x = 8.9. So it reaches well past x = 6.
        let far_x = s.positions.iter().map(|p| p.x).fold(f32::MIN, f32::max);
        let (six, _) = cam.project(Vec3::new(6.0, 0.0, 0.0)).unwrap();
        assert!(far_x > six.x, "reaches out east: {far_x} vs {}", six.x);
        // Nothing west of the house.
        let (west, _) = cam.project(Vec3::new(0.9, 0.0, 0.0)).unwrap();
        assert!(s.positions.iter().all(|p| p.x >= west.x - 1.0));
        // A lamp inside throws nothing; nor does one out of reach.
        let inside = StageLight { position: Vec3::new(2.0, 0.0, 1.0), ..lamp };
        assert!(caster_shadow(&cam, &house, Frame::IDENTITY, &[inside]).positions.is_empty());
        let far = StageLight { position: Vec3::new(-40.0, 0.0, 3.0), ..lamp };
        assert!(caster_shadow(&cam, &house, Frame::IDENTITY, &[far]).positions.is_empty());
    }

    #[test]
    fn a_walker_by_two_lamps_throws_two_shadows() {
        let mut world = World::new();
        world.spawn((Transform::default(), Stage::default()));
        for x in [-3.0, 3.0] {
            world.spawn((
                Transform::default(),
                PointLight2D::default(),
                StageBillboard { position: [x, 0.0, 2.5], light_radius: 8.0, ..Default::default() },
            ));
        }
        let hero = world.spawn((
            Transform::default(),
            SpriteComponent { texture_id: "hero.png".into(), size: Some([32.0, 64.0]), anchor: [0.5, 0.0], ..Default::default() },
            StageBillboard { shadow: Some(StageShadow::default()), ..Default::default() },
        ));
        tick_stage(&mut world, 1.0 / 60.0);
        let quads = world.get::<&StageBillboard>(hero).unwrap().shadow_quads.clone();
        assert_eq!(quads.len(), 2, "one a lamp");
        // One runs right, the other left.
        let tips: Vec<f32> = quads.iter().map(|q| (q.corners[2].x + q.corners[3].x) * 0.5).collect();
        assert!(tips.iter().any(|&x| x > 1.0) && tips.iter().any(|&x| x < -1.0), "{tips:?}");
        // A script can switch it off.
        set_field(&mut world, hero, "shadow", &serde_json::Value::Null).unwrap();
        tick_stage(&mut world, 1.0 / 60.0);
        assert!(world.get::<&StageBillboard>(hero).unwrap().shadow_quads.is_empty());
    }

    #[test]
    fn the_camera_follows_and_scripts_set_fields() {
        let mut world = World::new();
        let stage = world.spawn((Transform::default(), Stage { follow: "Hero".into(), follow_offset: [0.0, 0.0, 1.0], ..Default::default() }));
        let hero = world.spawn((Name("Hero".into()), Transform::default(), StageBillboard::default()));
        place(&mut world, hero, 3.0, 4.0, 0.0).unwrap();
        tick_stage(&mut world, 1.0 / 60.0);
        assert_eq!(world.get::<&Stage>(stage).unwrap().target, [3.0, 4.0, 1.0]);
        set_field(&mut world, stage, "yaw", &serde_json::json!(0.5)).unwrap();
        set_field(&mut world, hero, "squash", &serde_json::json!(0.3)).unwrap();
        assert_eq!(world.get::<&StageBillboard>(hero).unwrap().squash, Some(0.3));
        assert!(set_field(&mut world, stage, "nonsense", &serde_json::json!(1)).is_err());
        assert_eq!(world.get::<&Stage>(stage).unwrap().yaw, 0.5);
    }
}
