//! jEngine Build Pipeline — packages games for Windows (.exe), Web (WASM) and
//! Android (.apk).

pub mod android;
pub mod android_smoke;
pub mod app;
pub mod asset_packs;
pub mod liveops_publish;
pub mod asset_pipeline;
pub mod compression;
pub mod desktop;
pub mod engine_root;
pub mod ios;
pub mod native_run;
pub mod new_project;
pub mod packages;
pub mod packer;
pub mod progress;
pub mod psd;
pub mod psd_import;
pub mod secret_store;
pub mod servers;
pub mod report;
pub mod stage;
pub mod viewport;
pub mod web;
pub mod web_platform;
pub mod wasm_opt;
pub mod windows;

/// Where cargo leaves the artifacts of a build run in `workspace`:
/// `CARGO_TARGET_DIR` when set (relative to the workspace if relative),
/// else `<workspace>/target`. Everything that looks for a binary cargo just
/// built must ask here - with a target directory on another drive, a fixed
/// `<workspace>/target` found nothing and the build failed after compiling.
pub fn cargo_target_dir(workspace: &std::path::Path) -> std::path::PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR").filter(|v| !v.is_empty()) {
        Some(dir) => workspace.join(dir),
        None => workspace.join("target"),
    }
}

/// Stamp `settings.debug_overlay` into a project document for a build.
///
/// The F3 stats HUD / F12 inspector are development tools, so a shipping
/// build can turn them off. Both runtimes read this key from the
/// `project.jengine` they ship beside and default to enabled when it is
/// absent, so an older project keeps the overlay.
///
/// No-op on a document with no `settings` object - there would be nothing
/// to read the flag out of at runtime either.
pub fn set_debug_overlay(doc: &mut serde_json::Value, enabled: bool) {
    if let Some(settings) = doc.get_mut("settings").and_then(|s| s.as_object_mut()) {
        settings.insert(
            "debug_overlay".to_string(),
            serde_json::Value::Bool(enabled),
        );
    }
}

/// Graphics backend selection, shared by the platform builders.
///
/// wgpu compiles one code path per graphics API. A project names the ones
/// its target should carry (`settings.graphics_backends.<platform>`), and
/// this maps them onto the `backend-*` cargo features that `jengine-core`
/// exposes. Unknown names are ignored rather than failing a build, so a
/// project file written by a newer editor still builds here.
pub mod graphics {
    /// Map backend names to the cargo features of the crate being built.
    ///
    /// `web` selects features on `jengine-web`, native on `jengine-runtime`;
    /// both forward to the same `jengine-core/backend-*` features.
    pub fn features_for(backends: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for name in backends {
            let feature = match name.trim().to_ascii_lowercase().as_str() {
                "webgpu" => "backend-webgpu",
                "webgl" => "backend-webgl",
                "dx12" | "directx12" => "backend-dx12",
                "vulkan" => "backend-vulkan",
                "metal" => "backend-metal",
                other => {
                    log::warn!("unknown graphics backend '{other}' - ignored");
                    continue;
                }
            };
            if !out.iter().any(|f| f == feature) {
                out.push(feature.to_string());
            }
        }
        out
    }

    /// Read `settings.graphics_backends.<platform>` from a project file.
    /// Returns `None` when the project does not pin any, so the caller can
    /// fall back to the crate's own defaults.
    pub fn backends_for_platform(
        project_path: &std::path::Path,
        platform: &str,
    ) -> Option<Vec<String>> {
        let text = std::fs::read_to_string(project_path.join("project.jengine")).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        let list = value
            .get("settings")?
            .get("graphics_backends")?
            .get(platform)?
            .as_array()?;
        let names: Vec<String> = list
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        if names.is_empty() { None } else { Some(names) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_overlay_flag_is_written_both_ways() {
        let mut doc = serde_json::json!({ "settings": { "name": "g" } });
        set_debug_overlay(&mut doc, false);
        assert_eq!(doc["settings"]["debug_overlay"], serde_json::json!(false));
        set_debug_overlay(&mut doc, true);
        assert_eq!(doc["settings"]["debug_overlay"], serde_json::json!(true));
        // Nothing else in the document is disturbed.
        assert_eq!(doc["settings"]["name"], serde_json::json!("g"));
    }

    #[test]
    fn debug_overlay_flag_skips_a_document_without_settings() {
        // Runtimes read the key out of `settings`; with no such object
        // there is nowhere to put it, and inventing one would be a
        // surprising edit to someone else's project file.
        let mut doc = serde_json::json!({ "packages": [] });
        set_debug_overlay(&mut doc, false);
        assert!(doc.get("settings").is_none());
    }
}
