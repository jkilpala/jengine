//! Importing a layered Photoshop document (`jengine_builder::psd_import`):
//! a sprite per layer and a prefab of them, from the asset browser.

use std::path::Path;

use jengine_builder::psd_import::{ImportOptions, ImportReport, Origin, import_psd};

/// Import `relative_path` (project-relative, under `assets/`). `origin` is
/// `"bottom"` for the canvas's bottom middle, else its centre. Async, as a
/// big document takes a moment and a sync command would hold the window.
#[tauri::command(async)]
pub fn import_psd_layers(
    project_path: String,
    relative_path: String,
    origin: Option<String>,
    include_hidden: Option<bool>,
) -> Result<ImportReport, String> {
    let options = ImportOptions {
        origin: if origin.as_deref() == Some("bottom") { Origin::Bottom } else { Origin::Centre },
        include_hidden: include_hidden.unwrap_or(false),
    };
    import_psd(&Path::new(&project_path).join(relative_path), &options).map_err(|e| format!("{e:#}"))
}
