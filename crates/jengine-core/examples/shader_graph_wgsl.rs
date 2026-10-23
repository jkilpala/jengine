//! Print the WGSL a `.shader` graph generates - what the pipeline cache
//! compiles for it.
//!
//! `cargo run -p jengine-core --example shader_graph_wgsl -- path/to/x.shader`
//!
//! For reading why a graph draws what it draws: the generated code names
//! every node's output `n<id>_<pin>`, so a wrong wire is findable.

fn main() {
    let path = std::env::args().nth(1).expect("usage: shader_graph_wgsl <file.shader>");
    let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let graph: jengine_core::renderer::shader_graph::ShaderGraph =
        serde_json::from_str(&json).unwrap_or_else(|e| panic!("parse {path}: {e}"));
    match jengine_core::renderer::shader_codegen::generate(&graph) {
        Ok(g) => print!("{}", g.wgsl),
        Err(e) => {
            eprintln!("{path}: {e:#}");
            std::process::exit(1);
        }
    }
}
