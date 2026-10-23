//! **Rich text in the UI** (Oct 2026): entity-UI `ui_text` and `.gui` Text
//! nodes read the same markup world text does (`renderer::rich_text`) -
//! coloured words, effects, inline icons - and `ui_text` has the typewriter.
//! Until then only world text had markup, so a journal or a shop panel drawn
//! in the UI was one colour per text, with icons placed beside it by hand.
//! Found by the showcase's The Quest Journal.

use hecs::World;
use jengine_core::ui::gui_document::{GuiDocument, GuiNodeType};
use jengine_core::ui::{UIElement, UIText};

fn rich(text: &str) -> UIText {
    UIText { text: text.into(), rich: true, font_size: 64.0, ..Default::default() }
}

#[test]
fn the_motion_step_runs_a_ui_typewriter() {
    let mut world = World::new();
    let e = world.spawn((UIElement::default(), UIText { reveal_speed: 20.0, ..rich("hello there") }));
    jengine_core::ecs::behaviors::tick_motion(&mut world, 0.25, None, false);
    jengine_core::ecs::behaviors::tick_motion(&mut world, 0.25, None, false);
    let t = world.get::<&UIText>(e).unwrap();
    assert!((t.reveal.unwrap().elapsed - 0.25).abs() < 1e-6, "started on the first tick, advanced on the second");
}

