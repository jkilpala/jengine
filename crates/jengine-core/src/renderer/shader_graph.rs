//! Shader graph data model — visual shader nodes, edges, and type system.
//!
//! Serialized to `.shader` asset files. The code generator in [`super::shader_codegen`]
//! converts a `ShaderGraph` into GLSL source for both native (330 core) and WebGL2 (300 es).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Unique identifier for a node within a graph.
pub type NodeId = u32;

/// Pin data type — determines compatibility for connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PinType {
    Float,
    Vec2,
    Vec3,
    Vec4,
    Sampler2D,
}

impl PinType {
    /// GLSL type name for code generation.
    pub fn glsl_type(&self) -> &'static str {
        match self {
            Self::Float => "float",
            Self::Vec2 => "vec2",
            Self::Vec3 => "vec3",
            Self::Vec4 => "vec4",
            Self::Sampler2D => "sampler2D",
        }
    }

    /// WGSL type name for code generation.
    pub fn wgsl_type(&self) -> &'static str {
        match self {
            Self::Float => "f32",
            Self::Vec2 => "vec2<f32>",
            Self::Vec3 => "vec3<f32>",
            Self::Vec4 => "vec4<f32>",
            // No WGSL spelling; a sampler is bound, never a local.
            Self::Sampler2D => "f32",
        }
    }

    /// Number of scalar components (for automatic conversion).
    pub fn components(&self) -> usize {
        match self {
            Self::Float => 1,
            Self::Vec2 => 2,
            Self::Vec3 => 3,
            Self::Vec4 => 4,
            Self::Sampler2D => 0,
        }
    }

    /// Check if this type can be implicitly converted to `target`.
    pub fn can_convert_to(&self, target: &PinType) -> bool {
        if self == target {
            return true;
        }
        // Sampler2D cannot convert to/from anything.
        if *self == PinType::Sampler2D || *target == PinType::Sampler2D {
            return false;
        }
        // Float can promote to any vector type.
        if *self == PinType::Float {
            return true;
        }
        // Vectors can truncate or extend (vec4 → vec3, vec2 → vec3, etc.)
        true
    }
}

/// Direction of a pin on a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PinDirection {
    Input,
    Output,
}

/// A pin (input or output) on a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinDef {
    /// Display name (e.g. "Color", "UV", "Result").
    pub name: String,
    /// Data type of this pin.
    pub pin_type: PinType,
    /// Direction — input or output.
    pub direction: PinDirection,
}

/// A connection between two pins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    /// Source node ID.
    pub from_node: NodeId,
    /// Output pin index on the source node.
    pub from_pin: usize,
    /// Destination node ID.
    pub to_node: NodeId,
    /// Input pin index on the destination node.
    pub to_pin: usize,
}

fn default_color_white() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

