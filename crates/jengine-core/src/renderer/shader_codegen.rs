//! WGSL code generator — converts a [`ShaderGraph`] into a complete WGSL
//! module (vertex + fragment) that targets the wgpu sprite pipeline.
//!
//! The emitted module matches the bind-group layout used by graph-driven
//! materials in the wgpu backend:
//!
//! ```text
//! @group(0) @binding(0) var<uniform> globals: Globals;   // view_projection + time
//! @group(0) @binding(1) var u_texture: texture_2d<f32>;
//! @group(0) @binding(2) var u_sampler: sampler;
//! ```
//!
//! The vertex shader is the canonical sprite vertex shader (matches
//! `assets/shaders/wgsl/sprite.wgsl`), plus - when the graph has a
//! `VertexOffset` node - the offset that node is given, added to each vertex
//! before it is projected. Both halves are generated the same way: by
//! topologically sorting from their output node backward, emitting one `let`
//! binding per node output. The vertex half is a function of the same
//! `VsOut` the fragment half reads, filled in for that vertex, so every node
//! means the same thing on both sides.
//!
//! Note: graphs authored in the editor still use the `CustomGlsl` node
//! variant name for historical reasons, but the embedded code is now
//! treated as raw WGSL and pasted verbatim into the fragment shader.

use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};

use super::shader_graph::{NodeId, NodeType, PinType, ShaderGraph, node_pins};

/// Generated WGSL source for a shader graph.
#[derive(Debug, Clone)]
pub struct GeneratedShader {
    /// Complete WGSL module — vertex + fragment entry points in one file.
    pub wgsl: String,
}

/// Generate a WGSL module from a shader graph.
pub fn generate(graph: &ShaderGraph) -> Result<GeneratedShader> {
    let errors = graph.validate();
    if !errors.is_empty() {
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        bail!("Shader graph validation failed:\n{}", msgs.join("\n"));
    }

    let output = graph.output_node().ok_or_else(|| anyhow::anyhow!("No output node"))?.id;
    let fragment_body = generate_body(graph, output, "vec4<f32>(0.0, 0.0, 0.0, 1.0)")?;
    let vertex_body = match graph.vertex_offset_node() {
        Some(v) => Some(vertex_stage(&generate_body(graph, v.id, "vec2<f32>(0.0)")?)),
        None => None,
    };

    let mut wgsl = String::with_capacity(2048);
    wgsl.push_str(MODULE_PREAMBLE);
    match &vertex_body {
        None => wgsl.push_str(VERTEX_PLAIN),
        Some(body) => {
            wgsl.push_str(VERTEX_MOVED);
            wgsl.push_str("\n// === Generated vertex offset ===\n");
            wgsl.push_str("fn graph_offset(in: VsOut) -> vec2<f32> {\n");
            wgsl.push_str("    let uv = in.uvq.xy / in.uvq.z;\n");
            wgsl.push_str(body);
            wgsl.push_str("}\n\n");
        }
    }
    wgsl.push_str(MASK_WGSL);
    wgsl.push_str(INSTANCE_WGSL);
    // The scene copy, only for a graph that reads it - its pipeline has a
    // second bind group (`WgpuSpriteBatch::scene_bind_group_layout`).
    if uses_scene_color(graph) {
        wgsl.push_str(SCENE_BINDINGS);
    }
    // The procedural nodes' helpers, only when a node calls them.
    if graph.nodes.iter().any(|n| matches!(n.node_type, NodeType::Noise { .. } | NodeType::Voronoi)) {
        wgsl.push_str(NOISE_LIBRARY);
    }
    wgsl.push_str("\n// === Generated fragment shader ===\n");
    wgsl.push_str("fn graph_color(in: VsOut) -> vec4<f32> {\n");
    // The perspective divide, once; every node that reads the texture
    // coordinate reads `uv`. `q` is 1 on every sprite vertex, so for a
    // graph on a sprite this is `in.uvq.xy` exactly.
    wgsl.push_str("    let uv = in.uvq.xy / in.uvq.z;\n");
    wgsl.push_str(&fragment_body);
    wgsl.push_str("}\n\n");
    wgsl.push_str(FRAGMENT_ENTRIES);

    Ok(GeneratedShader { wgsl })
}

/// The fragment entry points: `fs_main` for every blend but multiply, and
/// `fs_premultiplied` for multiply, whose blend arithmetic assumes a
/// premultiplied colour (`renderer::backend::wgpu::sprite`,
/// `SHADOW_MULTIPLY_BLEND`). The graph itself is `graph_color`.
///
/// Both draw a **cutout** when the flush asks (`globals.time.y`,
/// `WgpuSpriteBatch::set_cutout`): black, at the graph's own alpha - how a lit
/// sprite hides unlit things behind it in the emission layer, and an unlit one
/// darkens the lit scene. A cutout used to go through the default pipeline,
/// which knows nothing of the graph, so a graph whose alpha is its shape (fog,
/// a dissolve) cut out its whole quad: the showcase's Day & Night fog blacked
/// out the pond and every lamp glow under it.
/// The sprite mask, shared with the default sprite shader (`renderer::mask`).
const MASK_WGSL: &str = include_str!("../../assets/shaders/wgsl/sprite_mask.wgsl");

/// The instanced sprite entry, `vs_instanced` - every graph has it, so a graph
/// material draws instanced sprites as the default shader does.
const INSTANCE_WGSL: &str = include_str!("../../assets/shaders/wgsl/sprite_instance.wgsl");

const FRAGMENT_ENTRIES: &str = "// A masked draw keeps what its mask covers (renderer::mask).
fn masked(in: VsOut, c: vec4<f32>) -> vec4<f32> {
    let keep = mask_coverage(in.world);
    if (keep <= 0.0) {
        discard;
    }
    return vec4<f32>(c.rgb, c.a * keep);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let c = masked(in, graph_color(in));
    if (globals.time.y > 0.5) {
        return vec4<f32>(0.0, 0.0, 0.0, c.a);
    }
    return c;
}

@fragment
fn fs_premultiplied(in: VsOut) -> @location(0) vec4<f32> {
    let c = masked(in, graph_color(in));
    if (globals.time.y > 0.5) {
        return vec4<f32>(0.0, 0.0, 0.0, c.a);
    }
    return vec4<f32>(c.rgb * c.a, c.a);
}
";

/// WGSL module preamble: bind group, vertex struct, vertex entry point.
///
/// Mirrors `assets/shaders/wgsl/sprite.wgsl` so graph-driven materials slot
/// into the same `WgpuSpriteBatch` vertex buffer layout. The Globals struct
/// adds a `time` scalar so `TimeInput` nodes resolve to a uniform value.
const MODULE_PREAMBLE: &str = r#"// Generated by jengine_core::renderer::shader_codegen — do not hand-edit.

