//! **Screenshots** (`Screenshot.take`, Oct 2026; `renderer::screenshot`):
//! the finished frame copied into a texture a sprite can show, and
//! natively saved as a PNG. There was no way to keep a picture of the game
//! from inside it - no photo mode, no save-slot thumbnail. The copy is made
//! in `WgpuFrame::present` and delivered in `ecs::carve::apply_carves`, two
//! steps every host already takes, so no host was wired. Found by the
//! showcase's Photo Mode.

mod support;

use jengine_core::asset::AssetManager;
use jengine_core::renderer::screenshot::{self, Request};

#[test]
fn a_request_reads_its_table_and_gets_an_id() {
    let a = screenshot::request(&serde_json::json!({ "texture": "photos/1", "file": "one", "scale": 0.25 }));
    let b = screenshot::request(&serde_json::Value::Null);
    assert!(b > a, "ids count up");
    assert!(screenshot::wanted());
    let q = screenshot::take_queued();
    assert_eq!(
        q,
        vec![
            Request { id: a, texture: Some("photos/1".into()), file: Some("one".into()), scale: 0.25 },
            Request { id: b, texture: None, file: None, scale: 1.0 },
        ]
    );
    assert!(!screenshot::wanted());
    assert!(!screenshot::is_done(a), "nothing is done until it is written");
}

#[test]
fn rows_lose_their_padding_and_their_bgr_order() {
    // Two pixels a row, rows padded to 12 bytes: blue, green / red, white.
    let data = [255, 0, 0, 255, 0, 255, 0, 255, 9, 9, 9, 9, 0, 0, 255, 255, 255, 255, 255, 255, 9, 9, 9, 9];
    let rgba = screenshot::unpad_rgba(&data, 2, 2, 12, true);
    assert_eq!(rgba, vec![0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255]);
}

#[test]
fn a_thumbnail_is_the_average_of_what_it_shrinks() {
    let rgba: Vec<u8> = [[0u8, 0, 0, 255], [200, 100, 50, 255], [100, 100, 100, 255], [100, 0, 50, 255]].concat();
    let (px, w, h) = screenshot::shrink(&rgba, 2, 2, 0.5);
    assert_eq!((w, h), (1, 1));
    assert_eq!(px, vec![100, 50, 50, 255]);
}

#[test]
fn a_written_shot_is_a_texture_a_file_and_done() {
    let dir = std::env::temp_dir().join(format!("jengine_shots_{}", std::process::id()));
    // SAFETY-free in practice: this binary reads JENGINE_SAVE_DIR only here.
    unsafe { std::env::set_var("JENGINE_SAVE_DIR", &dir) };
    let mut assets = AssetManager::new();
    let rgba = vec![10u8; 4 * 4 * 4];
    let r = Request { id: 77, texture: Some("photos/test".into()), file: Some("../evil:name".into()), scale: 0.5 };
    screenshot::write(&[r], &rgba, 4, 4, &mut assets);
    let tex = assets.get_texture_bytes("photos/test").expect("the texture is there");
    assert_eq!((tex.width, tex.height), (2, 2), "at half size");
    let path = screenshot::file_path("../evil:name");
    assert!(path.starts_with(dir.join("screenshots")), "a name cannot climb out: {}", path.display());
    assert!(path.is_file(), "the PNG was written: {}", path.display());
    assert!(screenshot::is_done(77));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The whole path on a GPU: a frame drawn, copied, mapped and delivered.
#[cfg(feature = "gpu-wgpu")]
#[test]
fn a_frame_is_copied_out_and_delivered() {
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::screenshot::gpu;
    let Some(gpu_) = HeadlessGpu::new(64, 32) else {
        eprintln!("screenshots_capture_the_frame: no GPU adapter - skipped");
        return;
    };
    let id = screenshot::request(&serde_json::json!({ "texture": "photos/frame" }));
    let mut encoder = gpu_.encoder();
    drop(gpu_.color_pass(&mut encoder, wgpu::Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }));
    let capture = gpu::begin(&gpu_.device, 64, 32, gpu_.color_format).expect("a capture for a wanted shot");
    capture.encode(&mut encoder, gpu_.texture());
    gpu_.submit(encoder);
    capture.submitted();
    let mut assets = AssetManager::new();
    for _ in 0..50 {
        gpu_.device.poll(wgpu::Maintain::Wait);
        gpu::deliver(&mut assets);
        if screenshot::is_done(id) {
            break;
        }
    }
    assert!(screenshot::is_done(id), "delivered once the GPU finished");
    let tex = assets.get_texture_bytes("photos/frame").expect("the frame is a texture");
    assert_eq!((tex.width, tex.height), (64, 32));
    let px = &tex.rgba[(16 * 64 + 32) * 4..(16 * 64 + 32) * 4 + 4];
    assert!(px[0] > 240 && px[1] < 10 && px[2] < 10, "the red it was cleared to: {px:?}");
}

/// Every host presents through `WgpuFrame::present` and calls
/// `apply_carves`, the two steps a screenshot rides on.
#[test]
fn every_host_presents_through_the_frame_and_delivers() {
    for (name, src) in [
        ("native", &support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs"))),
        ("editor viewport", &support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs"))),
        ("web", &support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs"))),
    ] {
        assert!(src.contains("frame.present("), "{name} does not present through WgpuFrame");
        assert!(src.contains("apply_carves("), "{name} never calls apply_carves, which delivers screenshots");
    }
}
