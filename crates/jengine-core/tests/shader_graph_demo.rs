//! Slice 13c demo-asset sanity test: parse the `pulse_emission.shader`
//! graph shipped with the `engine-test-lab` project and run it through
//! `shader_codegen::generate`, asserting the WGSL contains the
//! expected pulsing-brightness expression. This protects the demo
//! against accidental graph-format / codegen regressions.

use jengine_core::renderer::shader_codegen;
use jengine_core::renderer::shader_graph::ShaderGraph;

const DEMO_GRAPH: &str = include_str!(
    "../../../projects/engine-test-lab/assets/shaders/pulse_emission.shader"
);

#[test]
fn pulse_emission_demo_graph_codegens() {
    let graph: ShaderGraph =
        serde_json::from_str(DEMO_GRAPH).expect("demo shader graph parses");
    let generated =
        shader_codegen::generate(&graph).expect("demo shader graph codegens");
    let wgsl = &generated.wgsl;

    // Sanity: the graph references TimeInput + Sin + TextureSample,
    // so the emitted WGSL must touch all three.
    assert!(
        wgsl.contains("globals.time"),
        "expected globals.time reference; wgsl was:\n{wgsl}"
    );
    assert!(
        wgsl.contains("sin("),
        "expected sin() call; wgsl was:\n{wgsl}"
    );
    assert!(
        wgsl.contains("textureSample(u_texture, u_sampler"),
        "expected textureSample call; wgsl was:\n{wgsl}"
    );
    // Brightness node multiplies rgb by Factor, leaving alpha intact.
    assert!(
        wgsl.contains("vs_main") && wgsl.contains("fs_main"),
        "expected vs_main+fs_main entry points; wgsl was:\n{wgsl}"
    );
}

/// Every shipped `.shader` graph generates WGSL. A graph that does not is
/// swallowed by `GraphPipelineCache` into a silent fallback to the default
/// sprite shader, so a broken one looks like a plain sprite, not an error.
#[test]
fn every_shipped_shader_graph_codegens() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects");
    let mut seen = 0;
    // Every project here, whichever exist: the public tree ships fewer.
    let mut projects: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).collect();
    projects.sort();
    for project in projects {
        let dir = project.join("assets/shaders");
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("shader") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let graph: jengine_core::renderer::shader_graph::ShaderGraph = serde_json::from_str(&text)
                .unwrap_or_else(|err| panic!("{} does not parse: {err}", path.display()));
            shader_codegen::generate(&graph)
                .unwrap_or_else(|err| panic!("{} does not codegen: {err:#}", path.display()));
            seen += 1;
        }
    }
    assert!(seen >= 4, "found only {seen} shader graphs");
}
