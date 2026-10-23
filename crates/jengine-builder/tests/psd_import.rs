//! The PSD importer on a document written by another program:
//! `fixtures/layers.psd`, made by psd-tools (`fixtures/make_layers_psd.py`),
//! so the reader is checked against a writer that is not its own mirror.

use std::path::PathBuf;

use jengine_builder::psd::read_psd;
use jengine_builder::psd_import::{ImportOptions, Origin, import_psd};
use jengine_core::scene::format::EntityData;

fn fixture() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/layers.psd")).expect("the fixture")
}

#[test]
fn reads_every_layer_as_psd_tools_wrote_it() {
    let doc = read_psd(&fixture()).unwrap();
    assert_eq!((doc.width, doc.height), (64, 48));
    let names: Vec<&str> = doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Back", "Upper", "Lower", "Body", "Hidden", "Glow"], "bottom first, groups unwrapped");
    let by = |n: &str| doc.layers.iter().find(|l| l.name == n).unwrap();

    let back = by("Back");
    assert_eq!((back.left, back.top, back.width, back.height), (0, 0, 64, 48));
    assert_eq!(&back.rgba[..4], &[20, 40, 60, 255], "a raw channel");

    let upper = by("Upper");
    assert_eq!(upper.groups, ["Arm"]);
    assert_eq!((upper.left, upper.top, upper.width, upper.height), (40, 10, 6, 10));
    // The gradient: x * 40, y * 25 - a wrong row or channel order shows here.
    let px = |x: usize, y: usize| &upper.rgba[(y * 6 + x) * 4..(y * 6 + x) * 4 + 4];
    assert_eq!(px(3, 5), &[120, 125, 200, 255]);
    assert_eq!(px(5, 9), &[200, 225, 200, 255]);
    assert_eq!(by("Lower").groups, ["Arm"]);

    assert_eq!(by("Body").opacity, 128);
    assert!(by("Body").visible);
    assert!(!by("Hidden").visible);
    let glow = by("Glow");
    assert_eq!(glow.blend, "scrn");
    assert_eq!(&glow.rgba[..4], &[255, 255, 0, 180], "its own alpha channel");
}

/// A project-shaped scratch directory holding the fixture.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jengine-psd-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets/art")).unwrap();
    let psd = dir.join("assets/art/layers.psd");
    std::fs::write(&psd, fixture()).unwrap();
    psd
}

#[test]
fn imports_a_prefab_that_draws_as_the_document_does() {
    let psd = scratch("import");
    let assets = psd.parent().unwrap().parent().unwrap().to_path_buf();
    let report = import_psd(&psd, &ImportOptions::default()).unwrap();
    assert_eq!(report.prefab, "prefabs/layers.prefab");
    assert_eq!(
        report.sprites,
        [
            "sprites/layers/back.png",
            "sprites/layers/arm_upper.png",
            "sprites/layers/arm_lower.png",
            "sprites/layers/body.png",
            "sprites/layers/glow.png"
        ]
    );
    assert_eq!(report.skipped, ["Hidden (hidden)"]);

    // The pictures are the layers' pixels.
    let upper = image::open(assets.join("sprites/layers/arm_upper.png")).unwrap().to_rgba8();
    assert_eq!(upper.dimensions(), (6, 10));
    assert_eq!(upper.get_pixel(3, 5).0, [120, 125, 200, 255]);

    // The prefab is one the engine reads, its parts the document's tree.
    let text = std::fs::read_to_string(assets.join("prefabs/layers.prefab")).unwrap();
    let prefab: EntityData = serde_json::from_str(&text).expect("an engine prefab");
    assert_eq!(prefab.name, "layers");
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(json["components"]["sorting_group"].is_object());
    let kids = json["children"].as_array().unwrap();
    let names: Vec<&str> = kids.iter().map(|k| k["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Back", "Arm", "Body", "Glow"]);
    let pos = |v: &serde_json::Value| -> [f64; 2] {
        let p = &v["components"]["transform"]["position"];
        [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()]
    };
    // The canvas's centre (32, 24) is the root; y runs up.
    assert_eq!(pos(&kids[0]), [0.0, 0.0], "the full-canvas back");
    let body = &kids[2];
    assert_eq!(pos(body), [-12.0, 4.0], "Body's centre (20, 20)");
    assert_eq!(body["components"]["sprite"]["color"][3].as_f64(), Some(0.502));
    assert_eq!(body["components"]["sprite"]["size"], serde_json::json!([20.0, 24.0]));
    // The group sits at its layers' middle (43, 21); its layers about it.
    let arm = &kids[1];
    assert_eq!(pos(arm), [11.0, 3.0]);
    let arm_kids = arm["children"].as_array().unwrap();
    assert_eq!(pos(&arm_kids[0]), [0.0, 6.0], "Upper's centre (43, 15)");
    assert_eq!(pos(&arm_kids[1]), [0.0, -5.0], "Lower's centre (43, 26)");
    // Draw order bottom up, and the screen blend kept.
    let z: Vec<i64> = [&kids[0], &arm_kids[0], &arm_kids[1], &kids[2], &kids[3]]
        .iter()
        .map(|k| k["components"]["sprite"]["z_order"].as_i64().unwrap())
        .collect();
    assert_eq!(z, [0, 1, 2, 3, 4]);
    assert_eq!(kids[3]["components"]["material"]["blend_mode"], "Screen");
    assert!(kids[0]["components"].get("material").is_none());
}

