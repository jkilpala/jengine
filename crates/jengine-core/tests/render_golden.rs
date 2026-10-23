//! Pixel tests: render through the real pipeline and compare against a
//! known-good image.
//!
//! Every other check in this repo looks at the data going *in* - sort keys,
//! uv rects, layout rects - and nothing looked at the pixels coming out. The
//! costly bugs of Sep 2026 all lived in that gap: a `uv_rect` written as
//! `[x, y, w, h]` drew a smeared slice (BUGS #78, #82), an atlas whose image
//! did not match its metadata drew blanks (#81), five entity-UI widgets drew
//! nothing at all (#58). Each was found by a person looking at the screen.
//!
//! Needs a GPU. Without an adapter - headless CI, a sandbox - each test prints
//! why and passes, because a renderer test that cannot render proves nothing
//! either way. Run them where there is a GPU before trusting a render change.
//!
//! **Updating a golden:** delete it, or run with `JENGINE_UPDATE_GOLDEN=1`, and
//! look at the new image before committing it. A golden is only worth what the
//! eyes that approved it saw.

#![cfg(feature = "gpu-wgpu")]

use std::path::{Path, PathBuf};

use glam::Vec2;
use hecs::World;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::ecs::components::{GuiComponent, SpriteComponent, TextComponent, Transform};
use jengine_core::ecs::systems::resolve_sprite_regions;
use jengine_core::ecs::wgpu_systems::{
    UiFlush, render_gui_wgpu, render_sprites_wgpu_multi_materialized, render_text_wgpu, render_ui_wgpu,
};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::text::built_in_font_atlas;
use jengine_core::ui::gui_document::load_gui;
use jengine_core::ui::scaling::UiScale;
use jengine_core::ui::{UIElement, UIPanel, UIProgressBar, UISlider, UIToggle};

const W: u32 = 256;
const H: u32 = 128;

/// A 2x2 sheet of flat colours, each cell 32x32: the four quadrants a region
/// test needs, with no dependency on any project's art.
fn quad_sheet() -> RawTextureData {
    const CELL: u32 = 32;
    let (w, h) = (CELL * 2, CELL * 2);
    let colours = [
        [220u8, 60, 60, 255],
        [60, 200, 90, 255],
        [70, 110, 230, 255],
        [235, 190, 40, 255],
    ];
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let cell = (y / CELL) * 2 + (x / CELL);
            rgba.extend_from_slice(&colours[cell as usize]);
        }
    }
    RawTextureData { width: w, height: h, rgba }
}

fn quad_atlas() -> jengine_core::renderer::atlas::AtlasData {
    serde_json::from_value(serde_json::json!({
        "width": 64, "height": 64,
        "regions": [
            { "name": "tl", "x": 0,  "y": 0,  "width": 32, "height": 32 },
            { "name": "tr", "x": 32, "y": 0,  "width": 32, "height": 32 },
            { "name": "bl", "x": 0,  "y": 32, "width": 32, "height": 32 },
            { "name": "br", "x": 32, "y": 32, "width": 32, "height": 32 }
        ]
    }))
    .unwrap()
}

fn sprite_at(x: f32, y: f32, region: &str) -> (Transform, SpriteComponent) {
    (
        Transform { position: Vec2::new(x, y), ..Default::default() },
        SpriteComponent {
            texture_id: "sheet.png".into(),
            region: Some(region.into()),
            size: Some([48.0, 48.0]),
            ..Default::default()
        },
    )
}

/// The style every text fixture shares: built-in face, native size.
fn text_style() -> TextComponent {
    TextComponent {
        text: String::new(),
        font_id: String::new(),
        font_scale: 1.0,
        color: [1.0, 1.0, 1.0, 1.0],
        z_order: 0,
        sort_layer: None,
        wrap_width: 0.0,
        align: Default::default(),
        ..Default::default()
    }
}

fn text_at(x: f32, y: f32, text: &str, color: [f32; 4]) -> (Transform, TextComponent) {
    (
        Transform { position: Vec2::new(x, y), ..Default::default() },
        TextComponent { text: text.into(), color, ..text_style() },
    )
}

/// The document `gui_documents_draw_their_widgets` renders. Authored as JSON
/// rather than built from the types so it exercises the loader too - a scene
/// ships JSON, and a field that stops deserializing is the failure mode.
const GOLDEN_GUI: &str = r#"{
  "name": "Golden",
  "anchor": "TopLeft",
  "root": {
    "id": "root",
    "node_type": { "type": "Panel" },
    "style": {
      "width": { "type": "Percent", "value": 100.0 },
      "height": { "type": "Percent", "value": 100.0 },
      "padding": { "top": 10.0, "right": 10.0, "bottom": 10.0, "left": 10.0 },
      "bg_color": [0.13, 0.14, 0.20, 1.0],
      "flex": { "direction": "Column", "gap": 8.0, "align_items": "Start", "justify_content": "Start" }
    },
    "children": [
      {
        "id": "title",
        "node_type": {
          "type": "Text", "text": "Score: {{score}}", "font_id": "",
          "font_size": 16.0, "color": [1.0, 1.0, 1.0, 1.0], "alignment": "Left"
        },
        "style": {
          "width": { "type": "Fixed", "value": 160.0 },
          "height": { "type": "Fixed", "value": 20.0 }
        },
        "children": []
      },
      {
        "id": "bar",
        "node_type": { "type": "ProgressBar", "value": 0.6, "min": 0.0, "max": 1.0 },
        "style": {
          "width": { "type": "Fixed", "value": 160.0 },
          "height": { "type": "Fixed", "value": 16.0 }
        },
        "children": []
      },
      {
        "id": "go",
        "node_type": { "type": "Button", "label": "Start", "font_id": "", "font_size": 14.0 },
        "style": {
          "width": { "type": "Fixed", "value": 80.0 },
          "height": { "type": "Fixed", "value": 24.0 }
        },
        "children": []
      },
      {
        "id": "icon",
        "node_type": {
          "type": "Image", "texture_id": "sheet.png", "tint": [1.0, 1.0, 1.0, 1.0],
          "uv_rect": [0.5, 0.0, 1.0, 0.5]
        },
        "style": {
          "width": { "type": "Fixed", "value": 32.0 },
          "height": { "type": "Fixed", "value": 32.0 },
          "constraints": {
            "right": { "type": "Px", "value": 14.0 },
            "top": { "type": "Px", "value": 14.0 }
          }
        },
        "children": []
      }
    ]
  }
}"#;

#[test]
fn sprites_draw_their_named_atlas_regions() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };

    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("sheet.png".into(), quad_sheet());
    assets.register_atlas("sheet.png".into(), quad_atlas());

    // One sprite per region, left to right in the order tl, tr, bl, br: each
    // square must show its own flat colour. A uv_rect read as [x, y, w, h]
    // instead of min/max corners shows mixed or empty squares here.
    let mut world = World::new();
    world.spawn(sprite_at(-90.0, 0.0, "tl"));
    world.spawn(sprite_at(-30.0, 0.0, "tr"));
    world.spawn(sprite_at(30.0, 0.0, "bl"));
    world.spawn(sprite_at(90.0, 0.0, "br"));
    resolve_sprite_regions(&mut world, &assets);

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    assets
        .ensure_wgpu_texture("sheet.png", &gpu.device, &gpu.queue)
        .expect("upload sheet");

    // Through the same materialized path the runtimes draw with; the
    // test-only cloning path it used to go through is gone.
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color { r: 0.05, g: 0.05, b: 0.07, a: 1.0 });
        batch.begin_frame();
        render_sprites_wgpu_multi_materialized(
            &world,
            &SortLayers::default(),
            &mut batch,
            Vec2::splat(32.0),
            |b, tex, _mat| {
                let texture = assets.get_wgpu_texture(tex).expect("sheet uploaded");
                b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture);
            },
        );
    }
    gpu.submit(encoder);

    compare_to_golden("sprite_regions", &gpu.read_rgba());
}

#[test]
fn entity_ui_widgets_draw() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };

    // A panel, a half-full progress bar, a slider at a quarter, and a toggle
    // that is on. Until Sep 2026 only the panel drew anything (BUGS #58).
    let mut world = World::new();
    world.spawn((
        UIElement {
            anchor: jengine_core::ui::UIAnchor::TopLeft,
            offset: Vec2::new(70.0, -24.0),
            size: Vec2::new(120.0, 24.0),
            ..Default::default()
        },
        UIPanel { bg_color: [0.25, 0.25, 0.32, 1.0] },
    ));
    world.spawn((
        UIElement {
            anchor: jengine_core::ui::UIAnchor::TopLeft,
            offset: Vec2::new(70.0, -60.0),
            size: Vec2::new(120.0, 20.0),
            ..Default::default()
        },
        UIProgressBar { value: 0.5, ..Default::default() },
    ));
    world.spawn((
        UIElement {
            anchor: jengine_core::ui::UIAnchor::TopLeft,
            offset: Vec2::new(70.0, -88.0),
            size: Vec2::new(120.0, 20.0),
            ..Default::default()
        },
        UISlider { value: 0.25, ..Default::default() },
    ));
    world.spawn((
        UIElement {
            anchor: jengine_core::ui::UIAnchor::TopLeft,
            offset: Vec2::new(200.0, -88.0),
            size: Vec2::new(40.0, 20.0),
            ..Default::default()
        },
        UIToggle { checked: true, ..Default::default() },
    ));

    let mut assets = AssetManager::new();
    // Widget chrome is the white pixel tinted; every host registers it.
    assets.insert_texture_bytes(
        jengine_core::asset::WHITE_TEXTURE_ID.into(),
        RawTextureData { width: 1, height: 1, rgba: vec![255, 255, 255, 255] },
    );

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let ui_vp = glam::Mat4::orthographic_rh_gl(0.0, W as f32, 0.0, H as f32, -1.0, 1.0);
    batch.begin_frame();

    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color { r: 0.05, g: 0.05, b: 0.07, a: 1.0 });
        let device = &gpu.device;
        let queue = &gpu.queue;
        let mut flush = |batch: &mut WgpuSpriteBatch, what: UiFlush<'_>| {
            let id = match what {
                UiFlush::Quad(None) => jengine_core::asset::WHITE_TEXTURE_ID,
                UiFlush::Quad(Some(id)) => id,
                // No fonts and no clipping in this fixture.
                UiFlush::Font(_) | UiFlush::Clip(_) => return,
            };
            if assets.ensure_wgpu_texture(id, device, queue).is_err() {
                batch.discard_pending();
                return;
            }
            let texture = assets.get_wgpu_texture(id).expect("just ensured");
            batch.flush(device, queue, &mut pass, &ui_vp, texture);
        };
        render_ui_wgpu(&world, &mut batch, None, W as f32, H as f32, &mut flush);
    }
    gpu.submit(encoder);

    compare_to_golden("entity_ui", &gpu.read_rgba());
}

/// A centred UI label of two lines centres **each** line. It measured the
/// whole string as one line until Oct 2026, so a wide line over a short one
/// was pushed left by the short one's width too, and both started at the
/// same x - a game's sign-in hint hung off the left of its card.
#[test]
fn centred_ui_text_centres_every_line() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let mut world = World::new();
    world.spawn((
        UIElement {
            anchor: jengine_core::ui::UIAnchor::Center,
            offset: Vec2::ZERO,
            size: Vec2::new(220.0, 80.0),
            ..Default::default()
        },
        jengine_core::ui::UIText {
            text: "MMMMMMMM
II".into(),
            font_size: 20.0,
            alignment: jengine_core::ecs::components::TextAlign::Center,
            ..Default::default()
        },
    ));
    let (rgba, aw, ah, font) = built_in_font_atlas();
    const BUILTIN_FONT_ID: &str = "__font/builtin";
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(BUILTIN_FONT_ID.into(), RawTextureData { width: aw, height: ah, rgba });
    assets.mark_sdf(BUILTIN_FONT_ID);
    assets.ensure_wgpu_texture(BUILTIN_FONT_ID, &gpu.device, &gpu.queue).expect("upload");
    let fonts = FrameFonts::builtin_only(&font, BUILTIN_FONT_ID);
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let ui_vp = glam::Mat4::orthographic_rh_gl(0.0, W as f32, 0.0, H as f32, -1.0, 1.0);
    batch.begin_frame();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 });
        let assets = &assets;
        let mut flush = |batch: &mut WgpuSpriteBatch, what: UiFlush<'_>| {
            let UiFlush::Font(id) = what else {
                batch.discard_pending();
                return;
            };
            batch.flush(&gpu.device, &gpu.queue, &mut pass, &ui_vp, assets.get_wgpu_texture(id).expect("font"));
        };
        render_ui_wgpu(&world, &mut batch, Some(&fonts), W as f32, H as f32, &mut flush);
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();

    // The ink of each half of the screen - the top line and the bottom one -
    // must be centred on the middle column.
    let ink_span = |rows: std::ops::Range<u32>| -> (u32, u32) {
        let (mut lo, mut hi) = (u32::MAX, 0);
        for y in rows {
            for x in 0..img.width {
                if img.rgba[((y * img.width + x) * 4) as usize] > 128 {
                    lo = lo.min(x);
                    hi = hi.max(x);
                }
            }
        }
        assert!(lo <= hi, "no ink in this half");
        (lo, hi)
    };
    for (name, span) in [("first", ink_span(0..H / 2)), ("second", ink_span(H / 2..H))] {
        let mid = (span.0 + span.1) as f32 * 0.5;
        assert!((mid - W as f32 * 0.5).abs() <= 3.0, "the {name} line spans {span:?}, centred at {mid}, not {}", W / 2);
    }
}