/// The type of operation a node performs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NodeType {
    // === Input Nodes ===
    /// UV coordinates: outputs the interpolated texture UV.
    UvInput,
    /// Color constant: outputs a user-defined RGBA color.
    ColorInput {
        #[serde(default = "default_color_white")]
        color: [f32; 4],
    },
    /// The **sprite's own vertex colour** (RGBA), which is the one per-entity
    /// value a graph can read.
    ///
    /// Added Sep 2026, and the reason is worth stating: before it, a graph
    /// could read UV, time, position, the sampled texture and constants -
    /// none of which the game knows anything about. `MaterialComponent`
    /// carries a `uniforms` map that four documented script functions write
    /// (`set_material_uniform_float` and friends) and **no render path reads**;
    /// the sprite bind group has three entries, and none of them is a
    /// per-material uniform buffer. So a graph material could not depend on
    /// anything a game computes, which is why no game had ever used one.
    ///
    /// This is the cheap half of the fix. `Transform`/`SpriteComponent`
    /// already push a per-vertex colour that `set_sprite_color` writes and
    /// every runtime honours, so exposing it costs one binding nothing and
    /// breaks no batching - a real per-entity uniform would, since sprites
    /// batch by `(material, texture)`. See `docs/parity-gaps.md`.
    VertexColorInput,
    /// Time: outputs accumulated time (float).
    TimeInput,
    /// A **material parameter**: a `vec4` the game sets per material instance
    /// with `set_material_uniform_*` (a float arrives as `(v, 0, 0, 0)`).
    /// Up to four names per graph; they take slots in sorted order (see
    /// `renderer::material_params`). Sep 2026 - before it,
    /// `MaterialComponent.uniforms` was written by four script functions and
    /// read by nothing.
    MaterialParam {
        name: String,
    },
    /// The fragment's position in **world** space. (Until Sep 2026 this
    /// returned the sprite's UV and said otherwise; no shipped graph used it.)
    PositionInput,
    /// The fragment's position on the screen, 0..1 with `y` down - the space
    /// `SceneColor`'s offset is in.
    ScreenPosition,
    /// A world point to where it lands on the screen (0..1, `y` down), through
    /// the camera the sprite is drawn with. With `PositionInput` it mirrors:
    /// `WorldToScreen((x, 2 * surface - y)) - ScreenPosition` is the offset
    /// at which `SceneColor` finds what stands above a water line.
    WorldToScreen,

    // === Constants ===
    /// A literal float value.
    FloatConstant { value: f32 },
    /// A literal vec2 value.
    Vec2Constant { value: [f32; 2] },
    /// A literal vec3 value.
    Vec3Constant { value: [f32; 3] },
    /// A literal vec4 value (color).
    Vec4Constant { value: [f32; 4] },

    // === Texture ===
    /// **What is already drawn** under this pixel, moved by `Offset` (in
    /// screen UV, so 0.01 is a hundredth of the screen): Unity's Scene Color,
    /// Godot's screen texture. What heat haze, refraction and water that bends
    /// what is under it are made of. A sprite whose graph uses it is drawn
    /// after the rest of the world, over a copy of it - so it sits above every
    /// other sprite whatever its z, and cannot see itself. Its alpha is always
    /// 1 (the float target's own alpha runs past 1 under additive sprites).
    /// Input: Offset (vec2); output: Color (vec4).
    SceneColor,
    /// Sample a texture at UV coordinates.
    /// Input 0: UV (vec2), Output 0: Color (vec4), Output 1: R (float), Output 2: G (float), Output 3: B (float), Output 4: A (float).
    TextureSample,
    /// The sprite's texture's size: **Texel** is one texel in UV (`1 /
    /// size`), **Size** is the size in pixels. What a graph samples its
    /// neighbours with - an outline, an edge detect, a blur - a texel apart
    /// whatever the texture's size. Godot's `TEXTURE_PIXEL_SIZE`. Outputs:
    /// Texel (vec2), Size (vec2).
    TexelSize,

    // === Math ===
    /// Add two values.
    Add,
    /// Subtract: A - B.
    Subtract,
    /// Multiply two values.
    Multiply,
    /// Divide: A / B.
    Divide,
    /// Modulo: mod(A, B).
    Modulo,
    /// Power: pow(A, B).
    Power,
    /// Square root: sqrt(A).
    SquareRoot,
    /// Absolute value: abs(A).
    Abs,
    /// Negate: -A.
    Negate,
    /// Floor: floor(A).
    Floor,
    /// Ceil: ceil(A).
    Ceil,
    /// Clamp: clamp(Value, Min, Max).
    Clamp,
    /// Linear interpolation: mix(A, B, T).
    Mix,
    /// Step: step(Edge, X).
    Step,
    /// SmoothStep: smoothstep(Edge0, Edge1, X).
    SmoothStep,
    /// Sine: sin(A).
    Sin,
    /// Cosine: cos(A).
    Cos,
    /// Min of two values.
    Min,
    /// Max of two values.
    Max,
    /// Dot product of two vectors.
    Dot,
    /// Length of a vector.
    Length,
    /// Normalize a vector.
    Normalize,
    /// Distance between two points.
    Distance,

    // === Procedural ===
    /// Smooth noise, 0..1: `octaves` layers of gradient (Perlin-style) noise
    /// at `Scale` cells per UV unit, each twice as fine, half as strong and
    /// turned against the last (fbm). What a
    /// dissolve's edge, a hologram's flicker, heat shimmer and drifting smoke
    /// start from - Unity's Simple Noise, Godot's noise texture. Before Sep
    /// 2026 a graph had no noise at all and had to paste it as raw WGSL.
    /// Inputs: UV (vec2), Scale (float); output: Value (float).
    Noise {
        #[serde(default = "default_noise_octaves")]
        octaves: u32,
    },
    /// Cellular (Worley) noise: the distance from UV to the nearest of a
    /// jittered grid of points, and a random value per cell - water caustics,
    /// cracks, scales. Inputs: UV (vec2), Scale (float); outputs: Distance
    /// (float, about 0..1), Cell (float, 0..1), and Edge (float, F2 - F1:
    /// near zero along every border between cells - a crisp outline).
    Voronoi,

    // === Vector ===
    /// Combine scalars into a vector: (X, Y) → vec2, (X, Y, Z) → vec3, (X, Y, Z, W) → vec4.
    Combine {
        /// Number of components: 2, 3, or 4.
        components: u8,
    },
    /// Split a vector into scalar components.
    Split,

    // === Color Operations ===
    /// Mix two colors: mix(A, B, Factor).
    ColorMix,
    /// Adjust brightness (multiply RGB by scalar).
    Brightness,
    /// Adjust saturation (desaturate/saturate).
    Saturation,
    /// Adjust contrast.
    Contrast,

    // === Effect Presets ===
    /// Emission glow: adds emission color × strength × alpha.
    Emission,
    /// Outline detection: samples neighboring texels.
    Outline,

    // === Output ===
    /// Final fragment color output.
    FragmentOutput,
    /// **Moves the vertices** (Oct 2026; Godot's `VERTEX`, Unity Shader
    /// Graph's vertex Position): an offset in world units added to each
    /// vertex before it is projected - grass and banners swaying, a jelly
    /// wobbling. Optional, at most one. The nodes feeding it run once per
    /// vertex, so they read that vertex's UV, colour and world position: a
    /// sprite's four corners (an offset times `1 - uv.y` leans its top and
    /// keeps its foot), every point of a `mesh`. A texture read there takes
    /// the full-size mip; `SceneColor` cannot feed it. The colour side sees
    /// the moved position in `PositionInput`.
    VertexOffset,

    // === Custom GLSL ===
    /// User-defined GLSL code block with custom pins.
    CustomGlsl {
        /// Raw GLSL code.
        code: String,
        /// Input pin definitions (user-defined).
        inputs: Vec<CustomPin>,
        /// Output pin definitions (user-defined).
        outputs: Vec<CustomPin>,
    },
}

