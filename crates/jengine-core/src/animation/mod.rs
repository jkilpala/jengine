//! Animation system — sprite sheet frame animations, frame triggers, and
//! animation state machines.

pub mod easing;
pub mod tween;
pub mod asset;
pub mod skeletal;
pub mod rig;
#[cfg(feature = "skinned_mesh")]
pub mod skinned_mesh;
#[cfg(feature = "ik")]
pub mod ik;
#[cfg(feature = "jiggle")]
pub mod jiggle;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A trigger that fires when a specific animation frame is reached.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FrameTrigger {
    /// Play a sound effect.
    PlaySound { clip_id: String, #[serde(default = "default_volume")] volume: f32 },
    /// Emit a named event that scripts can listen for.
    EmitEvent { event_name: String, #[serde(default, skip_serializing_if = "Option::is_none")] data: Option<serde_json::Value> },
}

fn default_volume() -> f32 {
    1.0
}

fn full_uv_rect() -> [f32; 4] {
    [0.0, 0.0, 1.0, 1.0]
}

/// A single frame in a sprite animation — stores the atlas UV region
/// and optionally a per-frame texture (for multi-image animations).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationFrame {
    /// UV rectangle: [u_min, v_min, u_max, v_max]. Replaced by the region's
    /// rect once `region` resolves; defaults to the whole texture so a frame
    /// can be authored with a region alone.
    #[serde(default = "full_uv_rect")]
    pub uv_rect: [f32; 4],
    /// Name of a region in the atlas this frame draws from - the frame's
    /// `texture_id`, or the sprite's texture in spritesheet mode. When set,
    /// `uv_rect` is filled from the atlas (see
    /// [`SpriteAnimator::resolve_frame_regions`]), so a clip says
    /// `"region": "grunt_walk_2"` instead of hand-computed coordinates that
    /// silently go wrong the moment the sheet is re-laid-out.
    ///
    /// The atlas must be loaded (a scene's `atlas_refs`); until it is, the
    /// frame draws with its authored `uv_rect`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Optional per-frame texture path (relative to assets/).
    /// When set, the animation system swaps the sprite's texture_id each frame.
    /// When None, all frames share the sprite's existing texture (spritesheet mode).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture_id: Option<String>,
    /// Triggers that fire when this frame is reached.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<FrameTrigger>,
}

/// A triggered event from an animation tick — returned so the caller can process it.
#[derive(Debug, Clone)]
pub struct TriggeredEvent {
    /// The entity that triggered this event.
    pub entity_id: u64,
    /// The trigger that fired.
    pub trigger: FrameTrigger,
}

/// Split a frame's triggers into the two things a runtime has to do with
/// them: play a sound, and tell a script.
///
/// Every runtime ticks sprite animations and every one of them threw the
/// result away (`let _sprite_events = ...`), so a clip with a sound on a
/// frame was silent and `on_animation_event` - a handler name the metadata
/// has always accepted - was fired by nobody. The split lives here because
/// the three runtimes play sounds and call scripts in three different ways
/// but must agree on which trigger is which.
///
/// Returns `(sounds, events)`: `(entity, clip_id, volume)` and
/// `(entity, event_name, data)`.
#[allow(clippy::type_complexity)]
pub fn split_triggers(
    events: &[TriggeredEvent],
) -> (Vec<(u64, String, f32)>, Vec<(u64, String, Option<serde_json::Value>)>) {
    let mut sounds = Vec::new();
    let mut fired = Vec::new();
    for ev in events {
        match &ev.trigger {
            FrameTrigger::PlaySound { clip_id, volume } => {
                sounds.push((ev.entity_id, clip_id.clone(), *volume));
            }
            FrameTrigger::EmitEvent { event_name, data } => {
                fired.push((ev.entity_id, event_name.clone(), data.clone()));
            }
        }
    }
    (sounds, fired)
}

/// Result of an animation tick — what changed this frame.
#[derive(Debug, Clone)]
pub struct AnimTickResult {
    /// The current frame's UV rectangle.
    pub uv_rect: [f32; 4],
    /// If the frame has a per-frame texture, this is the texture path to switch to.
    pub texture_id: Option<String>,
    /// Triggers that fired on this frame (only on frame transitions).
    pub triggered: Vec<FrameTrigger>,
    /// Property animation values sampled at the current frame position.
    pub properties: PropertyAnimResult,
}

// ---------------------------------------------------------------------------
// Parameter Animations (property keyframe tracks)
// ---------------------------------------------------------------------------

/// Which property a keyframe track animates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AnimatableProperty {
    // Transform
    PositionX,
    PositionY,
    Rotation,
    ScaleX,
    ScaleY,
    // Sprite color
    ColorR,
    ColorG,
    ColorB,
    ColorA,
    // UI Element
    UiOffsetX,
    UiOffsetY,
    UiSizeX,
    UiSizeY,
    // UI Text
    UiTextFontSize,
    UiTextColorR,
    UiTextColorG,
    UiTextColorB,
    UiTextColorA,
    // UI Image tint
    UiImageColorR,
    UiImageColorG,
    UiImageColorB,
    UiImageColorA,
    // UI Panel
    UiPanelColorR,
    UiPanelColorG,
    UiPanelColorB,
    UiPanelColorA,
    // UI Button
    UiButtonColorR,
    UiButtonColorG,
    UiButtonColorB,
    UiButtonColorA,
    // UI Progress Bar
    UiProgressValue,
    UiProgressFgR,
    UiProgressFgG,
    UiProgressFgB,
    UiProgressFgA,
    // UI Slider
    UiSliderValue,
}

