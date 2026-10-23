//! Skeletal animation (slice 3.5.C).
//!
//! Provides time-based bone keyframe playback that drives the local
//! [`Transform`] of bones inside a skeleton built with slice 3.5.B
//! (`SkeletonRoot` + `Bone`). The hierarchy resolver (slice 3.5.A) then
//! composes those locals into `WorldTransform`s for rendering.
//!
//! Pipeline per frame:
//! ```text
//! update_skeletal_players  →  resolve_world_transforms  →  render
//! ```
//!
//! Distinct from [`crate::animation::AnimationClip`], which is the
//! sprite-sheet frame animation system. Sprite-frame and skeletal
//! animation can coexist on the same entity (the former drives the
//! sprite's UVs, the latter drives bone Transforms).

use std::collections::HashMap;

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Bone, Parent, SkeletonRoot, Transform};
use crate::ecs::skeleton::find_bone;
use hecs::{Entity, World};

/// Single keyframe on a `Vec2` track (position or scale).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Vec2Key {
    /// Time in seconds from the clip start.
    pub time: f32,
    pub value: [f32; 2],
}

/// Single keyframe on a scalar track (rotation, radians).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ScalarKey {
    /// Time in seconds from the clip start.
    pub time: f32,
    pub value: f32,
}

/// All keyframes targeting a single bone within a clip. Any track that
/// is empty is treated as "do not override" — the bone keeps whatever
/// local value it had before sampling.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BoneTrack {
    pub bone_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub position: Vec<Vec2Key>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rotation: Vec<ScalarKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scale: Vec<Vec2Key>,
}

/// A full skeletal-animation clip: a fixed duration and one
/// [`BoneTrack`] per animated bone. Authored as a `.skelanim` JSON
/// file or constructed programmatically by importers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkeletalClip {
    pub name: String,
    /// Clip length in seconds. Playback wraps or clamps at this value
    /// depending on [`SkeletalPlayer::looping`].
    pub duration: f32,
    #[serde(default)]
    pub tracks: Vec<BoneTrack>,
}

/// Plays a [`SkeletalClip`] back against the [`SkeletonRoot`] on the
/// **same entity**. To use, attach to the same entity that carries
/// `SkeletonRoot`.
///
/// Two clips can be mixed (Sep 2026): `blend_clip` at `blend` (0 is `clip`
/// alone, 1 is `blend_clip` alone), both sampled at the **same phase** - the
/// same fraction of their own length - so a walk and a run of different
/// lengths blend step for step, and the cycle's pace moves between theirs
/// with the weight. That is a 1D blend space: walk into a run by speed. And
/// a change of clip can **crossfade** ([`Self::crossfade`]): the clip being
/// left keeps playing and fades out while the new one fades in. Before
/// either, a change of clip snapped the whole skeleton in one frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkeletalPlayer {
    /// Asset id of the clip to play. The clip must be registered in
    /// the [`crate::asset::AssetManager`] under this id.
    pub clip: String,
    /// Current playback head, in seconds.
    #[serde(default)]
    pub time: f32,
    /// Playback rate multiplier. `1.0` = real-time; negative values
    /// play backwards.
    #[serde(default = "default_speed")]
    pub speed: f32,
    /// When true, time wraps at `duration`. When false, time clamps
    /// at `duration` and `playing` flips to `false` on completion.
    #[serde(default = "default_true")]
    pub looping: bool,
    /// When false, the player holds its current pose without
    /// advancing `time`.
    #[serde(default = "default_true")]
    pub playing: bool,
    /// A second clip mixed in by `blend`, sampled at the same phase as
    /// `clip`. Empty for none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub blend_clip: String,
    /// How much of `blend_clip`: 0 is `clip` alone, 1 is `blend_clip`
    /// alone.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub blend: f32,
    /// A crossfade in progress: the pose being faded out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade: Option<SkeletalFade>,
    /// A 2D blend space, played while `clip` is empty (Oct 2026): see
    /// [`BlendSpace2D`]. `time` is then the cycle's phase, 0 to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend_space: Option<BlendSpace2D>,
    /// Clips laid over the pose above, in order (Oct 2026): see
    /// [`AnimLayer`]. An upper body that aims while the legs run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<AnimLayer>,
}

/// How an [`AnimLayer`] meets the pose under it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LayerMode {
    /// Its pose replaces the one under it, by its weight - draw a bow over
    /// a walk.
    #[default]
    Override,
    /// Its motion **away from its own first frame** is added to the pose
    /// under it - a breath, a recoil, a flinch that any base clip keeps.
    /// It moves only bones the pose under it animates.
    Additive,
}

/// A clip played over the base pose on part of the skeleton (Oct 2026;
/// Unity's Animator layers with avatar masks, Godot's blend tree filters):
/// its own clip, playhead and weight, limited to the bones of its `mask` -
/// each named bone **and every bone below it**. Layers apply in order, each
/// over everything before it. `Animation.play_layer` / `stop_layer` /
/// `set_layer_weight` fade the weight; a one-shot layer (`looping: false`)
/// fades itself out over `release` seconds when its clip ends, and calls
/// the root's `on_layer_end`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AnimLayer {
    /// What scripts call it (`"upper"`).
    pub name: String,
    pub clip: String,
    pub time: f32,
    pub speed: f32,
    pub looping: bool,
    pub playing: bool,
    /// How much of it there is, 0 to 1.
    pub weight: f32,
    /// Bones it drives - each and all below it; empty for every bone.
    pub mask: Vec<String>,
    pub mode: LayerMode,
    /// Seconds a one-shot takes to fade out once its clip ends (0 holds
    /// its last pose).
    pub release: f32,
    /// Where `weight` is heading, and how fast (per second) - a fade.
    #[serde(skip)]
    pub target: Option<(f32, f32)>,
}