/// A nine-slice UI image keeps its corners at `scale` times their pixels and
/// stretches only the edges and the middle - the frame of a kit panel. Before
/// Oct 2026 entity UI could only stretch an image whole, so a 48-pixel panel
/// drawn as a window had corners a fifth of its width.
#[test]
fn a_nine_slice_ui_image_keeps_its_corners() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    // 6 x 6: 2-pixel red corners, blue edges, a green middle.
    let mut rgba = Vec::new();
    for y in 0..6 {
        for x in 0..6 {
            let edge_x = !(2..4).contains(&x);
            let edge_y = !(2..4).contains(&y);
            rgba.extend_from_slice(match (edge_x, edge_y) {
                (true, true) => &[255, 0, 0, 255],
                (false, false) => &[0, 255, 0, 255],
                _ => &[0, 0, 255, 255],
            });
        }
    }
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("frame.png".into(), RawTextureData { width: 6, height: 6, rgba });
    let mut world = World::new();
    world.spawn((
        UIElement {
            anchor: jengine_core::ui::UIAnchor::TopLeft,
            offset: Vec2::new(20.0, -20.0),
            size: Vec2::new(120.0, 60.0),
            ..Default::default()
        },
        jengine_core::ui::UIImage {
            texture_id: "frame.png".into(),
            nine_slice: Some(jengine_core::ui::UiNineSlice { left: 2.0, right: 2.0, top: 2.0, bottom: 2.0, scale: 4.0 }),
            ..Default::default()
        },
    ));
    resolve_sprite_regions(&mut world, &assets);

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let ui_vp = glam::Mat4::orthographic_rh_gl(0.0, W as f32, 0.0, H as f32, -1.0, 1.0);
    batch.begin_frame();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 });
        let device = &gpu.device;
        let queue = &gpu.queue;
        let mut flush = |batch: &mut WgpuSpriteBatch, what: UiFlush<'_>| {
            let UiFlush::Quad(Some(id)) = what else {
                batch.discard_pending();
                return;
            };
            assets.ensure_wgpu_texture(id, device, queue).expect("upload");
            batch.flush(device, queue, &mut pass, &ui_vp, assets.get_wgpu_texture(id).expect("just ensured"));
        };
        render_ui_wgpu(&world, &mut batch, None, W as f32, H as f32, &mut flush);
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let at = |x: u32, y: u32| {
        let i = ((y * img.width + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    };
    let (red, green, blue) = ([255, 0, 0], [0, 255, 0], [0, 0, 255]);
    // The element spans x 20..140, y 20..80; corners are 8 pixels square.
    for (x, y, want, what) in [
        (23, 23, red, "top-left corner"),
        (136, 76, red, "bottom-right corner"),
        (32, 23, blue, "top edge just past the corner"),
        (23, 50, blue, "left edge"),
        (80, 50, green, "middle"),
        (130, 50, green, "middle near the right edge"),
    ] {
        assert_eq!(at(x, y), want, "{what} at ({x}, {y})");
    }
}

/// A `ui_image` names an atlas region like a sprite and draws only that
/// region. Before Sep 2026 it could not name one at all: an icon from a sheet
/// needed a hand-computed `uv_rect`, or it drew the whole sheet.
#[test]
fn a_ui_image_draws_its_atlas_region() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("sheet.png".into(), quad_sheet());
    assets.register_atlas("sheet.png".into(), quad_atlas());

    // Two icons, 48 square, their centres at (64, 64) and (160, 64) from the
    // top-left of the screen.
    let icon = |x: f32, region: &str| {
        (
            UIElement {
                anchor: jengine_core::ui::UIAnchor::TopLeft,
                offset: Vec2::new(x, -40.0),
                size: Vec2::new(48.0, 48.0),
                ..Default::default()
            },
            jengine_core::ui::UIImage {
                texture_id: "sheet.png".into(),
                region: Some(region.into()),
                ..Default::default()
            },
        )
    };
    let mut world = World::new();
    world.spawn(icon(40.0, "tr"));
    world.spawn(icon(136.0, "bl"));
    resolve_sprite_regions(&mut world, &assets);

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let ui_vp = glam::Mat4::orthographic_rh_gl(0.0, W as f32, 0.0, H as f32, -1.0, 1.0);
    batch.begin_frame();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color { r: 0.05, g: 0.05, b: 0.07, a: 1.0 });
        let device = &gpu.device;
        let queue = &gpu.queue;
        let mut flush = |batch: &mut WgpuSpriteBatch, what: UiFlush<'_>| {
            let UiFlush::Quad(Some(id)) = what else {
                batch.discard_pending();
                return;
            };
            assets.ensure_wgpu_texture(id, device, queue).expect("upload sheet");
            let texture = assets.get_wgpu_texture(id).expect("just ensured");
            batch.flush(device, queue, &mut pass, &ui_vp, texture);
        };
        render_ui_wgpu(&world, &mut batch, None, W as f32, H as f32, &mut flush);
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();

    // Each icon is its region's flat colour all over - the whole sheet would
    // show four colours, one per quadrant.
    let at = |x: u32, y: u32| {
        let i = ((y * img.width + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    };
    let near = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(p, q)| (*p as i32 - q as i32).abs() <= 3);
    for (cx, want, name) in [(64u32, [60u8, 200, 90], "tr"), (160, [70, 110, 230], "bl")] {
        for (dx, dy) in [(0i32, 0i32), (-18, -18), (18, -18), (-18, 18), (18, 18)] {
            let got = at((cx as i32 + dx) as u32, (64 + dy) as u32);
            assert!(near(got, want), "icon `{name}` at +({dx},{dy}) is {got:?}, want {want:?}");
        }
    }
    compare_to_golden("ui_image_regions", &img);
}

#[test]
fn text_draws_with_the_builtin_font() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };

    // The built-in face, rasterized exactly as every host rasterizes it. A
    // change to the subset, the shelf packing or the glyph rects moves pixels
    // here - which is the point: `font_atlas_digest` proves the atlas is the
    // same bytes, and this proves those bytes still draw as letters.
    let (rgba, aw, ah, font) = built_in_font_atlas();
    const BUILTIN_FONT_ID: &str = "__font/builtin";
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(
        BUILTIN_FONT_ID.into(),
        RawTextureData { width: aw, height: ah, rgba },
    );
    assets.mark_sdf(BUILTIN_FONT_ID);
    let fonts = FrameFonts::builtin_only(&font, BUILTIN_FONT_ID);

    let mut world = World::new();
    // An empty `font_id` means the built-in face.
    world.spawn(text_at(-118.0, 52.0, "Hello, jEngine", [1.0, 1.0, 1.0, 1.0]));
    // Latin-1 and Latin Extended-A: in the subset since Sep 2026, so blank
    // boxes here mean a glyph is rasterized but no longer subset.
    world.spawn(text_at(
        -118.0,
        30.0,
        "\u{e4} \u{f6} \u{e9} \u{df} \u{f1} \u{142}",
        [0.95, 0.78, 0.25, 1.0],
    ));
    // Both ways a line ends: an explicit newline and a wrap at `wrap_width`.
    world.spawn((
        Transform { position: Vec2::new(-118.0, 6.0), ..Default::default() },
        TextComponent {
            text: "explicit break\nand a wrapped run of words".into(),
            wrap_width: 120.0,
            color: [0.45, 0.8, 1.0, 1.0],
            ..text_style()
        },
    ));

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    batch.begin_frame();

    assets
        .ensure_wgpu_texture(BUILTIN_FONT_ID, &gpu.device, &gpu.queue)
        .expect("upload font atlas");

    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color { r: 0.05, g: 0.05, b: 0.07, a: 1.0 });
        let assets = &assets;
        let glyphs = render_text_wgpu(
            &world,
            &SortLayers::default(),
            &mut batch,
            &fonts,
            |batch: &mut WgpuSpriteBatch, texture_id: &str| {
                let texture = assets.get_wgpu_texture(texture_id).expect("font uploaded");
                batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture);
            },
        );
        assert!(glyphs > 0, "no glyphs queued - the text path drew nothing at all");
    }
    gpu.submit(encoder);

    compare_to_golden("text_builtin_font", &gpu.read_rgba());
}

/// Small text keeps its thin letters, wherever they fall between pixels.
///
/// The built-in face is a distance field baked at three times its layout
/// size, so every size is minified - and the atlas used to get a mip chain
/// like any linearly filtered texture. A level's texel averages the distances
/// under it, and the ridge of an `l` or an `i` is narrower than the texels
/// small text samples: averaged with the background its peak fell under the
/// edge and the letter thinned to nothing. All three MMOs (Oct 2026) showed it
/// at HUD sizes - "W nd", "Bless ng", "ha f minute". Here a row of `l`s at
/// quarter-pixel offsets, at the sizes a HUD uses: each must keep a bright
/// stem.
#[test]
fn small_text_keeps_its_thin_letters() {
    const SW: u32 = 320;
    const SH: u32 = 176;
    let Some(gpu) = HeadlessGpu::new(SW, SH) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let (rgba, aw, ah, font) = built_in_font_atlas();
    const BUILTIN_FONT_ID: &str = "__font/builtin";
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(BUILTIN_FONT_ID.into(), RawTextureData { width: aw, height: ah, rgba });
    assets.mark_sdf(BUILTIN_FONT_ID);
    let fonts = FrameFonts::builtin_only(&font, BUILTIN_FONT_ID);

    // Rows at six sizes; in each, sixteen `l`s 15.25 px apart, so the stems
    // land at every quarter of a pixel. The smaller three are a HUD's 14 px
    // in a window three quarters of the project's size (Liar's Dice in a
    // two-client capture: "sixes" drew as "s:xes") - a field baked three
    // times finer than the layout is sampled five texels apart there, which
    // steps over a stem.
    let scales = [0.75f32, 0.7, 0.65, 0.6, 0.55, 0.5];
    let (x0, step) = (-150.0f32, 15.25f32);
    let mut world = World::new();
    for (row, scale) in scales.iter().enumerate() {
        for i in 0..16 {
            world.spawn((
                Transform { position: Vec2::new(x0 + i as f32 * step, 64.0 - row as f32 * 26.0), ..Default::default() },
                TextComponent { text: "l".into(), font_scale: *scale, ..text_style() },
            ));
        }
    }

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(SW as f32, SH as f32);
    let vp = camera.view_projection();
    batch.begin_frame();
    assets.ensure_wgpu_texture(BUILTIN_FONT_ID, &gpu.device, &gpu.queue).expect("upload font atlas");
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        let assets = &assets;
        render_text_wgpu(&world, &SortLayers::default(), &mut batch, &fonts, |batch: &mut WgpuSpriteBatch, id: &str| {
            let texture = assets.get_wgpu_texture(id).expect("font uploaded");
            batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture);
        });
    }
    gpu.submit(encoder);
    let image = gpu.read_rgba();

    // The brightest pixel in each letter's own column band, across the rows.
    let mut faint = Vec::new();
    for (row, scale) in scales.iter().enumerate() {
        // World y up, image y down; a row's glyphs sit within 26 px.
        let top = (SH as f32 / 2.0 - (64.0 - row as f32 * 26.0) - 16.0).max(0.0) as u32;
        let bottom = (top + 26).min(SH);
        for i in 0..16 {
            let left = (SW as f32 / 2.0 + x0 + i as f32 * step - 2.0).max(0.0) as u32;
            let right = (left + 10).min(SW);
            let mut best = 0u8;
            for y in top..bottom {
                for x in left..right {
                    best = best.max(image.rgba[((y * SW + x) * 4) as usize]);
                }
            }
            if best < 170 {
                faint.push(format!("scale {scale}, l #{i}: brightest {best}"));
            }
        }
    }
    if !faint.is_empty() {
        save_png(&std::env::temp_dir().join("small_text_keeps_its_thin_letters.png"), &image);
    }
    assert!(faint.is_empty(), "thin letters faded (image in the temp dir):\n  {}", faint.join("\n  "));
}