struct Globals {
    view_projection: mat4x4<f32>,
    // x = seconds since runtime start (TimeInput nodes read globals.time.x);
    // y = 1 when this flush draws cutouts (black at the graph's alpha).
    time:            vec4<f32>,
    // This flush's material parameters (MaterialParam nodes), one vec4 per
    // name in sorted order - see renderer::material_params.
    params:          array<vec4<f32>, 4>,
    // This flush's sprite mask (renderer::mask, sprite_mask.wgsl); kind 0 is none.
    mask_xf:         array<vec4<f32>, 2>,
    mask_info:       vec4<f32>,
    mask_pts:        array<vec4<f32>, 8>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var u_texture: texture_2d<f32>;
@group(0) @binding(2) var u_sampler: sampler;

struct VsIn {
    @location(0) position: vec2<f32>,
    @location(1) uvq:      vec3<f32>,
    @location(2) color:    vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       uvq:           vec3<f32>,
    @location(1)       color:         vec4<f32>,
    // The vertex's world position, for PositionInput / ScreenPosition.
    @location(2)       world:         vec2<f32>,
};

// A world point to the screen, 0..1 with y down (WorldToScreen, ScreenPosition).
fn jg_world_to_screen(p: vec2<f32>) -> vec2<f32> {
    let c = globals.view_projection * vec4<f32>(p, 0.0, 1.0);
    let ndc = c.xy / c.w;
    return vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
}
"#;

/// The sprite vertex shader, for a graph that does not move its vertices.
/// The body is `sprite_vertex`, which `vs_main` and the instanced entry
/// (`sprite_instance.wgsl`) both run.
const VERTEX_PLAIN: &str = "
@vertex
fn vs_main(in: VsIn) -> VsOut {
    return sprite_vertex(in);
}

fn sprite_vertex(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip_position = globals.view_projection * vec4<f32>(in.position, 0.0, 1.0);
    out.uvq = in.uvq;
    out.color = in.color;
    out.world = in.position;
    return out;
}
";

/// The vertex shader of a graph with a `VertexOffset`: the vertex as the
/// fragment would see it, handed to the graph's offset, then moved and
/// projected. `PositionInput` on the colour side reads the moved position.
const VERTEX_MOVED: &str = "
@vertex
fn vs_main(in: VsIn) -> VsOut {
    return sprite_vertex(in);
}

fn sprite_vertex(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip_position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    out.uvq = in.uvq;
    out.color = in.color;
    out.world = in.position;
    out.world = in.position + graph_offset(out);
    out.clip_position = globals.view_projection * vec4<f32>(out.world, 0.0, 1.0);
    return out;
}
";

/// A generated body made fit for the vertex stage: WGSL has no implicit
/// derivatives there, so every `textureSample(t, s, uv)` becomes
/// `textureSampleLevel(t, s, uv, 0.0)` - the full-size mip.
fn vertex_stage(body: &str) -> String {
    const CALL: &str = "textureSample(";
    let mut out = String::with_capacity(body.len() + 64);
    let mut rest = body;
    while let Some(at) = rest.find(CALL) {
        out.push_str(&rest[..at]);
        out.push_str("textureSampleLevel(");
        rest = &rest[at + CALL.len()..];
        // Copy the arguments up to the call's own closing parenthesis.
        let mut depth = 1;
        let mut end = rest.len();
        for (i, ch) in rest.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push_str(&rest[..end]);
        out.push_str(", 0.0");
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Whether a graph reads the scene drawn under it (`NodeType::SceneColor`) -
/// and so needs the scene copy bound, and drawing after the rest of the world.
pub fn uses_scene_color(graph: &ShaderGraph) -> bool {
    graph.nodes.iter().any(|n| matches!(n.node_type, NodeType::SceneColor))
}

/// The scene copy a `SceneColor` node samples: group 1, which only a graph
/// that reads the scene has.
const SCENE_BINDINGS: &str = r#"
@group(1) @binding(0) var u_scene: texture_2d<f32>;
@group(1) @binding(1) var u_scene_sampler: sampler;
"#;

/// Helpers for the `Noise` and `Voronoi` nodes: a hash, value noise, fbm and
/// cellular noise. Hash-based rather than a texture, so a graph needs no
/// second binding and noise never tiles.
const NOISE_LIBRARY: &str = r#"
// === Procedural noise (Noise / Voronoi nodes) ===
// An integer hash of a lattice cell, 0..1. Integers, not a float trick like
// fract(p * 123.34): on large coordinates the GPU may round the same corner
// differently from its two neighbouring cells (fused multiply-add), so the
// corner hashes to two values and the field jumps along the lattice line -
// visible seams, found in the showcase's Shader Gallery.
fn jn_hash(c: vec2<i32>, seed: u32) -> f32 {
    var h = (bitcast<u32>(c.x) * 0x8da6b343u) ^ (bitcast<u32>(c.y) * 0xd8163841u) ^ (seed * 0xcb1ab31fu);
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h) * (1.0 / 4294967296.0);
}

// Gradient (Perlin-style) noise, about -0.7..0.7: a random direction per
// lattice corner, quintic-smoothed. Value noise was tried first and is blocky -
// its features line up with the lattice, which a colour ramp turns into
// rectangles.
fn jn_gradient(c: vec2<i32>, f: vec2<f32>) -> f32 {
    let a = jn_hash(c, 0u) * 6.2831853;
    return dot(vec2<f32>(cos(a), sin(a)), f);
}

fn jn_noise(p: vec2<f32>) -> f32 {
    let fl = floor(p);
    let c = vec2<i32>(fl);
    let f = p - fl;
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = jn_gradient(c, f);
    let b = jn_gradient(c + vec2<i32>(1, 0), f - vec2<f32>(1.0, 0.0));
    let d0 = jn_gradient(c + vec2<i32>(0, 1), f - vec2<f32>(0.0, 1.0));
    let d1 = jn_gradient(c + vec2<i32>(1, 1), f - vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(d0, d1, u.x), u.y);
}

// fbm, 0..1: each octave twice as fine, half as strong and turned by about 37
// degrees, so no octave's lattice lines up with another's. Stretched so a
// typical field spans most of 0..1 - gradient noise clusters about its middle.
fn jn_fbm(p: vec2<f32>, octaves: u32) -> f32 {
    let turn = mat2x2<f32>(0.8, 0.6, -0.6, 0.8);
    var sum = 0.0;
    var amp = 0.5;
    var norm = 0.0;
    var q = p;
    for (var i: u32 = 0u; i < octaves; i = i + 1u) {
        sum = sum + jn_noise(q) * amp;
        norm = norm + amp;
        amp = amp * 0.5;
        q = turn * q * 2.0 + vec2<f32>(17.1, 9.7);
    }
    return clamp(sum / max(norm, 1e-4) * 1.25 + 0.5, 0.0, 1.0);
}

// x = distance to the nearest cell point (F1), y = that cell's random value,
// z = F2 - F1: near zero along every border between two cells - the crisp
// outline caustics and force fields are drawn with.
fn jn_voronoi(p: vec2<f32>) -> vec3<f32> {
    let fl = floor(p);
    let c = vec2<i32>(fl);
    let f = p - fl;
    var best = 8.0;
    var second = 8.0;
    var cell = 0.0;
    for (var y: i32 = -1; y <= 1; y = y + 1) {
        for (var x: i32 = -1; x <= 1; x = x + 1) {
            let id = c + vec2<i32>(x, y);
            let pt = vec2<f32>(f32(x), f32(y)) + vec2<f32>(jn_hash(id, 1u), jn_hash(id, 2u));
            let d = length(pt - f);
            if (d < best) {
                second = best;
                best = d;
                cell = jn_hash(id, 3u);
            } else if (d < second) {
                second = d;
            }
        }
    }
    return vec3<f32>(best, cell, second - best);
}
"#;

// ─── Fragment body emission ───────────────────────────────────────────

/// The body of the function computing `output`'s input - the fragment's
/// colour, or the vertex's offset - ending in its `return`; `fallback` when
/// nothing is plugged in.
fn generate_body(graph: &ShaderGraph, output: NodeId, fallback: &str) -> Result<String> {
    let params = graph.param_names();
    if params.len() > crate::renderer::material_params::MAX_PARAMS {
        bail!(
            "a graph reads at most {} material parameters; this one names {}: {:?}",
            crate::renderer::material_params::MAX_PARAMS,
            params.len(),
            params
        );
    }
    let sorted = topological_sort(graph, output)?;

    let mut src = String::with_capacity(1024);
    let mut var_names: HashMap<NodeId, Vec<String>> = HashMap::new();
    // The type each node's outputs *actually* ended up with, which is not
    // always what `node_pins` declares - see `widens_with_its_inputs`.
    let mut resolved: HashMap<NodeId, Vec<PinType>> = HashMap::new();

    for &node_id in &sorted {
        let node = graph.get_node(node_id).unwrap();
        let (inputs_def, outputs_def) = node_pins(&node.node_type);

        // What is actually plugged into each input pin, before any coercion:
        // the expression and the type it really is.
        let mut sources: Vec<Option<(String, PinType)>> = Vec::with_capacity(inputs_def.len());
        for pin_idx in 0..inputs_def.len() {
            let Some(edge) = graph.input_edge(node_id, pin_idx) else {
                sources.push(None);
                continue;
            };
            let source_vars = var_names
                .get(&edge.from_node)
                .ok_or_else(|| anyhow::anyhow!("Missing source for node {}", edge.from_node))?;
            let source_var = source_vars
                .get(edge.from_pin)
                .ok_or_else(|| {
                    anyhow::anyhow!("Invalid pin {} on node {}", edge.from_pin, edge.from_node)
                })?
                .clone();
            // The resolved type where we have one, the declared type otherwise.
            let source_type = resolved
                .get(&edge.from_node)
                .and_then(|ts| ts.get(edge.from_pin))
                .copied()
                .unwrap_or_else(|| {
                    let source_node = graph.get_node(edge.from_node).unwrap();
                    let (_, outs) = node_pins(&source_node.node_type);
                    outs[edge.from_pin].pin_type
                });
            sources.push(Some((source_var, source_type)));
        }

        // **Componentwise math keeps the width of what it is given.** These
        // ops are elementwise in WGSL, so a `Multiply` of two vec4s should
        // stay a vec4 - but their pins declare `Float`, so coercing to the
        // declaration truncated both sides to `.x` and returned grey.
        // Promoting both operands to the wider of the two is valid WGSL for
        // every op in this set and leaves an all-scalar graph exactly as it
        // was.
        let widen = widens_with_its_inputs(&node.node_type).then(|| {
            sources
                .iter()
                .flatten()
                .map(|(_, t)| *t)
                .reduce(wider)
                .unwrap_or(PinType::Float)
        });

        // **Geometry works at the width it is given, too.** `Dot`, `Length`,
        // `Distance` and `Normalize` declare `vec4` pins, and a `vec2`
        // extended to a `vec4` is padded with an alpha of 1 - so the length
        // of `uv - 0.5` came out at least 1, and every radial effect built on
        // it (a ripple, a shockwave, a vignette) was flat. Their inputs are
        // taken at the widest width that reaches them instead; an all-scalar
        // one keeps the declared `vec4`, because WGSL's `dot` and `normalize`
        // take no scalars.
        let geometric = is_geometric(&node.node_type)
            .then(|| sources.iter().flatten().map(|(_, t)| *t).reduce(wider))
            .flatten()
            .filter(|t| *t != PinType::Float);

        let mut input_exprs: Vec<String> = Vec::with_capacity(inputs_def.len());
        for (pin_idx, pin_def) in inputs_def.iter().enumerate() {
            let target = widen.or(geometric).unwrap_or(pin_def.pin_type);
            match &sources[pin_idx] {
                Some((expr, ty)) => input_exprs.push(convert_type(expr, ty, &target)),
                None => input_exprs.push(default_value(&target)),
            }
        }

        let out_ty = widen;
        let output_vars =
            emit_node(node_id, &node.node_type, &input_exprs, &outputs_def, out_ty, &params, &mut src)?;
        // `Normalize` returns the width it was given; the other geometric
        // ops return their declared scalar.
        let out_ty = out_ty.or(geometric.filter(|_| matches!(node.node_type, NodeType::Normalize)));
        if let Some(t) = out_ty {
            resolved.insert(node_id, vec![t; output_vars.len()]);
        } else {
            resolved.insert(
                node_id,
                outputs_def.iter().map(|o| o.pin_type).collect(),
            );
        }
        var_names.insert(node_id, output_vars);
    }

    // The return is whatever the output node resolved to.
    if let Some(output_vars) = var_names.get(&output)
        && let Some(result) = output_vars.first()
    {
        src.push_str(&format!("    return {};\n", result));
    } else {
        src.push_str(&format!("    return {fallback};\n"));
    }

    Ok(src)
}

/// Topological sort via DFS from the output node, collecting reachable nodes
/// in dependency order.
fn topological_sort(graph: &ShaderGraph, output_id: NodeId) -> Result<Vec<NodeId>> {
    let mut visited = HashSet::new();
    let mut order = Vec::new();
    topo_dfs(graph, output_id, &mut visited, &mut order)?;
    Ok(order)
}

fn topo_dfs(
    graph: &ShaderGraph,
    node_id: NodeId,
    visited: &mut HashSet<NodeId>,
    order: &mut Vec<NodeId>,
) -> Result<()> {
    if visited.contains(&node_id) {
        return Ok(());
    }
    visited.insert(node_id);

    let node = graph
        .get_node(node_id)
        .ok_or_else(|| anyhow::anyhow!("Node {} not found", node_id))?;
    let (inputs, _) = node_pins(&node.node_type);

    for pin_idx in 0..inputs.len() {
        if let Some(edge) = graph.input_edge(node_id, pin_idx) {
            topo_dfs(graph, edge.from_node, visited, order)?;
        }
    }
    order.push(node_id);
    Ok(())
}

fn var_name(node_id: NodeId, pin_index: usize) -> String {
    format!("n{}_{}", node_id, pin_index)
}

/// Emit WGSL for a single node and return the variable names that other
/// nodes can read its outputs from.
fn emit_node(
    node_id: NodeId,
    node_type: &NodeType,
    inputs: &[String],
    outputs: &[super::shader_graph::PinDef],
    // `Some` for the elementwise math ops, carrying the width their operands
    // were promoted to; `None` for everything else, which keeps its declared
    // output type.
    widened: Option<PinType>,
    params: &[String],
    src: &mut String,
) -> Result<Vec<String>> {
    let math_ty = widened.map(|t| t.wgsl_type()).unwrap_or("f32");
    let _ = outputs;
    let mut out_vars = Vec::new();

    match node_type {
        // ── Input Nodes ────────────────────────────────────────────────
        NodeType::UvInput => {
            // `uv` is the perspective-divided coordinate the fragment
            // prologue computes from the projective vertex input.
            out_vars.push("uv".to_string());
        }
        NodeType::ColorInput { color } => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {} = vec4<f32>({:.6}, {:.6}, {:.6}, {:.6});\n",
                vn, color[0], color[1], color[2], color[3]
            ));
            out_vars.push(vn);
        }
        NodeType::VertexColorInput => {
            // The vertex colour is already interpolated into `VsOut.color` by
            // the shared preamble - the same channel `set_sprite_color` writes
            // and the default sprite shader multiplies by. Exposing it needs
            // no new binding and no change to how sprites batch.
            out_vars.push("in.color".to_string());
        }
        NodeType::TimeInput => {
            out_vars.push("globals.time.x".to_string());
        }
        NodeType::MaterialParam { name } => {
            let slot = params.iter().position(|p| p == name).unwrap_or(0);
            out_vars.push(format!("globals.params[{slot}]"));
        }
        NodeType::PositionInput => {
            out_vars.push("in.world".to_string());
        }
        NodeType::ScreenPosition => {
            out_vars.push("jg_world_to_screen(in.world)".to_string());
        }
        NodeType::WorldToScreen => {
            out_vars.push(format!("jg_world_to_screen({})", inputs[0]));
        }