fn default_noise_octaves() -> u32 {
    4
}

/// A user-defined pin on a CustomGlsl node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomPin {
    /// Pin name (becomes the variable name in GLSL).
    pub name: String,
    /// Pin data type.
    pub pin_type: PinType,
}

/// A node in the shader graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShaderNode {
    /// Unique identifier within this graph.
    pub id: NodeId,
    /// The operation this node performs.
    pub node_type: NodeType,
    /// Display position in the editor (x, y).
    pub position: [f32; 2],
    /// Optional display label (overrides default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Metadata about the shader graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShaderGraphMeta {
    /// Human-readable name for this shader.
    pub name: String,
    /// Optional description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Author.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

/// Complete shader graph — nodes, edges, and metadata.
///
/// Serialized as `.shader` asset files (JSON format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShaderGraph {
    /// Graph metadata.
    pub meta: ShaderGraphMeta,
    /// All nodes in the graph.
    pub nodes: Vec<ShaderNode>,
    /// Connections between node pins.
    pub edges: Vec<Edge>,
    /// Next node ID to assign (monotonically increasing).
    #[serde(default)]
    pub next_id: NodeId,
}

impl ShaderGraph {
    /// The graph's `MaterialParam` names in slot order (sorted, no repeats):
    /// the order `shader_codegen` places them in and the sprite pass packs
    /// an instance's values by.
    pub fn param_names(&self) -> Vec<String> {
        crate::renderer::material_params::slot_order(self.nodes.iter().filter_map(|n| match &n.node_type {
            NodeType::MaterialParam { name } => Some(name.as_str()),
            _ => None,
        }))
    }