/// Each alignment puts its text where it says, measured from the ink.
///
/// World text had no alignment: "centred" meant an author picking
/// `x = -width / 2` by hand, and four test-lab scenes had labels starting at
/// their column's centre instead of sitting on it. With `align`, a string
/// anchored at `x` must draw its ink centred on `x`, ending at `x`, or
/// starting at `x` - one line and two, since each line is placed on its own.
#[test]
fn text_align_places_each_line_about_its_anchor() {
    use jengine_core::ecs::components::TextAlign;

    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let (rgba, aw, ah, font) = built_in_font_atlas();
    const BUILTIN_FONT_ID: &str = "__font/builtin";
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(BUILTIN_FONT_ID.into(), RawTextureData { width: aw, height: ah, rgba });
    assets.mark_sdf(BUILTIN_FONT_ID);
    assets.ensure_wgpu_texture(BUILTIN_FONT_ID, &gpu.device, &gpu.queue).expect("upload font atlas");
    let fonts = FrameFonts::builtin_only(&font, BUILTIN_FONT_ID);

    // Ink columns of `text` drawn at world x = 0 with `align`, in pixels.
    let ink = |text: &str, align: TextAlign| -> (i32, i32) {
        let mut world = World::new();
        world.spawn((
            Transform { position: Vec2::new(0.0, 20.0), ..Default::default() },
            TextComponent { text: text.into(), align, ..text_style() },
        ));
        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
        let vp = Camera2D::new(W as f32, H as f32).view_projection();
        batch.begin_frame();
        let mut encoder = gpu.encoder();
        {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            render_text_wgpu(&world, &SortLayers::default(), &mut batch, &fonts, |b, id| {
                b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture(id).unwrap());
            });
        }
        gpu.submit(encoder);
        let img = gpu.read_rgba();
        let lit: Vec<i32> = (0..W as i32)
            .filter(|&x| (0..H).any(|y| img.rgba[((y * W + x as u32) * 4) as usize] > 60))
            .collect();
        (*lit.first().expect("no ink"), *lit.last().unwrap())
    };

    let mid = W as i32 / 2;
    for text in ["Centre me", "two lines
of unequal width"] {
        let (l, r) = ink(text, TextAlign::Center);
        assert!(((l + r) / 2 - mid).abs() <= 3, "{text:?} centred: ink {l}..{r}, centre {mid}");
        let (_, r) = ink(text, TextAlign::Right);
        assert!((r - mid).abs() <= 3, "{text:?} right-aligned: ink ends at {r}, anchor {mid}");
        let (l, _) = ink(text, TextAlign::Left);
        assert!((l - mid).abs() <= 3, "{text:?} left-aligned: ink starts at {l}, anchor {mid}");
    }
}

#[test]
fn gui_documents_draw_their_widgets() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };

    // One of each widget that draws differently: a panel background,
    // templated text, a progress bar at 60%, a button, and an image showing
    // one atlas quadrant by uv rect - #78 and #82 were exactly an image node
    // whose uv rect was in the wrong layout.
    let doc = load_gui(GOLDEN_GUI).expect("gui document");
    let mut gui = GuiComponent { gui_id: "golden.gui".into(), ..Default::default() };
    gui.variables.insert("score".into(), serde_json::json!(1280));

    let (rgba, aw, ah, font) = built_in_font_atlas();
    const BUILTIN_FONT_ID: &str = "__font/builtin";
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(
        BUILTIN_FONT_ID.into(),
        RawTextureData { width: aw, height: ah, rgba },
    );
    assets.mark_sdf(BUILTIN_FONT_ID);
    assets.insert_texture_bytes("sheet.png".into(), quad_sheet());
    assets.insert_texture_bytes(
        jengine_core::asset::WHITE_TEXTURE_ID.into(),
        RawTextureData { width: 1, height: 1, rgba: vec![255, 255, 255, 255] },
    );
    let fonts = FrameFonts::builtin_only(&font, BUILTIN_FONT_ID);

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let ui_vp = glam::Mat4::orthographic_rh_gl(0.0, W as f32, 0.0, H as f32, -1.0, 1.0);
    batch.begin_frame();

    for id in [BUILTIN_FONT_ID, "sheet.png", jengine_core::asset::WHITE_TEXTURE_ID] {
        assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).expect("upload");
    }

    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color { r: 0.05, g: 0.05, b: 0.07, a: 1.0 });
        let assets = &assets;
        let mut flush = |batch: &mut WgpuSpriteBatch, what: UiFlush<'_>| {
            // A clip is a scissor change, not a draw - the same shape every
            // host closure has.
            if let UiFlush::Clip(clip) = what {
                let (x, y, w, h) = match clip {
                    Some(r) => {
                        let x = (r.x.max(0.0) as u32).min(W);
                        let y = (r.y.max(0.0) as u32).min(H);
                        (
                            x,
                            y,
                            (r.width.max(0.0) as u32).min(W - x),
                            (r.height.max(0.0) as u32).min(H - y),
                        )
                    }
                    None => (0, 0, W, H),
                };
                pass.set_scissor_rect(x, y, w, h);
                return;
            }
            let id = match what {
                UiFlush::Quad(None) => jengine_core::asset::WHITE_TEXTURE_ID,
                UiFlush::Quad(Some(id)) | UiFlush::Font(id) => id,
                UiFlush::Clip(_) => unreachable!("handled above"),
            };
            let texture = assets
                .get_wgpu_texture(id)
                .unwrap_or_else(|| panic!("texture {id} uploaded"));
            batch.flush(&gpu.device, &gpu.queue, &mut pass, &ui_vp, texture);
        };
        let (quads, glyphs) =
            render_gui_wgpu(&[(doc, gui)], &mut batch, Some(&fonts), W as f32, H as f32, &mut flush);
        assert!(quads > 0 && glyphs > 0, "GUI drew {quads} quads and {glyphs} glyphs");
    }
    gpu.submit(encoder);

    compare_to_golden("gui_document", &gpu.read_rgba());
}

#[test]
fn a_gui_document_scales_to_fit_a_smaller_window() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };

    // The same document as `gui_documents_draw_their_widgets`, in a window
    // half the size it was authored for. Drawn unscaled it runs off the
    // right-hand edge - which is exactly what scene 33 did in the editor,
    // and what `ui::scaling` exists to fix.
    //
    // This renders it the way a runtime does: lay out against the virtual
    // size, project over that same size. If the two ever disagree the
    // document is cropped or shrunk twice, and both show up here.
    let doc = load_gui(GOLDEN_GUI).expect("gui document");
    let mut gui = GuiComponent { gui_id: "golden.gui".into(), ..Default::default() };
    gui.variables.insert("score".into(), serde_json::json!(1280));

    let scale = UiScale::from_reference((W * 2) as f32, (H * 2) as f32);
    let (uw, uh) = scale.virtual_size(W as f32, H as f32);
    assert_eq!((uw, uh), ((W * 2) as f32, (H * 2) as f32), "factor should be 0.5");

    let (rgba, aw, ah, font) = built_in_font_atlas();
    const BUILTIN_FONT_ID: &str = "__font/builtin";
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(
        BUILTIN_FONT_ID.into(),
        RawTextureData { width: aw, height: ah, rgba },
    );
    assets.mark_sdf(BUILTIN_FONT_ID);
    assets.insert_texture_bytes("sheet.png".into(), quad_sheet());
    assets.insert_texture_bytes(
        jengine_core::asset::WHITE_TEXTURE_ID.into(),
        RawTextureData { width: 1, height: 1, rgba: vec![255, 255, 255, 255] },
    );
    let fonts = FrameFonts::builtin_only(&font, BUILTIN_FONT_ID);

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let ui_vp = glam::Mat4::orthographic_rh_gl(0.0, uw, 0.0, uh, -1.0, 1.0);
    batch.begin_frame();

    for id in [BUILTIN_FONT_ID, "sheet.png", jengine_core::asset::WHITE_TEXTURE_ID] {
        assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).expect("upload");
    }

    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color { r: 0.05, g: 0.05, b: 0.07, a: 1.0 });
        let assets = &assets;
        let mut flush = |batch: &mut WgpuSpriteBatch, what: UiFlush<'_>| {
            if let UiFlush::Clip(_) = what {
                return;
            }
            let id = match what {
                UiFlush::Quad(None) => jengine_core::asset::WHITE_TEXTURE_ID,
                UiFlush::Quad(Some(id)) | UiFlush::Font(id) => id,
                UiFlush::Clip(_) => unreachable!("handled above"),
            };
            let texture = assets
                .get_wgpu_texture(id)
                .unwrap_or_else(|| panic!("texture {id} uploaded"));
            batch.flush(&gpu.device, &gpu.queue, &mut pass, &ui_vp, texture);
        };
        // Geometry only, no glyphs. At a half-scale factor a glyph quad lands
        // mid-texel, so its coverage is genuinely ambiguous and two adapters
        // resolve it differently - one letter of "Score" differed by 7 pixels
        // between runs on this machine. Text is pinned at 1:1 by
        // `text_builtin_font` and `gui_document`; what this golden is for is
        // that the whole document lands inside a window half its authored
        // size, which the rects show and the letters only make flaky.
        let _ = &fonts;
        render_gui_wgpu(&[(doc, gui)], &mut batch, None, uw, uh, &mut flush);
    }
    gpu.submit(encoder);

    compare_to_golden("gui_document_scaled", &gpu.read_rgba());
}

/// Render one lit sprite through the real resolution path and read it back.
///
/// Goes through `lit_sprite_textures` - the same function every runtime's
/// lit-sprite pass calls - so a normal map is uploaded exactly as a game's
/// would be, not by hand in the test.
/// A lit frame through the deferred path every runtime draws with since Sep
/// 2026: the sprite's colour in the world pass, its normal (or flat) in the
/// normal pass, one light that reads the normal buffer, then the composite.
/// `extra` sprites are drawn over it, flat, in both passes.
fn render_lit(
    gpu: &HeadlessGpu,
    diffuse: RawTextureData,
    normal: Option<RawTextureData>,
    light_at: Vec2,
) -> RawTextureData {
    render_lit_with(gpu, diffuse, normal, light_at, &[])
}

fn render_lit_with(
    gpu: &HeadlessGpu,
    diffuse: RawTextureData,
    normal: Option<RawTextureData>,
    light_at: Vec2,
    extra: &[(Vec2, Vec2)],
) -> RawTextureData {
    render_lit_full(gpu, diffuse, normal, light_at, extra, jengine_core::ecs::components::DEFAULT_LIGHT_HEIGHT)
}

fn render_lit_full(
    gpu: &HeadlessGpu,
    diffuse: RawTextureData,
    normal: Option<RawTextureData>,
    light_at: Vec2,
    extra: &[(Vec2, Vec2)],
    height: f32,
) -> RawTextureData {
    use jengine_core::ecs::components::{AmbientLight, MaterialComponent, PointLight2D};
    use jengine_core::ecs::wgpu_systems::{lit_sprite_textures, render_normals_wgpu};
    use jengine_core::renderer::backend::wgpu::{WgpuLightingPass, WgpuLitSpritePipeline};
    use jengine_core::renderer::lighting::gather_lights;

    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("diffuse.png".into(), diffuse);
    assets.insert_texture_bytes("__white".into(), solid(1, 1, [255, 255, 255, 255]));
    assets.ensure_wgpu_texture("diffuse.png", &gpu.device, &gpu.queue).expect("diffuse");
    assets.ensure_wgpu_texture("__white", &gpu.device, &gpu.queue).expect("white");
    let has_normal = normal.is_some();
    if let Some(n) = normal {
        assets.insert_texture_bytes("normal.png".into(), n);
    }

    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0, 1.0, 1.0], intensity: 0.12 },));
    world.spawn((
        Transform { position: light_at, rotation: 0.0, scale: Vec2::ONE },
        // 0.7, not the old forward pass's 1.6: the deferred lights give a flat
        // surface the light's full intensity (the forward pass gave it ~45%),
        // and a face turned toward the light up to 2.2x that. At 1.6 every
        // such face clips to one colour in an LDR frame and the bumps lose
        // their highlights - the headroom a normal map needs.
        PointLight2D { color: [1.0, 1.0, 1.0], intensity: 0.7, radius: 600.0, falloff: 1.0, shadow_softness: 0.0, height, casts_shadows: true, ..Default::default() },
    ));
    world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "diffuse.png".into(), size: Some([112.0, 112.0]), ..Default::default() },
        MaterialComponent {
            material_id: "lit_sprite".into(),
            normal_map_id: has_normal.then(|| "normal.png".to_string()),
            ..Default::default()
        },
    ));
    for (pos, size) in extra {
        world.spawn((
            Transform { position: *pos, rotation: 0.0, scale: Vec2::ONE },
            SpriteComponent {
                texture_id: "__white".into(),
                color: [0.6, 0.6, 0.6, 1.0],
                size: Some([size.x, size.y]),
                z_order: 5,
                ..Default::default()
            },
        ));
    }

    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let mut lit = WgpuLitSpritePipeline::new(&gpu.device, gpu.color_format).expect("lit pipeline");
    let flat = WgpuLitSpritePipeline::flat_normal_texture(&gpu.device, &gpu.queue);
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let fonts_atlas = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&fonts_atlas.3, "__font");

    let mut encoder = gpu.encoder();
    {
        use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, render_sprites_with_shadows_wgpu_materialized};
        let mut pass = lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color { r: 0.05, g: 0.05, b: 0.07, a: 1.0 }));
        batch.begin_frame();
        let assets = &assets;
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0),
            jengine_core::ecs::wgpu_systems::WorldLayer::All,
            |b, tex, kind| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                if let SpriteShadowFlush::Sprite { .. } = kind {
                    b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
                }
            },
        );
    }
    {
        let mut np = lp.normal_pass(&mut encoder);
        lit.begin_frame();
        let assets = &mut assets;
        render_normals_wgpu(&world, &SortLayers::default(), &mut lit, Vec2::splat(64.0), |pipe, d, n| {
            let (diffuse, normal, missing) =
                lit_sprite_textures(assets, &gpu.device, &gpu.queue, d, n, &flat).expect("textures resolve");
            assert_eq!(missing, None, "the normal map was uploaded, so none should be reported missing");
            pipe.flush_normals(&gpu.device, &gpu.queue, &mut np, &vp, diffuse, normal);
        });
    }
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &gather_lights(&world));
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    gpu.read_rgba()
}