        // ── Constants ──────────────────────────────────────────────────
        NodeType::FloatConstant { value } => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!("    let {}: f32 = {:.6};\n", vn, value));
            out_vars.push(vn);
        }
        NodeType::Vec2Constant { value } => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {} = vec2<f32>({:.6}, {:.6});\n",
                vn, value[0], value[1]
            ));
            out_vars.push(vn);
        }
        NodeType::Vec3Constant { value } => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {} = vec3<f32>({:.6}, {:.6}, {:.6});\n",
                vn, value[0], value[1], value[2]
            ));
            out_vars.push(vn);
        }
        NodeType::Vec4Constant { value } => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {} = vec4<f32>({:.6}, {:.6}, {:.6}, {:.6});\n",
                vn, value[0], value[1], value[2], value[3]
            ));
            out_vars.push(vn);
        }

        // ── Texture ────────────────────────────────────────────────────
        NodeType::TexelSize => {
            let vn = var_name(node_id, 1);
            src.push_str(&format!("    let {vn}: vec2<f32> = vec2<f32>(textureDimensions(u_texture));\n"));
            out_vars.push(format!("(vec2<f32>(1.0) / {vn})"));
            out_vars.push(vn);
        }
        NodeType::TextureSample => {
            let uv = inputs.first().map(|s| s.as_str()).unwrap_or("uv");
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {} = textureSample(u_texture, u_sampler, {});\n",
                vn, uv
            ));
            out_vars.push(vn.clone());
            // R, G, B, A channel outputs.
            out_vars.push(format!("({}).r", vn));
            out_vars.push(format!("({}).g", vn));
            out_vars.push(format!("({}).b", vn));
            out_vars.push(format!("({}).a", vn));
        }

        // ── Binary math (scalar) ───────────────────────────────────────
        NodeType::Add => emit_binop(node_id, "+", inputs, src, math_ty, &mut out_vars),
        NodeType::Subtract => emit_binop(node_id, "-", inputs, src, math_ty, &mut out_vars),
        NodeType::Multiply => emit_binop(node_id, "*", inputs, src, math_ty, &mut out_vars),
        NodeType::Divide => emit_binop(node_id, "/", inputs, src, math_ty, &mut out_vars),
        NodeType::Modulo => {
            // WGSL `%` works for floats (remainder).
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {}: f32 = ({}) - ({}) * floor(({}) / ({}));\n",
                vn, inputs[0], inputs[1], inputs[0], inputs[1]
            ));
            out_vars.push(vn);
        }
        NodeType::Power => emit_call1or2(node_id, "pow", &[&inputs[0], &inputs[1]], src, "f32", &mut out_vars),
        NodeType::Min => emit_call1or2(node_id, "min", &[&inputs[0], &inputs[1]], src, "f32", &mut out_vars),
        NodeType::Max => emit_call1or2(node_id, "max", &[&inputs[0], &inputs[1]], src, "f32", &mut out_vars),

        // ── Unary math ─────────────────────────────────────────────────
        NodeType::SquareRoot => emit_call1or2(node_id, "sqrt", &[&inputs[0]], src, "f32", &mut out_vars),
        NodeType::Abs => emit_call1or2(node_id, "abs", &[&inputs[0]], src, "f32", &mut out_vars),
        NodeType::Negate => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!("    let {}: f32 = -({});\n", vn, inputs[0]));
            out_vars.push(vn);
        }
        NodeType::Floor => emit_call1or2(node_id, "floor", &[&inputs[0]], src, "f32", &mut out_vars),
        NodeType::Ceil => emit_call1or2(node_id, "ceil", &[&inputs[0]], src, "f32", &mut out_vars),
        NodeType::Sin => emit_call1or2(node_id, "sin", &[&inputs[0]], src, "f32", &mut out_vars),
        NodeType::Cos => emit_call1or2(node_id, "cos", &[&inputs[0]], src, "f32", &mut out_vars),

        // ── Ternary math ───────────────────────────────────────────────
        NodeType::Clamp => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {}: f32 = clamp({}, {}, {});\n",
                vn, inputs[0], inputs[1], inputs[2]
            ));
            out_vars.push(vn);
        }
        NodeType::Mix | NodeType::ColorMix => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {} = mix({}, {}, {});\n",
                vn, inputs[0], inputs[1], inputs[2]
            ));
            out_vars.push(vn);
        }
        NodeType::Step => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {}: f32 = step({}, {});\n",
                vn, inputs[0], inputs[1]
            ));
            out_vars.push(vn);
        }
        NodeType::SmoothStep => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {}: f32 = smoothstep({}, {}, {});\n",
                vn, inputs[0], inputs[1], inputs[2]
            ));
            out_vars.push(vn);
        }

        // ── Vector ─────────────────────────────────────────────────────
        NodeType::Dot => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {}: f32 = dot({}, {});\n",
                vn, inputs[0], inputs[1]
            ));
            out_vars.push(vn);
        }
        NodeType::Length => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!("    let {}: f32 = length({});\n", vn, inputs[0]));
            out_vars.push(vn);
        }
        NodeType::Normalize => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!("    let {} = normalize({});\n", vn, inputs[0]));
            out_vars.push(vn);
        }
        NodeType::Distance => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {}: f32 = distance({}, {});\n",
                vn, inputs[0], inputs[1]
            ));
            out_vars.push(vn);
        }

        NodeType::SceneColor => {
            // The fragment's own pixel over the copy's size is its screen UV.
            // Alpha is 1: the scene is opaque, and the float target's alpha
            // is not - additive sprites add to it past 1, and an alpha-blended
            // sprite carrying that alpha would subtract what is under it.
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {}: vec4<f32> = vec4<f32>(textureSample(u_scene, u_scene_sampler, in.clip_position.xy / vec2<f32>(textureDimensions(u_scene)) + {}).rgb, 1.0);\n",
                vn, inputs[0]
            ));
            out_vars.push(vn);
        }
        NodeType::Noise { octaves } => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {}: f32 = jn_fbm({} * {}, {}u);\n",
                vn,
                inputs[0],
                inputs[1],
                (*octaves).clamp(1, 8)
            ));
            out_vars.push(vn);
        }
        NodeType::Voronoi => {
            let v = format!("n{}_vor", node_id);
            src.push_str(&format!("    let {}: vec3<f32> = jn_voronoi({} * {});\n", v, inputs[0], inputs[1]));
            let (d, c, e) = (var_name(node_id, 0), var_name(node_id, 1), var_name(node_id, 2));
            src.push_str(&format!("    let {}: f32 = {}.x;\n", d, v));
            src.push_str(&format!("    let {}: f32 = {}.y;\n", c, v));
            src.push_str(&format!("    let {}: f32 = {}.z;\n", e, v));
            out_vars.push(d);
            out_vars.push(c);
            out_vars.push(e);
        }

        NodeType::Combine { components } => {
            let vn = var_name(node_id, 0);
            match components {
                2 => {
                    src.push_str(&format!(
                        "    let {} = vec2<f32>({}, {});\n",
                        vn, inputs[0], inputs[1]
                    ));
                }
                3 => {
                    src.push_str(&format!(
                        "    let {} = vec3<f32>({}, {}, {});\n",
                        vn, inputs[0], inputs[1], inputs[2]
                    ));
                }
                _ => {
                    src.push_str(&format!(
                        "    let {} = vec4<f32>({}, {}, {}, {});\n",
                        vn, inputs[0], inputs[1], inputs[2], inputs[3]
                    ));
                }
            }
            out_vars.push(vn);
        }

        NodeType::Split => {
            let v = &inputs[0];
            out_vars.push(format!("({}).x", v));
            out_vars.push(format!("({}).y", v));
            out_vars.push(format!("({}).z", v));
            out_vars.push(format!("({}).w", v));
        }

        // ── Color operations ───────────────────────────────────────────
        NodeType::Brightness => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {vn} = vec4<f32>(({c}).rgb * ({a}), ({c}).a);\n",
                vn = vn,
                c = inputs[0],
                a = inputs[1]
            ));
            out_vars.push(vn);
        }
        NodeType::Saturation => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {vn}_lum = dot(({c}).rgb, vec3<f32>(0.2126, 0.7152, 0.0722));\n    let {vn} = vec4<f32>(mix(vec3<f32>({vn}_lum), ({c}).rgb, {a}), ({c}).a);\n",
                vn = vn,
                c = inputs[0],
                a = inputs[1]
            ));
            out_vars.push(vn);
        }
        NodeType::Contrast => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {vn} = vec4<f32>((({c}).rgb - vec3<f32>(0.5)) * ({a}) + vec3<f32>(0.5), ({c}).a);\n",
                vn = vn,
                c = inputs[0],
                a = inputs[1]
            ));
            out_vars.push(vn);
        }

        // ── Effects ────────────────────────────────────────────────────
        NodeType::Emission => {
            let vn = var_name(node_id, 0);
            src.push_str(&format!(
                "    let {vn} = ({c}) + ({e}) * ({s}) * ({c}).a;\n",
                vn = vn,
                c = inputs[0],
                e = inputs[1],
                s = inputs[2]
            ));
            out_vars.push(vn);
        }
        NodeType::Outline => {
            let vn = var_name(node_id, 0);
            // Sample 4 neighboring texels for a crude alpha-based outline.
            src.push_str(&format!(
                "    let {vn}_px = ({w}) / ({ts});\n    var {vn}_a = textureSample(u_texture, u_sampler, ({uv}) + vec2<f32>({vn}_px.x, 0.0)).a;\n    {vn}_a = {vn}_a + textureSample(u_texture, u_sampler, ({uv}) - vec2<f32>({vn}_px.x, 0.0)).a;\n    {vn}_a = {vn}_a + textureSample(u_texture, u_sampler, ({uv}) + vec2<f32>(0.0, {vn}_px.y)).a;\n    {vn}_a = {vn}_a + textureSample(u_texture, u_sampler, ({uv}) - vec2<f32>(0.0, {vn}_px.y)).a;\n    let {vn}_clamped = clamp({vn}_a, 0.0, 1.0);\n    let {vn}_base = textureSample(u_texture, u_sampler, {uv});\n    let {vn} = mix(({oc}) * {vn}_clamped, {vn}_base, {vn}_base.a);\n",
                vn = vn,
                uv = inputs[0],
                oc = inputs[1],
                w = inputs[2],
                ts = inputs[3]
            ));
            out_vars.push(vn);
        }

        // ── Output ─────────────────────────────────────────────────────
        NodeType::FragmentOutput | NodeType::VertexOffset => {
            if !inputs.is_empty() {
                out_vars.push(inputs[0].clone());
            }
        }

        // ── Custom code (now WGSL) ─────────────────────────────────────
        NodeType::CustomGlsl {
            code,
            inputs: custom_inputs,
            outputs: custom_outputs,
        } => {
            // The node's result variables, then its code in a block of its
            // own: inputs and outputs go by their pin names inside it, so the
            // code assigns `offset = ...` rather than guessing the variable a
            // node id makes, and two custom nodes may share pin names (at
            // function scope their `let`s collided, the WGSL did not compile,
            // and the graph silently fell back to the default shader).
            for pin in custom_outputs {
                src.push_str(&format!(
                    "    var n{}_{} : {} = {};\n",
                    node_id,
                    pin.name,
                    wgsl_type(&pin.pin_type),
                    default_value(&pin.pin_type)
                ));
            }
            src.push_str("    {\n");
            for (i, pin) in custom_inputs.iter().enumerate() {
                let value = inputs.get(i).cloned().unwrap_or_else(|| default_value(&pin.pin_type));
                src.push_str(&format!("        let {} : {} = {};\n", pin.name, wgsl_type(&pin.pin_type), value));
            }
            for pin in custom_outputs {
                src.push_str(&format!(
                    "        var {} : {} = {};\n",
                    pin.name,
                    wgsl_type(&pin.pin_type),
                    default_value(&pin.pin_type)
                ));
            }
            src.push_str("        // -- Custom WGSL --\n");
            for line in code.lines() {
                src.push_str(&format!("        {}\n", line));
            }
            src.push_str("        // -- End Custom --\n");
            for pin in custom_outputs {
                src.push_str(&format!("        n{}_{} = {};\n", node_id, pin.name, pin.name));
            }
            src.push_str("    }\n");
            for pin in custom_outputs {
                out_vars.push(format!("n{}_{}", node_id, pin.name));
            }
        }
    }

    Ok(out_vars)
}