impl Default for AnimLayer {
    fn default() -> Self {
        Self {
            name: String::new(),
            clip: String::new(),
            time: 0.0,
            speed: 1.0,
            looping: true,
            playing: true,
            weight: 1.0,
            mask: Vec::new(),
            mode: LayerMode::Override,
            release: 0.25,
            target: None,
        }
    }
}

impl AnimLayer {
    fn fade_to(&mut self, weight: f32, seconds: f32) {
        let weight = weight.clamp(0.0, 1.0);
        if seconds <= 0.0 {
            self.weight = weight;
            self.target = None;
        } else {
            self.target = Some((weight, (weight - self.weight).abs().max(1e-3) / seconds));
        }
    }

    /// Whether it adds anything this frame.
    fn active(&self) -> bool {
        !self.clip.is_empty() && (self.weight > 0.0 || self.target.is_some_and(|(w, _)| w > 0.0))
    }
}

/// A clip placed in a [`BlendSpace2D`].
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct BlendPoint {
    pub clip: String,
    pub at: [f32; 2],
}

/// Clips placed on two axes and mixed by where `position` is (Oct 2026;
/// Godot's `AnimationNodeBlendSpace2D`, Unity's 2D Freeform blend tree): a
/// walk and a run in eight directions about an idle, an aim up and down. A
/// [`SkeletalPlayer`] plays it while its `clip` is empty - every clip with a
/// weight, all at the **same phase**, the cycle's pace between theirs by the
/// weights - so `Animation.play(e, "")` (or a crossfade to `""`) goes back to
/// it from a one-off clip. `Animation.set_blend_position` moves `position`.
///
/// The weights are gradient band interpolation (Unity's Freeform
/// Cartesian): each point's is the least of `1 - (p - a).(b - a) / |b - a|^2`
/// over every other point `b`, clamped at 0, and then all are normalised. So
/// a point's clip is the whole pose at that point, the mix is continuous
/// between points, and a position outside them all is still defined.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct BlendSpace2D {
    pub points: Vec<BlendPoint>,
    #[serde(default)]
    pub position: [f32; 2],
}

impl BlendSpace2D {
    /// Each point's weight at `position`, summing to 1.
    pub fn weights(&self) -> Vec<f32> {
        let at: Vec<Vec2> = self.points.iter().map(|p| Vec2::from_array(p.at)).collect();
        blend_weights(&at, Vec2::from_array(self.position))
    }
}

/// Gradient band weights of `points` at `p`, summing to 1 (see
/// [`BlendSpace2D`]); empty for no points.
pub fn blend_weights(points: &[Vec2], p: Vec2) -> Vec<f32> {
    let mut w: Vec<f32> = points
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let mut h = 1.0_f32;
            for (j, b) in points.iter().enumerate() {
                let ab = *b - *a;
                let len2 = ab.length_squared();
                if j != i && len2 > 1.0e-12 {
                    h = h.min(1.0 - (p - *a).dot(ab) / len2);
                }
            }
            h.max(0.0)
        })
        .collect();
    let sum: f32 = w.iter().sum();
    if sum > 0.0 {
        w.iter_mut().for_each(|x| *x /= sum);
    } else if let Some(k) = (0..points.len())
        .min_by(|&a, &b| (points[a] - p).length_squared().total_cmp(&(points[b] - p).length_squared()))
    {
        w[k] = 1.0;
    }
    w
}

/// The pose a [`SkeletalPlayer`] is fading out of: its clips, where they
/// were, and how far the fade has gone.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SkeletalFade {
    pub clip: String,
    pub time: f32,
    #[serde(default)]
    pub blend_clip: String,
    #[serde(default)]
    pub blend: f32,
    /// Seconds the fade takes, and how many have passed.
    pub duration: f32,
    #[serde(default)]
    pub elapsed: f32,
    /// The blend space being faded out, when it was one (`time` is then
    /// its phase).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space: Option<BlendSpace2D>,
}

fn default_speed() -> f32 { 1.0 }
fn default_true() -> bool { true }
fn is_zero(v: &f32) -> bool { *v == 0.0 }

impl SkeletalPlayer {
    /// Start `clip` from its beginning, at once. `Animation.play`.
    pub fn play(&mut self, clip: &str) {
        self.clip = clip.to_string();
        self.time = 0.0;
        self.playing = true;
        self.fade = None;
    }

    /// Start `clip` from its beginning and fade the current pose out over
    /// `seconds` - the clip being left keeps playing while it fades.
    /// `Animation.crossfade`. A fade of 0 (or nothing playing yet) is
    /// [`Self::play`].
    pub fn crossfade(&mut self, clip: &str, seconds: f32) {
        let from_space = self.plays_space();
        if seconds <= 0.0 || (self.clip.is_empty() && !from_space) {
            self.play(clip);
            return;
        }
        self.fade = Some(SkeletalFade {
            clip: std::mem::take(&mut self.clip),
            time: self.time,
            blend_clip: self.blend_clip.clone(),
            blend: self.blend,
            duration: seconds,
            elapsed: 0.0,
            space: if from_space { self.blend_space.clone() } else { None },
        });
        self.clip = clip.to_string();
        self.time = 0.0;
        self.playing = true;
    }

    /// Whether the blend space is what plays: it has points and `clip` is
    /// empty.
    pub fn plays_space(&self) -> bool {
        self.clip.is_empty() && self.blend_space.as_ref().is_some_and(|s| !s.points.is_empty())
    }

    /// Move the blend space's position. `Animation.set_blend_position`;
    /// nothing without a blend space.
    pub fn set_blend_position(&mut self, x: f32, y: f32) {
        if let Some(space) = self.blend_space.as_mut() {
            space.position = [x, y];
        }
    }