/// A flat sprite standing on a normal-mapped floor hides the floor's bumps.
///
/// Normal maps used to be a forward pass drawn after every other sprite, so
/// the floor covered the crate. Now the crate's colour is drawn over the
/// floor in z order - and its flat normal over the floor's in the normal
/// pass, so the light sees a flat crate, not a crate-shaped patch of bumps.
#[test]
fn a_sprite_over_a_normal_mapped_floor_hides_its_bumps() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let assets_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects/engine-test-lab/assets/sprites");
    let load = |name: &str| {
        RawTextureData::from_bytes(&std::fs::read(assets_dir.join(name)).expect("read texture"))
            .expect("decode texture")
    };
    let img = render_lit_with(
        &gpu,
        load("bumps128.png"),
        Some(load("bumps128_n.png")),
        Vec2::new(90.0, 50.0),
        &[(Vec2::new(-20.0, 0.0), Vec2::new(40.0, 40.0))],
    );
    // Spread of brightness inside the crate (flat, one colour) against an
    // equal patch of floor beside it (bumps).
    let spread = |x0: u32, y0: u32| {
        let mut lo = 255u8;
        let mut hi = 0u8;
        for y in y0..y0 + 24 {
            for x in x0..x0 + 24 {
                let v = img.rgba[((y * W + x) * 4 + 1) as usize];
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        hi - lo
    };
    // Crate: centre at (-20, 0) -> pixel (W/2 - 20, H/2); floor patch to its right.
    let crate_spread = spread(W / 2 - 32, H / 2 - 12);
    let floor_spread = spread(W / 2 + 12, H / 2 - 12);
    assert!(floor_spread > 20, "the floor should show its bumps (spread {floor_spread})");
    assert!(
        crate_spread < floor_spread / 3,
        "the crate shows the floor's bumps through it (spread {crate_spread} vs floor {floor_spread})"
    );
}

fn solid(w: u32, h: u32, rgba: [u8; 4]) -> RawTextureData {
    RawTextureData { width: w, height: h, rgba: rgba.repeat((w * h) as usize) }
}

/// A normal map pointing straight out of the screen must light exactly like no
/// normal map at all.
///
/// It is the plainest statement of what a normal map *is*, and it is exactly
/// what was broken: every texture - normal maps included - was uploaded
/// `Rgba8UnormSrgb`, so sampling gamma-decoded the vectors. A flat texel
/// `(128, 128, 255)` came back as about `(0.22, 0.22, 1.0)`, decoding to a
/// normal leaning some 40 degrees down-left. Every "flat" pixel leaned, every
/// bump leaned the same way, and scene 12's normal-mapped row looked faint and
/// lopsided rather than sculpted.
///
/// The pipeline's own 1x1 flat fallback was always linear, which is why the
/// two disagreed - and why this comparison is the test.
#[test]
fn a_flat_normal_map_lights_like_no_normal_map() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let diffuse = || solid(16, 16, [200, 170, 130, 255]);
    let light = Vec2::new(90.0, 40.0);

    let without = render_lit(&gpu, diffuse(), None, light);
    let with_flat = render_lit(&gpu, diffuse(), Some(solid(16, 16, [128, 128, 255, 255])), light);

    let mut worst = 0i16;
    for (a, b) in without.rgba.iter().zip(with_flat.rgba.iter()) {
        worst = worst.max((*a as i16 - *b as i16).abs());
    }
    // 128/255 is not exactly 0.5, so a whisker of difference is expected.
    assert!(
        worst <= 4,
        "a flat normal map shaded differently from no normal map (worst channel delta \
         {worst}). If this is large, normal maps are being gamma-decoded again - they \
         must upload linear (`ensure_wgpu_normal_texture`), not as sRGB colour."
    );
}

/// Scene 12's bump sprite under one light: sculpted, not smeared.
#[test]
fn a_normal_mapped_sprite_is_shaded_per_pixel() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let assets_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../projects/engine-test-lab/assets/sprites");
    let load = |name: &str| {
        RawTextureData::from_bytes(&std::fs::read(assets_dir.join(name)).expect("read texture"))
            .expect("decode texture")
    };
    let img = render_lit(
        &gpu,
        load("bumps128.png"),
        Some(load("bumps128_n.png")),
        Vec2::new(90.0, 50.0),
    );
    compare_to_golden("lit_sprite_normal_map", &img);
}

/// A sphere's normals under a light from the side: at the default height the
/// whole face is lit, and a low light (`PointLight2D::height`) leaves the far
/// side dark - a planet with a night side. Before `height` every light sat
/// at 0.5, so a normal-mapped sphere could never show a terminator.
#[test]
fn a_low_light_gives_a_sphere_a_night_side() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    // 64 x 64 of sphere normals, encoded 0..255 (+y up, green = up).
    let n = 64u32;
    let mut normal = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let nx = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let ny = -((y as f32 + 0.5) / n as f32 * 2.0 - 1.0);
            let r2 = (nx * nx + ny * ny).min(0.999);
            let v = [nx * 0.95, ny * 0.95, (1.0 - r2).sqrt()];
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            for c in v {
                normal.push(((c / len * 0.5 + 0.5) * 255.0).round() as u8);
            }
            normal.push(255);
        }
    }
    let sphere = RawTextureData { width: n, height: n, rgba: normal };
    let white = || solid(4, 4, [255, 255, 255, 255]);
    // The light far to the left, level with the sprite's centre.
    let light = Vec2::new(-250.0, 0.0);
    let px = |img: &RawTextureData, x: i32| {
        let (cx, cy) = (W as i32 / 2 + x, H as i32 / 2);
        img.rgba[((cy as u32 * W + cx as u32) * 4) as usize]
    };
    let high = render_lit_full(&gpu, white(), Some(sphere.clone()), light, &[], jengine_core::ecs::components::DEFAULT_LIGHT_HEIGHT);
    let low = render_lit_full(&gpu, white(), Some(sphere), light, &[], 0.1);
    // 15 units right of centre: just past the middle, on the far side. A
    // light 27 degrees up still reaches it; one 6 degrees up does not, and
    // leaves only the 0.12 ambient (about 97 in sRGB).
    let (high_far, low_far) = (px(&high, 15), px(&low, 15));
    let (high_near, low_near) = (px(&high, -40), px(&low, -40));
    assert!(high_far > 130, "at the default height the middle of the sphere is lit: {high_far}");
    assert!(low_far < 105, "a low light leaves the far side dark (ambient only): {low_far}");
    assert!(low_near >= high_near, "and lights the near side at least as brightly: {low_near} vs {high_near}");
}

/// A baked lightmap lights the scene under it, and only there.
///
/// `LightmapComponent` was documented as "rendered into the light
/// accumulation buffer" for months while no runtime drew it at all (BUGS
/// #95): a lightmap entity loaded, saved and showed in the inspector, and the
/// scene stayed exactly as dark. So this renders a white scene under 10%
/// ambient with a white lightmap over the left half only, through the real
/// lighting pass, and reads both halves back.
#[test]
fn a_lightmap_lights_what_is_under_it() {
    use jengine_core::ecs::components::{AmbientLight, LightmapComponent};
    use jengine_core::ecs::wgpu_systems::render_lightmaps_wgpu;
    use jengine_core::renderer::backend::wgpu::WgpuLightingPass;
    use jengine_core::renderer::lighting::gather_lights;

    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("lightmap.png".into(), solid(8, 8, [255, 255, 255, 255]));
    assets.ensure_wgpu_texture("lightmap.png", &gpu.device, &gpu.queue).expect("upload");

    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0, 1.0, 1.0], intensity: 0.1 },));
    world.spawn((
        Transform { position: Vec2::new(-(W as f32) / 4.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
        LightmapComponent {
            texture_id: "lightmap.png".into(),
            size: [W as f32 / 2.0, H as f32],
            ..Default::default()
        },
    ));

    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    drop(lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE)));
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &gather_lights(&world));
    {
        let mut pass = lp.light_buffer_pass(&mut encoder);
        let drawn = render_lightmaps_wgpu(&world, &mut batch, |b, tex_id, _| {
            let tex = assets.get_wgpu_texture(tex_id).expect("uploaded above");
            b.flush_additive(&gpu.device, &gpu.queue, &mut pass, &vp, tex);
        });
        assert_eq!(drawn, 1);
    }
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    let img = gpu.read_rgba();

    let px = |x: u32, y: u32| img.rgba[((y * W + x) * 4) as usize];
    let (lit, dark) = (px(W / 4, H / 2), px(3 * W / 4, H / 2));
    assert!(lit > 200, "the lightmapped half is {lit}/255 - the lightmap added no light");
    assert!(dark < 110, "the half with no lightmap is {dark}/255 - it should be near the 10% ambient");
}

/// **The game reads a lightmap as the screen draws it.** `light_at` could not
/// see lightmaps at all, so a player in a baked sunbeam was lit on screen and
/// dark to the game; `light_at_textured` adds them from their pixels. This
/// renders one through the real lighting pass - turned, stretched, a sub-rect
/// of its texture, with a half-transparent texel - and checks the composite
/// against the CPU's answer pixel by pixel.
#[test]
fn the_cpu_reads_a_lightmap_as_the_gpu_draws_it() {
    use jengine_core::ecs::components::{AmbientLight, LightmapComponent};
    use jengine_core::ecs::wgpu_systems::render_lightmaps_wgpu;
    use jengine_core::renderer::backend::wgpu::WgpuLightingPass;
    use jengine_core::renderer::lighting::{gather_lights, light_at_textured};

    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");

    // 4 x 4 texels, each its own colour; one of them half transparent.
    let mut rgba = Vec::new();
    for ty in 0..4u8 {
        for tx in 0..4u8 {
            let a = if (tx, ty) == (2, 2) { 128 } else { 255 };
            rgba.extend_from_slice(&[40 + tx * 60, 30 + ty * 70, 200 - tx * 30 - ty * 20, a]);
        }
    }
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("lm.png".into(), RawTextureData { width: 4, height: 4, rgba });
    assets.ensure_wgpu_texture("lm.png", &gpu.device, &gpu.queue).expect("upload");

    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0, 1.0, 1.0], intensity: 0.05 },));
    world.spawn((
        Transform { position: Vec2::new(10.0, -4.0), rotation: 0.5, scale: Vec2::new(1.5, 1.0) },
        LightmapComponent {
            texture_id: "lm.png".into(),
            uv_rect: [0.0, 0.25, 0.75, 1.0],
            intensity: 0.8,
            size: [96.0, 64.0],
        },
    ));

    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let lights = gather_lights(&world);
    let mut encoder = gpu.encoder();
    drop(lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE)));
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &lights);
    {
        let mut pass = lp.light_buffer_pass(&mut encoder);
        render_lightmaps_wgpu(&world, &mut batch, |b, tex_id, _| {
            let tex = assets.get_wgpu_texture(tex_id).expect("uploaded above");
            b.flush_additive(&gpu.device, &gpu.queue, &mut pass, &vp, tex);
        });
    }
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    let img = gpu.read_rgba();

    let to_srgb = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let e = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (e * 255.0).round() as i32
    };
    // Where a pixel falls in the quad, in texels of the visible sub-rect (3 x 3).
    let (s, c) = 0.5f32.sin_cos();
    let texel_pos = |p: Vec2| {
        let d = p - Vec2::new(10.0, -4.0);
        let local = Vec2::new(d.x * c + d.y * s, -d.x * s + d.y * c);
        Vec2::new((local.x / 144.0 + 0.5) * 3.0, (0.5 - local.y / 64.0) * 3.0)
    };
    let (mut compared, mut inside) = (0, 0);
    for py in (2..H).step_by(3) {
        for px in (2..W).step_by(3) {
            let world_p = Vec2::new(px as f32 + 0.5 - W as f32 / 2.0, H as f32 / 2.0 - (py as f32 + 0.5));
            let t = texel_pos(world_p);
            // Rasterisation and nearest sampling are both exact away from an
            // edge; on one, either side is right. Stay clear of them.
            let edge = |v: f32| (v - v.round()).abs() < 0.12;
            if edge(t.x) || edge(t.y) {
                continue;
            }
            if (0.0..3.0).contains(&t.x) && (0.0..3.0).contains(&t.y) {
                inside += 1;
            }
            let want = light_at_textured(world_p, &lights, |id| assets.get_texture_bytes(id));
            let i = ((py * W + px) * 4) as usize;
            for ch in 0..3 {
                let got = img.rgba[i + ch] as i32;
                let exp = to_srgb(want[ch]);
                assert!(
                    (got - exp).abs() <= 3,
                    "pixel ({px}, {py}) channel {ch}: the screen shows {got}, the CPU says {exp} (texel {t:?})"
                );
            }
            compared += 1;
        }
    }
    assert!(inside > 150, "too few pixels inside the lightmap to mean anything: {inside}");
    assert!(compared > 1000, "{compared}");
}