/// Ops that are elementwise in WGSL, so their result is as wide as their
/// operands rather than the `Float` their pins declare.
///
/// Deliberately not every math node: `Clamp`, `Mix`, `SmoothStep` and `Step`
/// have mixed-arity signatures, and `Dot`, `Length` and `Distance` return a
/// scalar from vectors on purpose. Widening those would need a rule each, and
/// a wrong rule generates WGSL that does not compile - which the pipeline
/// cache swallows into a silent fallback.
fn widens_with_its_inputs(t: &NodeType) -> bool {
    matches!(
        t,
        NodeType::Add
            | NodeType::Subtract
            | NodeType::Multiply
            | NodeType::Divide
            | NodeType::Modulo
            | NodeType::Power
            | NodeType::Min
            | NodeType::Max
    )
}

/// The geometric ops: they take vectors of any width and must not have one
/// padded - see where their inputs are converted.
fn is_geometric(t: &NodeType) -> bool {
    matches!(t, NodeType::Dot | NodeType::Length | NodeType::Distance | NodeType::Normalize)
}

/// The wider of two pin types, by component count. A `Sampler2D` never
/// reaches here (it cannot connect to a math pin), and falls back to itself.
fn wider(a: PinType, b: PinType) -> PinType {
    if b.components() > a.components() { b } else { a }
}

