//! Every runtime must draw baked lightmaps, inside its lighting pass, and
//! load their textures.
//!
//! `LightmapComponent` shipped documented as drawn into the light buffer and
//! was drawn by nothing (BUGS #95). `render_golden`'s
//! `a_lightmap_lights_what_is_under_it` checks the pixels; this checks each
//! runtime makes the call, in the one place it works - after the lights are
//! accumulated and before the scene is multiplied by them - and fetches the
//! texture, which no other component in the scene names.

mod support;

static NATIVE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs")));
static VIEWPORT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs")));
static WEB: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs")));
const EDITOR_LOADER: &str = include_str!("../../../editor/src/lib/Viewport.svelte");

#[test]
fn every_runtime_draws_lightmaps_between_the_lights_and_the_composite() {
    for (name, src) in [("native", NATIVE.as_str()), ("viewport", VIEWPORT.as_str()), ("web", WEB.as_str())] {
        let at = |needle: &str| {
            src.find(needle).unwrap_or_else(|| panic!("{name}: `{needle}` not found"))
        };
        let (lights, maps, composite) = (
            at(".render_lights("),
            at("render_lightmaps_wgpu("),
            at(".composite("),
        );
        assert!(
            lights < maps && maps < composite,
            "{name}: lightmaps must be drawn after `render_lights` (which clears the light \
             buffer to ambient) and before `composite` (which reads it)"
        );
        // A lightmap with a graph material is drawn through the graph
        // (`lightmap_graph_materials` checks the pixels).
        let window = &src[maps..(maps + 2500).min(src.len())];
        assert!(
            window.contains("|batch, tex_id, material|")
                && window.contains("material_pipeline(")
                && window.contains("flush_with_pipeline("),
            "{name}: a lightmap's graph material is ignored - it draws as a plain texture"
        );
    }
}

/// Native and web preload through the one shared list,
/// `scene::format::world_texture_ids` - their own copies had drifted apart -
/// so the lightmap has to be in that list and both have to call it.
const SHARED_LIST: &str = include_str!("../src/scene/format.rs");

#[test]
fn every_host_loads_lightmap_textures() {
    // Normalised, and searched with escapes rather than a literal line
    // break: git checks the source out with CRLF on Windows, where a string
    // holding a real newline found nothing and the unwrap panicked.
    let shared = SHARED_LIST.replace("\r\n", "\n");
    let list = &shared[shared.find("pub fn world_texture_ids").expect("the shared list")..];
    assert!(
        list[..list.find("\n}\n").unwrap()].contains("LightmapComponent>()"),
        "`world_texture_ids` no longer collects a lightmap's `texture_id`"
    );
    for (name, src, needle) in [
        ("native", NATIVE.as_str(), "world_texture_ids(world)"),
        ("web", WEB.as_str(), "world_texture_ids(world)"),
        ("editor", EDITOR_LOADER, "lightmap?.texture_id"),
    ] {
        assert!(
            src.contains(needle),
            "{name} never collects a lightmap's `texture_id` when deciding which textures to \
             load, so the lightmap draws nothing there"
        );
    }
}