/// **Coloured glass on screen is coloured glass to the game.** A white point
/// light behind a red pane and a stone: the GPU's shadow and the CPU's
/// `light_at` must agree pixel by pixel, including the red that gets through
/// the pane. `cpu_light_matches_the_shader` pins the two by their source
/// text; this pins them by what is drawn. A light puzzle scored on
/// the CPU side does so too, so a disagreement here is a puzzle the screen says is solved
/// and the game says is not.
#[test]
fn the_cpu_reads_coloured_shadows_as_the_gpu_draws_them() {
    use jengine_core::ecs::components::{AmbientLight, LightOccluder, PointLight2D};
    use jengine_core::renderer::backend::wgpu::WgpuLightingPass;
    use jengine_core::renderer::lighting::{gather_lights, light_at};

    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");

    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0, 1.0, 1.0], intensity: 0.03 },));
    world.spawn((
        Transform { position: Vec2::new(-100.0, 0.0), ..Default::default() },
        PointLight2D { color: [1.0, 1.0, 1.0], intensity: 1.3, radius: 300.0, falloff: 1.0, shadow_softness: 0.0, height: jengine_core::ecs::components::DEFAULT_LIGHT_HEIGHT, casts_shadows: true, ..Default::default() },
    ));
    // A red pane above the light's line, a stone below it.
    world.spawn((
        Transform { position: Vec2::new(-30.0, 24.0), ..Default::default() },
        LightOccluder { enabled: true, size: [20.0, 30.0], shadow_tint: [1.0, 0.0, 0.0], shadow_opacity: 1.0, ..Default::default() },
    ));
    world.spawn((
        Transform { position: Vec2::new(-30.0, -24.0), ..Default::default() },
        LightOccluder { enabled: true, size: [20.0, 30.0], shadow_opacity: 1.0, ..Default::default() },
    ));

    let camera = Camera2D::new(W as f32, H as f32);
    let lights = gather_lights(&world);
    let mut encoder = gpu.encoder();
    drop(lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE)));
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &lights);
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    let img = gpu.read_rgba();

    let to_srgb = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let e = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (e * 255.0).round() as i32
    };
    let (mut compared, mut red_only, mut dark) = (0, 0, 0);
    for py in (1..H).step_by(2) {
        for px in (1..W).step_by(2) {
            let p = Vec2::new(px as f32 + 0.5 - W as f32 / 2.0, H as f32 / 2.0 - (py as f32 + 0.5));
            let want = light_at(p, &lights);
            // A shadow's edge falls between pixels; on one, either side is
            // right. Only compare where a pixel's neighbourhood agrees.
            let steady = [(-1.5, 0.0), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5)]
                .iter()
                .all(|(dx, dy)| (light_at(p + Vec2::new(*dx, *dy), &lights) - want).abs().max_element() < 0.02);
            if !steady {
                continue;
            }
            let i = ((py * W + px) * 4) as usize;
            for ch in 0..3 {
                let got = img.rgba[i + ch] as i32;
                let exp = to_srgb(want[ch]);
                assert!(
                    (got - exp).abs() <= 3,
                    "pixel ({px}, {py}) channel {ch}: the screen shows {got}, the CPU says {exp}"
                );
            }
            compared += 1;
            if want.x > 0.2 && want.y < 0.05 {
                red_only += 1;
            }
            if want.max_element() < 0.05 && p.x > 0.0 {
                dark += 1;
            }
        }
    }
    assert!(compared > 3000, "{compared}");
    assert!(red_only > 50, "the pane should throw a red shadow the test looked at: {red_only}");
    assert!(dark > 50, "and the stone a dark one: {dark}");
}

/// **A skinned mesh sorts with the sprites by `z_order`.** It was a pass of
/// its own after every sprite, so a creature drew over the ground it walked
/// on whatever its z. Two one-bone squares cut from the sheet's red cell: the
/// left at z 5 under a blue sprite at z 50, the right at z 50 over one at z 5.
#[test]
fn a_skinned_mesh_sorts_with_the_sprites() {
    use jengine_core::animation::skinned_mesh::{BoneInfluence, SkinnedMesh, SkinnedVertex};
    use jengine_core::ecs::components::{Bone, Parent, SkeletonRoot};
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, render_sprites_with_shadows_wgpu_materialized};

    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("sheet.png".into(), quad_sheet());
    assets.ensure_wgpu_texture("sheet.png", &gpu.device, &gpu.queue).expect("upload");
    let (rgba, aw, ah, font) = built_in_font_atlas();
    assets.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
    assets.mark_sdf("__font");
    let fonts = FrameFonts::builtin_only(&font, "__font");

    let at = |x: f32, y: f32| Transform { position: Vec2::new(x, y), ..Default::default() };
    let mut world = World::new();
    let creature = |world: &mut World, x: f32, z: i32| {
        // A square about the bone, every vertex wholly on it; uvs on the
        // red cell of the sheet.
        let corners = [(-20.0, -20.0, 0.0, 0.5), (20.0, -20.0, 0.5, 0.5), (20.0, 20.0, 0.5, 0.0), (-20.0, 20.0, 0.0, 0.0)];
        let mesh = SkinnedMesh {
            texture_id: "sheet.png".into(),
            color: [1.0, 1.0, 1.0, 1.0],
            z_order: z,
            bones: vec!["body".into()],
            vertices: corners
                .iter()
                .map(|&(px, py, u, v)| SkinnedVertex {
                    rest_position: [px, py],
                    uv: [u * 0.98 + 0.005, v * 0.98 + 0.005],
                    influences: vec![BoneInfluence { bone_index: 0, weight: 1.0, offset: [px, py] }],
                })
                .collect(),
            indices: vec![0, 1, 2, 0, 2, 3],
        };
        let root = world.spawn((at(x, 20.0), SkeletonRoot { name: "creature".into() }, mesh));
        world.spawn((Transform::default(), Bone { name: "body".into(), length: 10.0 }, Parent(root)));
    };
    creature(&mut world, -80.0, 5);
    creature(&mut world, 80.0, 50);
    let blue_sprite = |z: i32| SpriteComponent {
        texture_id: "sheet.png".into(),
        uv_rect: Some([0.0, 0.5, 0.5, 1.0]),
        size: Some([40.0, 40.0]),
        z_order: z,
        ..Default::default()
    };
    world.spawn((at(-80.0, 20.0), blue_sprite(50)));
    world.spawn((at(80.0, 20.0), blue_sprite(5)));
    jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world);

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        let assets = &assets;
        render_sprites_with_shadows_wgpu_materialized(&world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), jengine_core::ecs::wgpu_systems::WorldLayer::All, |b, tex, kind| {
            let texture = assets.get_wgpu_texture(tex).expect("texture uploaded");
            match kind {
                SpriteShadowFlush::Shadow => b.flush_multiply(&gpu.device, &gpu.queue, &mut pass, &vp, texture),
                SpriteShadowFlush::Sprite { .. } => b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture),
            };
        });
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();

    let count = |x0: u32, x1: u32, y0: u32, y1: u32, pred: &dyn Fn(&[u8]) -> bool| -> usize {
        let mut n = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y * img.width + x) * 4) as usize;
                if pred(&img.rgba[i..i + 4]) {
                    n += 1;
                }
            }
        }
        n
    };
    let red = |p: &[u8]| p[0] > 150 && p[1] < 120 && p[2] < 120;
    let blue = |p: &[u8]| p[2] > 150 && p[0] < 120 && p[1] < 150;
    assert_eq!(count(30, 66, 26, 62, &red), 0, "a skinned mesh at z 5 shows over a sprite at z 50");
    assert!(count(30, 66, 26, 62, &blue) > 1000, "the sprite over the mesh did not draw");
    assert!(count(190, 226, 26, 62, &red) > 1000, "a skinned mesh at z 50 did not cover a sprite at z 5");
    assert_eq!(count(190, 226, 26, 62, &blue), 0, "a sprite at z 5 shows over a skinned mesh at z 50");

    compare_to_golden("skinned_mesh_in_sprite_pass", &img);
}

/// **A graph material reads the numbers a game sets on it.** Two sprites of
/// one material and one texture, each with its own `tint` in
/// `MaterialComponent.uniforms` - the map `set_material_uniform_vec4` writes,
/// which no render path read before Sep 2026. Through the real sprite pass,
/// they must come out in their own colours: the parameters are part of the
/// batching key, and each group sets them on the batch before it draws.
#[test]
fn a_graph_material_reads_its_instance_parameters() {
    use jengine_core::ecs::components::MaterialComponent;
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, render_sprites_with_shadows_wgpu_materialized};
    use jengine_core::renderer::graph_pipeline_cache::{GraphPipelineCache, GraphPipelineKey};
    use jengine_core::renderer::material::{BlendMode, MaterialData, UniformValue};
    use jengine_core::renderer::shader_graph::{NodeType, ShaderGraph};

    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    // The graph: output = the `tint` parameter.
    let mut graph = ShaderGraph::new("tinted");
    let tint = graph.add_node(NodeType::MaterialParam { name: "tint".into() }, [0.0, 0.0]);
    let out = graph.output_node().unwrap().id;
    graph.connect(tint, 0, out, 0);

    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("white.png".into(), solid(4, 4, [255, 255, 255, 255]));
    assets.ensure_wgpu_texture("white.png", &gpu.device, &gpu.queue).expect("upload");
    assets.insert_material(
        "tinted".into(),
        MaterialData { shader_graph_path: Some("mem/tinted.shader".into()), ..Default::default() },
    );
    let (rgba, aw, ah, font) = built_in_font_atlas();
    assets.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
    assets.mark_sdf("__font");
    let fonts = FrameFonts::builtin_only(&font, "__font");

    let batch_for_layout = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let mut cache = GraphPipelineCache::new();
    let pipeline = cache
        .insert_from_graph(
            &gpu.device,
            gpu.color_format,
            &batch_for_layout,
            GraphPipelineKey { shader_graph_path: "mem/tinted.shader".into(), blend_mode: BlendMode::Alpha },
            &graph,
        )
        .expect("a graph with a MaterialParam builds");

    let sprite = |x: f32, color: [f32; 4]| {
        let mut uniforms = std::collections::HashMap::new();
        uniforms.insert("tint".to_string(), UniformValue::Vec4(color));
        (
            Transform { position: Vec2::new(x, 0.0), ..Default::default() },
            SpriteComponent { texture_id: "white.png".into(), size: Some([48.0, 48.0]), ..Default::default() },
            MaterialComponent { material_id: "tinted".into(), uniforms, ..Default::default() },
        )
    };
    let mut world = World::new();
    world.spawn(sprite(-60.0, [1.0, 0.0, 0.0, 1.0]));
    world.spawn(sprite(60.0, [0.0, 1.0, 0.0, 1.0]));

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    let mut flushes = 0;
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        let assets = &assets;
        render_sprites_with_shadows_wgpu_materialized(&world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), jengine_core::ecs::wgpu_systems::WorldLayer::All, |b, tex, kind| {
            let texture = assets.get_wgpu_texture(tex).expect("texture uploaded");
            match kind {
                SpriteShadowFlush::Sprite { material_id: Some(_) } => {
                    flushes += 1;
                    b.flush_with_pipeline(&gpu.device, &gpu.queue, &mut pass, &vp, texture, &pipeline);
                }
                SpriteShadowFlush::Sprite { .. } => {
                    b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture);
                }
                SpriteShadowFlush::Shadow => {
                    b.flush_multiply(&gpu.device, &gpu.queue, &mut pass, &vp, texture);
                }
            };
        });
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let at = |x: u32, y: u32| {
        let i = ((y * img.width + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    };
    // 256 x 128, centre (128, 64): the left sprite about x 68, the right 188.
    let left = at(68, 64);
    let right = at(188, 64);
    assert!(left[0] > 200 && left[1] < 40, "the left sprite should be its own red: {left:?}");
    assert!(right[1] > 200 && right[0] < 40, "the right sprite should be its own green: {right:?}");
    assert_eq!(flushes, 2, "two parameter sets, two flushes; equal ones would share one");
    compare_to_golden("material_instance_params", &img);
}

// ── golden-image plumbing ───────────────────────────────────────────────────

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden")
}