/// A single keyframe — a value at a specific frame index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keyframe {
    /// Frame index where this keyframe sits.
    pub frame: usize,
    /// Value at this keyframe.
    pub value: f32,
}

/// A track of keyframes for a single animatable property.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyTrack {
    /// Which property this track animates.
    pub property: AnimatableProperty,
    /// Keyframes sorted by frame index. Must have at least one keyframe.
    pub keyframes: Vec<Keyframe>,
}

impl PropertyTrack {
    /// Sample the track value at a given frame, linearly interpolating between keyframes.
    pub fn sample(&self, frame: usize, elapsed_frac: f32) -> f32 {
        if self.keyframes.is_empty() {
            return 0.0;
        }
        if self.keyframes.len() == 1 {
            return self.keyframes[0].value;
        }

        // Continuous time position: frame index + fraction within frame
        let t = frame as f32 + elapsed_frac;

        // Before first keyframe
        if t <= self.keyframes[0].frame as f32 {
            return self.keyframes[0].value;
        }

        // After last keyframe
        let last = self.keyframes.last().unwrap();
        if t >= last.frame as f32 {
            return last.value;
        }

        // Find surrounding keyframes and interpolate
        for i in 0..self.keyframes.len() - 1 {
            let a = &self.keyframes[i];
            let b = &self.keyframes[i + 1];
            let af = a.frame as f32;
            let bf = b.frame as f32;
            if t >= af && t <= bf {
                let frac = if (bf - af).abs() < f32::EPSILON {
                    0.0
                } else {
                    (t - af) / (bf - af)
                };
                return a.value + (b.value - a.value) * frac;
            }
        }

        last.value
    }
}

/// Result of sampling all property tracks at a given frame.
#[derive(Debug, Clone, Default)]
pub struct PropertyAnimResult {
    pub position_x: Option<f32>,
    pub position_y: Option<f32>,
    pub rotation: Option<f32>,
    pub scale_x: Option<f32>,
    pub scale_y: Option<f32>,
    pub color_r: Option<f32>,
    pub color_g: Option<f32>,
    pub color_b: Option<f32>,
    pub color_a: Option<f32>,
    // UI Element
    pub ui_offset_x: Option<f32>,
    pub ui_offset_y: Option<f32>,
    pub ui_size_x: Option<f32>,
    pub ui_size_y: Option<f32>,
    // UI Text
    pub ui_text_font_size: Option<f32>,
    pub ui_text_color_r: Option<f32>,
    pub ui_text_color_g: Option<f32>,
    pub ui_text_color_b: Option<f32>,
    pub ui_text_color_a: Option<f32>,
    // UI Image
    pub ui_image_color_r: Option<f32>,
    pub ui_image_color_g: Option<f32>,
    pub ui_image_color_b: Option<f32>,
    pub ui_image_color_a: Option<f32>,
    // UI Panel
    pub ui_panel_color_r: Option<f32>,
    pub ui_panel_color_g: Option<f32>,
    pub ui_panel_color_b: Option<f32>,
    pub ui_panel_color_a: Option<f32>,
    // UI Button
    pub ui_button_color_r: Option<f32>,
    pub ui_button_color_g: Option<f32>,
    pub ui_button_color_b: Option<f32>,
    pub ui_button_color_a: Option<f32>,
    // UI Progress Bar
    pub ui_progress_value: Option<f32>,
    pub ui_progress_fg_r: Option<f32>,
    pub ui_progress_fg_g: Option<f32>,
    pub ui_progress_fg_b: Option<f32>,
    pub ui_progress_fg_a: Option<f32>,
    // UI Slider
    pub ui_slider_value: Option<f32>,
}

