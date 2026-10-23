//! The skeletal clip editor poses its preview by sampling tracks itself, and
//! the inspector draws a blend space's weights itself
//! (`editor/src/lib/skelEdit.ts`), so both are checked against the engine's
//! (`animation::skeletal::sample_scalar`, `sample_vec2`, `blend_weights`) on
//! one fixture, `fixtures/skeletal_sample.json` - the editor's half in
//! `skelEdit.test.ts`. A change to either that the other did not get fails
//! one of the two, and the editor stops showing what plays.

use glam::Vec2;
use jengine_core::animation::skeletal::{ScalarKey, Vec2Key, blend_weights, sample_scalar, sample_vec2};
use serde_json::Value;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn skeletal_sample_matches_the_editor() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/skeletal_sample.json"))
        .expect("the fixture");
    let fixture: Value = serde_json::from_str(&text).expect("JSON");
    let cases = fixture["cases"].as_array().expect("cases");
    assert!(cases.len() >= 12);
    for case in cases {
        let name = case["name"].as_str().unwrap_or("?");
        let want = &case["expected"];
        match case["kind"].as_str() {
            Some("scalar") => {
                let keys: Vec<ScalarKey> = serde_json::from_value(case["keys"].clone()).expect(name);
                let got = sample_scalar(&keys, case["t"].as_f64().unwrap() as f32).expect(name);
                assert!(close(got as f64, want.as_f64().unwrap()), "{name}: got {got}, want {want}");
            }
            Some("vec2") => {
                let keys: Vec<Vec2Key> = serde_json::from_value(case["keys"].clone()).expect(name);
                let got = sample_vec2(&keys, case["t"].as_f64().unwrap() as f32).expect(name);
                let w = want.as_array().unwrap();
                assert!(
                    close(got.x as f64, w[0].as_f64().unwrap()) && close(got.y as f64, w[1].as_f64().unwrap()),
                    "{name}: got {got}, want {want}"
                );
            }
            Some("blend") => {
                let points: Vec<Vec2> = case["points"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| Vec2::new(p[0].as_f64().unwrap() as f32, p[1].as_f64().unwrap() as f32))
                    .collect();
                let p = Vec2::new(case["p"][0].as_f64().unwrap() as f32, case["p"][1].as_f64().unwrap() as f32);
                let got = blend_weights(&points, p);
                let w: Vec<f64> = want.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
                assert_eq!(got.len(), w.len(), "{name}");
                for (g, e) in got.iter().zip(&w) {
                    assert!(close(*g as f64, *e), "{name}: got {got:?}, want {w:?}");
                }
            }
            other => panic!("{name}: unknown kind {other:?}"),
        }
    }
}
