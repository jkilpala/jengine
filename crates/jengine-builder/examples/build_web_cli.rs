//! Run a web build from the terminal.
//!
//! The Build UI in the editor is the normal way in, which makes the web
//! pipeline awkward to exercise while working on it. This does the same
//! thing headlessly:
//!
//! ```text
//! cargo run -p jengine-builder --example build_web_cli -- <project-dir> <output-dir>
//! ```
//!
//! Engine root is the repo the example was built from (its workspace root).

use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut args = std::env::args().skip(1);
    let project: PathBuf = match args.next() {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("usage: build_web_cli <project-dir> <output-dir>");
            std::process::exit(2);
        }
    };
    let output: PathBuf = match args.next() {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("usage: build_web_cli <project-dir> <output-dir>");
            std::process::exit(2);
        }
    };
    // `<crate>/..` twice lands on the workspace root.
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("workspace root");

    // release, no report, then the three WASM size options - off, matching the
    // editor's defaults, so a build from here matches what the editor ships.
    // The web portal is the project's own (`settings.web_platform`).
    jengine_builder::web::build_web(&engine, &project, &output, true, false, false, false, false, jengine_builder::wasm_opt::WasmOpt::WEB_BUILD_DEFAULT, true, true, None)
}