fn save_png(path: &Path, image: &RawTextureData) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create golden dir");
    }
    image::RgbaImage::from_raw(image.width, image.height, image.rgba.clone())
        .expect("image dimensions")
        .save(path)
        .unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}

/// The adapter the goldens were drawn on: `tests/golden/ADAPTER`, one line.
/// On that adapter a golden is compared strictly; on another, edges are
/// allowed to fall the other way ([`compare_to_golden`]).
fn reference_adapter() -> Option<String> {
    std::fs::read_to_string(golden_dir().join("ADAPTER")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// What a comparison found: pixels outside the tolerance, the worst channel,
/// and the mean absolute difference per channel.
struct Diff {
    differing: usize,
    worst: i16,
    mean: f64,
}

/// A small per-channel tolerance: different GPUs round the same blend
/// slightly differently, and a test that fails on a vendor's rounding gets
/// turned off rather than fixed.
const TOLERANCE: i16 = 4;

/// Mean absolute difference per channel a different adapter may show. Two
/// correct GPUs measured 0.13-0.78 (an RTX 4070 against the goldens drawn on
/// a Radeon 780M); a whole picture moved one pixel measured 5-9.
const MEAN_LIMIT: f64 = 1.5;

/// The share of pixels a different adapter may still differ in after edges
/// are allowed - an antialiased glyph edge brighter than any golden
/// neighbour. 0.15 % was measured; a sprite recoloured or missing is far more.
const STRAY_LIMIT: f64 = 0.005;

/// Whether a comparison passes: strictly on the goldens' adapter, within the
/// mean and stray limits on another.
fn passes(d: &Diff, pixels: usize, same: bool) -> bool {
    if same {
        d.differing == 0
    } else {
        d.mean <= MEAN_LIMIT && (d.differing as f64) <= STRAY_LIMIT * pixels as f64
    }
}

/// `expected` against `actual`, RGBA, `w` wide. Strict: every channel within
/// [`TOLERANCE`]. Loose (`edges`): a channel may instead lie within the range
/// of the expected 3x3 neighbourhood - an edge, or a nearest-texel tie, that
/// one GPU resolved to the neighbouring value - and the image's mean
/// difference is reported for the caller to bound.
fn diff(expected: &[u8], actual: &[u8], w: usize, edges: bool) -> Diff {
    let h = expected.len() / 4 / w;
    let mut differing = 0usize;
    let mut worst = 0i16;
    let mut total = 0u64;
    for y in 0..h {
        for x in 0..w {
            let mut bad = false;
            for c in 0..4 {
                let i = (y * w + x) * 4 + c;
                let (e, a) = (expected[i] as i16, actual[i] as i16);
                let delta = (e - a).abs();
                total += delta as u64;
                if delta <= TOLERANCE {
                    continue;
                }
                if edges {
                    let (mut lo, mut hi) = (255i16, 0i16);
                    for ny in y.saturating_sub(1)..(y + 2).min(h) {
                        for nx in x.saturating_sub(1)..(x + 2).min(w) {
                            let v = expected[(ny * w + nx) * 4 + c] as i16;
                            lo = lo.min(v);
                            hi = hi.max(v);
                        }
                    }
                    if a >= lo - TOLERANCE && a <= hi + TOLERANCE {
                        continue;
                    }
                }
                bad = true;
                worst = worst.max(delta);
            }
            if bad {
                differing += 1;
            }
        }
    }
    Diff { differing, worst, mean: total as f64 / expected.len().max(1) as f64 }
}

/// Compare `actual` against `tests/golden/<name>.png`.
///
/// Writes the golden when it is missing or `JENGINE_UPDATE_GOLDEN=1`; on a
/// mismatch, writes what was rendered next to the golden as `<name>.actual.png`
/// so the two can be looked at side by side, and says how many pixels differ.
///
/// **A golden is a picture of one GPU.** On the adapter named in
/// `tests/golden/ADAPTER` the comparison is strict. On any other - a laptop
/// whose default switched from its integrated GPU to its discrete one - two
/// correct rasterisers disagree at edges: a distance-field glyph sampled
/// between texels, a nearest-sampled normal map whose texel edge lands on a
/// pixel centre. There a pixel may match its neighbourhood instead, and the
/// whole image's mean difference must stay under [`MEAN_LIMIT`], so a picture
/// that moved or changed colour still fails. Update goldens on the reference
/// adapter (`WGPU_ADAPTER_NAME=<part of its name>`).
fn compare_to_golden(name: &str, actual: &RawTextureData) {
    let path = golden_dir().join(format!("{name}.png"));
    let updating = std::env::var("JENGINE_UPDATE_GOLDEN").is_ok_and(|v| v != "0");
    let here = jengine_core::renderer::backend::wgpu::headless::adapter_name().unwrap_or("").to_string();
    let reference = reference_adapter();
    let same = reference.as_deref().is_none_or(|r| r == here);
    if updating || !path.exists() {
        if !same {
            eprintln!(
                "warning: writing golden {name} on {here}, but the goldens were drawn on {} - \
                 set WGPU_ADAPTER_NAME so every golden is one adapter's",
                reference.as_deref().unwrap_or("?")
            );
        }
        save_png(&path, actual);
        eprintln!("wrote golden {} - look at it before committing", path.display());
        return;
    }

    let expected = image::open(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
        .to_rgba8();
    assert_eq!(
        (expected.width(), expected.height()),
        (actual.width, actual.height),
        "{name}: golden is {}x{}, rendered {}x{}",
        expected.width(),
        expected.height(),
        actual.width,
        actual.height
    );

    let d = diff(expected.as_raw(), &actual.rgba, actual.width as usize, !same);
    if !passes(&d, (actual.width * actual.height) as usize, same) {
        let actual_path = golden_dir().join(format!("{name}.actual.png"));
        save_png(&actual_path, actual);
        let mode = if same {
            String::new()
        } else {
            format!(
                " (on {here}, not the goldens' {}: edges allowed, mean {:.2} of at most {MEAN_LIMIT},                  at most {:.1} % of pixels)",
                reference.as_deref().unwrap_or("?"),
                d.mean,
                STRAY_LIMIT * 100.0
            )
        };
        panic!(
            "{name}: {} pixels differ (worst channel delta {}){mode}.\n\
             rendered: {}\n golden:   {}\n\
             If the new image is right, delete the golden or set \
             JENGINE_UPDATE_GOLDEN=1, then look at it before committing.",
            d.differing,
            d.worst,
            actual_path.display(),
            path.display()
        );
    }
}

/// The edge-tolerant comparison lets an edge pixel take its neighbour's value
/// and nothing more: a pixel of a colour found nowhere near it fails, and so
/// does a picture moved a pixel - its mean difference is far past the limit.
#[test]
fn a_golden_on_another_adapter_allows_edges_and_nothing_else() {
    // A 32 x 32 grey image with a white 16 x 16 square from (8, 8).
    let w = 32;
    let n = w * w;
    let img = |dx: usize, square: [u8; 4]| -> Vec<u8> {
        let mut v = vec![0u8; n * 4];
        for y in 0..w {
            for x in 0..w {
                let inside = (8 + dx..24 + dx).contains(&x) && (8..24).contains(&y);
                let c = if inside { square } else { [60, 60, 60, 255] };
                v[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&c);
            }
        }
        v
    };
    let golden = img(0, [255; 4]);
    // One edge pixel of the square came out grey (a tie broken the other way).
    let mut edge = golden.clone();
    edge[(8 * w + 8) * 4..(8 * w + 8) * 4 + 3].copy_from_slice(&[60, 60, 60]);
    assert!(!passes(&diff(&golden, &edge, w, false), n, true), "strict: the edge pixel fails");
    let d = diff(&golden, &edge, w, true);
    assert_eq!(d.differing, 0, "loose: it matches its neighbourhood");
    assert!(passes(&d, n, false));
    // The square red instead of white: no neighbour is red, and it is a
    // quarter of the picture.
    assert!(!passes(&diff(&golden, &img(0, [255, 0, 0, 255]), w, true), n, false));
    // The square moved one pixel: every pixel matches some neighbour, but the
    // mean is far past the limit.
    let moved = diff(&golden, &img(1, [255; 4]), w, true);
    assert_eq!(moved.differing, 0);
    assert!(!passes(&moved, n, false), "a moved picture must fail on its mean: {}", moved.mean);
}

// ---------------------------------------------- graph materials draw

/// **Does a `.shader` graph actually reach the screen?**
///
/// This is the question nothing could answer. `shader_codegen::generate`
/// proves a graph compiles to WGSL; it does not prove wgpu accepts that WGSL,
/// that the pipeline binds, or that the result is anything but black. And the
/// failure mode is quiet by design: `GraphPipelineCache` records a build error
/// and the caller falls back to the default sprite pipeline, so a broken
/// material draws a perfectly ordinary sprite.
///
/// The graph under test is `tests/fixtures/graphs/.../dr_reveal.shader`, a
/// copy of one a game shipped - a real graph rather than a minimal one. That
/// every shipped graph *compiles* is `shipped_shader_graphs_compile`'s job.
///
/// Three swatches, one material, one texture, differing only in their **vertex
/// colour**: below the rim threshold, astride it, well past. That is the whole
/// point of `NodeType::VertexColorInput` - a graph depending on something the
/// game decided - so if the three come out identical the node is not being
/// read even though the shader compiled.
#[test]
fn a_graph_material_reads_the_vertex_colour_and_draws() {
    use jengine_core::renderer::graph_pipeline_cache::{GraphPipelineCache, GraphPipelineKey};
    use jengine_core::renderer::material::BlendMode;

    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };

    let arcade = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/graphs");
    let graph_rel = "assets/shaders/dr_reveal.shader";
    assert!(
        arcade.join(graph_rel).exists(),
        "the test graph is missing: {}",
        arcade.join(graph_rel).display()
    );

    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("sheet.png".into(), quad_sheet());

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let mut cache = GraphPipelineCache::new();
    let key = GraphPipelineKey {
        shader_graph_path: graph_rel.to_string(),
        blend_mode: BlendMode::Alpha,
    };
    // **The assertion that matters most.** Everything below is about what the
    // pixels look like; this is about whether the pipeline exists at all, and
    // it is the step that silently falls back in the real runtime.
    let pipeline = cache
        .get_or_build(&gpu.device, gpu.color_format, &batch, Some(&arcade), &key)
        .expect("the shipped dr_reveal.shader must build a real wgpu pipeline");

    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    assets
        .ensure_wgpu_texture("sheet.png", &gpu.device, &gpu.queue)
        .expect("upload sheet");
    let texture = assets.get_wgpu_texture("sheet.png").expect("sheet uploaded");

    // Below the 0.55 rim threshold, astride it, past it - the three states an
    // attacker moves through as the searchlight finds it.
    let ks = [0.42f32, 0.72, 0.95];
    batch.begin_frame();
    batch.begin();
    for (i, k) in ks.iter().enumerate() {
        batch.draw_sprite_uv(
            Vec2::new(-70.0 + i as f32 * 70.0, 0.0),
            Vec2::splat(56.0),
            0.0,
            glam::Vec4::new(*k, *k, *k, 1.0),
            [0.0, 0.0, 0.5, 0.5],
        );
    }

    let mut encoder = gpu.encoder();
    {
        let mut pass =
            gpu.color_pass(&mut encoder, wgpu::Color { r: 0.02, g: 0.02, b: 0.03, a: 1.0 });
        let drawn =
            batch.flush_with_pipeline(&gpu.device, &gpu.queue, &mut pass, &vp, texture, &pipeline);
        assert_eq!(drawn, 3, "the graph pipeline drew {drawn} sprites, not 3");
    }
    gpu.submit(encoder);

    let img = gpu.read_rgba();

    // The three swatches must differ from each other. Identical means the
    // vertex colour is not reaching the shader, which compiles and renders
    // perfectly well as a constant.
    let sample = |sx: u32| -> (u32, u32, u32) {
        let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
        let (x0, y0) = (sx.saturating_sub(10), H / 2 - 10);
        for y in y0..(y0 + 20).min(img.height) {
            for x in x0..(x0 + 20).min(img.width) {
                let i = ((y * img.width + x) * 4) as usize;
                r += img.rgba[i] as u32;
                g += img.rgba[i + 1] as u32;
                b += img.rgba[i + 2] as u32;
            }
        }
        (r / 400, g / 400, b / 400)
    };
    let centres: Vec<(u32, u32, u32)> =
        (0..3).map(|i| sample(W / 2 - 70 + (i as u32) * 70)).collect();

    for (i, c) in centres.iter().enumerate() {
        assert!(
            c.0 + c.1 + c.2 > 30,
            "swatch {i} came out black ({c:?}) - the graph drew nothing visible"
        );
    }
    assert!(
        centres[0] != centres[1] && centres[1] != centres[2],
        "the three swatches are identical ({centres:?}) - the graph is not reading the \
         vertex colour, only compiling"
    );
    // And brighter input must give brighter output, or the mapping is inverted.
    let lum = |c: &(u32, u32, u32)| c.0 + c.1 + c.2;
    assert!(
        lum(&centres[0]) < lum(&centres[1]) && lum(&centres[1]) < lum(&centres[2]),
        "brightness does not increase with the vertex colour: {centres:?}"
    );

    compare_to_golden("graph_material_vertex_colour", &img);
}

/// A run of one texture longer than the batch holds is drawn in full.
///
/// `WgpuSpriteBatch` holds 8,192 quads and refused everything past that in
/// silence; the sprite loops queued a whole texture run between one `begin`
/// and one flush with no check, so scene 61's 16,000 one-texture boids drew
/// as 8,192 in every runtime, for as long as the swarm existed. Nothing
/// looked at pixels there and the count it measured was the count it
/// *queued*. This asserts the count that reached the GPU.
#[test]
fn a_texture_run_longer_than_one_batch_is_drawn_in_full() {
    use jengine_core::ecs::wgpu_systems::render_sprites_wgpu_multi_materialized;

    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("sheet.png".into(), quad_sheet());
    assets
        .ensure_wgpu_texture("sheet.png", &gpu.device, &gpu.queue)
        .expect("upload sheet");

    const N: usize = 8192 + 1;
    let mut world = World::new();
    for i in 0..N {
        world.spawn((
            Transform { position: Vec2::new((i % 64) as f32, (i / 64) as f32), ..Default::default() },
            SpriteComponent { texture_id: "sheet.png".into(), size: Some([1.0, 1.0]), ..Default::default() },
        ));
    }

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    let queued = {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_wgpu_multi_materialized(
            &world,
            &SortLayers::default(),
            &mut batch,
            Vec2::splat(1.0),
            |b, tex, _mat| {
                let texture = assets.get_wgpu_texture(tex).expect("sheet uploaded");
                b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture);
            },
        )
    };
    gpu.submit(encoder);

    assert_eq!(queued, N, "every sprite is queued");
    assert_eq!(batch.frame_sprite_count as usize, N, "every sprite reached a draw call");
    assert_eq!(batch.frame_draw_calls, 2, "one full batch and one of the remainder");
    assert_eq!(batch.frame_dropped, 0, "nothing was refused");
}

/// One `mesh` with more quads than a whole batch holds is drawn in full: the
/// batch keeps what did not fit and the sprite pass flushes and draws the
/// rest. It used to keep the first 8,192 quads and drop the others - Prism
/// Storm split its sparks over three meshes to stay under it.
#[test]
fn a_mesh_longer_than_one_batch_is_drawn_in_full() {
    use jengine_core::ecs::components::Mesh2D;
    use jengine_core::ecs::wgpu_systems::render_sprites_wgpu_multi_materialized;

    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("sheet.png".into(), quad_sheet());
    assets.ensure_wgpu_texture("sheet.png", &gpu.device, &gpu.queue).expect("upload sheet");

    const QUADS: usize = 8192 * 2 + 100;
    let mut vertices = Vec::with_capacity(QUADS * 4);
    let mut indices = Vec::with_capacity(QUADS * 6);
    for i in 0..QUADS {
        let (x, y) = ((i % 128) as f32, (i / 128) as f32 * 0.5);
        let base = (i * 4) as u32;
        for (dx, dy) in [(0.0, 0.0), (0.5, 0.0), (0.5, 0.5), (0.0, 0.5)] {
            vertices.push(serde_json::json!({ "position": [x + dx, y + dy], "uv": [dx * 2.0, dy * 2.0], "color": [1, 1, 1, 1] }));
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let mesh: Mesh2D = serde_json::from_value(serde_json::json!({
        "texture_id": "sheet.png", "vertices": vertices, "indices": indices,
    }))
    .expect("a mesh");
    let mut world = World::new();
    world.spawn((Transform::default(), mesh));

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_wgpu_multi_materialized(&world, &SortLayers::default(), &mut batch, Vec2::splat(1.0), |b, tex, _mat| {
            let texture = assets.get_wgpu_texture(tex).expect("sheet uploaded");
            b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture);
        });
    }
    gpu.submit(encoder);

    assert_eq!(batch.frame_dropped, 0, "nothing was refused");
    assert_eq!(batch.frame_sprite_count as usize, QUADS, "every quad reached a draw call");
    assert_eq!(batch.frame_draw_calls, 3, "two full batches and the remainder");
}

/// A tilemap layer showing more tiles of one tileset than a batch holds is
/// drawn in full. The tilemap pass queued a whole bucket and flushed once, so
/// the batch refused every tile past 8,192 - a territory-claiming game's
/// 160 x 96 picture as one layer, lost its bottom rows (the A1 fix had reached the
/// sprite and particle loops, not this one).
#[test]
fn a_tilemap_layer_longer_than_one_batch_is_drawn_in_full() {
    use jengine_core::ecs::wgpu_systems::render_tilemaps_wgpu;
    use jengine_core::tilemap::{Tilemap, TilemapLayer, TilesetDef};

    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("sheet.png".into(), quad_sheet());
    assets.ensure_wgpu_texture("sheet.png", &gpu.device, &gpu.queue).expect("upload sheet");

    // 128 x 72 one-unit cells fill the 256 x 128 view's middle: 9,216 tiles.
    let (cols, rows) = (128u32, 72u32);
    let n = (cols * rows) as usize;
    let mut world = World::new();
    world.spawn((
        Transform { position: Vec2::new(-(cols as f32) / 2.0, rows as f32 / 2.0), ..Default::default() },
        Tilemap {
            tile_size: [1.0, 1.0],
            layers: vec![TilemapLayer {
                name: "ground".into(),
                tiles: vec![1; n],
                width: cols,
                height: rows,
                visible: true,
                ..Default::default()
            }],
            tilesets: vec![TilesetDef {
                texture_id: "sheet.png".into(),
                tile_width: 32,
                tile_height: 32,
                columns: 2,
                tile_count: 4,
                first_gid: 1,
                image_width: 64,
                image_height: 64,
                ..Default::default()
            }],
            ..Default::default()
        },
    ));

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    let queued = {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_tilemaps_wgpu(&world, &SortLayers::default(), &mut batch, &camera, |b, tex| {
            let texture = assets.get_wgpu_texture(tex).expect("sheet uploaded");
            b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture);
        })
    };
    gpu.submit(encoder);

    assert_eq!(queued, n, "every tile is queued");
    assert_eq!(batch.frame_sprite_count as usize, n, "every tile reached a draw call");
    assert_eq!(batch.frame_dropped, 0, "nothing was refused");
}