#[test]
fn the_formats_keep_rich_and_plain_text_writes_neither() {
    let saved = serde_json::to_value(UIText { reveal_speed: 12.0, ..rich("[b]x[/b]") }).unwrap();
    assert_eq!((saved["rich"].as_bool(), saved["reveal_speed"].as_f64()), (Some(true), Some(12.0)));
    let plain = serde_json::to_value(UIText::default()).unwrap();
    assert!(plain.get("rich").is_none() && plain.get("reveal_speed").is_none());
    let node: GuiNodeType = serde_json::from_str(r#"{ "type": "Text", "text": "[b]x[/b]", "rich": true }"#).unwrap();
    assert!(matches!(node, GuiNodeType::Text { rich: true, .. }));
}

/// Native preloads every texture before the first frame; an icon it does
/// not know of draws as nothing there. The editor fetches on a miss; the web
/// build does not - it preloads, as native does (see below).
#[test]
fn ui_and_gui_icons_are_preloaded() {
    let mut world = World::new();
    world.spawn((UIElement::default(), rich("a [icon=sprites/key.png] key")));
    world.spawn((UIElement::default(), UIText { text: "[icon=sprites/not.png]".into(), ..Default::default() }));
    let ids = jengine_core::scene::format::world_texture_ids(&world);
    assert!(ids.contains("sprites/key.png"));
    assert!(!ids.contains("sprites/not.png"), "only rich text reads markup");
    let doc: GuiDocument = serde_json::from_value(serde_json::json!({
        "name": "d", "anchor": "TopLeft",
        "root": { "id": "root", "node_type": { "type": "Panel" }, "children": [
            { "id": "a", "node_type": { "type": "Image", "texture_id": "ui/frame.png" } },
            { "id": "b", "node_type": { "type": "Text", "text": "[icon=ui/coin.png] {{gold}}", "rich": true } },
            { "id": "c", "node_type": { "type": "Text", "text": "[icon=ui/plain.png]" } } ] }
    }))
    .unwrap();
    let mut ids = std::collections::BTreeSet::new();
    doc.root.texture_ids(&mut ids);
    assert_eq!(ids.into_iter().collect::<Vec<_>>(), ["ui/coin.png", "ui/frame.png"]);
}

/// The web build draws a `.gui` image only if it fetched it: a missing
/// texture is dropped with one warning, not fetched. Its preload walks the
/// world and the prefabs, which a document's images are in neither of, so
/// until Oct 2026 every `.gui` image was blank on the web (Menus & HUD's icon
/// sheet). Each path that fetches documents must fetch what they name.
#[test]
fn the_web_build_fetches_what_its_gui_documents_name() {
    let web = include_str!("../../jengine-web/src/lib.rs");
    let docs = web.matches("fetch_guis(").count() - 1; // less the definition
    let images = web.matches("gui_texture_ids(&guis").count();
    assert!(docs >= 3, "the scan found {docs} .gui fetches - fix it before trusting this");
    assert_eq!(docs, images, "a path fetches .gui documents without fetching their images");
    assert!(web.contains("doc.root.texture_ids(&mut ids)"), "gui_texture_ids no longer asks the documents");
}

#[cfg(feature = "gpu-wgpu")]
mod gpu {
    use super::*;
    use glam::Vec2;
    use jengine_core::ecs::components::TextReveal;
    use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
    use jengine_core::ecs::components::GuiComponent;
    use jengine_core::ecs::wgpu_systems::{UiFlush, render_gui_wgpu, render_ui_wgpu};
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 512;
    const H: u32 = 128;

    /// Draw the UI - entity UI from `world`, then the `.gui` documents -
    /// and read the image back.
    fn draw(gpu: &HeadlessGpu, world: &World, docs: &[(GuiDocument, GuiComponent)]) -> Vec<u8> {
        let (rgba, aw, ah, font) = built_in_font_atlas();
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
        assets.mark_sdf("__font");
        assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
        assets.insert_texture_bytes("icons/green.png".into(), RawTextureData { width: 4, height: 4, rgba: [0u8, 255, 0, 255].repeat(16) });
        let fonts = FrameFonts::builtin_only(&font, "__font");
        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
        let vp = glam::Mat4::orthographic_rh_gl(0.0, W as f32, 0.0, H as f32, -1.0, 1.0);
        batch.begin_frame();
        let mut encoder = gpu.encoder();
        {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            let (device, queue) = (&gpu.device, &gpu.queue);
            let mut flush = |batch: &mut WgpuSpriteBatch, what: UiFlush<'_>| {
                let id = match what {
                    UiFlush::Quad(None) => WHITE_TEXTURE_ID,
                    UiFlush::Quad(Some(id)) | UiFlush::Font(id) => id,
                    UiFlush::Clip(_) => return,
                };
                assets.ensure_wgpu_texture(id, device, queue).expect("known texture");
                batch.flush(device, queue, &mut pass, &vp, assets.get_wgpu_texture(id).expect("uploaded"));
            };
            render_ui_wgpu(world, &mut batch, Some(&fonts), W as f32, H as f32, &mut flush);
            render_gui_wgpu(docs, &mut batch, Some(&fonts), W as f32, H as f32, &mut flush);
        }
        gpu.submit(encoder);
        gpu.read_rgba().rgba
    }

    /// A text filling the screen, left-aligned.
    fn ui(text: UIText) -> World {
        let mut world = World::new();
        world.spawn((
            UIElement { anchor: jengine_core::ui::UIAnchor::Center, size: Vec2::new(W as f32 - 16.0, H as f32), ..Default::default() },
            text,
        ));
        world
    }

    /// Lit pixels in columns `x0..x1`: (reddish, whitish, greenish).
    fn census(img: &[u8], x0: u32, x1: u32) -> (usize, usize, usize) {
        let (mut red, mut white, mut green) = (0, 0, 0);
        for y in 0..H {
            for x in x0..x1 {
                let i = ((y * W + x) * 4) as usize;
                let (r, g, b) = (img[i], img[i + 1], img[i + 2]);
                if r > 150 && g < 90 && b < 90 {
                    red += 1;
                } else if r > 150 && g > 150 && b > 150 {
                    white += 1;
                } else if g > 150 && r < 90 && b < 90 {
                    green += 1;
                }
            }
        }
        (red, white, green)
    }

    /// The red word sits left of `split`, the white one right of it.
    fn red_then_white(img: &[u8], what: &str) {
        let (red, white, _) = census(img, 0, W);
        assert!(red > 200 && white > 200, "{what}: {red} red and {white} white pixels");
        let first = |want: fn([u8; 3]) -> bool| {
            (0..W).find(|&x| (0..H).any(|y| {
                let i = ((y * W + x) * 4) as usize;
                want([img[i], img[i + 1], img[i + 2]])
            }))
        };
        let r = first(|c| c[0] > 150 && c[1] < 90).unwrap();
        let w = first(|c| c[0] > 150 && c[1] > 150 && c[2] > 150).unwrap();
        assert!(r < w, "{what}: the red word comes first (red from {r}, white from {w})");
    }

    #[test]
    fn a_ui_text_draws_its_markup() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("rich_text_reaches_the_ui: no GPU adapter - skipped");
            return;
        };
        red_then_white(&draw(&gpu, &ui(rich("[color=#ff0000]HH[/color] HH")), &[]), "ui_text");
        // Without `rich` the markup is the text, in its own colour.
        let img = draw(&gpu, &ui(UIText { rich: false, ..rich("[color=#ff0000]HH[/color]") }), &[]);
        let (red, white, _) = census(&img, 0, W);
        assert!(red == 0 && white > 200, "brackets and all, in white: {red} red");
    }

    #[test]
    fn a_ui_typewriter_hides_what_it_has_not_reached() {
        let Some(gpu) = HeadlessGpu::new(W, H) else { return };
        let all = draw(&gpu, &ui(rich("HH HH")), &[]);
        let half = UIText { reveal_speed: 10.0, reveal: Some(TextReveal { key: 0, elapsed: 0.25 }), ..rich("HH HH") };
        let img = draw(&gpu, &ui(half), &[]);
        let (_, whole, _) = census(&all, 0, W);
        let (_, shown, _) = census(&img, 0, W);
        assert!(shown > 100 && shown * 3 < whole * 2, "about half shows: {shown} of {whole}");
    }

    #[test]
    fn a_ui_icon_draws_its_own_texture() {
        let Some(gpu) = HeadlessGpu::new(W, H) else { return };
        let img = draw(&gpu, &ui(rich("HH [icon=icons/green.png] HH")), &[]);
        let (_, white, green) = census(&img, 0, W);
        assert!(green > 400, "the icon: {green} green pixels");
        assert!(white > 200, "and the words around it");
    }

    #[test]
    fn a_gui_text_node_draws_its_markup() {
        let Some(gpu) = HeadlessGpu::new(W, H) else { return };
        let doc: GuiDocument = serde_json::from_value(serde_json::json!({
            "name": "t", "anchor": "TopLeft",
            "root": { "id": "root", "node_type": { "type": "Panel" },
                "style": { "width": { "type": "Fixed", "value": W as f32 }, "height": { "type": "Fixed", "value": H as f32 },
                           "bg_color": [0.0, 0.0, 0.0, 0.0] },
                "children": [ { "id": "t", "node_type": { "type": "Text", "text": "[color=#ff0000]HH[/color] {{word}} [icon=icons/green.png]",
                    "font_size": 64.0, "rich": true },
                    "style": { "width": { "type": "Fixed", "value": W as f32 }, "height": { "type": "Fixed", "value": H as f32 } } } ] }
        }))
        .unwrap();
        let mut comp = GuiComponent { gui_id: "t".into(), visible: true, ..Default::default() };
        comp.variables.insert("word".into(), serde_json::json!("HH"));
        let img = draw(&gpu, &World::new(), &[(doc, comp)]);
        red_then_white(&img, ".gui");
        let (_, _, green) = census(&img, 0, W);
        assert!(green > 300, "the icon after the substituted word: {green} green pixels");
    }
}