    /// Mix `clip` in at `weight` (clamped to 0..1), at the same phase.
    /// The layer called `name`, made (driving every bone) if there is none.
    fn layer_mut(&mut self, name: &str) -> &mut AnimLayer {
        match self.layers.iter().position(|l| l.name == name) {
            Some(i) => &mut self.layers[i],
            None => {
                self.layers.push(AnimLayer { name: name.to_string(), weight: 0.0, ..Default::default() });
                self.layers.last_mut().expect("just pushed")
            }
        }
    }

    /// Play `clip` on layer `name` from its start and fade the layer in over
    /// `seconds`; `looping` decides whether it ends. `Animation.play_layer`.
    pub fn play_layer(&mut self, name: &str, clip: &str, seconds: f32, looping: bool) {
        let layer = self.layer_mut(name);
        layer.clip = clip.to_string();
        layer.time = 0.0;
        layer.playing = true;
        layer.looping = looping;
        layer.fade_to(1.0, seconds);
    }

    /// Fade layer `name` out over `seconds`. `Animation.stop_layer`.
    pub fn stop_layer(&mut self, name: &str, seconds: f32) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.name == name) {
            layer.fade_to(0.0, seconds);
        }
    }

    /// Fade layer `name` to `weight` over `seconds`.
    /// `Animation.set_layer_weight`.
    pub fn set_layer_weight(&mut self, name: &str, weight: f32, seconds: f32) {
        self.layer_mut(name).fade_to(weight, seconds);
    }

    /// A script's layer command: `"play"` (`clip`, fade in over `seconds`,
    /// `looping`), `"stop"` (fade out over `seconds`) or `"weight"` (fade to
    /// `value` over `seconds`). The one place every runtime applies them.
    pub fn layer_op(&mut self, op: &str, layer: &str, clip: &str, value: f32, seconds: f32, looping: bool) {
        match op {
            "play" => self.play_layer(layer, clip, seconds, looping),
            "stop" => self.stop_layer(layer, seconds),
            "weight" => self.set_layer_weight(layer, value, seconds),
            other => log::warn!("[animation] unknown layer op '{other}'"),
        }
    }

    /// `Animation.set_blend`; an empty clip or a weight of 0 turns it off.
    pub fn set_blend(&mut self, clip: &str, weight: f32) {
        self.blend_clip = clip.to_string();
        self.blend = if clip.is_empty() { 0.0 } else { weight.clamp(0.0, 1.0) };
    }
}

impl Default for SkeletalPlayer {
    fn default() -> Self {
        Self {
            clip: String::new(),
            time: 0.0,
            speed: 1.0,
            looping: true,
            playing: true,
            blend_clip: String::new(),
            blend: 0.0,
            fade: None,
            blend_space: None,
            layers: Vec::new(),
        }
    }
}

// --- Sampling ---------------------------------------------------------------

/// Linearly interpolate a Vec2 track at `t` seconds. Holds the first /
/// last value outside the keyframe range. Returns `None` when the
/// track is empty (caller must skip overriding).
pub fn sample_vec2(track: &[Vec2Key], t: f32) -> Option<Vec2> {
    if track.is_empty() {
        return None;
    }
    if track.len() == 1 || t <= track[0].time {
        let v = track[0].value;
        return Some(Vec2::new(v[0], v[1]));
    }
    let last = track.last().unwrap();
    if t >= last.time {
        return Some(Vec2::new(last.value[0], last.value[1]));
    }
    for w in track.windows(2) {
        let a = w[0];
        let b = w[1];
        if t >= a.time && t <= b.time {
            let span = (b.time - a.time).max(f32::EPSILON);
            let frac = (t - a.time) / span;
            return Some(Vec2::new(
                a.value[0] + (b.value[0] - a.value[0]) * frac,
                a.value[1] + (b.value[1] - a.value[1]) * frac,
            ));
        }
    }
    let v = last.value;
    Some(Vec2::new(v[0], v[1]))
}

/// Linearly interpolate a scalar track at `t` seconds. See
/// [`sample_vec2`] for boundary behavior.
pub fn sample_scalar(track: &[ScalarKey], t: f32) -> Option<f32> {
    if track.is_empty() {
        return None;
    }
    if track.len() == 1 || t <= track[0].time {
        return Some(track[0].value);
    }
    let last = track.last().unwrap();
    if t >= last.time {
        return Some(last.value);
    }
    for w in track.windows(2) {
        let a = w[0];
        let b = w[1];
        if t >= a.time && t <= b.time {
            let span = (b.time - a.time).max(f32::EPSILON);
            let frac = (t - a.time) / span;
            return Some(a.value + (b.value - a.value) * frac);
        }
    }
    Some(last.value)
}

// --- System -----------------------------------------------------------------

/// Sample value pulled from a clip for a specific bone at one tick.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct SampledLocal {
    position: Option<Vec2>,
    rotation: Option<f32>,
    scale: Option<Vec2>,
}

/// A clip by id - its asset path - or, failing that, by its `name`, so a
/// script can say `"walk"` for `animations/walk.skelanim`.
fn find_clip<'a>(clips: &'a HashMap<String, SkeletalClip>, id: &str) -> Option<&'a SkeletalClip> {
    if id.is_empty() {
        return None;
    }
    clips.get(id).or_else(|| clips.values().find(|c| c.name == id))
}

/// One clip's pose at `t` seconds, by bone name.
fn sample_clip(clip: &SkeletalClip, t: f32) -> Vec<(&str, SampledLocal)> {
    clip.tracks
        .iter()
        .map(|track| {
            (
                track.bone_name.as_str(),
                SampledLocal {
                    position: sample_vec2(&track.position, t),
                    rotation: sample_scalar(&track.rotation, t),
                    scale: sample_vec2(&track.scale, t),
                },
            )
        })
        .collect()
}

