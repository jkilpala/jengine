//! The timeline editor scrubs a scene by sampling tracks itself
//! (`editor/src/lib/timelineEdit.ts`), so its sampling is checked against
//! the engine's (`ecs::timeline::sample`) on one fixture,
//! `fixtures/timeline_sample.json` - the editor's half in
//! `timelineEdit.test.ts`. A change to either that the other did not get
//! fails one of the two, and the dope sheet stops lying about what plays.
#![cfg(feature = "timeline")]

use jengine_core::ecs::timeline::{Track, sample};
use serde_json::Value;

#[test]
fn timeline_sample_matches_the_editor() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/timeline_sample.json"))
        .expect("the fixture");
    let fixture: Value = serde_json::from_str(&text).expect("JSON");
    let cases = fixture["cases"].as_array().expect("cases");
    assert!(cases.len() >= 10);
    for case in cases {
        let name = case["name"].as_str().unwrap_or("?");
        let track: Track = serde_json::from_value(case["track"].clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let t = case["t"].as_f64().expect("t") as f32;
        let got = sample(&track, t).expect("a value");
        let want = &case["expected"];
        let close = match (&got, want) {
            (Value::Number(a), Value::Number(b)) => (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-4,
            (Value::Array(a), Value::Array(b)) => {
                a.len() == b.len()
                    && a.iter().zip(b).all(|(x, y)| match (x.as_f64(), y.as_f64()) {
                        (Some(x), Some(y)) => (x - y).abs() < 1e-4,
                        _ => x == y,
                    })
            }
            _ => &got == want,
        };
        assert!(close, "{name}: got {got}, want {want}");
    }
}