#[test]
fn a_character_stands_on_the_bottom_edge_and_hidden_layers_can_come_in() {
    let psd = scratch("options");
    let assets = psd.parent().unwrap().parent().unwrap().to_path_buf();
    let report = import_psd(&psd, &ImportOptions { origin: Origin::Bottom, include_hidden: true }).unwrap();
    assert!(report.skipped.is_empty());
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(assets.join(report.prefab)).unwrap()).unwrap();
    let back = &json["children"][0]["components"]["transform"]["position"];
    assert_eq!((back[0].as_f64(), back[1].as_f64()), (Some(0.0), Some(24.0)), "half the canvas above the feet");
    assert_eq!(json["children"].as_array().unwrap().len(), 5);
}

#[test]
fn a_file_outside_assets_is_refused() {
    let dir = std::env::temp_dir().join(format!("jengine-psd-outside-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let psd = dir.join("loose.psd");
    std::fs::write(&psd, fixture()).unwrap();
    let err = import_psd(&psd, &ImportOptions::default()).unwrap_err().to_string();
    assert!(err.contains("not under an assets directory"), "{err}");
}

/// The imported prefab drawn on the CPU - each part's sprite at its place,
/// bottom up, by its opacity and blend - against psd-tools' own composite
/// of the document (`fixtures/layers_expected.png`). This is "dropped in a
/// scene, it looks like the PSD", short of the renderer.
#[test]
fn the_prefab_composites_to_the_document() {
    let psd = scratch("composite");
    let assets = psd.parent().unwrap().parent().unwrap().to_path_buf();
    let report = import_psd(&psd, &ImportOptions::default()).unwrap();
    let prefab: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(assets.join(&report.prefab)).unwrap()).unwrap();
    let expected =
        image::open(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/layers_expected.png")).unwrap().to_rgba8();
    let (w, h) = expected.dimensions();

    // Every sprite with its world position (parts sit in their parent's frame).
    struct Part {
        z: i64,
        centre: [f64; 2],
        texture: String,
        alpha: f64,
        screen: bool,
    }
    fn walk(node: &serde_json::Value, at: [f64; 2], out: &mut Vec<Part>) {
        let c = &node["components"];
        let p = &c["transform"]["position"];
        let here = [at[0] + p[0].as_f64().unwrap_or(0.0), at[1] + p[1].as_f64().unwrap_or(0.0)];
        if let Some(s) = c.get("sprite") {
            out.push(Part {
                z: s["z_order"].as_i64().unwrap(),
                centre: here,
                texture: s["texture_id"].as_str().unwrap().to_string(),
                alpha: s["color"][3].as_f64().unwrap(),
                screen: c["material"]["blend_mode"] == "Screen",
            });
        }
        for k in node["children"].as_array().into_iter().flatten() {
            walk(k, here, out);
        }
    }
    let mut parts = Vec::new();
    walk(&prefab, [0.0, 0.0], &mut parts);
    parts.sort_by_key(|p| p.z);

    let mut canvas = vec![[0.0f64; 4]; (w * h) as usize];
    for part in &parts {
        let img = image::open(assets.join(&part.texture)).unwrap().to_rgba8();
        let (iw, ih) = img.dimensions();
        // Root at the canvas's centre, y up.
        let left = (w as f64 / 2.0 + part.centre[0] - iw as f64 / 2.0).round() as i64;
        let top = (h as f64 / 2.0 - part.centre[1] - ih as f64 / 2.0).round() as i64;
        for (x, y, px) in img.enumerate_pixels() {
            let (cx, cy) = (left + x as i64, top + y as i64);
            if cx < 0 || cy < 0 || cx >= w as i64 || cy >= h as i64 {
                continue;
            }
            let d = &mut canvas[(cy as u32 * w + cx as u32) as usize];
            let a = px.0[3] as f64 / 255.0 * part.alpha;
            for ch in 0..3 {
                let s = px.0[ch] as f64 / 255.0;
                let src = if part.screen { 1.0 - (1.0 - s) * (1.0 - d[ch]) } else { s };
                d[ch] = src * a + d[ch] * (1.0 - a);
            }
            d[3] = a + d[3] * (1.0 - a);
        }
    }
    let mut worst = 0i32;
    for (x, y, want) in expected.enumerate_pixels() {
        let got = canvas[(y * w + x) as usize];
        for ch in 0..4 {
            let diff = ((got[ch] * 255.0).round() as i32 - want.0[ch] as i32).abs();
            assert!(diff <= 3, "pixel ({x}, {y}) channel {ch}: got {:?}, psd-tools drew {:?}", got, want.0);
            worst = worst.max(diff);
        }
    }
    assert!(worst <= 3);
}