fn lerp_angle(a: f32, b: f32, w: f32) -> f32 {
    // Exact at the ends, so a pose mixed at full weight is that pose.
    if w <= 0.0 {
        return a;
    }
    if w >= 1.0 {
        return b;
    }
    let d = (b - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    a + d * w
}

/// Mix `b` into `a` at weight `w`. A channel only one side animates is
/// taken from that side: a clip that does not move a bone leaves it to the
/// other.
fn mix_local(a: SampledLocal, b: SampledLocal, w: f32) -> SampledLocal {
    fn pick<T: Copy>(a: Option<T>, b: Option<T>, f: impl Fn(T, T) -> T) -> Option<T> {
        match (a, b) {
            (Some(x), Some(y)) => Some(f(x, y)),
            (x, None) => x,
            (None, y) => y,
        }
    }
    SampledLocal {
        position: pick(a.position, b.position, |x, y| x.lerp(y, w)),
        rotation: pick(a.rotation, b.rotation, |x, y| lerp_angle(x, y, w)),
        scale: pick(a.scale, b.scale, |x, y| x.lerp(y, w)),
    }
}

fn mix_poses<'a>(a: Vec<(&'a str, SampledLocal)>, b: Vec<(&'a str, SampledLocal)>, w: f32) -> Vec<(&'a str, SampledLocal)> {
    if w <= 0.0 {
        return a;
    }
    if w >= 1.0 && a.iter().all(|(n, _)| b.iter().any(|(m, _)| m == n)) {
        return b;
    }
    let mut out = a;
    for (name, lb) in b {
        match out.iter_mut().find(|(n, _)| *n == name) {
            Some((_, la)) => *la = mix_local(*la, lb, w),
            None => out.push((name, lb)),
        }
    }
    out
}

/// Where the phase of `clip` + `blend_clip` at `blend` moves per second of
/// playback: between the two clips' rates by the weight, so the blended
/// cycle's pace is between theirs.
fn phase_rate(a: &SkeletalClip, b: Option<&SkeletalClip>, blend: f32) -> f32 {
    let ra = if a.duration > 0.0 { 1.0 / a.duration } else { 0.0 };
    match b {
        Some(b) if b.duration > 0.0 && blend > 0.0 => ra + (1.0 / b.duration - ra) * blend,
        _ => ra,
    }
}

/// The pose of `clip` (+ `blend_clip` at `blend`) at `time` seconds into
/// `clip`: both sampled at the same phase.
fn blended_pose<'a>(
    clips: &'a HashMap<String, SkeletalClip>,
    clip: &'a SkeletalClip,
    time: f32,
    blend_clip: &str,
    blend: f32,
) -> Vec<(&'a str, SampledLocal)> {
    let pose = sample_clip(clip, time);
    match find_clip(clips, blend_clip) {
        Some(other) if blend > 0.0 => {
            let phase = if clip.duration > 0.0 { time / clip.duration } else { 0.0 };
            mix_poses(pose, sample_clip(other, phase * other.duration), blend)
        }
        _ => pose,
    }
}

/// Where a blend space's phase moves per second: its clips' rates mixed by
/// their weights.
fn space_rate(clips: &HashMap<String, SkeletalClip>, space: &BlendSpace2D) -> f32 {
    let (mut rate, mut total) = (0.0, 0.0);
    for (point, w) in space.points.iter().zip(space.weights()) {
        if let Some(clip) = find_clip(clips, &point.clip)
            && w > 0.0
            && clip.duration > 0.0
        {
            rate += w / clip.duration;
            total += w;
        }
    }
    if total > 0.0 { rate / total } else { 0.0 }
}

/// A blend space's pose at `phase` (0..1): every clip with a weight sampled
/// at that phase, mixed by the weights (a running weighted mean).
fn space_pose<'a>(
    clips: &'a HashMap<String, SkeletalClip>,
    space: &BlendSpace2D,
    phase: f32,
) -> Vec<(&'a str, SampledLocal)> {
    let mut pose = Vec::new();
    let mut total = 0.0;
    for (point, w) in space.points.iter().zip(space.weights()) {
        let Some(clip) = find_clip(clips, &point.clip) else { continue };
        if w <= 0.0 {
            continue;
        }
        let sampled = sample_clip(clip, phase * clip.duration);
        total += w;
        pose = if total == w { sampled } else { mix_poses(pose, sampled, w / total) };
    }
    pose
}

/// Whether bone `name` of the skeleton at `root` is in `mask`: it, or a
/// bone above it, is named there. An empty mask holds every bone.
fn in_mask(world: &World, root: Entity, name: &str, mask: &[String]) -> bool {
    if mask.is_empty() {
        return true;
    }
    let Some(mut e) = find_bone(world, root, name) else { return false };
    for _ in 0..64 {
        if world.get::<&Bone>(e).is_ok_and(|b| mask.iter().any(|m| *m == b.name)) {
            return true;
        }
        match world.get::<&Parent>(e) {
            Ok(p) if p.0 != root => e = p.0,
            _ => return false,
        }
    }
    false
}