    /// Create a new empty shader graph with a default fragment output node.
    pub fn new(name: &str) -> Self {
        let output_node = ShaderNode {
            id: 0,
            node_type: NodeType::FragmentOutput,
            position: [400.0, 200.0],
            label: None,
        };
        Self {
            meta: ShaderGraphMeta {
                name: name.to_string(),
                description: None,
                author: None,
            },
            nodes: vec![output_node],
            edges: Vec::new(),
            next_id: 1,
        }
    }

    /// Add a node to the graph, returning its assigned ID.
    pub fn add_node(&mut self, node_type: NodeType, position: [f32; 2]) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(ShaderNode {
            id,
            node_type,
            position,
            label: None,
        });
        id
    }

    /// Remove a node and all edges connected to it.
    pub fn remove_node(&mut self, node_id: NodeId) {
        self.nodes.retain(|n| n.id != node_id);
        self.edges
            .retain(|e| e.from_node != node_id && e.to_node != node_id);
    }

    /// Connect two pins. Returns false if the connection already exists.
    pub fn connect(
        &mut self,
        from_node: NodeId,
        from_pin: usize,
        to_node: NodeId,
        to_pin: usize,
    ) -> bool {
        // Check for duplicate.
        let exists = self.edges.iter().any(|e| {
            e.from_node == from_node
                && e.from_pin == from_pin
                && e.to_node == to_node
                && e.to_pin == to_pin
        });
        if exists {
            return false;
        }
        // Remove any existing connection to this input pin (each input has one source).
        self.edges
            .retain(|e| !(e.to_node == to_node && e.to_pin == to_pin));
        self.edges.push(Edge {
            from_node,
            from_pin,
            to_node,
            to_pin,
        });
        true
    }

    /// Disconnect an edge.
    pub fn disconnect(&mut self, to_node: NodeId, to_pin: usize) {
        self.edges
            .retain(|e| !(e.to_node == to_node && e.to_pin == to_pin));
    }

    /// Find the node with the given ID.
    pub fn get_node(&self, id: NodeId) -> Option<&ShaderNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Find the mutable node with the given ID.
    pub fn get_node_mut(&mut self, id: NodeId) -> Option<&mut ShaderNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// Find the output node (FragmentOutput).
    pub fn output_node(&self) -> Option<&ShaderNode> {
        self.nodes
            .iter()
            .find(|n| matches!(n.node_type, NodeType::FragmentOutput))
    }

    /// The vertex output ([`NodeType::VertexOffset`]), if the graph moves its
    /// vertices.
    pub fn vertex_offset_node(&self) -> Option<&ShaderNode> {
        self.nodes.iter().find(|n| matches!(n.node_type, NodeType::VertexOffset))
    }

    /// Every node `id` reads from, directly or not.
    pub fn upstream_of(&self, id: NodeId) -> std::collections::HashSet<NodeId> {
        let mut seen = std::collections::HashSet::new();
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            for e in self.edges.iter().filter(|e| e.to_node == n) {
                if seen.insert(e.from_node) {
                    stack.push(e.from_node);
                }
            }
        }
        seen
    }

    /// Get the edge connected to a specific input pin, if any.
    pub fn input_edge(&self, node_id: NodeId, pin_index: usize) -> Option<&Edge> {
        self.edges
            .iter()
            .find(|e| e.to_node == node_id && e.to_pin == pin_index)
    }

    /// Get all edges originating from a specific output pin.
    pub fn output_edges(&self, node_id: NodeId, pin_index: usize) -> Vec<&Edge> {
        self.edges
            .iter()
            .filter(|e| e.from_node == node_id && e.from_pin == pin_index)
            .collect()
    }

    /// Get the pin definitions for a node type.
    pub fn node_pins(node_type: &NodeType) -> (Vec<PinDef>, Vec<PinDef>) {
        node_pins(node_type)
    }

    /// Validate the graph: check for cycles, disconnected output, type mismatches.
    pub fn validate(&self) -> Vec<GraphError> {
        let mut errors = Vec::new();

        // Must have exactly one FragmentOutput node.
        let output_count = self
            .nodes
            .iter()
            .filter(|n| matches!(n.node_type, NodeType::FragmentOutput))
            .count();
        if output_count == 0 {
            errors.push(GraphError::NoOutput);
        } else if output_count > 1 {
            errors.push(GraphError::MultipleOutputs);
        }

        // At most one vertex output, and nothing per pixel feeding it.
        let vertex_outputs: Vec<&ShaderNode> =
            self.nodes.iter().filter(|n| matches!(n.node_type, NodeType::VertexOffset)).collect();
        if vertex_outputs.len() > 1 {
            errors.push(GraphError::MultipleVertexOffsets);
        }
        if !self.has_cycle()
            && let Some(v) = vertex_outputs.first()
            && self
                .upstream_of(v.id)
                .iter()
                .any(|id| self.get_node(*id).is_some_and(|n| matches!(n.node_type, NodeType::SceneColor)))
        {
            errors.push(GraphError::VertexReadsScene);
        }

        // Check for cycles using DFS.
        if self.has_cycle() {
            errors.push(GraphError::Cycle);
        }

        // Check that the output node's Color input is connected.
        if let Some(output) = self.output_node()
            && self.input_edge(output.id, 0).is_none() {
                errors.push(GraphError::OutputDisconnected);
            }

        // Validate edge endpoints.
        for edge in &self.edges {
            let from_exists = self.get_node(edge.from_node).is_some();
            let to_exists = self.get_node(edge.to_node).is_some();
            if !from_exists || !to_exists {
                errors.push(GraphError::InvalidEdge {
                    from: edge.from_node,
                    to: edge.to_node,
                });
            }
        }

        errors
    }

    /// Check if the graph contains any cycles (would cause infinite loop in codegen).
    fn has_cycle(&self) -> bool {
        let mut visited = HashMap::new();
        for node in &self.nodes {
            if self.dfs_cycle(node.id, &mut visited) {
                return true;
            }
        }
        false
    }

    /// DFS cycle detection. States: 0 = unvisited, 1 = visiting, 2 = done.
    fn dfs_cycle(&self, node_id: NodeId, visited: &mut HashMap<NodeId, u8>) -> bool {
        match visited.get(&node_id) {
            Some(1) => return true, // Back edge → cycle.
            Some(2) => return false, // Already fully explored.
            _ => {}
        }
        visited.insert(node_id, 1);

        // Follow edges backwards from inputs to find dependencies.
        let (inputs, _) = self
            .get_node(node_id)
            .map(|n| node_pins(&n.node_type))
            .unwrap_or_default();
        for pin_idx in 0..inputs.len() {
            if let Some(edge) = self.input_edge(node_id, pin_idx)
                && self.dfs_cycle(edge.from_node, visited) {
                    return true;
                }
        }

        visited.insert(node_id, 2);
        false
    }
}