/// Sample all property tracks at the given frame/elapsed fraction.
pub fn sample_property_tracks(tracks: &[PropertyTrack], frame: usize, elapsed_frac: f32) -> PropertyAnimResult {
    let mut result = PropertyAnimResult::default();
    for track in tracks {
        let val = track.sample(frame, elapsed_frac);
        match track.property {
            AnimatableProperty::PositionX => result.position_x = Some(val),
            AnimatableProperty::PositionY => result.position_y = Some(val),
            AnimatableProperty::Rotation => result.rotation = Some(val),
            AnimatableProperty::ScaleX => result.scale_x = Some(val),
            AnimatableProperty::ScaleY => result.scale_y = Some(val),
            AnimatableProperty::ColorR => result.color_r = Some(val),
            AnimatableProperty::ColorG => result.color_g = Some(val),
            AnimatableProperty::ColorB => result.color_b = Some(val),
            AnimatableProperty::ColorA => result.color_a = Some(val),
            AnimatableProperty::UiOffsetX => result.ui_offset_x = Some(val),
            AnimatableProperty::UiOffsetY => result.ui_offset_y = Some(val),
            AnimatableProperty::UiSizeX => result.ui_size_x = Some(val),
            AnimatableProperty::UiSizeY => result.ui_size_y = Some(val),
            AnimatableProperty::UiTextFontSize => result.ui_text_font_size = Some(val),
            AnimatableProperty::UiTextColorR => result.ui_text_color_r = Some(val),
            AnimatableProperty::UiTextColorG => result.ui_text_color_g = Some(val),
            AnimatableProperty::UiTextColorB => result.ui_text_color_b = Some(val),
            AnimatableProperty::UiTextColorA => result.ui_text_color_a = Some(val),
            AnimatableProperty::UiImageColorR => result.ui_image_color_r = Some(val),
            AnimatableProperty::UiImageColorG => result.ui_image_color_g = Some(val),
            AnimatableProperty::UiImageColorB => result.ui_image_color_b = Some(val),
            AnimatableProperty::UiImageColorA => result.ui_image_color_a = Some(val),
            AnimatableProperty::UiPanelColorR => result.ui_panel_color_r = Some(val),
            AnimatableProperty::UiPanelColorG => result.ui_panel_color_g = Some(val),
            AnimatableProperty::UiPanelColorB => result.ui_panel_color_b = Some(val),
            AnimatableProperty::UiPanelColorA => result.ui_panel_color_a = Some(val),
            AnimatableProperty::UiButtonColorR => result.ui_button_color_r = Some(val),
            AnimatableProperty::UiButtonColorG => result.ui_button_color_g = Some(val),
            AnimatableProperty::UiButtonColorB => result.ui_button_color_b = Some(val),
            AnimatableProperty::UiButtonColorA => result.ui_button_color_a = Some(val),
            AnimatableProperty::UiProgressValue => result.ui_progress_value = Some(val),
            AnimatableProperty::UiProgressFgR => result.ui_progress_fg_r = Some(val),
            AnimatableProperty::UiProgressFgG => result.ui_progress_fg_g = Some(val),
            AnimatableProperty::UiProgressFgB => result.ui_progress_fg_b = Some(val),
            AnimatableProperty::UiProgressFgA => result.ui_progress_fg_a = Some(val),
            AnimatableProperty::UiSliderValue => result.ui_slider_value = Some(val),
        }
    }
    result
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationClip {
    pub name: String,
    pub frames: Vec<AnimationFrame>,
    /// Seconds per frame.
    pub frame_duration: f32,
    /// Whether the animation loops back to frame 0 after the last frame.
    pub looping: bool,
    /// Property animation tracks — keyframe-driven property animations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub property_tracks: Vec<PropertyTrack>,
}

/// Sprite animator component — drives sprite sheet animations on an entity.
///
/// Attach to an entity that also has `SpriteComponent`. The animation system
/// will update `SpriteComponent.uv_rect` each frame based on the active clip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpriteAnimator {
    /// Named animation clips available on this entity.
    /// May be empty when `clip_source` is set — the scene loader fills
    /// this in from the referenced `.anim` file before the world is built.
    #[serde(default)]
    pub clips: HashMap<String, AnimationClip>,
    /// Currently playing clip name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_clip: Option<String>,
    /// Current frame index within the active clip.
    #[serde(default)]
    pub current_frame: usize,
    /// Elapsed time on the current frame (seconds).
    #[serde(default)]
    pub elapsed: f32,
    /// Whether the animator is actively playing.
    #[serde(default)]
    pub playing: bool,
    /// Playback speed multiplier (1.0 = normal, 2.0 = double speed).
    #[serde(default = "default_speed")]
    pub speed: f32,
    /// When true, start playing from a random frame instead of frame 0.
    /// Useful for multiple entities with the same animation to look unsynchronized.
    ///
    /// Applied once, on the first tick that has a clip playing. The offset is
    /// derived from the entity, so it differs between entities and is the same
    /// on every run. This field was authorable in the inspector and read by
    /// nothing until Sep 2026.
    #[serde(default, skip_serializing_if = "is_false")]
    pub random_start: bool,
    /// Whether `random_start` has been applied. Runtime state.
    #[serde(skip)]
    pub start_randomized: bool,
    /// Whether every frame naming a `region` has been resolved against its
    /// atlas. Runtime state: cleared by deserialization, so a loaded or
    /// spawned animator resolves once, on the first tick its atlas is loaded.
    #[serde(skip)]
    pub regions_resolved: bool,
    /// Optional path (relative to assets/) to a `.anim` file whose clips should
    /// be loaded into this animator at scene-load time. When set, the scene
    /// loader replaces `clips` with the file's contents and (if the .anim
    /// declares a `texture_id` and the sibling sprite has none) sets the
    /// sprite's texture too. Pure authoring convenience — the runtime
    /// component itself never reads from disk after the resolve pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_source: Option<String>,
}

fn default_speed() -> f32 {
    1.0
}

fn is_false(v: &bool) -> bool {
    !*v
}

impl Default for SpriteAnimator {
    fn default() -> Self {
        Self {
            clips: HashMap::new(),
            current_clip: None,
            current_frame: 0,
            elapsed: 0.0,
            playing: false,
            speed: 1.0,
            random_start: false,
            start_randomized: false,
            regions_resolved: false,
            clip_source: None,
        }
    }
}

impl SpriteAnimator {
    /// Play a named clip from the beginning.
    pub fn play(&mut self, clip_name: &str) {
        if self.clips.contains_key(clip_name) {
            self.current_clip = Some(clip_name.to_string());
            self.current_frame = 0;
            self.elapsed = 0.0;
            self.playing = true;
        } else {
            log::warn!("Animation clip '{}' not found", clip_name);
        }
    }