/// Sprites outside the view a host set are not queued; inside, they are;
/// with no view set, everything is.
///
/// The sprite passes never culled: every sprite in the world was collected,
/// sorted and uploaded every frame, and the entry points did not even take
/// the camera. `renderer::frame_view` is what the hosts set once per pass.
#[test]
fn sprites_outside_the_frame_view_are_not_drawn() {
    use jengine_core::ecs::wgpu_systems::render_sprites_wgpu_multi_materialized;
    use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};

    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("sheet.png".into(), quad_sheet());
    assets.ensure_wgpu_texture("sheet.png", &gpu.device, &gpu.queue).expect("upload sheet");

    let mut world = World::new();
    for i in 0..10 {
        // Inside: within the 256 x 128 view about the origin.
        world.spawn((
            Transform { position: Vec2::new(i as f32 * 10.0 - 50.0, 0.0), ..Default::default() },
            SpriteComponent { texture_id: "sheet.png".into(), size: Some([8.0, 8.0]), ..Default::default() },
        ));
        // Far outside, by more than any sprite's reach.
        world.spawn((
            Transform { position: Vec2::new(5000.0 + i as f32, 5000.0), ..Default::default() },
            SpriteComponent { texture_id: "sheet.png".into(), size: Some([8.0, 8.0]), ..Default::default() },
        ));
    }

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut draw = |set_view: bool| -> usize {
        if set_view { set_frame_view(&camera) } else { clear_frame_view() }
        let mut encoder = gpu.encoder();
        let queued = {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            batch.begin_frame();
            render_sprites_wgpu_multi_materialized(
                &world,
                &SortLayers::default(),
                &mut batch,
                Vec2::splat(1.0),
                |b, tex, _mat| {
                    let texture = assets.get_wgpu_texture(tex).expect("sheet uploaded");
                    b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture);
                },
            )
        };
        gpu.submit(encoder);
        queued
    };
    assert_eq!(draw(false), 20, "no view set: nothing is culled");
    assert_eq!(draw(true), 10, "with the view set, only what it sees is queued");
    clear_frame_view();
}

/// A sprite sorted above a world text covers it.
///
/// World text used to be its own pass after every sprite pass, so a label at
/// z 5 drew over a sprite at z 50 and `z_order` on text only ordered text
/// against text. It is drawn by the interleaved sprite pass now, with the
/// font atlas as its texture group: the pixels under the sprite are the
/// sprite's.
#[test]
fn a_sprite_sorted_above_world_text_covers_it() {
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, render_sprites_with_shadows_wgpu_materialized};

    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let (rgba, aw, ah, font) = built_in_font_atlas();
    const FONT: &str = "__font/builtin";
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(FONT.into(), RawTextureData { width: aw, height: ah, rgba });
    assets.ensure_wgpu_texture(FONT, &gpu.device, &gpu.queue).expect("upload font");
    assets.insert_texture_bytes("sheet.png".into(), quad_sheet());
    assets.ensure_wgpu_texture("sheet.png", &gpu.device, &gpu.queue).expect("upload sheet");
    let fonts = FrameFonts::builtin_only(&font, FONT);

    let mut world = World::new();
    // White text at z 5, and a flat red square at z 50 over the same spot.
    world.spawn((
        Transform::default(),
        TextComponent { text: "HIDDEN".into(), z_order: 5, ..text_style() },
    ));
    // And text at the same z where nothing covers it, so the test can tell
    // "drawn underneath" from "not drawn at all".
    world.spawn((
        Transform { position: Vec2::new(-120.0, 0.0), ..Default::default() },
        TextComponent { text: "SEEN".into(), z_order: 5, ..text_style() },
    ));
    world.spawn((
        Transform::default(),
        SpriteComponent {
            texture_id: "sheet.png".into(),
            uv_rect: Some([0.0, 0.0, 0.5, 0.5]), // the red cell
            size: Some([120.0, 60.0]),
            z_order: 50,
            ..Default::default()
        },
    ));

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        let assets = &assets;
        render_sprites_with_shadows_wgpu_materialized(
            &world,
            &SortLayers::default(),
            &mut batch,
            &fonts,
            Vec2::splat(64.0),
            jengine_core::ecs::wgpu_systems::WorldLayer::All,
            |b, tex, kind| {
                let texture = assets.get_wgpu_texture(tex).expect("texture uploaded");
                match kind {
                    SpriteShadowFlush::Shadow => b.flush_multiply(&gpu.device, &gpu.queue, &mut pass, &vp, texture),
                    SpriteShadowFlush::Sprite { .. } => b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture),
                };
            },
        );
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    // Every pixel inside the square is the square's red: no white glyph
    // shows through. The square is 120 x 60 about the centre of 256 x 128.
    let mut white = 0;
    for y in 40..88 {
        for x in 80..176 {
            let i = ((y * img.width + x) * 4) as usize;
            if img.rgba[i + 1] > 150 && img.rgba[i + 2] > 150 {
                white += 1;
            }
        }
    }
    assert_eq!(white, 0, "{white} white pixels of text show through a sprite sorted above it");
    let mut seen = 0;
    for y in 0..img.height {
        for x in 0..40 {
            let i = ((y * img.width + x) * 4) as usize;
            if img.rgba[i + 1] > 150 && img.rgba[i + 2] > 150 {
                seen += 1;
            }
        }
    }
    assert!(seen > 0, "the uncovered text at the same z was not drawn at all");
}

/// The sheet's columns: red and blue on the left, green and yellow on the
/// right. A pixel is "right column" if it is green or yellow.
fn is_right_column(px: &[u8]) -> bool {
    (px[1] > 150 && px[0] < 120) || (px[0] > 150 && px[1] > 150 && px[2] < 120)
}

