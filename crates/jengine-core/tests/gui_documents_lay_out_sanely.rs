//! Every authored `.gui` in the repo must lay out to plausible rectangles.
//!
//! # The bug this was written for
//!
//! `SizeValue::Percent` was a 0–1 fraction in the engine while
//! `ConstraintValue::Percent` — the other percentage in the same layout system
//! — was per-hundred, and the GUI editor authored *and* previewed sizes as
//! per-hundred too. So a document authored in the editor with `50%` stored
//! `50.0` and the engine laid it out fifty times too large (BUGS #10).
//!
//! `jultima/assets/ui/dialogue.gui` (a project since removed) had been sitting in the repo with a node
//! **44,800 px wide at x = −21,760** on a 1280-wide screen. Every existing test
//! passed: the file parsed, the schema was valid, the layout function did not
//! error. Nothing asked whether the answer was *sensible*.
//!
//! That is what this asks. It is a smoke test, not a pixel check — a pixel
//! check would break on every intentional UI edit and get deleted. A rect that
//! is off the screen by a factor of ten is not a design decision.

use jengine_core::ui::gui_document::load_gui;
use jengine_core::ui::layout::compute_layout;

/// Screen sizes to lay out at. Two, because a percentage bug can hide at one.
const SCREENS: &[(f32, f32)] = &[(1280.0, 720.0), (800.0, 600.0)];

/// How much larger than the screen a node may legitimately be.
///
/// Content inside a scrolling container really can exceed the viewport, and
/// the largest real case in the repo is 1.25x (`equipment.gui`'s root). Four is
/// well clear of anything authored on purpose and nowhere near the 35x that a
/// per-hundred value read as a fraction produces.
const MAX_SCREEN_MULTIPLE: f32 = 4.0;

fn gui_files() -> Vec<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for dir in ["projects", "templates"] {
        collect(&root.join(dir), &mut out);
    }
    out.sort();
    out
}

fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "gui") {
            out.push(p);
        }
    }
}

#[test]
fn no_authored_gui_lays_out_absurdly() {
    let files = gui_files();
    assert!(
        files.len() >= 10,
        "only found {} .gui files — the scan has stopped matching",
        files.len()
    );

    for path in &files {
        let text = std::fs::read_to_string(path).expect("read gui");
        let doc = load_gui(&text)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));

        for &(sw, sh) in SCREENS {
            for entry in compute_layout(&doc, sw, sh) {
                let r = entry.rect;
                let name = path.file_name().unwrap().to_string_lossy();
                assert!(
                    r.x.is_finite() && r.y.is_finite() && r.width.is_finite() && r.height.is_finite(),
                    "{name} @{sw}x{sh}: node `{}` has a non-finite rect \
                     ({}, {}, {}, {})",
                    entry.id, r.x, r.y, r.width, r.height
                );
                assert!(
                    r.width <= sw * MAX_SCREEN_MULTIPLE && r.height <= sh * MAX_SCREEN_MULTIPLE,
                    "{name} @{sw}x{sh}: node `{}` is {:.0}x{:.0}, more than {MAX_SCREEN_MULTIPLE}x \
                     the screen. A percentage written in the wrong convention looks exactly \
                     like this — see `SizeValue::Percent`, which is per-hundred.",
                    entry.id, r.width, r.height
                );
                // Entirely off-screen in either direction. A node parked off
                // the edge on purpose (an off-screen slide-in) would still
                // overlap when its animation puts it back; one at x = -21,760
                // never does.
                assert!(
                    r.x + r.width > -sw && r.x < sw * 2.0,
                    "{name} @{sw}x{sh}: node `{}` sits at x={:.0} width={:.0}, entirely off \
                     a {sw}-wide screen",
                    entry.id, r.x, r.width
                );
            }
        }
    }
}

/// A width or height written as a 0–1 fraction is almost certainly the old
/// convention rather than a deliberate 1%-wide node.
///
/// This catches the mistake at the file rather than at the rect, which gives a
/// far better message than "your node is tiny".
#[test]
fn no_gui_writes_a_size_percentage_in_the_old_fraction_convention() {
    for path in gui_files() {
        let text = std::fs::read_to_string(&path).expect("read gui");
        let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let mut offenders: Vec<(String, f64)> = Vec::new();
        walk(&doc, &mut offenders);
        assert!(
            offenders.is_empty(),
            "{}: width/height percentages look like the old 0–1 fraction: {offenders:?}\n\
             `SizeValue::Percent` is per-hundred — `100.0` is full width, not `1.0` \
             (BUGS #10). A genuinely 1%-wide node would need this check relaxed.",
            path.display()
        );
    }
}

fn walk(node: &serde_json::Value, out: &mut Vec<(String, f64)>) {
    match node {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if matches!(key.as_str(), "width" | "height")
                    && value.get("type").and_then(|t| t.as_str()) == Some("Percent")
                    && let Some(v) = value.get("value").and_then(|v| v.as_f64())
                    && v > 0.0
                    && v <= 1.0
                {
                    out.push((key.clone(), v));
                }
                walk(value, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                walk(item, out);
            }
        }
        _ => {}
    }
}

/// The two percentages in the layout system must agree on what a percent is.
///
/// They did not, which is the whole of BUGS #10: one divided by 100 and the
/// other did not, in the same module, both called `Percent`.
#[test]
fn both_percentage_types_mean_per_hundred() {
    use jengine_core::ui::gui_document::{ConstraintValue, SizeValue};

    let parent = 400.0f32;
    let half_size = match SizeValue::Percent(50.0) {
        SizeValue::Percent(p) => parent * p / 100.0,
        _ => unreachable!(),
    };
    let half_constraint = ConstraintValue::Percent(50.0).resolve(parent);
    assert_eq!(half_size, 200.0);
    assert_eq!(
        half_constraint, half_size,
        "ConstraintValue::Percent and SizeValue::Percent disagree about what 50% means"
    );
}