/// Add `layer`'s motion away from `first` (its own first frame) to `pose`,
/// by `w`: angles and positions add their difference, scales multiply by
/// their ratio. A bone the pose does not animate is left alone - there is
/// nothing under it to add to.
fn add_poses<'a>(
    mut pose: Vec<(&'a str, SampledLocal)>,
    layer: Vec<(&'a str, SampledLocal)>,
    first: &[(&'a str, SampledLocal)],
    w: f32,
) -> Vec<(&'a str, SampledLocal)> {
    for (name, l) in layer {
        let Some(f) = first.iter().find(|(n, _)| *n == name).map(|(_, f)| *f) else { continue };
        let Some((_, base)) = pose.iter_mut().find(|(n, _)| *n == name) else { continue };
        if let (Some(b), Some(r), Some(r0)) = (base.rotation.as_mut(), l.rotation, f.rotation) {
            let d = (r - r0 + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            *b += d * w;
        }
        if let (Some(b), Some(p), Some(p0)) = (base.position.as_mut(), l.position, f.position) {
            *b += (p - p0) * w;
        }
        if let (Some(b), Some(sc), Some(s0)) = (base.scale.as_mut(), l.scale, f.scale) {
            let ratio = Vec2::new(
                if s0.x != 0.0 { sc.x / s0.x } else { 1.0 },
                if s0.y != 0.0 { sc.y / s0.y } else { 1.0 },
            );
            *b *= Vec2::ONE.lerp(ratio, w);
        }
    }
    pose
}

/// Step every layer of a player - its fade, its playhead - and lay each over
/// `pose` in order, within its mask. A one-shot that ends fades out over its
/// `release` and calls the root's `on_layer_end`.
fn apply_layers<'a>(
    world: &World,
    root: Entity,
    clips: &'a HashMap<String, SkeletalClip>,
    layers: &mut [AnimLayer],
    mut pose: Vec<(&'a str, SampledLocal)>,
    dt: f32,
) -> Vec<(&'a str, SampledLocal)> {
    for layer in layers.iter_mut() {
        if let Some((to, rate)) = layer.target {
            layer.weight = if layer.weight < to {
                (layer.weight + rate * dt).min(to)
            } else {
                (layer.weight - rate * dt).max(to)
            };
            if layer.weight == to {
                layer.target = None;
            }
        }
        let Some(clip) = find_clip(clips, &layer.clip) else { continue };
        if layer.playing {
            let rate = phase_rate(clip, None, 0.0);
            layer.playing = advance(&mut layer.time, clip, rate, layer.speed, layer.looping, dt);
            if !layer.playing {
                crate::ecs::callbacks::push(root, "on_layer_end");
                if layer.release > 0.0 {
                    layer.fade_to(0.0, layer.release);
                }
            }
        }
        if layer.weight <= 0.0 {
            continue;
        }
        let sampled: Vec<(&str, SampledLocal)> = sample_clip(clip, layer.time)
            .into_iter()
            .filter(|(name, _)| in_mask(world, root, name, &layer.mask))
            .collect();
        pose = match layer.mode {
            LayerMode::Override => mix_poses(pose, sampled, layer.weight),
            LayerMode::Additive => add_poses(pose, sampled, &sample_clip(clip, 0.0), layer.weight),
        };
    }
    pose
}

/// Advance a phase (0..1) by `dt` at `rate`, wrapping or clamping it.
/// Returns whether it is still playing.
fn advance_phase(phase: &mut f32, rate: f32, speed: f32, looping: bool, dt: f32) -> bool {
    *phase += dt * speed * rate;
    if looping {
        *phase = phase.rem_euclid(1.0);
        true
    } else if *phase >= 1.0 {
        *phase = 1.0;
        false
    } else if *phase < 0.0 {
        *phase = 0.0;
        false
    } else {
        true
    }
}

/// Advance a clip's playhead by `dt`, wrapping or clamping it. Returns
/// whether it is still playing.
fn advance(time: &mut f32, clip: &SkeletalClip, rate: f32, speed: f32, looping: bool, dt: f32) -> bool {
    // Time in `clip`'s seconds: the phase moves at `rate`, and one phase
    // is `duration` of them.
    *time += dt * speed * rate * clip.duration.max(0.0);
    if clip.duration <= 0.0 {
        return true;
    }
    if looping {
        *time = time.rem_euclid(clip.duration);
        true
    } else if *time >= clip.duration {
        *time = clip.duration;
        false
    } else if *time < 0.0 {
        *time = 0.0;
        false
    } else {
        true
    }
}

// --- System -----------------------------------------------------------------

/// Advance every [`SkeletalPlayer`] on a [`SkeletonRoot`] entity by
/// `dt` seconds and write sampled bone transforms into the world -
/// blending its `blend_clip` and fading out a crossfade's old pose.
///
/// Call **before** [`crate::ecs::hierarchy::resolve_world_transforms`]
/// each frame. Players whose `clip` id isn't found in `clips` are
/// skipped. A clip is found by its id (asset path) or its `name`.
pub fn update_skeletal_players(
    world: &mut World,
    clips: &HashMap<String, SkeletalClip>,
    dt: f32,
) {
    let _span = crate::profiling::trace::span("animation.skeletal");
    // Pass 1: advance time + collect (bone_entity, sampled local) pairs.
    // We can't hold a `world` query open while mutating Transforms, so
    // we buffer the writes.
    let mut pending: Vec<(Entity, SampledLocal)> = Vec::new();
    // A limp skeleton is posed by physics (`ecs::ragdoll`).
    let limp = crate::ecs::ragdoll::any_limp(world);

    {
        let mut players = world.query::<(&mut SkeletalPlayer, &SkeletonRoot)>();
        for (root_entity, (player, _root)) in players.iter() {
            let layered = player.layers.iter().any(|l| l.active());
            if !player.playing && !layered {
                continue;
            }
            if limp && crate::ecs::ragdoll::is_limp(&*world, root_entity) {
                continue;
            }
            // Its own pace (`time_scale` on the root), if it keeps one.
            let dt = dt * crate::ecs::time_scale::factor_of(&*world, root_entity);
            let (speed, looping) = (player.speed, player.looping);
            let mut pose = if !player.playing {
                // Stopped, but its layers still play over the pose it left.
                Vec::new()
            } else if player.plays_space() {
                let space = player.blend_space.as_ref().expect("plays_space has a space");
                let rate = space_rate(clips, space);
                player.playing = advance_phase(&mut player.time, rate, speed, looping, dt);
                space_pose(clips, player.blend_space.as_ref().expect("still there"), player.time)
            } else {
                match find_clip(clips, &player.clip) {
                    Some(clip) => {
                        let blend_clip = find_clip(clips, &player.blend_clip);
                        let rate = phase_rate(clip, blend_clip, player.blend);
                        player.playing = advance(&mut player.time, clip, rate, speed, looping, dt);
                        blended_pose(clips, clip, player.time, &player.blend_clip, player.blend)
                    }
                    // Missing clip - leave bones untouched (but for layers).
                    None if !layered => continue,
                    None => Vec::new(),
                }
            };

            // A crossfade: the old pose keeps playing and gives way.
            if let Some(fade) = player.fade.as_mut() {
                fade.elapsed += dt;
                let w = if fade.duration > 0.0 { (fade.elapsed / fade.duration).clamp(0.0, 1.0) } else { 1.0 };
                if w < 1.0 {
                    if let Some(space) = fade.space.as_ref() {
                        advance_phase(&mut fade.time, space_rate(clips, space), speed, true, dt);
                        pose = mix_poses(space_pose(clips, space, fade.time), pose, w);
                    } else if let Some(old) = find_clip(clips, &fade.clip) {
                        let old_rate = phase_rate(old, find_clip(clips, &fade.blend_clip), fade.blend);
                        advance(&mut fade.time, old, old_rate, speed, true, dt);
                        let old_pose = blended_pose(clips, old, fade.time, &fade.blend_clip, fade.blend);
                        pose = mix_poses(old_pose, pose, w);
                    }
                }
                if w >= 1.0 {
                    player.fade = None;
                }
            }

            // Layers over all of it, each within its mask.
            if layered || !player.layers.is_empty() {
                pose = apply_layers(&*world, root_entity, clips, &mut player.layers, pose, dt);
            }

            // Resolve each track to its bone entity. find_bone walks
            // the Parent chain bounded to descendants of this root, so
            // multiple skeletons stay isolated. The query above is a
            // *shared* borrow with runtime component flags, so a second
            // shared reborrow beside it is ordinary Rust.
            let world_ref: &World = &*world;
            for (name, sampled) in pose {
                let Some(bone) = find_bone(world_ref, root_entity, name) else {
                    continue;
                };
                if sampled != SampledLocal::default() {
                    pending.push((bone, sampled));
                }
            }
        }
    }

    // Pass 2: apply buffered writes.
    for (bone, sampled) in pending {
        let Ok(mut tx) = world.get::<&mut Transform>(bone) else {
            continue;
        };
        if let Some(p) = sampled.position {
            tx.position = p;
        }
        if let Some(r) = sampled.rotation {
            tx.rotation = r;
        }
        if let Some(s) = sampled.scale {
            tx.scale = s;
        }
    }

    // Jiggle bones swing off the pose just set (`animation::jiggle`) - here,
    // so every host that animates skeletons jiggles them too.
    crate::animation::jiggle::tick_jiggle(world, dt);

    // Silence unused-import warnings on configs where Bone/Parent
    // wouldn't otherwise be referenced from this module.
    let _ = std::marker::PhantomData::<(Bone, Parent)>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_vec2_holds_endpoints_and_interpolates() {
        let track = vec![
            Vec2Key { time: 0.0, value: [0.0, 0.0] },
            Vec2Key { time: 1.0, value: [10.0, 20.0] },
        ];
        assert_eq!(sample_vec2(&track, -1.0), Some(Vec2::new(0.0, 0.0)));
        assert_eq!(sample_vec2(&track, 0.0), Some(Vec2::new(0.0, 0.0)));
        assert_eq!(sample_vec2(&track, 0.5), Some(Vec2::new(5.0, 10.0)));
        assert_eq!(sample_vec2(&track, 1.0), Some(Vec2::new(10.0, 20.0)));
        assert_eq!(sample_vec2(&track, 2.0), Some(Vec2::new(10.0, 20.0)));
    }

    #[test]
    fn sample_scalar_handles_single_keyframe() {
        let track = vec![ScalarKey { time: 0.0, value: 1.5 }];
        assert_eq!(sample_scalar(&track, 0.0), Some(1.5));
        assert_eq!(sample_scalar(&track, 99.0), Some(1.5));
    }

    #[test]
    fn sample_returns_none_for_empty_track() {
        assert_eq!(sample_vec2(&[], 0.0), None);
        assert_eq!(sample_scalar(&[], 0.0), None);
    }

    fn make_skeleton() -> (World, Entity) {
        let mut w = World::new();
        let root = w.spawn((
            Transform::default(),
            SkeletonRoot { name: "rig".into() },
        ));
        w.spawn((
            Transform::default(),
            Parent(root),
            Bone { name: "arm".into(), length: 1.0 },
        ));
        (w, root)
    }

    fn make_clip() -> SkeletalClip {
        SkeletalClip {
            name: "wave".into(),
            duration: 1.0,
            tracks: vec![BoneTrack {
                bone_name: "arm".into(),
                position: vec![
                    Vec2Key { time: 0.0, value: [0.0, 0.0] },
                    Vec2Key { time: 1.0, value: [10.0, 0.0] },
                ],
                rotation: vec![],
                scale: vec![],
            }],
        }
    }

    #[test]
    fn update_writes_sampled_position_to_bone_transform() {
        let (mut world, root) = make_skeleton();
        world
            .insert_one(root, SkeletalPlayer {
                clip: "wave".into(),
                time: 0.0,
                speed: 1.0,
                looping: true,
                playing: true,
                ..Default::default()
            })
            .unwrap();
        let mut clips = HashMap::new();
        clips.insert("wave".to_string(), make_clip());

        update_skeletal_players(&mut world, &clips, 0.5);

        let arm = find_bone(&world, root, "arm").unwrap();
        let tx = world.get::<&Transform>(arm).unwrap();
        assert!((tx.position.x - 5.0).abs() < 1e-5);
        assert_eq!(tx.position.y, 0.0);

        let player = world.get::<&SkeletalPlayer>(root).unwrap();
        assert!((player.time - 0.5).abs() < 1e-5);
    }

    #[test]
    fn looping_player_wraps_time() {
        let (mut world, root) = make_skeleton();
        world
            .insert_one(root, SkeletalPlayer {
                clip: "wave".into(),
                time: 0.9,
                speed: 1.0,
                looping: true,
                playing: true,
                ..Default::default()
            })
            .unwrap();
        let mut clips = HashMap::new();
        clips.insert("wave".to_string(), make_clip());

        update_skeletal_players(&mut world, &clips, 0.5);
        let player = world.get::<&SkeletalPlayer>(root).unwrap();
        // 0.9 + 0.5 = 1.4, wrapped at duration 1.0 → 0.4
        assert!((player.time - 0.4).abs() < 1e-5);
        assert!(player.playing);
    }

    #[test]
    fn non_looping_player_clamps_and_stops() {
        let (mut world, root) = make_skeleton();
        world
            .insert_one(root, SkeletalPlayer {
                clip: "wave".into(),
                time: 0.9,
                speed: 1.0,
                looping: false,
                playing: true,
                ..Default::default()
            })
            .unwrap();
        let mut clips = HashMap::new();
        clips.insert("wave".to_string(), make_clip());

        update_skeletal_players(&mut world, &clips, 0.5);
        let player = world.get::<&SkeletalPlayer>(root).unwrap();
        assert_eq!(player.time, 1.0);
        assert!(!player.playing);
    }

    #[test]
    fn paused_player_does_not_advance() {
        let (mut world, root) = make_skeleton();
        world
            .insert_one(root, SkeletalPlayer {
                clip: "wave".into(),
                time: 0.25,
                speed: 1.0,
                looping: true,
                playing: false,
                ..Default::default()
            })
            .unwrap();
        let mut clips = HashMap::new();
        clips.insert("wave".to_string(), make_clip());

        update_skeletal_players(&mut world, &clips, 0.5);
        let player = world.get::<&SkeletalPlayer>(root).unwrap();
        assert_eq!(player.time, 0.25);

        // Bone transform must not have been overwritten.
        let arm = find_bone(&world, root, "arm").unwrap();
        let tx = world.get::<&Transform>(arm).unwrap();
        assert_eq!(tx.position, Vec2::ZERO);
    }

    #[test]
    fn missing_clip_id_is_silently_skipped() {
        let (mut world, root) = make_skeleton();
        world
            .insert_one(root, SkeletalPlayer {
                clip: "nope".into(),
                ..Default::default()
            })
            .unwrap();
        let clips = HashMap::new();
        update_skeletal_players(&mut world, &clips, 0.16);
        // No panic, bone untouched.
        let arm = find_bone(&world, root, "arm").unwrap();
        let tx = world.get::<&Transform>(arm).unwrap();
        assert_eq!(tx.position, Vec2::ZERO);
    }

    // --- Blending (Sep 2026) ------------------------------------------------

    fn clip(name: &str, duration: f32, x0: f32, x1: f32, rot: f32) -> SkeletalClip {
        SkeletalClip {
            name: name.into(),
            duration,
            tracks: vec![BoneTrack {
                bone_name: "arm".into(),
                position: vec![Vec2Key { time: 0.0, value: [x0, 0.0] }, Vec2Key { time: duration, value: [x1, 0.0] }],
                rotation: vec![ScalarKey { time: 0.0, value: rot }],
                scale: vec![],
            }],
        }
    }

    fn arm(world: &World, root: Entity) -> Transform {
        let arm = find_bone(world, root, "arm").unwrap();
        (*world.get::<&Transform>(arm).unwrap()).clone()
    }

    fn clips() -> HashMap<String, SkeletalClip> {
        let mut m = HashMap::new();
        // A walk of 1 s and a run of 0.5 s: from 0 to 10 and from 100 to 110.
        m.insert("anims/walk.skelanim".to_string(), clip("walk", 1.0, 0.0, 10.0, 0.0));
        m.insert("anims/run.skelanim".to_string(), clip("run", 0.5, 100.0, 110.0, 1.0));
        m
    }

    #[test]
    fn a_blend_samples_both_clips_at_one_phase_and_paces_between_them() {
        let (mut world, root) = make_skeleton();
        let mut p = SkeletalPlayer { clip: "anims/walk.skelanim".into(), ..Default::default() };
        p.set_blend("anims/run.skelanim", 0.5);
        world.insert_one(root, p).unwrap();
        // Half way: the phase moves at (1/1 + 1/0.5) / 2 = 1.5 a second.
        update_skeletal_players(&mut world, &clips(), 0.2);
        let t = world.get::<&SkeletalPlayer>(root).unwrap().time;
        assert!((t - 0.3).abs() < 1e-4, "phase 0.3 of the walk's second: {t}");
        // Walk at phase 0.3 is 3, run at phase 0.3 is 103: half way is 53.
        let a = arm(&world, root);
        assert!((a.position.x - 53.0).abs() < 1e-3, "{:?}", a.position);
        assert!((a.rotation - 0.5).abs() < 1e-4);
    }

    #[test]
    fn a_crossfade_moves_from_the_old_pose_to_the_new_over_its_length() {
        let (mut world, root) = make_skeleton();
        world.insert_one(root, SkeletalPlayer { clip: "walk".into(), ..Default::default() }).unwrap();
        let c = clips();
        update_skeletal_players(&mut world, &c, 0.0);
        assert_eq!(arm(&world, root).position.x, 0.0, "a clip is found by its name too");
        world.get::<&mut SkeletalPlayer>(root).unwrap().crossfade("run", 1.0);
        update_skeletal_players(&mut world, &c, 0.5);
        // Half way through the fade: walk at 0.5 s is 5, run at 0.5 s wraps to 0 -> 100.
        let x = arm(&world, root).position.x;
        assert!((x - 52.5).abs() < 1e-2, "{x}");
        update_skeletal_players(&mut world, &c, 0.6);
        assert!(world.get::<&SkeletalPlayer>(root).unwrap().fade.is_none(), "the fade ends");
        assert!(arm(&world, root).position.x >= 100.0, "all run now");
    }

    #[test]
    fn play_snaps_and_a_zero_fade_is_play() {
        let mut p = SkeletalPlayer { clip: "walk".into(), time: 0.4, ..Default::default() };
        p.crossfade("run", 0.0);
        assert_eq!((p.clip.as_str(), p.time, p.fade.is_none()), ("run", 0.0, true));
        p.crossfade("walk", 0.3);
        assert_eq!(p.fade.as_ref().map(|f| f.clip.as_str()), Some("run"));
        p.play("idle");
        assert!(p.fade.is_none());
    }

    /// A body, an arm under it and a hand under that; a leg beside them.
    fn rig() -> (World, Entity) {
        let mut w = World::new();
        let root = w.spawn((Transform::default(), SkeletonRoot { name: "rig".into() }));
        let body = w.spawn((Transform::default(), Parent(root), Bone { name: "body".into(), length: 1.0 }));
        let arm = w.spawn((Transform::default(), Parent(body), Bone { name: "arm".into(), length: 1.0 }));
        w.spawn((Transform::default(), Parent(arm), Bone { name: "hand".into(), length: 1.0 }));
        w.spawn((Transform::default(), Parent(root), Bone { name: "leg".into(), length: 1.0 }));
        (w, root)
    }

    /// One clip turning every bone of `rig` to `rot`, from `rot0` at its start.
    fn turn(name: &str, duration: f32, rot0: f32, rot: f32) -> SkeletalClip {
        SkeletalClip {
            name: name.into(),
            duration,
            tracks: ["body", "arm", "hand", "leg"]
                .iter()
                .map(|b| BoneTrack {
                    bone_name: b.to_string(),
                    position: vec![],
                    rotation: vec![ScalarKey { time: 0.0, value: rot0 }, ScalarKey { time: duration, value: rot }],
                    scale: vec![],
                })
                .collect(),
        }
    }

    fn rot(world: &World, root: Entity, bone: &str) -> f32 {
        world.get::<&Transform>(find_bone(world, root, bone).unwrap()).unwrap().rotation
    }

    fn layer_clips() -> HashMap<String, SkeletalClip> {
        let mut m = HashMap::new();
        m.insert("run".into(), turn("run", 1.0, 0.1, 0.1));
        m.insert("aim".into(), turn("aim", 1.0, 1.0, 1.0));
        m.insert("breathe".into(), turn("breathe", 1.0, 0.0, 0.4));
        m
    }

    #[test]
    fn a_masked_layer_drives_its_bones_and_the_ones_below_them() {
        let (mut world, root) = rig();
        let mut p = SkeletalPlayer { clip: "run".into(), ..Default::default() };
        p.layers.push(AnimLayer { name: "upper".into(), clip: "aim".into(), mask: vec!["arm".into()], ..Default::default() });
        world.insert_one(root, p).unwrap();
        update_skeletal_players(&mut world, &layer_clips(), 0.1);
        assert_eq!(rot(&world, root, "arm"), 1.0, "the masked bone aims");
        assert_eq!(rot(&world, root, "hand"), 1.0, "and the bone below it");
        assert_eq!(rot(&world, root, "body"), 0.1, "the bone above it runs");
        assert_eq!(rot(&world, root, "leg"), 0.1, "and the legs run");
    }

    #[test]
    fn a_layer_fades_in_over_the_base_and_a_one_shot_fades_itself_out() {
        let (mut world, root) = rig();
        let mut p = SkeletalPlayer { clip: "run".into(), ..Default::default() };
        p.play_layer("upper", "aim", 0.5, false);
        p.layers[0].release = 0.5;
        world.insert_one(root, p).unwrap();
        let clips = layer_clips();
        update_skeletal_players(&mut world, &clips, 0.25);
        let half = rot(&world, root, "arm");
        assert!((half - 0.55).abs() < 1e-4, "half faded in: {half}");
        crate::ecs::callbacks::take();
        // The aim (1 s) ends at 1.0, then takes its 0.5 s release to go.
        for _ in 0..3 {
            update_skeletal_players(&mut world, &clips, 0.25);
        }
        assert_eq!(rot(&world, root, "arm"), 1.0);
        assert!(crate::ecs::callbacks::take().iter().any(|c| c.entity == root && c.handler == "on_layer_end"));
        update_skeletal_players(&mut world, &clips, 0.25);
        update_skeletal_players(&mut world, &clips, 0.25);
        assert!((rot(&world, root, "arm") - 0.1).abs() < 1e-4, "faded back to the run");
    }

    #[test]
    fn an_additive_layer_adds_its_motion_to_any_base() {
        let (mut world, root) = rig();
        let mut p = SkeletalPlayer { clip: "run".into(), ..Default::default() };
        p.layers.push(AnimLayer {
            name: "breath".into(),
            clip: "breathe".into(),
            mode: LayerMode::Additive,
            weight: 0.5,
            ..Default::default()
        });
        world.insert_one(root, p).unwrap();
        update_skeletal_players(&mut world, &layer_clips(), 0.5);
        // Breathe is 0.2 from its first frame at 0.5 s; half of it on 0.1.
        assert!((rot(&world, root, "body") - 0.2).abs() < 1e-4, "{}", rot(&world, root, "body"));
    }

    #[test]
    fn layers_play_on_over_a_stopped_base() {
        let (mut world, root) = rig();
        let mut p = SkeletalPlayer { clip: "run".into(), playing: false, ..Default::default() };
        p.play_layer("upper", "aim", 0.0, true);
        world.insert_one(root, p).unwrap();
        update_skeletal_players(&mut world, &layer_clips(), 0.1);
        assert_eq!(rot(&world, root, "arm"), 1.0);
        let p = world.get::<&SkeletalPlayer>(root).unwrap();
        assert!((p.layers[0].time - 0.1).abs() < 1e-5 && p.time == 0.0, "the layer moves on, the base does not");
    }

}