fn emit_binop(
    node_id: NodeId,
    op: &str,
    inputs: &[String],
    src: &mut String,
    ty: &str,
    out_vars: &mut Vec<String>,
) {
    let vn = var_name(node_id, 0);
    src.push_str(&format!(
        "    let {}: {} = ({}) {} ({});\n",
        vn, ty, inputs[0], op, inputs[1]
    ));
    out_vars.push(vn);
}

fn emit_call1or2(
    node_id: NodeId,
    func: &str,
    args: &[&str],
    src: &mut String,
    ty: &str,
    out_vars: &mut Vec<String>,
) {
    let vn = var_name(node_id, 0);
    let arglist = args.join(", ");
    src.push_str(&format!(
        "    let {}: {} = {}({});\n",
        vn, ty, func, arglist
    ));
    out_vars.push(vn);
}

/// WGSL type name for a pin type.
fn wgsl_type(pin_type: &PinType) -> &'static str {
    match pin_type {
        PinType::Float => "f32",
        PinType::Vec2 => "vec2<f32>",
        PinType::Vec3 => "vec3<f32>",
        PinType::Vec4 => "vec4<f32>",
        PinType::Sampler2D => "sampler",
    }
}

/// Default literal for a pin type.
fn default_value(pin_type: &PinType) -> String {
    match pin_type {
        PinType::Float => "0.0".to_string(),
        PinType::Vec2 => "vec2<f32>(0.0)".to_string(),
        PinType::Vec3 => "vec3<f32>(0.0)".to_string(),
        PinType::Vec4 => "vec4<f32>(0.0, 0.0, 0.0, 1.0)".to_string(),
        PinType::Sampler2D => "u_sampler".to_string(),
    }
}

