//! Video (Oct 2026; Godot's `VideoStreamPlayer`, Unity's `VideoPlayer`): a
//! movie playing into a texture, so a sprite, a mesh or a UI image naming
//! that texture shows it - a cutscene filling the screen, a television in
//! the corner of a room, an animated poster.
//!
//! A [`VideoPlayer`] names a video file under `assets/` (`source`) and the
//! texture its frames go to (`texture_id`, the source's own path when left
//! empty). The [`VideoSystem`] each host owns plays it: the host gives it a
//! [`DecoderFactory`] - the platform's decoder, since there is no one video
//! decoder that runs everywhere - and calls [`VideoSystem::tick`] once a
//! frame before drawing. Frames arrive as RGBA pixels and are written into
//! the texture through the runtime-texture path every renderer already has
//! (`AssetManager::texture_bytes_mut` + `mark_texture_dirty`), so no draw
//! code knows a video from a picture.
//!
//! The **decoder owns the clock** ([`VideoDecoder::update`]): in a browser
//! the `<video>` element is the authority on time and a game cannot drive it
//! frame by frame, so the system asks each decoder for what is due rather
//! than telling it. The decoders:
//!
//! - Windows: Media Foundation, decoding on a thread of its own
//!   (`jengine-runtime`'s `video_mf`), so a frame never waits on the decoder;
//! - the editor and web builds: an HTML `<video>` element, copied into the
//!   texture each time it shows a new frame (`video::web`);
//! - Android: none yet - the texture shows black and the log says why.
//!
//! Video is **silent**: a soundtrack is an ordinary audio clip, played
//! beside it, so a game sounds the same on every runtime.

#[cfg(all(feature = "web", target_arch = "wasm32"))]
pub mod web;

use std::collections::{HashMap, HashSet};

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::asset::{AssetManager, RawTextureData};

fn default_true() -> bool {
    true
}
fn default_speed() -> f32 {
    1.0
}
fn no_seek() -> f32 {
    -1.0
}

/// `video_player`: plays a video into a texture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoPlayer {
    /// The video file, under `assets/` (`videos/intro.mp4`).
    pub source: String,
    /// The texture the frames go to; the source's path when empty.
    #[serde(default)]
    pub texture_id: String,
    #[serde(default = "default_true")]
    pub playing: bool,
    #[serde(default = "default_true")]
    pub looping: bool,
    #[serde(default = "default_speed")]
    pub speed: f32,
    /// A time to jump to, in seconds, taken on the next tick (then -1).
    #[serde(default = "no_seek")]
    pub seek: f32,
}

impl Default for VideoPlayer {
    fn default() -> Self {
        Self { source: String::new(), texture_id: Default::default(), playing: true, looping: true, speed: 1.0, seek: -1.0 }
    }
}

impl VideoPlayer {
    /// The texture this player's frames go to.
    pub fn target(&self) -> &str {
        if self.texture_id.is_empty() { &self.source } else { &self.texture_id }
    }
}

/// One decoded frame: RGBA8, top row first.
#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// How a player wants its video played this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayState {
    pub playing: bool,
    pub looping: bool,
    pub speed: f32,
}

/// A platform's video decoder, one per playing video. It keeps its own
/// clock (see the module docs).
pub trait VideoDecoder {
    /// Move on by `dt` seconds (when playing) and return a new frame if one
    /// is due - `None` when the one on show is still right.
    fn update(&mut self, dt: f64, state: PlayState) -> Option<VideoFrame>;
    /// Jump to `t` seconds.
    fn seek(&mut self, t: f64);
    /// Whether a video that does not loop has played to its end.
    fn ended(&self) -> bool;
}

/// Makes the decoder for a video file (its path under `assets/`), or `None`
/// when this platform cannot play it.
pub type DecoderFactory = Box<dyn FnMut(&str) -> Option<Box<dyn VideoDecoder>>>;

/// The handler a player's script gets when a video that does not loop ends.
pub const END_HANDLER: &str = "on_video_end";

struct Playing {
    source: String,
    target: String,
    decoder: Box<dyn VideoDecoder>,
    ended: bool,
}

/// Every video playing, and the factory that makes their decoders. Each
/// host owns one.
pub struct VideoSystem {
    factory: DecoderFactory,
    playing: HashMap<Entity, Playing>,
    /// Sources no decoder could open, so each is reported once.
    failed: HashSet<String>,
}

