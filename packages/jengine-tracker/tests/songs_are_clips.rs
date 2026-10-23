//! A `.song` reaches the engine's clip loaders once the package is in a
//! host: `jengine_core::audio::transcode` - what both audio backends ask
//! before decoding a file - renders it, with its loop point.

use jengine_core::audio::transcode;
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::host::PackageHost;

#[test]
fn a_host_with_the_package_plays_songs_as_clips() {
    let song = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/sunny-fields.song")).unwrap();

    let mut host = PackageHost::new();
    host.add_package(jengine_tracker::package());
    host.initialize(&mut ComponentRegistry::new());

    // Not a song: left to the decoder.
    assert!(transcode::transcode("sounds/hit.wav", b"RIFF").is_none());

    // Prepared at preload, then taken by the loader.
    transcode::prepare("music/sunny-fields.song", &song).unwrap();
    let t = transcode::transcode("music/sunny-fields.song", &song).unwrap().unwrap();
    assert_eq!(t.rate, jengine_tracker::RATE);
    assert!(t.duration() > 40.0 && t.duration() < 60.0, "{}", t.duration());
    assert!(t.left.iter().any(|s| s.abs() > 0.1));
    // Sunny Fields loops to order 1, past its intro.
    let at = t.loop_start_seconds().expect("a loop point past the top");
    assert!(at > 0.0 && at < t.duration());

    // A broken song is an error naming it, not a decode failure.
    let e = transcode::transcode("music/broken.song", br#"{"version": 1, "order": []}"#).unwrap().unwrap_err();
    assert!(e.contains("broken.song") && e.contains("no instruments"), "{e}");
}
