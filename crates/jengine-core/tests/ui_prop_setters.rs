//! BUGS #6 — entity-UI widgets are writable from scripts.
//!
//! Before `set_ui_prop`, a `UIProgressBar` authored in a scene kept its
//! authored value forever: the script API had `set_ui_text`, `set_ui_visible`
//! and `set_ui_position` and nothing else. These lock down the shared
//! dispatcher every runtime routes through.

use hecs::World;
use jengine_core::ui::{
    UIElement, UIImage, UIPanel, UIProgressBar, UISlider, UIText, UITextInput, UIToggle,
    apply_ui_prop,
};
use serde_json::json;

#[test]
fn sets_progress_bar_value_and_clamps_to_range() {
    let mut world = World::new();
    let e = world.spawn((UIProgressBar {
        value: 0.0,
        min: 0.0,
        max: 100.0,
        ..Default::default()
    },));

    assert!(apply_ui_prop(&mut world, e, "value", &json!(42.5)));
    assert_eq!(world.get::<&UIProgressBar>(e).unwrap().value, 42.5);

    // Out-of-range writes clamp rather than corrupting the bar's rendering:
    // a fill ratio outside 0..1 would draw past the frame.
    apply_ui_prop(&mut world, e, "value", &json!(999.0));
    assert_eq!(world.get::<&UIProgressBar>(e).unwrap().value, 100.0);
    apply_ui_prop(&mut world, e, "value", &json!(-5.0));
    assert_eq!(world.get::<&UIProgressBar>(e).unwrap().value, 0.0);
}

#[test]
fn accepts_numbers_that_arrive_as_strings() {
    // Values cross the bridge as JSON, but the Fengari and QuickJS paths can
    // hand a number over as a string. Both must land.
    let mut world = World::new();
    let e = world.spawn((UISlider {
        value: 0.0,
        min: 0.0,
        max: 1.0,
        ..Default::default()
    },));

    assert!(apply_ui_prop(&mut world, e, "value", &json!("0.75")));
    assert_eq!(world.get::<&UISlider>(e).unwrap().value, 0.75);
}

#[test]
fn sets_toggle_checked_from_bool_and_from_stringy_flags() {
    let mut world = World::new();
    let e = world.spawn((UIToggle::default(),));

    assert!(apply_ui_prop(&mut world, e, "checked", &json!(true)));
    assert!(world.get::<&UIToggle>(e).unwrap().checked);
    assert!(apply_ui_prop(&mut world, e, "checked", &json!("false")));
    assert!(!world.get::<&UIToggle>(e).unwrap().checked);
    assert!(apply_ui_prop(&mut world, e, "checked", &json!(1)));
    assert!(world.get::<&UIToggle>(e).unwrap().checked);
}

#[test]
fn text_reaches_both_text_and_text_input() {
    let mut world = World::new();
    let label = world.spawn((UIText::default(),));
    let field = world.spawn((UITextInput::default(),));

    assert!(apply_ui_prop(&mut world, label, "text", &json!("hello")));
    assert!(apply_ui_prop(&mut world, field, "text", &json!("typed")));
    assert_eq!(world.get::<&UIText>(label).unwrap().text, "hello");
    assert_eq!(world.get::<&UITextInput>(field).unwrap().text, "typed");
}

#[test]
fn sets_colors_from_rgba_arrays() {
    let mut world = World::new();
    let panel = world.spawn((UIPanel::default(),));
    let img = world.spawn((UIImage::default(),));

    assert!(apply_ui_prop(&mut world, panel, "bg_color", &json!([1.0, 0.0, 0.0, 1.0])));
    assert_eq!(world.get::<&UIPanel>(panel).unwrap().bg_color, [1.0, 0.0, 0.0, 1.0]);

    assert!(apply_ui_prop(&mut world, img, "color", &json!([0.0, 0.5, 1.0, 0.25])));
    assert_eq!(world.get::<&UIImage>(img).unwrap().color, [0.0, 0.5, 1.0, 0.25]);
}

#[test]
fn unknown_property_and_wrong_widget_both_report_no_write() {
    let mut world = World::new();
    let e = world.spawn((UIText::default(), UIElement::default()));

    // Unknown property: nothing in the engine claims it.
    assert!(!apply_ui_prop(&mut world, e, "nonsense", &json!(1)));
    // Known property, but this entity has no Toggle — a no-op, and the caller
    // is told so it can log rather than silently doing nothing.
    assert!(!apply_ui_prop(&mut world, e, "checked", &json!(true)));
    // Same property name on a widget that does have it still works.
    assert!(apply_ui_prop(&mut world, e, "visible", &json!(false)));
    assert!(!world.get::<&UIElement>(e).unwrap().visible);
}

// --- reader half -----------------------------------------------------------

#[test]
fn reads_back_what_was_written() {
    use jengine_core::ui::read_ui_prop;
    let mut world = World::new();
    let e = world.spawn((UISlider { value: 0.0, min: 0.0, max: 1.0, ..Default::default() },));

    apply_ui_prop(&mut world, e, "value", &json!(0.4));
    // Compared with tolerance, not for equality: widgets store f32 and JSON
    // carries f64, so 0.4 comes back as 0.4000000059604645. Rounding it in the
    // reader would be worse — it would quietly lie about what is stored.
    let got = read_ui_prop(&world, e, "value").unwrap().as_f64().unwrap();
    assert!((got - 0.4).abs() < 1e-6, "read back {got}");
}