    /// Stop the current animation.
    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// Advance the animation by `dt` seconds. Returns the current frame result
    /// if animation is playing (or None if paused / no clip).
    pub fn tick(&mut self, dt: f32) -> Option<AnimTickResult> {
        if !self.playing {
            return None;
        }

        let clip_name = self.current_clip.as_ref()?;
        let clip = self.clips.get(clip_name)?;

        let len = clip.frames.len();
        if len == 0 {
            return None;
        }
        // A clip edited or hot-reloaded to fewer frames leaves the index past
        // the end; clamp rather than index out of bounds below.
        self.current_frame = self.current_frame.min(len - 1);

        self.elapsed += dt * self.speed;

        // Every whole frame duration that has elapsed is a frame to advance.
        // This used to advance at most one per tick and subtract one duration,
        // so a clip faster than the frame rate (or sped up with `speed`) played
        // slower than authored, and after a hitch the surplus stayed in
        // `elapsed`, making the clip race for many frames afterwards.
        let steps = if clip.frame_duration > 0.0 {
            let n = (self.elapsed / clip.frame_duration).floor();
            if n >= 1.0 {
                self.elapsed -= n * clip.frame_duration;
                n as usize
            } else {
                0
            }
        } else {
            // No duration to divide by: one frame per tick, as it always was.
            self.elapsed = 0.0;
            1
        };

        let mut triggered = Vec::new();

        if steps > 0 {
            if clip.looping {
                // Each frame's triggers fire at most once per tick: a hitch long
                // enough to wrap the clip several times should play a footstep
                // once, not once per lap.
                for i in 1..=steps.min(len) {
                    let frame = (self.current_frame + i) % len;
                    triggered.extend(clip.frames[frame].triggers.iter().cloned());
                }
                self.current_frame = (self.current_frame + steps % len) % len;
            } else {
                let last = len - 1;
                let target = self.current_frame.saturating_add(steps).min(last);
                for frame in self.current_frame + 1..=target {
                    triggered.extend(clip.frames[frame].triggers.iter().cloned());
                }
                // Stepping past the last frame is what finishes the clip, so
                // the last frame is shown for its full duration first.
                if self.current_frame.saturating_add(steps) > last {
                    self.playing = false;
                }
                self.current_frame = target;
            }
        }

        let frame = &clip.frames[self.current_frame];
        let elapsed_frac = if clip.frame_duration > 0.0 {
            self.elapsed / clip.frame_duration
        } else {
            0.0
        };
        let properties = sample_property_tracks(&clip.property_tracks, self.current_frame, elapsed_frac);
        Some(AnimTickResult {
            uv_rect: frame.uv_rect,
            texture_id: frame.texture_id.clone(),
            triggered,
            properties,
        })
    }

    /// Get the UV rect for the current frame without advancing.
    pub fn current_uv(&self) -> Option<[f32; 4]> {
        let clip_name = self.current_clip.as_ref()?;
        let clip = self.clips.get(clip_name)?;
        clip.frames.get(self.current_frame).map(|f| f.uv_rect)
    }

    /// Get the full frame result for the current frame without advancing.
    /// Useful for showing the first-frame preview in edit mode.
    pub fn current_frame_result(&self) -> Option<AnimTickResult> {
        let clip_name = self.current_clip.as_ref()?;
        let clip = self.clips.get(clip_name)?;
        clip.frames.get(self.current_frame).map(|f| {
            let elapsed_frac = if clip.frame_duration > 0.0 {
                self.elapsed / clip.frame_duration
            } else {
                0.0
            };
            let properties = sample_property_tracks(&clip.property_tracks, self.current_frame, elapsed_frac);
            AnimTickResult {
                uv_rect: f.uv_rect,
                texture_id: f.texture_id.clone(),
                triggered: Vec::new(),
                properties,
            }
        })
    }

    /// Fill `uv_rect` for every frame that names a `region`, from the atlas
    /// registered under that frame's texture - its own `texture_id`, or
    /// `sprite_texture` for a spritesheet frame. Returns whether nothing is
    /// left to resolve.
    ///
    /// A frame whose atlas is not loaded yet stays pending, so the caller
    /// retries next tick; the editor and web builds load atlases
    /// asynchronously. A region the loaded atlas does not have is reported
    /// once and left at its authored `uv_rect` rather than retried forever.
    pub fn resolve_frame_regions(
        &mut self,
        sprite_texture: Option<&str>,
        assets: &crate::asset::AssetManager,
    ) -> bool {
        let mut pending = false;
        for clip in self.clips.values_mut() {
            for (i, frame) in clip.frames.iter_mut().enumerate() {
                let Some(region) = frame.region.as_deref() else {
                    continue;
                };
                let Some(atlas_id) = frame.texture_id.as_deref().or(sprite_texture) else {
                    log::warn!(
                        "animation '{}' frame {i}: region '{region}' has no texture to look it up in",
                        clip.name
                    );
                    continue;
                };
                let Some(atlas) = assets.get_atlas_data(atlas_id) else {
                    pending = true;
                    continue;
                };
                match atlas.get_uv_rect(region) {
                    Some(uv) => frame.uv_rect = uv,
                    None => log::warn!(
                        "animation '{}' frame {i}: atlas '{atlas_id}' has no region '{region}'",
                        clip.name
                    ),
                }
            }
        }
        !pending
    }

