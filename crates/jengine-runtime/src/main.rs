//! The stock jEngine runtime binary.
//!
//! Everything lives in the library beside this file. A project that ships its
//! own Rust packages gets a *generated* main of the same shape, with
//! `run_with` and its packages — see `jengine-builder`'s project-runtime
//! codegen and `docs/writing-packages.md`.

fn main() -> anyhow::Result<()> {
    jengine_runtime::run()
}