#[test]
fn exposes_button_state_the_ui_system_maintains() {
    use jengine_core::ui::{UIButton, read_ui_prop};
    let mut world = World::new();
    let e = world.spawn((UIButton::default(),));

    // `pressed` / `hovered` are written by the button system, not by scripts —
    // reading them is the only way a script can react to a click, so a plain
    // `false` must come back as false and not as "no such property".
    assert_eq!(read_ui_prop(&world, e, "pressed"), Some(json!(false)));
    world.get::<&mut UIButton>(e).unwrap().pressed = true;
    assert_eq!(read_ui_prop(&world, e, "pressed"), Some(json!(true)));
}

#[test]
fn missing_widget_reads_none_not_a_default() {
    use jengine_core::ui::read_ui_prop;
    let mut world = World::new();
    let e = world.spawn((UIText::default(),));

    // A Text has no `checked`. None reaches scripts as nil, keeping "no such
    // widget" distinguishable from a genuine `false`.
    assert_eq!(read_ui_prop(&world, e, "checked"), None);
    assert_eq!(read_ui_prop(&world, e, "nonsense"), None);
}

/// Every property the reader answers is in the list the browser hosts
/// publish, or `get_ui_prop` reads it natively and `nil` in the editor.
#[test]
fn the_browser_hosts_publish_every_readable_property() {
    use jengine_core::ui::{READABLE_UI_PROPS, UIImage, UIPanel, UIText, UIElement, read_ui_prop};
    let src = include_str!("../src/ui/mod.rs");
    let reader = &src[src.find("pub fn read_ui_prop").unwrap()..src.find("pub fn apply_ui_prop").unwrap()];
    for line in reader.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix('"')
            && let Some(end) = rest.find("\" =>")
        {
            let prop = &rest[..end];
            assert!(READABLE_UI_PROPS.contains(&prop), "`{prop}` is readable but not in READABLE_UI_PROPS");
        }
    }
    // And the region an icon is cut from reads back.
    let mut world = hecs::World::new();
    let e = world.spawn((UIElement::default(), UIImage { region: Some("lemon".into()), ..Default::default() }));
    assert_eq!(read_ui_prop(&world, e, "region"), Some(json!("lemon")));
    let _ = (UIPanel::default(), UIText::default());
}

/// Re-pointing an icon's region clears the resolved rect, or the old icon
/// stays on screen.
#[test]
fn setting_a_ui_image_region_makes_it_resolve_again() {
    use jengine_core::ui::{UIElement, UIImage, apply_ui_prop};
    let mut world = hecs::World::new();
    let e = world.spawn((
        UIElement::default(),
        UIImage {
            region: Some("lemon".into()),
            uv_rect: Some([0.0, 0.0, 0.5, 0.5]),
            region_resolved: true,
            ..Default::default()
        },
    ));
    assert!(apply_ui_prop(&mut world, e, "region", &json!("ice")));
    let img = world.get::<&UIImage>(e).unwrap();
    assert_eq!(img.region.as_deref(), Some("ice"));
    assert!(img.uv_rect.is_none() && !img.region_resolved);
}

/// An icon names its atlas region like a sprite, and the shared pass every
/// runtime runs fills in the rect - then fills it in again when the script
/// swaps the icon.
#[test]
fn a_ui_image_resolves_its_atlas_region() {
    use jengine_core::asset::AssetManager;
    use jengine_core::ecs::systems::resolve_sprite_regions;
    use jengine_core::ui::{UIElement, UIImage, apply_ui_prop};
    let mut assets = AssetManager::new();
    let atlas: jengine_core::renderer::atlas::AtlasData = serde_json::from_value(json!({
        "width": 128, "height": 32,
        "regions": [
            { "name": "lemon", "x": 0,  "y": 0, "width": 32, "height": 32 },
            { "name": "ice",   "x": 64, "y": 0, "width": 32, "height": 32 }
        ]
    }))
    .unwrap();
    assets.insert_atlas_data("atlases/icons.png".into(), atlas);
    let mut world = hecs::World::new();
    let e = world.spawn((
        UIElement::default(),
        UIImage { texture_id: "atlases/icons.png".into(), region: Some("lemon".into()), ..Default::default() },
    ));
    resolve_sprite_regions(&mut world, &assets);
    assert_eq!(world.get::<&UIImage>(e).unwrap().uv_rect, Some([0.0, 0.0, 0.25, 1.0]));

    apply_ui_prop(&mut world, e, "region", &json!("ice"));
    resolve_sprite_regions(&mut world, &assets);
    assert_eq!(world.get::<&UIImage>(e).unwrap().uv_rect, Some([0.5, 0.0, 0.75, 1.0]));
}

#[test]
fn size_resizes_any_element() {
    let mut world = World::new();
    let e = world.spawn((UIElement { size: glam::Vec2::new(100.0, 50.0), ..Default::default() },));
    assert!(apply_ui_prop(&mut world, e, "size", &json!([470.0, 180.5])));
    assert_eq!(world.get::<&UIElement>(e).unwrap().size, glam::Vec2::new(470.0, 180.5));
    // Not a pair: refused, and the size is left alone.
    assert!(!apply_ui_prop(&mut world, e, "size", &json!(12)));
    assert!(!apply_ui_prop(&mut world, e, "size", &json!([1.0])));
    assert_eq!(world.get::<&UIElement>(e).unwrap().size, glam::Vec2::new(470.0, 180.5));
}

/// A script's spelling and a scene's both name an anchor; neither falls back
/// to top-left in silence.
#[test]
fn anchors_are_read_in_either_spelling() {
    use jengine_core::ui::UIAnchor;
    for (name, want) in [
        ("top_right", UIAnchor::TopRight),
        ("TopRight", UIAnchor::TopRight),
        ("bottom-center", UIAnchor::BottomCenter),
        ("Center", UIAnchor::Center),
        ("CENTER_LEFT", UIAnchor::CenterLeft),
        ("nonsense", UIAnchor::TopLeft),
    ] {
        assert_eq!(UIAnchor::from_name(name), want, "{name}");
    }
}