    /// Apply `random_start` once: jump to a frame and a point within it picked
    /// from `seed`. Does nothing until a clip with frames is playing, so an
    /// animator started later by a script still gets it.
    pub fn apply_random_start(&mut self, seed: u64) {
        if !self.random_start || self.start_randomized || !self.playing {
            return;
        }
        let Some(clip) = self.current_clip.as_ref().and_then(|c| self.clips.get(c)) else {
            return;
        };
        if clip.frames.is_empty() {
            return;
        }
        // splitmix64: a cheap, well-mixed hash, so neighbouring entity ids
        // do not land on neighbouring frames.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        self.current_frame = (z % clip.frames.len() as u64) as usize;
        let within = ((z >> 32) as u32) as f32 / u32::MAX as f32;
        self.elapsed = within * clip.frame_duration.max(0.0);
        self.start_randomized = true;
    }

    /// Check if the animator is currently playing.
    pub fn is_playing(&self) -> bool {
        self.playing
    }
}

// ---------------------------------------------------------------------------
// Animation State Machine
// ---------------------------------------------------------------------------

/// A parameter value used in state machine transition conditions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum AnimParam {
    Bool(bool),
    Float(f32),
    /// One-shot trigger — consumed after the transition fires.
    Trigger,
}

/// Condition that must be met for a state transition to occur.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TransitionCondition {
    /// A boolean parameter must equal the given value.
    BoolEquals { param: String, value: bool },
    /// A float parameter must be greater than the given threshold.
    FloatGreaterThan { param: String, threshold: f32 },
    /// A float parameter must be less than the given threshold.
    FloatLessThan { param: String, threshold: f32 },
    /// The current animation clip must have finished (non-looping only).
    ClipFinished,
    /// A named trigger parameter must be set.
    TriggerSet { param: String },
}

/// A transition from one animation state to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    /// Target state name.
    pub target: String,
    /// All conditions must be true for this transition to fire.
    pub conditions: Vec<TransitionCondition>,
}

/// A single state in the animation state machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationState {
    /// The animation clip to play in this state.
    pub clip_name: String,
    /// Transitions out of this state, evaluated in order.
    pub transitions: Vec<StateTransition>,
}

/// Animation state machine component — manages transitions between animation states.
///
/// Attach alongside `SpriteAnimator`. The state machine drives which clip is
/// playing based on parameter values and transition conditions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnimStateMachine {
    /// Named states — each maps to a clip and has outgoing transitions.
    pub states: HashMap<String, AnimationState>,
    /// Currently active state name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_state: Option<String>,
    /// Parameters used to evaluate transition conditions.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub parameters: HashMap<String, AnimParam>,
}

impl AnimStateMachine {
    /// Set the initial state and start the corresponding clip on the animator.
    pub fn start(&mut self, state_name: &str, animator: &mut SpriteAnimator) {
        if let Some(state) = self.states.get(state_name) {
            self.current_state = Some(state_name.to_string());
            animator.play(&state.clip_name);
        } else {
            log::warn!("Animation state '{}' not found", state_name);
        }
    }

    /// Evaluate transitions and advance the state machine.
    /// Returns the name of the new state if a transition occurred.
    pub fn tick(&mut self, animator: &mut SpriteAnimator) -> Option<String> {
        let current = self.current_state.as_ref()?;
        let state = self.states.get(current)?.clone();

        for transition in &state.transitions {
            if self.evaluate_conditions(&transition.conditions, animator) {
                let target = transition.target.clone();
                if let Some(target_state) = self.states.get(&target) {
                    animator.play(&target_state.clip_name);
                    self.current_state = Some(target.clone());
                    // Consume any trigger params that were used
                    self.consume_triggers(&transition.conditions);
                    return Some(target);
                }
            }
        }

        None
    }

    /// Set a boolean parameter.
    pub fn set_bool(&mut self, name: &str, value: bool) {
        self.parameters
            .insert(name.to_string(), AnimParam::Bool(value));
    }

    /// Set a float parameter.
    pub fn set_float(&mut self, name: &str, value: f32) {
        self.parameters
            .insert(name.to_string(), AnimParam::Float(value));
    }

    /// Set a one-shot trigger — consumed after the next transition that uses it.
    pub fn set_trigger(&mut self, name: &str) {
        self.parameters
            .insert(name.to_string(), AnimParam::Trigger);
    }

    fn evaluate_conditions(
        &self,
        conditions: &[TransitionCondition],
        animator: &SpriteAnimator,
    ) -> bool {
        conditions.iter().all(|cond| match cond {
            TransitionCondition::BoolEquals { param, value } => {
                matches!(self.parameters.get(param), Some(AnimParam::Bool(v)) if v == value)
            }
            TransitionCondition::FloatGreaterThan { param, threshold } => {
                matches!(self.parameters.get(param), Some(AnimParam::Float(v)) if v > threshold)
            }
            TransitionCondition::FloatLessThan { param, threshold } => {
                matches!(self.parameters.get(param), Some(AnimParam::Float(v)) if v < threshold)
            }
            TransitionCondition::ClipFinished => !animator.is_playing(),
            TransitionCondition::TriggerSet { param } => {
                matches!(self.parameters.get(param), Some(AnimParam::Trigger))
            }
        })
    }