/// Adapt a WGSL expression from one pin type to another. WGSL is strictly
/// typed, so promotions/truncations must be explicit.
fn convert_type(expr: &str, from: &PinType, to: &PinType) -> String {
    if from == to {
        return expr.to_string();
    }
    match (from, to) {
        // Scalar → vector (splat).
        (PinType::Float, PinType::Vec2) => format!("vec2<f32>({})", expr),
        (PinType::Float, PinType::Vec3) => format!("vec3<f32>({})", expr),
        (PinType::Float, PinType::Vec4) => format!("vec4<f32>(vec3<f32>({}), 1.0)", expr),

        // Vector truncation.
        (PinType::Vec4, PinType::Vec3) => format!("({}).xyz", expr),
        (PinType::Vec4, PinType::Vec2) => format!("({}).xy", expr),
        (PinType::Vec3, PinType::Vec2) => format!("({}).xy", expr),

        // Vector extension.
        (PinType::Vec2, PinType::Vec3) => format!("vec3<f32>({}, 0.0)", expr),
        (PinType::Vec2, PinType::Vec4) => format!("vec4<f32>({}, 0.0, 1.0)", expr),
        (PinType::Vec3, PinType::Vec4) => format!("vec4<f32>({}, 1.0)", expr),

        // Vector → scalar: take .x.
        (PinType::Vec2, PinType::Float)
        | (PinType::Vec3, PinType::Float)
        | (PinType::Vec4, PinType::Float) => format!("({}).x", expr),

        // Sampler cannot convert.
        _ => expr.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `MaterialParam` reads its slot of `globals.params`, slots going to
    /// names in sorted order - the rule the sprite pass packs values by.
    #[test]
    fn material_params_read_their_slot_in_sorted_order() {
        let mut graph = ShaderGraph::new("params");
        let b = graph.add_node(NodeType::MaterialParam { name: "tint".into() }, [0.0, 0.0]);
        let _a = graph.add_node(NodeType::MaterialParam { name: "glow".into() }, [0.0, 1.0]);
        let out = graph.output_node().unwrap().id;
        graph.connect(b, 0, out, 0);
        assert_eq!(graph.param_names(), vec!["glow".to_string(), "tint".to_string()]);
        let wgsl = generate(&graph).expect("codegen").wgsl;
        assert!(wgsl.contains("params:          array<vec4<f32>, 4>"), "the globals carry the parameters");
        assert!(wgsl.contains("globals.params[1]"), "`tint` sorts second: {wgsl}");
    }

    #[test]
    fn a_graph_with_more_than_four_parameters_is_refused() {
        let mut graph = ShaderGraph::new("too many");
        let mut last = 0;
        for n in ["a", "b", "c", "d", "e"] {
            last = graph.add_node(NodeType::MaterialParam { name: n.into() }, [0.0, 0.0]);
        }
        let out = graph.output_node().unwrap().id;
        graph.connect(last, 0, out, 0);
        let err = generate(&graph).unwrap_err().to_string();
        assert!(err.contains("at most 4"), "{err}");
    }
    use crate::renderer::shader_graph::{GraphError, NodeType, ShaderGraph};

    #[test]
    fn basic_texture_passthrough_generates() {
        let mut graph = ShaderGraph::new("Basic Tint");
        let uv_id = graph.add_node(NodeType::UvInput, [0.0, 0.0]);
        let tex_id = graph.add_node(NodeType::TextureSample, [150.0, 0.0]);
        graph.connect(uv_id, 0, tex_id, 0);
        let output_id = graph.output_node().unwrap().id;
        graph.connect(tex_id, 0, output_id, 0);

        let shader = generate(&graph).expect("codegen succeeds");
        assert!(shader.wgsl.contains("@vertex"));
        assert!(shader.wgsl.contains("@fragment"));
        assert!(shader.wgsl.contains("textureSample(u_texture, u_sampler"));
        assert!(shader.wgsl.contains("return"));
    }

    #[test]
    fn time_input_references_globals_time() {
        let mut graph = ShaderGraph::new("Time wave");
        let time_id = graph.add_node(NodeType::TimeInput, [0.0, 0.0]);
        let sin_id = graph.add_node(NodeType::Sin, [100.0, 0.0]);
        graph.connect(time_id, 0, sin_id, 0);
        let output_id = graph.output_node().unwrap().id;
        graph.connect(sin_id, 0, output_id, 0);

        let shader = generate(&graph).expect("codegen succeeds");
        assert!(shader.wgsl.contains("globals.time"));
        assert!(shader.wgsl.contains("sin("));
    }

    #[test]
    fn disconnected_output_is_rejected() {
        let graph = ShaderGraph::new("Empty");
        let errors = graph.validate();
        assert!(errors.iter().any(|e| matches!(e, GraphError::OutputDisconnected)));
        assert!(generate(&graph).is_err());
    }

    #[test]
    fn cycle_is_rejected() {
        let mut graph = ShaderGraph::new("Cycle");
        let a = graph.add_node(NodeType::Add, [0.0, 0.0]);
        let b = graph.add_node(NodeType::Add, [100.0, 0.0]);
        graph.connect(a, 0, b, 0);
        graph.connect(b, 0, a, 0);
        let errors = graph.validate();
        assert!(errors.iter().any(|e| matches!(e, GraphError::Cycle)));
    }

    /// **The one per-entity value a graph can read.**
    ///
    /// Before `VertexColorInput` a graph's only dynamic inputs were `time`,
    /// the UV and the sampled texture, so a material could not depend on
    /// anything the game knew - which is the whole reason no game in this
    /// repo had used one. `MaterialComponent.uniforms` looks like the answer
    /// and is not: four script functions write it and no render path reads
    /// it, because the sprite bind group has three entries and none of them
    /// is a per-material uniform buffer.
    ///
    /// The vertex colour is already there. `set_sprite_color` writes it, the
    /// preamble interpolates it into `VsOut.color`, and the default sprite
    /// shader multiplies by it - so a graph can read it for free, with no new
    /// binding and no change to how sprites batch.
    #[test]
    fn a_graph_can_read_the_sprites_own_vertex_colour() {
        let mut graph = ShaderGraph::new("vertex colour");
        let vc = graph.add_node(NodeType::VertexColorInput, [0.0, 0.0]);
        let output_id = graph.output_node().unwrap().id;
        graph.connect(vc, 0, output_id, 0);

        let shader = generate(&graph).expect("codegen succeeds");
        assert!(
            shader.wgsl.contains("in.color"),
            "the generated shader never reads the vertex colour:
{}",
            shader.wgsl
        );
        // And the preamble really does carry it through to the fragment stage,
        // or `in.color` would not compile.
        assert!(shader.wgsl.contains("@location(1)       color:         vec4<f32>"));
        assert!(shader.wgsl.contains("out.color = in.color;"));
    }

    /// It needs no binding the sprite pipeline does not already have - which
    /// is the property that makes it cheap, and the one that would break if
    /// somebody "improved" it into a uniform.
    #[test]
    fn reading_the_vertex_colour_adds_no_binding() {
        let mut graph = ShaderGraph::new("vertex colour");
        let vc = graph.add_node(NodeType::VertexColorInput, [0.0, 0.0]);
        let output_id = graph.output_node().unwrap().id;
        graph.connect(vc, 0, output_id, 0);
        let shader = generate(&graph).expect("codegen succeeds");
        // Five: the globals, the texture and its sampler, and since Oct 2026
        // a texture mask's texture and sampler (`sprite_mask.wgsl`), which
        // every sprite flush binds.
        assert_eq!(
            shader.wgsl.matches("@group(0) @binding").count(),
            5,
            "the sprite pipeline binds exactly five things; a sixth would need the bind group layout changed in lockstep"
        );
    }

    /// **Elementwise math keeps the width of what it is given.**
    ///
    /// Their pins declare `Float`, and before Sep 2026 that declaration was
    /// the whole story: `convert_type` dutifully coerced both operands with
    /// `.x`, so a graph multiplying a sampled texture by the vertex colour
    /// emitted `((tex).x) * ((in.color).x)` and returned
    /// `vec4(vec3(that), 1.0)`. That compiles, builds a pipeline, and renders
    /// in **greyscale** - a tower-defence game's attackers came out grey, and it read
    /// as an art choice rather than as a bug.
    #[test]
    fn elementwise_math_keeps_the_width_of_its_inputs() {
        let mut graph = ShaderGraph::new("vec math");
        let uv = graph.add_node(NodeType::UvInput, [0.0, 0.0]);
        let tex = graph.add_node(NodeType::TextureSample, [1.0, 0.0]);
        let vc = graph.add_node(NodeType::VertexColorInput, [0.0, 1.0]);
        let mul = graph.add_node(NodeType::Multiply, [2.0, 0.0]);
        graph.connect(uv, 0, tex, 0);
        graph.connect(tex, 0, mul, 0);
        graph.connect(vc, 0, mul, 1);
        let out = graph.output_node().unwrap().id;
        graph.connect(mul, 0, out, 0);

        let wgsl = generate(&graph).expect("codegen succeeds").wgsl;
        assert!(
            wgsl.contains(") * (in.color);") && wgsl.contains(": vec4<f32> = ("),
            "the multiply did not stay a vec4:
{wgsl}"
        );
        assert!(
            !wgsl.contains(").x) * ((in.color).x)"),
            "both operands were truncated to their red channel:
{wgsl}"
        );
    }

    /// The length of `uv - 0.5` is the distance from the middle - not
    /// `length(vec4(uv - 0.5, 0.0, 1.0))`, which is at least 1 and made every
    /// radial effect flat (the showcase's Spellcraft shockwave never bent
    /// anything, and a lightmap's ripple drew no light at all).
    #[test]
    fn geometry_is_not_padded_with_an_alpha() {
        let mut graph = ShaderGraph::new("radial");
        let uv = graph.add_node(NodeType::UvInput, [0.0, 0.0]);
        let half = graph.add_node(NodeType::Vec2Constant { value: [0.5, 0.5] }, [0.0, 1.0]);
        let d = graph.add_node(NodeType::Subtract, [1.0, 0.0]);
        graph.connect(uv, 0, d, 0);
        graph.connect(half, 0, d, 1);
        let len = graph.add_node(NodeType::Length, [2.0, 0.0]);
        graph.connect(d, 0, len, 0);
        let dist = graph.add_node(NodeType::Distance, [2.0, 1.0]);
        graph.connect(uv, 0, dist, 0);
        graph.connect(half, 0, dist, 1);
        let dot = graph.add_node(NodeType::Dot, [2.0, 2.0]);
        graph.connect(d, 0, dot, 0);
        graph.connect(d, 0, dot, 1);
        let norm = graph.add_node(NodeType::Normalize, [2.0, 3.0]);
        graph.connect(d, 0, norm, 0);
        let split = graph.add_node(NodeType::Split, [3.0, 3.0]);
        graph.connect(norm, 0, split, 0);
        let sum = graph.add_node(NodeType::Add, [3.0, 0.0]);
        graph.connect(len, 0, sum, 0);
        graph.connect(dist, 0, sum, 1);
        let sum2 = graph.add_node(NodeType::Add, [4.0, 0.0]);
        graph.connect(sum, 0, sum2, 0);
        graph.connect(dot, 0, sum2, 1);
        let sum3 = graph.add_node(NodeType::Add, [5.0, 0.0]);
        graph.connect(sum2, 0, sum3, 0);
        graph.connect(split, 0, sum3, 1);
        let out = graph.output_node().unwrap().id;
        graph.connect(sum3, 0, out, 0);

        let wgsl = generate(&graph).expect("codegen succeeds").wgsl;
        for padded in ["length(vec4", "distance(vec4", "dot(vec4", "normalize(vec4"] {
            assert!(!wgsl.contains(padded), "a vec2 was padded into a vec4 with alpha 1 (`{padded}`):\n{wgsl}");
        }
        for call in ["length(n", "distance(uv, n", "dot(n", "normalize(n"] {
            assert!(wgsl.contains(call), "`{call}` should take the vec2 as it is:\n{wgsl}");
        }
        #[cfg(feature = "gpu-wgpu")]
        naga::front::wgsl::parse_str(&wgsl).expect("the generated WGSL parses");
    }

    /// Custom code names its inputs and outputs by their pins, and two custom
    /// nodes may use the same names.
    #[test]
    fn custom_code_is_scoped_and_assigns_by_pin_name() {
        use crate::renderer::shader_graph::CustomPin;
        let mut graph = ShaderGraph::new("custom");
        let uv = graph.add_node(NodeType::UvInput, [0.0, 0.0]);
        let custom = |code: &str| NodeType::CustomGlsl {
            code: code.into(),
            inputs: vec![CustomPin { name: "p".into(), pin_type: PinType::Vec2 }],
            outputs: vec![CustomPin { name: "k".into(), pin_type: PinType::Float }],
        };
        let a = graph.add_node(custom("k = p.x * 2.0;"), [0.0, 0.0]);
        let b = graph.add_node(custom("k = p.y + 1.0;"), [0.0, 0.0]);
        graph.connect(uv, 0, a, 0);
        graph.connect(uv, 0, b, 0);
        let sum = graph.add_node(NodeType::Add, [0.0, 0.0]);
        graph.connect(a, 0, sum, 0);
        graph.connect(b, 0, sum, 1);
        let out = graph.output_node().unwrap().id;
        graph.connect(sum, 0, out, 0);
        let wgsl = generate(&graph).expect("codegen succeeds").wgsl;
        assert!(wgsl.contains(&format!("n{a}_k = k;")), "{wgsl}");
        #[cfg(feature = "gpu-wgpu")]
        naga::front::wgsl::parse_str(&wgsl).expect("two custom nodes with the same pin names parse");
    }

    /// And a graph that only ever handles scalars is left exactly as it was -
    /// `pulse_emission.shader`, the Test Lab's original, must not change.
    #[test]
    fn all_scalar_math_is_still_emitted_as_f32() {
        let mut graph = ShaderGraph::new("scalar");
        let t = graph.add_node(NodeType::TimeInput, [0.0, 0.0]);
        let half = graph.add_node(NodeType::FloatConstant { value: 0.5 }, [0.0, 1.0]);
        let mul = graph.add_node(NodeType::Multiply, [1.0, 0.0]);
        graph.connect(t, 0, mul, 0);
        graph.connect(half, 0, mul, 1);
        let out = graph.output_node().unwrap().id;
        graph.connect(mul, 0, out, 0);

        let wgsl = generate(&graph).expect("codegen succeeds").wgsl;
        assert!(
            wgsl.contains(": f32 = (globals.time.x) * ("),
            "a float multiply should still be a float:
{wgsl}"
        );
        assert!(!wgsl.contains("vec4<f32> = (globals.time.x)"), "it must not have widened");
    }

    /// Mixed widths promote to the wider side rather than truncating to the
    /// narrower - `colour * scalar` is the commonest thing a material does.
    #[test]
    fn a_vector_times_a_scalar_stays_a_vector() {
        let mut graph = ShaderGraph::new("mixed");
        let vc = graph.add_node(NodeType::VertexColorInput, [0.0, 0.0]);
        let k = graph.add_node(NodeType::FloatConstant { value: 2.0 }, [0.0, 1.0]);
        let mul = graph.add_node(NodeType::Multiply, [1.0, 0.0]);
        graph.connect(vc, 0, mul, 0);
        graph.connect(k, 0, mul, 1);
        let out = graph.output_node().unwrap().id;
        graph.connect(mul, 0, out, 0);

        let wgsl = generate(&graph).expect("codegen succeeds").wgsl;
        assert!(
            wgsl.contains(": vec4<f32> = (in.color) * ("),
            "vec4 * f32 should be a vec4:
{wgsl}"
        );
    }

    /// The ops that are **not** widened, because their signatures are not
    /// elementwise. `Dot` returning a scalar from two vectors is the point of
    /// `Dot`, and widening `SmoothStep` would generate WGSL that does not
    /// compile - which the pipeline cache swallows into a silent fallback to
    /// the default sprite shader.
    #[test]
    fn nothing_but_the_elementwise_ops_widens() {
        for t in [
            NodeType::Dot,
            NodeType::Length,
            NodeType::Distance,
            NodeType::Clamp,
            NodeType::Mix,
            NodeType::SmoothStep,
            NodeType::Step,
        ] {
            assert!(!widens_with_its_inputs(&t), "{t:?} must not widen");
        }
        for t in [NodeType::Add, NodeType::Multiply, NodeType::Min, NodeType::Max] {
            assert!(widens_with_its_inputs(&t), "{t:?} should widen");
        }
    }

    #[test]
    fn preamble_matches_sprite_bindings() {
        // Sanity: the generated module's bind group layout must match the
        // sprite pipeline (group 0: globals, texture, sampler).
        let mut graph = ShaderGraph::new("noop");
        let tex_id = graph.add_node(NodeType::TextureSample, [0.0, 0.0]);
        let output_id = graph.output_node().unwrap().id;
        graph.connect(tex_id, 0, output_id, 0);

        let shader = generate(&graph).expect("codegen succeeds");
        assert!(shader.wgsl.contains("@group(0) @binding(0) var<uniform> globals"));
        assert!(shader.wgsl.contains("@group(0) @binding(1) var u_texture"));
        assert!(shader.wgsl.contains("@group(0) @binding(2) var u_sampler"));
    }

    /// A graph that moves its vertices: the offset is computed per vertex
    /// (`graph_offset`), added before projection, and a texture read there
    /// takes an explicit mip - WGSL has no implicit derivatives in a vertex
    /// shader. Without the node the vertex shader is what it always was.
    #[test]
    fn a_vertex_offset_runs_in_the_vertex_stage() {
        let mut plain = ShaderGraph::new("plain");
        let white = plain.add_node(NodeType::ColorInput { color: [1.0; 4] }, [0.0, 0.0]);
        let out = plain.output_node().unwrap().id;
        plain.connect(white, 0, out, 0);
        let plain = generate(&plain).expect("codegen").wgsl;
        assert!(!plain.contains("graph_offset"), "no offset without the node");
        assert_eq!(plain.matches("fn vs_main").count(), 1);

        let mut graph = ShaderGraph::new("sway");
        let uv = graph.add_node(NodeType::UvInput, [0.0, 0.0]);
        let tex = graph.add_node(NodeType::TextureSample, [0.0, 0.0]);
        graph.connect(uv, 0, tex, 0);
        let split = graph.add_node(NodeType::Split, [0.0, 0.0]);
        graph.connect(tex, 0, split, 0);
        let time = graph.add_node(NodeType::TimeInput, [0.0, 0.0]);
        let sin = graph.add_node(NodeType::Sin, [0.0, 0.0]);
        graph.connect(time, 0, sin, 0);
        let lean = graph.add_node(NodeType::Multiply, [0.0, 0.0]);
        graph.connect(sin, 0, lean, 0);
        graph.connect(split, 3, lean, 1);
        let dir = graph.add_node(NodeType::Vec2Constant { value: [12.0, 0.0] }, [0.0, 0.0]);
        let off = graph.add_node(NodeType::Multiply, [0.0, 0.0]);
        graph.connect(dir, 0, off, 0);
        graph.connect(lean, 0, off, 1);
        let vo = graph.add_node(NodeType::VertexOffset, [0.0, 0.0]);
        graph.connect(off, 0, vo, 0);
        let out = graph.output_node().unwrap().id;
        graph.connect(tex, 0, out, 0);
        let wgsl = generate(&graph).expect("codegen").wgsl;
        assert!(wgsl.contains("fn graph_offset(in: VsOut) -> vec2<f32>"), "{wgsl}");
        assert!(wgsl.contains("out.world = in.position + graph_offset(out);"), "{wgsl}");
        let vertex = &wgsl[wgsl.find("fn graph_offset").unwrap()..];
        let vertex = &vertex[..vertex.find("
}
").unwrap()];
        assert!(vertex.contains("textureSampleLevel(u_texture, u_sampler, uv, 0.0)"), "{vertex}");
        assert!(!vertex.contains("textureSample(u_texture"), "{vertex}");
        #[cfg(feature = "gpu-wgpu")]
        {
            let module = naga::front::wgsl::parse_str(&wgsl).expect("parses");
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
                .validate(&module)
                .expect("a vertex stage that samples a texture validates");
        }
    }

    #[test]
    fn the_vertex_offset_cannot_read_the_scene_or_be_doubled() {
        let mut graph = ShaderGraph::new("bad");
        let scene = graph.add_node(NodeType::SceneColor, [0.0, 0.0]);
        let split = graph.add_node(NodeType::Split, [0.0, 0.0]);
        graph.connect(scene, 0, split, 0);
        let vo = graph.add_node(NodeType::VertexOffset, [0.0, 0.0]);
        graph.connect(split, 0, vo, 0);
        let errors = graph.validate();
        assert!(errors.iter().any(|e| e.to_string().contains("Scene Color cannot feed")), "{errors:?}");
        let mut two = ShaderGraph::new("two");
        two.add_node(NodeType::VertexOffset, [0.0, 0.0]);
        two.add_node(NodeType::VertexOffset, [0.0, 0.0]);
        assert!(two.validate().iter().any(|e| e.to_string().contains("Multiple vertex offset")));
        assert!(generate(&two).is_err());
    }
}
