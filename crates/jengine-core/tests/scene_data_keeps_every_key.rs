//! A scene saved by the editor keeps every top-level key it was loaded with.
//!
//! The editor loads a scene as `SceneData` and saves it back, and pushes that
//! same re-serialized copy to the viewport for Play. `SceneData` had five
//! fields and serde dropped everything else - so a save deleted `audio_refs`
//! (68 scenes), `atlas_refs` (52) and `paths` from disk, and Play ran without
//! them: test-lab scene 64's patrol routes never reached the viewport and
//! nothing moved (BUGS #157). This round-trips every scene in every project.

use jengine_core::scene::format::SceneData;

/// Equal, allowing for `f32` fields: `0.55` comes back as `0.550000011920929`.
fn same(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value::*;
    match (a, b) {
        (Number(x), Number(y)) => (x.as_f64().unwrap() - y.as_f64().unwrap()).abs() < 1e-5,
        (Array(x), Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b)),
        (Object(x), Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

#[test]
fn every_scene_round_trips_through_scene_data_with_all_its_keys() {
    let projects = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../projects");
    let mut checked = 0;
    let mut lost = Vec::new();
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n == ".jengine") {
                    continue;
                }
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "scene") {
                out.push(p);
            }
        }
    }
    let mut scenes = Vec::new();
    walk(&projects, &mut scenes);
    for path in scenes {
        let text = std::fs::read_to_string(&path).unwrap();
        let Ok(original) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        // Only scenes the editor can open as SceneData at all.
        let Ok(scene) = serde_json::from_str::<SceneData>(&text) else { continue };
        let saved: serde_json::Value = serde_json::to_value(&scene).unwrap();
        checked += 1;
        for (key, value) in original.as_object().unwrap() {
            if key == "entities" {
                continue; // entity bodies are normalised by ComponentBundle, by design
            }
            if key == "version" && value == 1 {
                continue; // v1 is the default and deliberately not written
            }
            if !saved.get(key).is_some_and(|v| same(v, value)) {
                lost.push(format!("{}: `{key}`", path.display()));
            }
        }
    }
    assert!(checked > 100, "only {checked} scenes checked - the scan broke");
    assert!(lost.is_empty(), "a SceneData round trip lost or changed:\n  {}", lost.join("\n  "));
}