/// Validation errors for the shader graph.
#[derive(Debug, Clone)]
pub enum GraphError {
    /// No FragmentOutput node found.
    NoOutput,
    /// Multiple FragmentOutput nodes.
    MultipleOutputs,
    /// More than one VertexOffset node.
    MultipleVertexOffsets,
    /// A SceneColor node feeds the VertexOffset - the scene is read per
    /// pixel, after the vertices are placed.
    VertexReadsScene,
    /// Graph contains a cycle.
    Cycle,
    /// FragmentOutput Color input is not connected.
    OutputDisconnected,
    /// Edge references non-existent node.
    InvalidEdge { from: NodeId, to: NodeId },
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoOutput => write!(f, "No fragment output node in shader graph"),
            Self::MultipleOutputs => write!(f, "Multiple fragment output nodes"),
            Self::MultipleVertexOffsets => write!(f, "Multiple vertex offset nodes"),
            Self::VertexReadsScene => write!(f, "Scene Color cannot feed the vertex offset: the scene is read per pixel"),
            Self::Cycle => write!(f, "Shader graph contains a cycle"),
            Self::OutputDisconnected => write!(f, "Fragment output color input is not connected"),
            Self::InvalidEdge { from, to } => {
                write!(f, "Edge references invalid nodes: {from} → {to}")
            }
        }
    }
}