/// A textured trapezoid with `q` is mapped in perspective: the boundary
/// between the sheet's left and right columns (u = 0.5) runs straight from
/// the middle of the far edge to the middle of the near edge. Without `q`
/// the two triangles interpolate on their own and that boundary kinks at
/// the diagonal - the affine warp every PS1 floor had. (The *horizontal*
/// boundary stays straight either way on a trapezoid with horizontal
/// edges, which is why this looks at the vertical one.) This is the only
/// test of the perspective divide that sees pixels; every sprite golden
/// above passes with `q = 1` and proves nothing about any other value.
#[test]
fn a_mesh_with_q_is_textured_in_perspective() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("sheet.png".into(), quad_sheet());
    assets.ensure_wgpu_texture("sheet.png", &gpu.device, &gpu.queue).expect("upload sheet");
    let texture = assets.get_wgpu_texture("sheet.png").expect("sheet uploaded");
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();

    // The far edge on top, 60 wide; the near edge below, 200 wide.
    let corners = [
        Vec2::new(-30.0, 50.0),
        Vec2::new(30.0, 50.0),
        Vec2::new(100.0, -50.0),
        Vec2::new(-100.0, -50.0),
    ];
    let uv = [[0.0f32, 0.0f32], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let mut render = |q_far: f32| -> RawTextureData {
        let uvq: Vec<[f32; 3]> = uv
            .iter()
            .enumerate()
            .map(|(i, uv)| {
                let q = if i < 2 { q_far } else { 1.0 };
                [uv[0] * q, uv[1] * q, q]
            })
            .collect();
        batch.begin_frame();
        batch.begin();
        let used = batch.draw_mesh(&corners, &uvq, &[[1.0; 4]; 4], &[0, 1, 2, 0, 2, 3]);
        assert_eq!(used, 1, "a quad's two triangles take one slot");
        let mut encoder = gpu.encoder();
        {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture);
        }
        gpu.submit(encoder);
        gpu.read_rgba()
    };
    // `q` is the ratio of the edges: 60 / 200.
    let perspective = render(0.3);
    let flat = render(1.0);

    // Along each row through the trapezoid, the column where the left-hand
    // colours give way to the right-hand ones.
    let boundary_columns = |img: &RawTextureData| -> Vec<u32> {
        (H / 2 - 40..H / 2 + 40)
            .step_by(8)
            .map(|y| {
                (W / 2 - 100..W / 2 + 100)
                    .find(|&x| {
                        let i = ((y * img.width + x) * 4) as usize;
                        is_right_column(&img.rgba[i..i + 4])
                    })
                    .expect("the row crosses the trapezoid")
            })
            .collect()
    };
    let spread = |cols: &[u32]| cols.iter().max().unwrap() - cols.iter().min().unwrap();
    let cols_q = boundary_columns(&perspective);
    let cols_flat = boundary_columns(&flat);
    assert!(
        spread(&cols_q) <= 1,
        "with q the u = 0.5 line should be straight down the middle, got columns {cols_q:?}"
    );
    assert!(
        spread(&cols_flat) >= 10,
        "without q the line should kink at the diagonal, got columns {cols_flat:?} - if it does not, \
         the test is not looking at a trapezoid"
    );
    compare_to_golden("mesh_perspective", &perspective);
}

/// A `Mesh2D` is drawn in the sprite pass, in z order with sprites and
/// text: a mesh at z 50 covers text at z 5, and a mesh at z 5 is covered by
/// a sprite at z 50. The second half is the one no separate mesh pass can
/// pass, and it is what lets a monster sprite stand in front of a wall.
#[test]
fn a_mesh_draws_in_z_order_with_sprites_and_text() {
    use jengine_core::ecs::components::Mesh2D;
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, render_sprites_with_shadows_wgpu_materialized};

    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let (rgba, aw, ah, font) = built_in_font_atlas();
    const FONT: &str = "__font/builtin";
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(FONT.into(), RawTextureData { width: aw, height: ah, rgba });
    assets.ensure_wgpu_texture(FONT, &gpu.device, &gpu.queue).expect("upload font");
    assets.insert_texture_bytes("sheet.png".into(), quad_sheet());
    assets.ensure_wgpu_texture("sheet.png", &gpu.device, &gpu.queue).expect("upload sheet");
    let fonts = FrameFonts::builtin_only(&font, FONT);

    let mut world = World::new();
    // White text at z 5 under a red mesh at z 50.
    world.spawn((
        Transform::default(),
        TextComponent { text: "HIDDEN".into(), z_order: 5, ..text_style() },
    ));
    let mut over = Mesh2D::quad("sheet.png", [[-60.0, 30.0], [60.0, 30.0], [60.0, -30.0], [-60.0, -30.0]]);
    over.uv_rect = Some([0.0, 0.0, 0.5, 0.5]); // the red cell
    over.z_order = 50;
    world.spawn((Transform::default(), over));
    // A green mesh at z 5 under a blue sprite at z 50, off to the left.
    let mut under = Mesh2D::quad("sheet.png", [[-20.0, 20.0], [20.0, 20.0], [20.0, -20.0], [-20.0, -20.0]]);
    under.uv_rect = Some([0.5, 0.0, 1.0, 0.5]); // the green cell
    under.z_order = 5;
    world.spawn((Transform { position: Vec2::new(-100.0, 0.0), ..Default::default() }, under));
    world.spawn((
        Transform { position: Vec2::new(-100.0, 0.0), ..Default::default() },
        SpriteComponent {
            texture_id: "sheet.png".into(),
            uv_rect: Some([0.0, 0.5, 0.5, 1.0]), // the blue cell
            size: Some([40.0, 40.0]),
            z_order: 50,
            ..Default::default()
        },
    ));

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        let assets = &assets;
        render_sprites_with_shadows_wgpu_materialized(
            &world,
            &SortLayers::default(),
            &mut batch,
            &fonts,
            Vec2::splat(64.0),
            jengine_core::ecs::wgpu_systems::WorldLayer::All,
            |b, tex, kind| {
                let texture = assets.get_wgpu_texture(tex).expect("texture uploaded");
                match kind {
                    SpriteShadowFlush::Shadow => b.flush_multiply(&gpu.device, &gpu.queue, &mut pass, &vp, texture),
                    SpriteShadowFlush::Sprite { .. } => b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture),
                };
            },
        );
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();

    let count = |x0: u32, x1: u32, y0: u32, y1: u32, pred: &dyn Fn(&[u8]) -> bool| -> usize {
        let mut n = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y * img.width + x) * 4) as usize;
                if pred(&img.rgba[i..i + 4]) {
                    n += 1;
                }
            }
        }
        n
    };
    let white = |p: &[u8]| p[0] > 150 && p[1] > 150 && p[2] > 150;
    let red = |p: &[u8]| p[0] > 150 && p[1] < 120 && p[2] < 120;
    let green = |p: &[u8]| p[1] > 150 && p[0] < 120;
    let blue = |p: &[u8]| p[2] > 150 && p[0] < 120;
    // The red mesh is 120 x 60 about the centre of 256 x 128.
    assert_eq!(count(70, 186, 36, 92, &white), 0, "text shows through a mesh sorted above it");
    assert!(count(70, 186, 36, 92, &red) > 5000, "the mesh over the text did not draw");
    // The green mesh is 40 x 40 at x = -100: pixels 8..48. It must be blue.
    assert_eq!(count(10, 46, 46, 82, &green), 0, "a mesh at z 5 shows over a sprite at z 50");
    assert!(count(10, 46, 46, 82, &blue) > 1000, "the sprite over the mesh did not draw");

    compare_to_golden("mesh_in_sprite_pass", &img);
}

/// A `VectorPath` is drawn in the sprite pass, in z order with sprites
/// (Sep 2026). It used to be a pass of its own after every sprite, so a path
/// covered every sprite whatever its `z_order` - which is exactly wrong for a
/// route drawn *under* the plane flying it, the case that moved it.
///
/// Left: a red path at z 5 under a blue sprite at z 50 - the old pass fails
/// this. Right: a green path at z 50 over a blue sprite at z 5. Across the
/// bottom: an open, stroked route, the shape a drawn flight path takes.
#[test]
fn a_vector_path_draws_in_z_order_with_sprites() {
    use jengine_core::ecs::components::{PathCommand, VectorPath};
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, render_sprites_with_shadows_wgpu_materialized};

    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let (rgba, aw, ah, font) = built_in_font_atlas();
    const FONT: &str = "__font/builtin";
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(FONT.into(), RawTextureData { width: aw, height: ah, rgba });
    assets.ensure_wgpu_texture(FONT, &gpu.device, &gpu.queue).expect("upload font");
    // Paths draw against the white texture, as a sprite with no texture does.
    let white = jengine_core::asset::WHITE_TEXTURE_ID;
    assets.insert_texture_bytes(
        white.into(),
        RawTextureData { width: 1, height: 1, rgba: vec![255, 255, 255, 255] },
    );
    assets.ensure_wgpu_texture(white, &gpu.device, &gpu.queue).expect("upload white");
    assets.insert_texture_bytes("sheet.png".into(), quad_sheet());
    assets.ensure_wgpu_texture("sheet.png", &gpu.device, &gpu.queue).expect("upload sheet");
    let fonts = FrameFonts::builtin_only(&font, FONT);

    let square = |half: f32, fill: [f32; 4], z: i32| VectorPath {
        commands: vec![
            PathCommand::MoveTo { x: -half, y: -half },
            PathCommand::LineTo { x: half, y: -half },
            PathCommand::LineTo { x: half, y: half },
            PathCommand::LineTo { x: -half, y: half },
        ],
        fill_color: Some(fill),
        closed: true,
        z_order: z,
        ..Default::default()
    };
    let blue_sprite = |z: i32| SpriteComponent {
        texture_id: "sheet.png".into(),
        uv_rect: Some([0.0, 0.5, 0.5, 1.0]), // the blue cell
        size: Some([40.0, 40.0]),
        z_order: z,
        ..Default::default()
    };
    let at = |x: f32, y: f32| Transform { position: Vec2::new(x, y), ..Default::default() };

    let mut world = World::new();
    world.spawn((at(-80.0, 20.0), square(20.0, [1.0, 0.0, 0.0, 1.0], 5)));
    world.spawn((at(-80.0, 20.0), blue_sprite(50)));
    world.spawn((at(80.0, 20.0), square(20.0, [0.0, 1.0, 0.0, 1.0], 50)));
    world.spawn((at(80.0, 20.0), blue_sprite(5)));
    world.spawn((
        at(0.0, -40.0),
        VectorPath {
            commands: vec![
                PathCommand::MoveTo { x: -100.0, y: 0.0 },
                PathCommand::LineTo { x: 0.0, y: 10.0 },
                PathCommand::LineTo { x: 100.0, y: 0.0 },
            ],
            fill_color: None,
            stroke_color: Some([1.0, 1.0, 1.0, 1.0]),
            stroke_width: 4.0,
            ..Default::default()
        },
    ));

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        let assets = &assets;
        render_sprites_with_shadows_wgpu_materialized(
            &world,
            &SortLayers::default(),
            &mut batch,
            &fonts,
            Vec2::splat(64.0),
            jengine_core::ecs::wgpu_systems::WorldLayer::All,
            |b, tex, kind| {
                let texture = assets.get_wgpu_texture(tex).expect("texture uploaded");
                match kind {
                    SpriteShadowFlush::Shadow => b.flush_multiply(&gpu.device, &gpu.queue, &mut pass, &vp, texture),
                    SpriteShadowFlush::Sprite { .. } => b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture),
                };
            },
        );
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();

    let count = |x0: u32, x1: u32, y0: u32, y1: u32, pred: &dyn Fn(&[u8]) -> bool| -> usize {
        let mut n = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y * img.width + x) * 4) as usize;
                if pred(&img.rgba[i..i + 4]) {
                    n += 1;
                }
            }
        }
        n
    };
    let red = |p: &[u8]| p[0] > 150 && p[1] < 120 && p[2] < 120;
    let green = |p: &[u8]| p[1] > 150 && p[0] < 120;
    let blue = |p: &[u8]| p[2] > 150 && p[0] < 120 && p[1] < 150;
    let white = |p: &[u8]| p[0] > 200 && p[1] > 200 && p[2] > 200;
    // 256 x 128, centre (128, 64), y up. Left pair at x = -80, y = +20:
    // pixels 28..68 across, 24..64 down.
    assert_eq!(count(30, 66, 26, 62, &red), 0, "a path at z 5 shows over a sprite at z 50");
    assert!(count(30, 66, 26, 62, &blue) > 1000, "the sprite over the path did not draw");
    // Right pair at x = +80: pixels 188..228.
    assert!(count(190, 226, 26, 62, &green) > 1000, "a path at z 50 did not cover a sprite at z 5");
    assert_eq!(count(190, 226, 26, 62, &blue), 0, "a sprite at z 5 shows over a path at z 50");
    // The route: a 4-unit white line about y = -40, i.e. pixel row ~104.
    assert!(count(40, 216, 90, 118, &white) > 300, "the stroked route did not draw");

    compare_to_golden("vector_path_in_sprite_pass", &img);
}
