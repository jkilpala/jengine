//! The editor's post-process panel writes the parameter names the renderer
//! reads.
//!
//! The Inspector's sliders used to write `u_threshold`, `u_intensity` and so
//! on, while `renderer::backend::wgpu::postprocess` reads `threshold`,
//! `intensity`. Every slider did nothing, for as long as the panel existed -
//! and nothing could notice, because a parameter the renderer does not know
//! is not an error, just a default. The panel draws its sliders from one
//! table now (`editor/src/lib/postProcess.ts`, `PP_EFFECTS`); this reads that
//! table and the renderer's `from_params` functions and compares them, per
//! effect.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// Every `"..."` string that follows `marker` in `text`.
fn quoted_after(text: &str, marker: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(marker) {
        rest = &rest[i + marker.len()..];
        if let Some(open) = rest.find('"') {
            let after = &rest[open + 1..];
            if let Some(close) = after.find('"') {
                out.push(after[..close].to_string());
            }
        }
    }
    out
}

/// Parameter names the renderer reads, by effect: the keys each
/// `<Effect>U::from_params` passes to `get_f32` / `get_vec4`.
fn renderer_params() -> BTreeMap<&'static str, BTreeSet<String>> {
    let src = read("crates/jengine-core/src/renderer/backend/wgpu/postprocess.rs");
    let mut out = BTreeMap::new();
    for (effect, uniform) in
        [
            ("bloom", "BloomU"),
            ("vignette", "VignetteU"),
            ("color_grade", "ColorGradeU"),
            ("chromatic", "ChromaticU"),
            ("pixelate", "PixelateU"),
            ("crt", "CrtU"),
            ("light_shafts", "LightShaftsU"),
            ("tilt_shift", "TiltShiftU"),
            ("blur", "BlurU"),
            ("motion_blur", "MotionBlurU"),
            ("lens_flare", "LensFlareU"),
            ("depth_of_field", "DepthOfFieldU"),
            ("depth_haze", "DepthHazeU"),
        ]
    {
        let start = src
            .find(&format!("impl {uniform} {{"))
            .unwrap_or_else(|| panic!("no `impl {uniform}` in postprocess.rs - the scrape broke"));
        let body = &src[start..];
        let end = body[1..].find("\nimpl ").map(|i| i + 1).unwrap_or(body.len());
        let body = &body[..end];
        let mut keys: BTreeSet<String> = quoted_after(body, "get_f32(params, ").into_iter().collect();
        keys.extend(quoted_after(body, "get_vec4(params, "));
        assert!(!keys.is_empty(), "found no parameters for {effect} - the scrape broke");
        out.insert(effect, keys);
    }
    // Film grain is drawn by the output pass, which reads its parameters in
    // `execute` rather than through an effect uniform of its own.
    let at = src.find("e.name == \"film_grain\"").expect("film_grain in postprocess.rs - the scrape broke");
    let body = &src[at..];
    let body = &body[..body.find("});").expect("end of the film grain block")];
    let keys: BTreeSet<String> = quoted_after(body, "get_f32(&e.params, ").into_iter().collect();
    assert!(!keys.is_empty(), "found no parameters for film_grain - the scrape broke");
    out.insert("film_grain", keys);
    // FXAA likewise (Oct 2026).
    let at = src.find("e.name == \"fxaa\"").expect("fxaa in postprocess.rs - the scrape broke");
    let body = &src[at..];
    let body = &body[..body.find(");").expect("end of the fxaa block")];
    let keys: BTreeSet<String> = quoted_after(body, "get_f32(&e.params, ").into_iter().collect();
    assert!(!keys.is_empty(), "found no parameters for fxaa - the scrape broke");
    out.insert("fxaa", keys);
    // Paint and halftone likewise (Oct 2026); halftone reads colours too.
    for effect in ["paint", "halftone", "boil", "lens_rain"] {
        let at = src.find(&format!("e.name == \"{effect}\"")).unwrap_or_else(|| panic!("{effect} in postprocess.rs - the scrape broke"));
        let body = &src[at..];
        let body = &body[..body.find(");").unwrap_or_else(|| panic!("end of the {effect} block"))];
        let mut keys: BTreeSet<String> = quoted_after(body, "get_f32(&e.params, ").into_iter().collect();
        keys.extend(quoted_after(body, "get_vec4(&e.params, "));
        assert!(keys.contains("strength") || keys.contains("amount"), "found no parameters for {effect} - the scrape broke");
        out.insert(effect, keys);
    }
    // The palette reads its own parameters (`renderer::palette`).
    let pal = read("crates/jengine-core/src/renderer/palette.rs");
    let keys: BTreeSet<String> = quoted_after(&pal, "num(p, ").into_iter().collect();
    assert!(keys.contains("preset"), "found no parameters for palette - the scrape broke");
    out.insert("palette", keys);
    out
}

/// Parameter names the editor panel writes, by effect.
fn editor_params() -> BTreeMap<String, BTreeSet<String>> {
    let src = read("editor/src/lib/postProcess.ts");
    let table_start = src.find("export const PP_EFFECTS").expect("PP_EFFECTS in postProcess.ts");
    let table = &src[table_start..];
    let table = &table[..table.find("\n];").expect("end of PP_EFFECTS")];
    let mut out = BTreeMap::new();
    // Each effect's block starts at `name: "<effect>"`.
    let names = quoted_after(table, "name: ");
    for (i, name) in names.iter().enumerate() {
        let from = table.find(&format!("name: \"{name}\"")).unwrap();
        let to = names
            .get(i + 1)
            .map(|next| table.find(&format!("name: \"{next}\"")).unwrap())
            .unwrap_or(table.len());
        let block = &table[from..to];
        out.insert(name.clone(), quoted_after(block, "key: ").into_iter().collect::<BTreeSet<_>>());
    }
    assert!(out.len() >= 4, "found {} effects in PP_EFFECTS - the scrape broke", out.len());
    out
}

#[test]
fn every_editor_parameter_is_one_the_renderer_reads() {
    let renderer = renderer_params();
    let editor = editor_params();
    let mut wrong = Vec::new();
    for (effect, keys) in &editor {
        let Some(known) = renderer.get(effect.as_str()) else {
            wrong.push(format!("effect `{effect}` - the renderer has no such effect"));
            continue;
        };
        for key in keys {
            if !known.contains(key) {
                wrong.push(format!("{effect}.{key} (the renderer reads {known:?})"));
            }
        }
    }
    assert!(wrong.is_empty(), "the editor's post panel writes parameters nothing reads:\n  {}", wrong.join("\n  "));
}

#[test]
fn every_renderer_effect_has_a_panel() {
    let editor = editor_params();
    for effect in renderer_params().keys() {
        assert!(editor.contains_key(*effect), "`{effect}` has no row in the editor's post panel");
    }
}