/// Get the input and output pin definitions for a node type.
pub fn node_pins(node_type: &NodeType) -> (Vec<PinDef>, Vec<PinDef>) {
    use PinDirection::*;
    use PinType::*;

    let inp = |name: &str, t: PinType| PinDef {
        name: name.to_string(),
        pin_type: t,
        direction: Input,
    };
    let out = |name: &str, t: PinType| PinDef {
        name: name.to_string(),
        pin_type: t,
        direction: Output,
    };

    match node_type {
        // Inputs (no inputs, one output)
        NodeType::UvInput => (vec![], vec![out("UV", Vec2)]),
        NodeType::ColorInput { .. } => (vec![], vec![out("Color", Vec4)]),
        NodeType::VertexColorInput => (vec![], vec![out("Vertex Color", Vec4)]),
        NodeType::TimeInput => (vec![], vec![out("Time", Float)]),
        NodeType::MaterialParam { .. } => (vec![], vec![out("Value", Vec4)]),
        NodeType::PositionInput => (vec![], vec![out("Position", Vec2)]),
        NodeType::ScreenPosition => (vec![], vec![out("Screen", Vec2)]),
        NodeType::WorldToScreen => (vec![inp("World", Vec2)], vec![out("Screen", Vec2)]),

        // Constants
        NodeType::FloatConstant { .. } => (vec![], vec![out("Value", Float)]),
        NodeType::Vec2Constant { .. } => (vec![], vec![out("Value", Vec2)]),
        NodeType::Vec3Constant { .. } => (vec![], vec![out("Value", Vec3)]),
        NodeType::Vec4Constant { .. } => (vec![], vec![out("Value", Vec4)]),

        NodeType::SceneColor => (vec![inp("Offset", Vec2)], vec![out("Color", Vec4)]),
        NodeType::TexelSize => (vec![], vec![out("Texel", Vec2), out("Size", Vec2)]),

        // Texture
        NodeType::TextureSample => (
            vec![inp("UV", Vec2)],
            vec![
                out("Color", Vec4),
                out("R", Float),
                out("G", Float),
                out("B", Float),
                out("A", Float),
            ],
        ),

        // Binary math (A, B → Result). These declarations are the *fallback*:
        // `shader_codegen` widens them to whatever is actually connected, so
        // a `Multiply` of two vec4s stays a vec4. Before Sep 2026 the
        // declaration was the whole story and every one of these truncated its
        // inputs to `.x` - a graph multiplying a texture by a colour emitted
        // `(tex).x * (colour).x` and returned it as grey, which compiles,
        // renders, and looks like a deliberate greyscale effect.
        NodeType::Add | NodeType::Subtract | NodeType::Multiply | NodeType::Divide
        | NodeType::Modulo | NodeType::Power | NodeType::Min | NodeType::Max => (
            vec![inp("A", Float), inp("B", Float)],
            vec![out("Result", Float)],
        ),

        // Unary math (A → Result)
        NodeType::SquareRoot | NodeType::Abs | NodeType::Negate | NodeType::Floor
        | NodeType::Ceil | NodeType::Sin | NodeType::Cos => {
            (vec![inp("A", Float)], vec![out("Result", Float)])
        }

        // Clamp (Value, Min, Max → Result)
        NodeType::Clamp => (
            vec![inp("Value", Float), inp("Min", Float), inp("Max", Float)],
            vec![out("Result", Float)],
        ),

        // Mix/Lerp (A, B, T → Result)
        NodeType::Mix | NodeType::ColorMix => (
            vec![inp("A", Vec4), inp("B", Vec4), inp("T", Float)],
            vec![out("Result", Vec4)],
        ),

        // Step (Edge, X → Result)
        NodeType::Step => (
            vec![inp("Edge", Float), inp("X", Float)],
            vec![out("Result", Float)],
        ),

        // SmoothStep (Edge0, Edge1, X → Result)
        NodeType::SmoothStep => (
            vec![
                inp("Edge0", Float),
                inp("Edge1", Float),
                inp("X", Float),
            ],
            vec![out("Result", Float)],
        ),

        // Dot product (A, B → scalar)
        NodeType::Dot => (
            vec![inp("A", Vec4), inp("B", Vec4)],
            vec![out("Result", Float)],
        ),

        // Length (A → scalar)
        NodeType::Length => (vec![inp("A", Vec4)], vec![out("Result", Float)]),

        // Normalize (A → same type)
        NodeType::Normalize => (vec![inp("A", Vec4)], vec![out("Result", Vec4)]),

        // Distance (A, B → scalar)
        NodeType::Distance => (
            vec![inp("A", Vec4), inp("B", Vec4)],
            vec![out("Result", Float)],
        ),

        // Procedural noise (UV, Scale → value)
        NodeType::Noise { .. } => (
            vec![inp("UV", Vec2), inp("Scale", Float)],
            vec![out("Value", Float)],
        ),
        NodeType::Voronoi => (
            vec![inp("UV", Vec2), inp("Scale", Float)],
            vec![out("Distance", Float), out("Cell", Float), out("Edge", Float)],
        ),

        // Combine (scalars → vector)
        NodeType::Combine { components } => {
            let inputs = match components {
                2 => vec![inp("X", Float), inp("Y", Float)],
                3 => vec![inp("X", Float), inp("Y", Float), inp("Z", Float)],
                _ => vec![
                    inp("X", Float),
                    inp("Y", Float),
                    inp("Z", Float),
                    inp("W", Float),
                ],
            };
            let out_type = match components {
                2 => Vec2,
                3 => Vec3,
                _ => Vec4,
            };
            (inputs, vec![out("Vector", out_type)])
        }

        // Split (vector → scalars)
        NodeType::Split => (
            vec![inp("Vector", Vec4)],
            vec![
                out("X", Float),
                out("Y", Float),
                out("Z", Float),
                out("W", Float),
            ],
        ),

        // Brightness (Color, Factor → Result)
        NodeType::Brightness => (
            vec![inp("Color", Vec4), inp("Factor", Float)],
            vec![out("Result", Vec4)],
        ),

        // Saturation (Color, Amount → Result)
        NodeType::Saturation => (
            vec![inp("Color", Vec4), inp("Amount", Float)],
            vec![out("Result", Vec4)],
        ),

        // Contrast (Color, Amount → Result)
        NodeType::Contrast => (
            vec![inp("Color", Vec4), inp("Amount", Float)],
            vec![out("Result", Vec4)],
        ),

        // Emission (Color, EmissionColor, Strength → Result)
        NodeType::Emission => (
            vec![
                inp("Color", Vec4),
                inp("EmissionColor", Vec4),
                inp("Strength", Float),
            ],
            vec![out("Result", Vec4)],
        ),

        // Outline (UV, OutlineColor, Width, TextureSize → Result)
        NodeType::Outline => (
            vec![
                inp("UV", Vec2),
                inp("OutlineColor", Vec4),
                inp("Width", Float),
                inp("TextureSize", Vec2),
            ],
            vec![out("Result", Vec4)],
        ),

        // Fragment output (Color → screen)
        NodeType::FragmentOutput => (vec![inp("Color", Vec4)], vec![]),
        NodeType::VertexOffset => (vec![inp("Offset", Vec2)], vec![]),

        // Custom GLSL
        NodeType::CustomGlsl {
            inputs, outputs, ..
        } => {
            let ins = inputs
                .iter()
                .map(|p| inp(&p.name, p.pin_type))
                .collect();
            let outs = outputs
                .iter()
                .map(|p| out(&p.name, p.pin_type))
                .collect();
            (ins, outs)
        }
    }
}

