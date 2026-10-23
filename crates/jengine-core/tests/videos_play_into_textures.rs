//! Video (`video`): a `video_player` plays a file into a texture through
//! the platform's decoder, which keeps the clock; frames are written into
//! the texture by the runtime-texture path, so any sprite naming it shows it.

use std::cell::RefCell;
use std::rc::Rc;

use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::video::{PlayState, VideoDecoder, VideoFrame, VideoPlayer, VideoSystem, set_video_field, video_texture_ids};

/// What the fake decoder was told, shared with the test.
#[derive(Default)]
struct Log {
    seeks: Vec<f64>,
    states: Vec<PlayState>,
}

/// A decoder that hands out a frame of `size` filled with an increasing
/// grey each update, and ends after `frames` when not looping.
struct Fake {
    log: Rc<RefCell<Log>>,
    size: (u32, u32),
    shown: u32,
    frames: u32,
}

impl VideoDecoder for Fake {
    fn update(&mut self, _dt: f64, state: PlayState) -> Option<VideoFrame> {
        self.log.borrow_mut().states.push(state);
        if !state.playing || (self.shown >= self.frames && !state.looping) {
            return None;
        }
        self.shown += 1;
        let (w, h) = self.size;
        Some(VideoFrame { width: w, height: h, rgba: vec![self.shown as u8; (w * h * 4) as usize] })
    }
    fn seek(&mut self, t: f64) {
        self.log.borrow_mut().seeks.push(t);
        self.shown = 0;
    }
    fn ended(&self) -> bool {
        self.shown >= self.frames
    }
}

fn system(log: Rc<RefCell<Log>>, size: (u32, u32), frames: u32) -> VideoSystem {
    VideoSystem::new(Box::new(move |_source: &str| {
        Some(Box::new(Fake { log: log.clone(), size, shown: 0, frames }) as Box<dyn VideoDecoder>)
    }))
}

#[test]
fn frames_go_into_the_texture_a_sprite_names() {
    let log = Rc::new(RefCell::new(Log::default()));
    let mut videos = system(log.clone(), (4, 2), 100);
    let mut world = World::new();
    world.spawn((VideoPlayer { source: "videos/a.mp4".into(), ..Default::default() },));
    let tv = world.spawn((VideoPlayer { source: "videos/a.mp4".into(), texture_id: "screens/tv".into(), ..Default::default() },));
    let mut assets = AssetManager::new();
    videos.tick(&mut world, &mut assets, 1.0 / 30.0);
    let t = assets.get_texture_bytes("videos/a.mp4").expect("the source's own path when no texture is named");
    assert_eq!((t.width, t.height), (4, 2));
    assert_eq!(t.rgba[0], 1, "the first frame");
    assert_eq!(assets.get_texture_bytes("screens/tv").unwrap().rgba[0], 1);
    videos.tick(&mut world, &mut assets, 1.0 / 30.0);
    assert_eq!(assets.get_texture_bytes("videos/a.mp4").unwrap().rgba[0], 2, "written in place");
    // Pause: the decoder is told, and no new frame comes.
    set_video_field(&world, tv, "playing", &serde_json::json!(false)).unwrap();
    videos.tick(&mut world, &mut assets, 1.0 / 30.0);
    assert_eq!(assets.get_texture_bytes("screens/tv").unwrap().rgba[0], 2);
    assert!(log.borrow().states.iter().any(|s| !s.playing));
}

#[test]
fn a_seek_reaches_the_decoder_once() {
    let log = Rc::new(RefCell::new(Log::default()));
    let mut videos = system(log.clone(), (2, 2), 100);
    let mut world = World::new();
    let e = world.spawn((VideoPlayer { source: "v.mp4".into(), ..Default::default() },));
    let mut assets = AssetManager::new();
    videos.tick(&mut world, &mut assets, 0.1);
    set_video_field(&world, e, "seek", &serde_json::json!(2.5)).unwrap();
    videos.tick(&mut world, &mut assets, 0.1);
    videos.tick(&mut world, &mut assets, 0.1);
    assert_eq!(log.borrow().seeks, vec![2.5]);
    assert_eq!(world.get::<&VideoPlayer>(e).unwrap().seek, -1.0, "taken");
}

#[test]
fn a_video_that_does_not_loop_ends_once_and_says_so() {
    let log = Rc::new(RefCell::new(Log::default()));
    let mut videos = system(log, (2, 2), 3);
    let mut world = World::new();
    let e = world.spawn((VideoPlayer { source: "v.mp4".into(), looping: false, ..Default::default() },));
    let mut assets = AssetManager::new();
    jengine_core::ecs::callbacks::take();
    for _ in 0..10 {
        videos.tick(&mut world, &mut assets, 0.1);
    }
    assert!(!world.get::<&VideoPlayer>(e).unwrap().playing, "stopped at its end");
    let owed = jengine_core::ecs::callbacks::take();
    assert_eq!(owed.len(), 1, "{owed:?}");
    assert_eq!((owed[0].entity, owed[0].handler), (e, "on_video_end"));
}

#[test]
fn a_platform_without_video_shows_black_and_the_texture_is_not_a_file() {
    let mut videos = VideoSystem::unsupported();
    let mut world = World::new();
    world.spawn((VideoPlayer { source: "videos/intro.mp4".into(), ..Default::default() },));
    world.spawn((jengine_core::ecs::components::SpriteComponent { texture_id: "videos/intro.mp4".into(), ..Default::default() },));
    let mut assets = AssetManager::new();
    videos.tick(&mut world, &mut assets, 0.1);
    let t = assets.get_texture_bytes("videos/intro.mp4").expect("a black placeholder");
    assert_eq!((t.width, t.height, &t.rgba[..]), (1, 1, &[0u8, 0, 0, 255][..]));
    assert!(video_texture_ids(&world).contains("videos/intro.mp4"));
    let to_load = jengine_core::scene::format::world_texture_ids(&world);
    assert!(!to_load.contains("videos/intro.mp4"), "a video's texture is written, not loaded");
}

#[test]
fn the_scene_keeps_it_and_scripts_set_it() {
    let mut world = World::new();
    let e = world.spawn((VideoPlayer { source: "v.mp4".into(), speed: 2.0, ..Default::default() },));
    set_video_field(&world, e, "looping", &serde_json::json!(false)).unwrap();
    assert!(set_video_field(&world, e, "volume", &serde_json::json!(1)).is_err(), "video is silent");
    let scene = jengine_core::scene::format::save_scene(&world);
    let saved = scene.entities.iter().find_map(|e| e.components.video_player.as_ref()).expect("saved");
    assert_eq!((saved.speed, saved.looping), (2.0, false));
}

#[test]
fn every_host_plays_videos() {
    for (name, src) in [
        ("native", include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs")),
        ("editor", include_str!("../../jengine-editor-viewport/src/lib.rs")),
        ("web", include_str!("../../jengine-web/src/lib.rs")),
    ] {
        assert!(src.contains(".video.tick("), "{name} never plays its videos");
    }
    let editor = include_str!("../../jengine-editor-viewport/src/lib.rs");
    assert!(editor.contains("pub fn viewport_load_video(") && editor.contains("pub fn viewport_take_video_requests("));
    let svelte = include_str!("../../../editor/src/lib/Viewport.svelte");
    assert!(svelte.contains("loadRequestedVideos();"), "the editor never hands the viewport its video files");
}