    fn consume_triggers(&mut self, conditions: &[TransitionCondition]) {
        for cond in conditions {
            if let TransitionCondition::TriggerSet { param } = cond {
                self.parameters.remove(param);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// What scripts read back
// ---------------------------------------------------------------------------

/// `Animation.get_current(entity)`: the sprite clip it plays, or else the clip
/// its skeleton plays. One rule, read by native's snapshot and the browsers'
/// mirror - native kept a map for it that nothing filled, and the browsers
/// read a field nothing published, so every runtime answered "".
pub fn current_clip_name(world: &hecs::World, entity: hecs::Entity) -> Option<String> {
    if let Ok(a) = world.get::<&SpriteAnimator>(entity)
        && let Some(clip) = &a.current_clip
    {
        return Some(clip.clone());
    }
    world
        .get::<&skeletal::SkeletalPlayer>(entity)
        .ok()
        .filter(|p| !p.clip.is_empty())
        .map(|p| p.clip.clone())
}

/// `Animation.is_playing(entity)`: its sprite animator or its skeleton is
/// advancing.
pub fn is_playing(world: &hecs::World, entity: hecs::Entity) -> bool {
    world.get::<&SpriteAnimator>(entity).is_ok_and(|a| a.playing)
        || world.get::<&skeletal::SkeletalPlayer>(entity).is_ok_and(|p| p.playing)
}

/// `Animation.get_state(entity)`: its state machine's current state.
pub fn state_name(world: &hecs::World, entity: hecs::Entity) -> Option<String> {
    world.get::<&AnimStateMachine>(entity).ok().and_then(|m| m.current_state.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_clip(frame_count: usize, looping: bool) -> AnimationClip {
        let frames: Vec<AnimationFrame> = (0..frame_count)
            .map(|i| {
                let u = i as f32 / frame_count as f32;
                AnimationFrame {
                    uv_rect: [u, 0.0, u + 1.0 / frame_count as f32, 1.0],
                    region: None,
                    texture_id: None,
                    triggers: Vec::new(),
                }
            })
            .collect();
        AnimationClip {
            name: "walk".to_string(),
            frames,
            frame_duration: 0.1,
            looping,
            property_tracks: Vec::new(),
        }
    }

    #[test]
    fn play_looping_animation() {
        let mut animator = SpriteAnimator::default();
        animator.clips.insert("walk".to_string(), make_test_clip(4, true));
        animator.play("walk");

        assert!(animator.is_playing());
        assert_eq!(animator.current_frame, 0);

        // Advance past first frame
        let _uv = animator.tick(0.11);
        assert_eq!(animator.current_frame, 1);

        // Advance to last frame
        animator.tick(0.11);
        animator.tick(0.11);
        assert_eq!(animator.current_frame, 3);

        // Should loop back
        animator.tick(0.11);
        assert_eq!(animator.current_frame, 0);
        assert!(animator.is_playing());
    }

    #[test]
    fn play_non_looping_stops() {
        let mut animator = SpriteAnimator::default();
        animator.clips.insert("die".to_string(), make_test_clip(3, false));
        animator.play("die");

        animator.tick(0.11);
        animator.tick(0.11);
        assert_eq!(animator.current_frame, 2);

        // Past the last frame: should stop
        animator.tick(0.11);
        assert_eq!(animator.current_frame, 2);
        assert!(!animator.is_playing());
    }

    #[test]
    fn a_clip_faster_than_the_frame_rate_keeps_its_speed() {
        // 100 fps clip, 20 fps game: five frames per tick. It used to advance
        // one, playing at a fifth of the authored speed.
        let mut animator = SpriteAnimator::default();
        let mut clip = make_test_clip(20, true);
        clip.frame_duration = 0.01;
        animator.clips.insert("walk".to_string(), clip);
        animator.play("walk");

        animator.tick(0.05);
        assert_eq!(animator.current_frame, 5);
        animator.tick(0.05);
        assert_eq!(animator.current_frame, 10);
        assert!(animator.elapsed < 0.01 + 1e-6, "surplus time left over: {}", animator.elapsed);
    }

    #[test]
    fn speed_is_honoured_above_the_frame_rate() {
        let mut animator = SpriteAnimator::default();
        animator.clips.insert("walk".to_string(), make_test_clip(8, true));
        animator.speed = 3.0;
        animator.play("walk");
        // 0.1 s per frame at 3x, a 0.1 s tick: three frames.
        animator.tick(0.1 + 1e-4);
        assert_eq!(animator.current_frame, 3);
    }

    #[test]
    fn a_hitch_does_not_leave_the_clip_racing_afterwards() {
        let mut animator = SpriteAnimator::default();
        animator.clips.insert("walk".to_string(), make_test_clip(4, true));
        animator.play("walk");

        animator.tick(1.05); // a one-second hitch: ten frames, 2.5 laps
        assert_eq!(animator.current_frame, 10 % 4);
        assert!(animator.elapsed < 0.1, "hitch time was kept: {}", animator.elapsed);

        // The next ordinary tick advances one frame, not another burst.
        animator.tick(0.1);
        assert_eq!(animator.current_frame, 3);
    }

    #[test]
    fn a_hitch_fires_each_frames_trigger_once() {
        let mut animator = SpriteAnimator::default();
        let mut clip = make_test_clip(3, true);
        clip.frames[1].triggers.push(FrameTrigger::EmitEvent {
            event_name: "footstep".to_string(),
            data: None,
        });
        animator.clips.insert("walk".to_string(), clip);
        animator.play("walk");

        // Ten laps' worth of time in one tick.
        let t = animator.tick(3.05).unwrap().triggered;
        assert_eq!(t.len(), 1, "one footstep, not one per lap: {t:?}");
    }

    #[test]
    fn a_non_looping_clip_crossed_in_one_tick_fires_what_it_passes_and_stops() {
        let mut animator = SpriteAnimator::default();
        let mut clip = make_test_clip(4, false);
        clip.frames[2].triggers.push(FrameTrigger::EmitEvent {
            event_name: "hit".to_string(),
            data: None,
        });
        animator.clips.insert("attack".to_string(), clip);
        animator.play("attack");

        let t = animator.tick(1.0).unwrap().triggered;
        assert_eq!(t.len(), 1, "the frame-2 trigger was skipped: {t:?}");
        assert_eq!(animator.current_frame, 3);
        assert!(!animator.is_playing());
    }

    #[test]
    fn a_shrunk_clip_does_not_panic() {
        let mut animator = SpriteAnimator::default();
        animator.clips.insert("walk".to_string(), make_test_clip(8, true));
        animator.play("walk");
        animator.current_frame = 7;
        animator.clips.insert("walk".to_string(), make_test_clip(2, true));
        assert!(animator.tick(0.01).is_some());
        assert!(animator.current_frame < 2);
    }

    #[test]
    fn random_start_spreads_identical_animators() {
        let frames: std::collections::HashSet<usize> = (0..32u64)
            .map(|seed| {
                let mut a = SpriteAnimator { random_start: true, ..Default::default() };
                a.clips.insert("walk".to_string(), make_test_clip(8, true));
                a.play("walk");
                a.apply_random_start(seed);
                assert!(a.start_randomized);
                a.current_frame
            })
            .collect();
        assert!(frames.len() > 4, "32 animators landed on only {} distinct frames", frames.len());
    }

    #[test]
    fn random_start_waits_for_a_clip_and_applies_once() {
        let mut a = SpriteAnimator { random_start: true, ..Default::default() };
        a.clips.insert("walk".to_string(), make_test_clip(8, true));
        a.apply_random_start(7);
        assert!(!a.start_randomized, "applied with nothing playing");

        a.play("walk");
        a.apply_random_start(7);
        assert!(a.start_randomized);
        a.current_frame = 0;
        a.apply_random_start(12345);
        assert_eq!(a.current_frame, 0, "applied a second time");

        let mut off = SpriteAnimator::default();
        off.clips.insert("walk".to_string(), make_test_clip(8, true));
        off.play("walk");
        off.apply_random_start(7);
        assert_eq!(off.current_frame, 0, "random_start is off");
    }

    fn atlas_2x1() -> crate::asset::AssetManager {
        let mut assets = crate::asset::AssetManager::new();
        let atlas: crate::renderer::atlas::AtlasData = serde_json::from_value(serde_json::json!({
            "width": 64, "height": 32,
            "regions": [
                { "name": "left",  "x": 0,  "y": 0, "width": 32, "height": 32 },
                { "name": "right", "x": 32, "y": 0, "width": 32, "height": 32 }
            ]
        }))
        .unwrap();
        assets.insert_atlas_data("sheet.png".into(), atlas);
        assets
    }

    fn region_clip(regions: &[&str], texture: Option<&str>) -> AnimationClip {
        AnimationClip {
            name: "walk".into(),
            frames: regions
                .iter()
                .map(|r| AnimationFrame {
                    uv_rect: [0.0, 0.0, 1.0, 1.0],
                    region: Some(r.to_string()),
                    texture_id: texture.map(str::to_string),
                    triggers: Vec::new(),
                })
                .collect(),
            frame_duration: 0.1,
            looping: true,
            property_tracks: Vec::new(),
        }
    }

    #[test]
    fn region_frames_resolve_against_the_sprite_texture() {
        let mut a = SpriteAnimator::default();
        a.clips.insert("walk".into(), region_clip(&["right", "left"], None));
        assert!(a.resolve_frame_regions(Some("sheet.png"), &atlas_2x1()));
        let frames = &a.clips["walk"].frames;
        assert_eq!(frames[0].uv_rect, [0.5, 0.0, 1.0, 1.0]);
        assert_eq!(frames[1].uv_rect, [0.0, 0.0, 0.5, 1.0]);
    }

    #[test]
    fn a_frame_texture_wins_over_the_sprite_texture() {
        let mut a = SpriteAnimator::default();
        a.clips.insert("walk".into(), region_clip(&["right"], Some("sheet.png")));
        assert!(a.resolve_frame_regions(Some("something_else.png"), &atlas_2x1()));
        assert_eq!(a.clips["walk"].frames[0].uv_rect, [0.5, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn region_frames_wait_for_their_atlas() {
        let mut a = SpriteAnimator::default();
        a.clips.insert("walk".into(), region_clip(&["right"], None));
        let empty = crate::asset::AssetManager::new();
        assert!(!a.resolve_frame_regions(Some("sheet.png"), &empty), "resolved with no atlas loaded");
        assert_eq!(a.clips["walk"].frames[0].uv_rect, [0.0, 0.0, 1.0, 1.0]);
        assert!(a.resolve_frame_regions(Some("sheet.png"), &atlas_2x1()));
    }

    #[test]
    fn an_unknown_region_does_not_block_resolution() {
        let mut a = SpriteAnimator::default();
        a.clips.insert("walk".into(), region_clip(&["nope"], None));
        assert!(a.resolve_frame_regions(Some("sheet.png"), &atlas_2x1()));
    }

    #[test]
    fn a_frame_can_be_authored_with_a_region_alone() {
        let f: AnimationFrame = serde_json::from_str(r#"{ "region": "left" }"#).unwrap();
        assert_eq!(f.region.as_deref(), Some("left"));
        assert_eq!(f.uv_rect, [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn stop_animation() {
        let mut animator = SpriteAnimator::default();
        animator.clips.insert("walk".to_string(), make_test_clip(4, true));
        animator.play("walk");
        animator.stop();
        assert!(!animator.is_playing());
    }

    #[test]
    fn frame_triggers_fire_on_transition() {
        let mut animator = SpriteAnimator::default();
        let mut clip = make_test_clip(3, true);
        clip.frames[1].triggers.push(FrameTrigger::PlaySound {
            clip_id: "step.wav".to_string(),
            volume: 1.0,
        });
        clip.frames[2].triggers.push(FrameTrigger::EmitEvent {
            event_name: "attack".to_string(),
            data: None,
        });
        animator.clips.insert("walk".to_string(), clip);
        animator.play("walk");

        // Frame 0 → no triggers (initial frame, no transition yet from tick)
        let r = animator.tick(0.05);
        assert!(r.unwrap().triggered.is_empty());

        // Frame 0 → 1: should fire PlaySound
        let r = animator.tick(0.11);
        let t = r.unwrap().triggered;
        assert_eq!(t.len(), 1);
        assert!(matches!(t[0], FrameTrigger::PlaySound { .. }));

        // Frame 1 → 2: should fire EmitEvent
        let r = animator.tick(0.11);
        let t = r.unwrap().triggered;
        assert_eq!(t.len(), 1);
        assert!(matches!(t[0], FrameTrigger::EmitEvent { .. }));
    }

    #[test]
    fn state_machine_transitions() {
        let mut animator = SpriteAnimator::default();
        animator.clips.insert("idle_clip".to_string(), make_test_clip(2, true));
        animator.clips.insert("walk_clip".to_string(), make_test_clip(4, true));

        let mut fsm = AnimStateMachine::default();
        fsm.states.insert("idle".to_string(), AnimationState {
            clip_name: "idle_clip".to_string(),
            transitions: vec![StateTransition {
                target: "walk".to_string(),
                conditions: vec![TransitionCondition::BoolEquals {
                    param: "moving".to_string(),
                    value: true,
                }],
            }],
        });
        fsm.states.insert("walk".to_string(), AnimationState {
            clip_name: "walk_clip".to_string(),
            transitions: vec![StateTransition {
                target: "idle".to_string(),
                conditions: vec![TransitionCondition::BoolEquals {
                    param: "moving".to_string(),
                    value: false,
                }],
            }],
        });

        // Start in idle
        fsm.start("idle", &mut animator);
        assert_eq!(fsm.current_state.as_deref(), Some("idle"));
        assert_eq!(animator.current_clip.as_deref(), Some("idle_clip"));

        // No transition without param
        let result = fsm.tick(&mut animator);
        assert!(result.is_none());

        // Set moving → transition to walk
        fsm.set_bool("moving", true);
        let result = fsm.tick(&mut animator);
        assert_eq!(result.as_deref(), Some("walk"));
        assert_eq!(animator.current_clip.as_deref(), Some("walk_clip"));

        // Set moving false → transition back to idle
        fsm.set_bool("moving", false);
        let result = fsm.tick(&mut animator);
        assert_eq!(result.as_deref(), Some("idle"));
    }

    #[test]
    fn trigger_param_consumed_after_transition() {
        let mut animator = SpriteAnimator::default();
        animator.clips.insert("idle_clip".to_string(), make_test_clip(2, true));
        animator.clips.insert("attack_clip".to_string(), make_test_clip(3, false));

        let mut fsm = AnimStateMachine::default();
        fsm.states.insert("idle".to_string(), AnimationState {
            clip_name: "idle_clip".to_string(),
            transitions: vec![StateTransition {
                target: "attack".to_string(),
                conditions: vec![TransitionCondition::TriggerSet {
                    param: "attack".to_string(),
                }],
            }],
        });
        fsm.states.insert("attack".to_string(), AnimationState {
            clip_name: "attack_clip".to_string(),
            transitions: vec![],
        });

        fsm.start("idle", &mut animator);
        fsm.set_trigger("attack");

        // Should transition and consume trigger
        let result = fsm.tick(&mut animator);
        assert_eq!(result.as_deref(), Some("attack"));
        assert!(!fsm.parameters.contains_key("attack"));
    }
}
