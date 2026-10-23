//! Every runtime must draw normal maps, and resolve their textures the same
//! way.
//!
//! Since Sep 2026 a normal-mapped sprite (`lit_sprite`, or any material with a
//! `normal_map_id`) is drawn by the ordinary sprite pass, in z order, and its
//! normal by `render_normals_wgpu` into the normal buffer the lights read. It
//! used to have a forward pass of its own, drawn after every other sprite - so
//! a normal-mapped floor covered whatever stood on it. The history below is
//! that pass's, and the lessons carry over to the normal pass unchanged.
//!
//! # Why a source scan
//!
//! The regular sprite pass skips every entity whose material is `lit_sprite`,
//! on the understanding that a dedicated lit pass draws it. **Only the native
//! runtime had one.** In the editor and on the web those sprites were never
//! drawn at all - not dim, absent - and the failure disguised itself well:
//! test-lab scene 12 still showed its point lights glowing on the background,
//! which reads convincingly as "the normal-map effect, but subtle". It was
//! reported as exactly that, twice.
//!
//! Nothing about a missing pass fails to compile, and the pass itself is
//! correct wherever it runs. Only the call sites can be checked.
//!
//! The one runtime that did draw them got normal maps wrong in a way this file
//! also guards: it resolved them through the colour upload path, which is sRGB,
//! so the shader gamma-decoded every vector. `render_golden`'s
//! `a_flat_normal_map_lights_like_no_normal_map` checks the pixels; this checks
//! that nobody routes around the one function that uploads them correctly.

mod support;

static NATIVE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs")));
static VIEWPORT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs")));
static WEB: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs")));

fn runtimes() -> [(&'static str, &'static str); 3] {
    [("native", NATIVE.as_str()), ("viewport", VIEWPORT.as_str()), ("web", WEB.as_str())]
}

#[test]
fn every_runtime_draws_normal_maps() {
    for (name, src) in runtimes() {
        assert!(
            src.contains("render_normals_wgpu(") && src.contains("needs_normal_pass("),
            "{name} has no normal pass, so its normal buffer is always flat and \
             every normal map there shades like a plain sprite - which looks \
             like a subtle effect rather than a missing one."
        );
        assert!(
            !src.contains("render_lit_sprites_wgpu"),
            "{name} still calls the old forward lit pass, which draws normal-mapped \
             sprites a second time, over everything"
        );
    }
}

/// The normal pass runs before the lights read the buffer, every frame.
#[test]
fn the_normal_pass_comes_before_the_lights() {
    for (name, src) in runtimes() {
        let normals = src.find("render_normals_wgpu(").unwrap_or_else(|| panic!("{name}: no normal pass"));
        let lights = src.find(".render_lights(").unwrap_or_else(|| panic!("{name}: no render_lights"));
        assert!(
            normals < lights,
            "{name}: the normal pass runs after the lights, so they read last frame's normals"
        );
    }
}

#[test]
fn every_runtime_resolves_lit_textures_through_the_shared_helper() {
    for (name, src) in runtimes() {
        assert!(
            src.contains("lit_sprite_textures("),
            "{name} resolves lit-sprite textures itself instead of through \
             `lit_sprite_textures`. That is how normal maps came to be uploaded \
             as sRGB colour in the one runtime that drew them at all."
        );
    }
}

/// A normal map must never reach the colour upload path by name.
///
/// The shape of the original bug, spelled out so it cannot come back quietly:
/// a normal-map id handed to `ensure_wgpu_texture`, which uploads sRGB.
#[test]
fn no_runtime_uploads_a_normal_map_as_colour() {
    for (name, src) in runtimes() {
        for needle in ["ensure_wgpu_texture(nid", "ensure_wgpu_texture(normal_id"] {
            assert!(
                !src.contains(needle),
                "{name} uploads a normal map through `ensure_wgpu_texture`, which \
                 is sRGB - every normal would be gamma-decoded and lean ~40 degrees. \
                 Use `ensure_wgpu_normal_texture` (via `lit_sprite_textures`)."
            );
        }
    }
}