impl VideoSystem {
    pub fn new(factory: DecoderFactory) -> Self {
        Self { factory, playing: HashMap::new(), failed: HashSet::new() }
    }

    /// A system with no decoder at all: every video shows black. What a
    /// platform without video support gets, so its log says so once.
    pub fn unsupported() -> Self {
        Self::new(Box::new(|_| None))
    }

    /// Forget every video - a scene change.
    pub fn clear(&mut self) {
        self.playing.clear();
    }

    /// Open, advance and present every `video_player` in `world`.
    pub fn tick(&mut self, world: &mut World, assets: &mut AssetManager, dt: f32) {
        let players: Vec<(Entity, VideoPlayer)> =
            world.query::<&VideoPlayer>().iter().map(|(e, p)| (e, p.clone())).collect();
        self.playing.retain(|e, _| players.iter().any(|(p, _)| p == e));
        for (e, player) in players {
            if player.source.is_empty() {
                continue;
            }
            let target = player.target().to_string();
            // Something to draw from the first frame, so nothing tries to
            // load the target as a picture file.
            if !assets.has_texture_bytes(&target) {
                assets.insert_texture_bytes(target.clone(), RawTextureData { width: 1, height: 1, rgba: vec![0, 0, 0, 255] });
            }
            let reopen = self.playing.get(&e).is_none_or(|p| p.source != player.source || p.target != target);
            if reopen {
                self.playing.remove(&e);
                match (self.factory)(&player.source) {
                    Some(decoder) => {
                        self.playing.insert(e, Playing { source: player.source.clone(), target: target.clone(), decoder, ended: false });
                    }
                    None => {
                        if self.failed.insert(player.source.clone()) {
                            log::warn!("video '{}': this platform cannot play it - its texture stays black", player.source);
                        }
                        continue;
                    }
                }
            }
            let Some(p) = self.playing.get_mut(&e) else { continue };
            if player.seek >= 0.0 {
                p.decoder.seek(player.seek as f64);
                p.ended = false;
                if let Ok(mut c) = world.get::<&mut VideoPlayer>(e) {
                    c.seek = -1.0;
                }
            }
            let state = PlayState { playing: player.playing, looping: player.looping, speed: player.speed.max(0.0) };
            if let Some(frame) = p.decoder.update(dt as f64, state) {
                present(assets, &p.target, frame);
            }
            if !player.looping && p.decoder.ended() && !p.ended {
                p.ended = true;
                if let Ok(mut c) = world.get::<&mut VideoPlayer>(e) {
                    c.playing = false;
                }
                crate::ecs::callbacks::push(e, END_HANDLER);
            }
        }
    }
}

/// Write a frame into its texture: in place when the size is the same, so
/// only the pixels go to the GPU; anew when it changed.
pub fn present(assets: &mut AssetManager, target: &str, frame: VideoFrame) {
    let (w, h) = (frame.width, frame.height);
    if w == 0 || h == 0 || frame.rgba.len() < (w * h * 4) as usize {
        return;
    }
    match assets.texture_bytes_mut(target) {
        Some(t) if t.width == w && t.height == h => {
            t.rgba.copy_from_slice(&frame.rgba[..(w * h * 4) as usize]);
            assets.mark_texture_dirty(target, 0, 0, w, h);
        }
        _ => {
            assets.insert_texture_bytes(target.to_string(), RawTextureData { width: w, height: h, rgba: frame.rgba });
            // The GPU copy was the old size: drop it, so the next draw
            // uploads the new one.
            #[cfg(feature = "gpu-wgpu")]
            assets.remove_wgpu_texture(target);
        }
    }
}

/// The textures videos draw into - made at run time, not files to load.
pub fn video_texture_ids(world: &World) -> std::collections::BTreeSet<String> {
    world.query::<&VideoPlayer>().iter().map(|(_, p)| p.target().to_string()).filter(|t| !t.is_empty()).collect()
}

/// `Video.set(e, field, value)`: write any field by name - `playing` to
/// pause and play, `seek` to jump, `source` to change the film.
pub fn set_video_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let Ok(mut c) = world.get::<&mut VideoPlayer>(entity) else {
        return Err("no video_player on that entity".into());
    };
    let mut whole = serde_json::to_value(&*c).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) {
        return Err(format!("'{field}' is not a video_player field"));
    }
    map.insert(field.to_string(), value.clone());
    *c = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    Ok(())
}
