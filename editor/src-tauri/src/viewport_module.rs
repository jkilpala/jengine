//! The viewport module of an **installed** editor.
//!
//! In a checkout the editor imports its WASM viewport from
//! `editor/src/lib/viewport-wasm/` through Vite, and loading a project's own
//! build (one with its Rust packages linked in) is a copy into that folder and
//! a reload. An installed editor has no such folder - its frontend is built
//! into the executable - so its frontend is built with the module left out
//! (`vite.config.ts` turns every import of it into an import of
//! `jengine-viewport://localhost/...`) and this serves it: the project's
//! module from a writable folder in the app's local data when one has been
//! loaded, else the stock module the editor shipped with, from its resources.
//! `jengine_builder::viewport` is told the same two folders (`installed.rs`), so a rebuild or
//! a cached switch lands where this reads.

use tauri::http::{Request, Response, StatusCode, header};

/// The URI scheme the frontend imports the module from.
pub const SCHEME: &str = "jengine-viewport";

/// Answer one request for a module file.
pub fn respond(request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let name = request.uri().path().trim_start_matches('/');
    let Some(file) = jengine_builder::viewport::MODULE_FILES.iter().find(|f| **f == name) else {
        return status(StatusCode::NOT_FOUND, format!("no viewport file {name}"));
    };
    let Some(dir) = jengine_builder::viewport::serving_dir() else {
        return status(StatusCode::NOT_FOUND, "this editor imports its viewport through Vite".into());
    };
    let path = dir.join(file);
    match std::fs::read(&path) {
        Ok(bytes) => Response::builder()
            .header(
                header::CONTENT_TYPE,
                if name.ends_with(".wasm") { "application/wasm" } else { "text/javascript" },
            )
            // The page's origin is the app's, not this scheme's: a module
            // script and the glue's fetch of its wasm are both CORS requests.
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            // A reload after loading another project's module must fetch it.
            .header(header::CACHE_CONTROL, "no-store")
            .body(bytes)
            .expect("a response with static headers builds"),
        Err(e) => {
            log::error!("Reading the viewport module {}: {e}", path.display());
            status(StatusCode::NOT_FOUND, format!("{}: {e}", path.display()))
        }
    }
}

fn status(code: StatusCode, text: String) -> Response<Vec<u8>> {
    Response::builder()
        .status(code)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(text.into_bytes())
        .expect("a response with static headers builds")
}