/// And every host must fetch a normal map's bytes in the first place.
///
/// Drawing the pass is not enough. After the editor and web gained it, scene
/// 12's bump row still shaded flat there, because neither ever loaded the
/// image a material names in `normal_map_id` - native preloaded it alongside
/// sprite textures, the editor's `loadSceneTextures` and web's
/// `wanted_texture_ids` only looked at sprites. The lit pass then fell back to
/// the flat normal, which is indistinguishable from "no effect".
#[test]
fn every_host_loads_normal_maps() {
    const EDITOR_LOADER: &str = include_str!("../../../editor/src/lib/Viewport.svelte");
    // Native and web preload through one shared list since Sep 2026 - their
    // own copies had drifted - so the field is read there, and both call it.
    // LF, whatever git checked out, and searched with an escape rather than a
    // literal line break: on a CRLF checkout the old search found nothing.
    let shared = include_str!("../src/scene/format.rs").replace("\r\n", "\n");
    let list = &shared[shared.find("pub fn world_texture_ids").expect("the shared list")..];
    // The reading of the field, not any mention of it - a doc comment naming
    // `normal_map_id` would satisfy a looser check while loading nothing.
    assert!(
        list[..list.find("\n}\n").unwrap()].contains("m.normal_map_id.as_deref()"),
        "`world_texture_ids` no longer collects a material's `normal_map_id`"
    );
    for (name, src, needle) in [
        ("native", NATIVE.as_str(), "world_texture_ids(world)"),
        ("web", WEB.as_str(), "world_texture_ids(world)"),
        ("editor", EDITOR_LOADER, "material?.normal_map_id"),
    ] {
        assert!(
            src.contains(needle),
            "{name} never collects `normal_map_id` when deciding which textures to \
             load, so a normal-mapped sprite shades against the flat fallback there \
             and looks exactly like a plain one"
        );
    }
}

/// Lit sprites, UI and GUI must be drawn after the lighting composite.
///
/// The composite multiplies the scene target by the light buffer - ambient plus
/// every light. Everything drawn into that target before it runs is lit by it.
/// That is right for ordinary sprites and world text. It was also where three
/// things were drawn that must not be:
///
/// * `lit_sprite` materials light themselves per pixel, so they were lit
///   **twice**: at ambient 0.15 a sprite away from any light came out at
///   0.15 x 0.15, about 2%. Scene 12 went black everywhere a light was not
///   directly overhead.
/// * Screen-space UI and GUI documents have no position in the lit world, yet
///   were darkened by whatever light fell on that part of the screen.
///
/// Every runtime drew all three into the one pass, then composited. This checks
/// the order in the source, since the order *is* the fix.
#[test]
fn the_self_lit_and_screen_space_passes_come_after_the_composite() {
    for (name, src) in runtimes() {
        let at = |needle: &str| {
            src.find(needle)
                .unwrap_or_else(|| panic!("{name}: `{needle}` not found"))
        };
        // The lighting composite call, however rustfmt wrapped it: the first
        // `.composite(` whose arguments start with the frame encoder.
        let composite = src
            .match_indices(".composite(")
            .find(|(i, _)| {
                src[*i..]
                    .chars()
                    .take(80)
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<String>()
                    .contains("composite(&mutframe.encoder")
            })
            .map(|(i, _)| i)
            .unwrap_or_else(|| panic!("{name}: no lighting composite call found"));
        assert!(
            // The world's collection opens the frame's sprite pass (one
            // collection, drawn per layer, since Oct 2026).
            at("render_world_items(") < composite,
            "{name}: world sprites and text (one interleaved pass since Sep 2026) are drawn after the composite, so they would never be lit"
        );
        // `Unlit` sprites and text - the opt-out for world content that must
        // not be darkened (scene 28's captions) - are drawn into the emission
        // buffer, which the composite adds after the light, so it has to be
        // drawn before the composite reads it.
        assert!(
            at("emission_pass(") < composite,
            "{name}: the emission pass runs after the composite, so unlit things are a frame late or never shown"
        );
        for pass in ["render_ui_wgpu(", "render_gui_wgpu("] {
            assert!(
                composite < at(pass),
                "{name}: `{pass}` runs before the lighting composite, so its output \
                 is multiplied by the light buffer - lit sprites get lit twice and \
                 screen-space UI is darkened by world lights"
            );
        }
    }
}
