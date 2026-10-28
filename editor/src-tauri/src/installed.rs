//! What an **installed** editor tells the builder at start: where its
//! viewport modules are (`viewport_module.rs` serves them) and the engine
//! source it ships, which builds compile from when the user has no engine
//! checkout of their own (`jengine_builder::engine_root`).
//!
//! Nothing here runs in `cargo tauri dev`, which runs inside a checkout.

use tauri::Manager;

/// The engine source in the editor's resources, made by `jengine engine-bundle`
/// in `build-editor-release.ps1` / `.sh`.
const ENGINE_SOURCE: &str = "engine-source.zip";

pub fn setup(app: &tauri::App) {
    if tauri::is_dev() {
        return;
    }
    let paths = app.path();
    let (local, resources) = match (paths.app_local_data_dir(), paths.resource_dir()) {
        (Ok(local), Ok(resources)) => (local, resources),
        (local, resources) => {
            log::error!("No folders for an installed editor (local data: {local:?}, resources: {resources:?})");
            return;
        }
    };
    jengine_builder::viewport::use_module_dirs(jengine_builder::viewport::ModuleDirs {
        active: local.join("viewport"),
        stock: resources.join("viewport"),
    });
    let archive = resources.join(ENGINE_SOURCE);
    if archive.is_file() {
        // One folder across updates: a new archive is unpacked over it and
        // its `target/` - the build cache - is kept.
        jengine_builder::engine_root::use_bundle(jengine_builder::engine_root::Bundle {
            archive,
            into: local.join("engine"),
        });
    } else {
        log::warn!("This editor ships no engine source ({} is missing)", archive.display());
    }
}