/// Categorized list of available node types for the editor palette.
pub fn node_palette() -> Vec<(&'static str, Vec<(&'static str, NodeType)>)> {
    vec![
        (
            "Input",
            vec![
                ("UV", NodeType::UvInput),
                ("Color", NodeType::ColorInput { color: [1.0, 1.0, 1.0, 1.0] }),
                ("Vertex Color", NodeType::VertexColorInput),
                ("Time", NodeType::TimeInput),
                ("Position", NodeType::PositionInput),
                ("Screen Position", NodeType::ScreenPosition),
                ("World To Screen", NodeType::WorldToScreen),
            ],
        ),
        (
            "Constants",
            vec![
                ("Float", NodeType::FloatConstant { value: 0.0 }),
                (
                    "Vec2",
                    NodeType::Vec2Constant {
                        value: [0.0, 0.0],
                    },
                ),
                (
                    "Vec3",
                    NodeType::Vec3Constant {
                        value: [0.0, 0.0, 0.0],
                    },
                ),
                (
                    "Vec4",
                    NodeType::Vec4Constant {
                        value: [0.0, 0.0, 0.0, 1.0],
                    },
                ),
            ],
        ),
        ("Output", vec![("Vertex Offset", NodeType::VertexOffset)]),
        ("Texture", vec![
            ("Sample", NodeType::TextureSample),
            ("Texel Size", NodeType::TexelSize),
            ("Scene Color", NodeType::SceneColor),
        ]),
        (
            "Procedural",
            vec![
                ("Noise", NodeType::Noise { octaves: 4 }),
                ("Voronoi", NodeType::Voronoi),
            ],
        ),
        (
            "Math",
            vec![
                ("Add", NodeType::Add),
                ("Subtract", NodeType::Subtract),
                ("Multiply", NodeType::Multiply),
                ("Divide", NodeType::Divide),
                ("Modulo", NodeType::Modulo),
                ("Power", NodeType::Power),
                ("Sqrt", NodeType::SquareRoot),
                ("Abs", NodeType::Abs),
                ("Negate", NodeType::Negate),
                ("Floor", NodeType::Floor),
                ("Ceil", NodeType::Ceil),
                ("Clamp", NodeType::Clamp),
                ("Mix", NodeType::Mix),
                ("Step", NodeType::Step),
                ("SmoothStep", NodeType::SmoothStep),
                ("Sin", NodeType::Sin),
                ("Cos", NodeType::Cos),
                ("Min", NodeType::Min),
                ("Max", NodeType::Max),
                ("Dot", NodeType::Dot),
                ("Length", NodeType::Length),
                ("Normalize", NodeType::Normalize),
                ("Distance", NodeType::Distance),
            ],
        ),
        (
            "Vector",
            vec![
                (
                    "Combine (vec2)",
                    NodeType::Combine { components: 2 },
                ),
                (
                    "Combine (vec3)",
                    NodeType::Combine { components: 3 },
                ),
                (
                    "Combine (vec4)",
                    NodeType::Combine { components: 4 },
                ),
                ("Split", NodeType::Split),
            ],
        ),
        (
            "Color",
            vec![
                ("Color Mix", NodeType::ColorMix),
                ("Brightness", NodeType::Brightness),
                ("Saturation", NodeType::Saturation),
                ("Contrast", NodeType::Contrast),
            ],
        ),
        (
            "Effects",
            vec![
                ("Emission", NodeType::Emission),
                ("Outline", NodeType::Outline),
            ],
        ),
    ]
}
