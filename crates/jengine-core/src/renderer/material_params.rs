//! **A material's parameters**: the numbers a game hands a shader graph.
//!
//! `MaterialComponent.uniforms` is what `set_material_uniform_float` / `_int`
//! / `_vec2` / `_vec4` write, and until Sep 2026 no render path read it: the
//! sprite bind group had nowhere to put a per-entity value, and a real one
//! would break batching, since sprites batch by `(material, texture)`. So a
//! graph material could depend on nothing the game computed.
//!
//! The fix is per **material instance**. A graph declares up to
//! [`MAX_PARAMS`] `MaterialParam` nodes by name; each flush of the sprite
//! batch carries four `vec4`s in its globals slot (`globals.params`); and
//! the sprite pass groups by the packed values as well as by material and
//! texture. Sprites whose parameters are equal still draw together - only a
//! different value splits a batch, which is the least it can cost.
//!
//! What joins the two ends is here, and it is global because both ends are:
//! the graph's parameter names are learned when its pipeline is compiled
//! (`GraphPipelineCache`), a material's graph when the material loads
//! (`AssetManager`), and the sprite pass - which has neither - asks
//! [`packed_for`]. A graph's names take slots in **sorted order**, the same
//! rule `shader_codegen` uses, so the slot a name gets never depends on which
//! names an entity happens to set.
//!
//! **A `.material` file's own `uniforms` are its defaults** (Oct 2026): the
//! value a parameter has on every sprite drawing with it, under whatever an
//! entity sets itself. Until then they were read by nothing - only an
//! entity's `MaterialComponent.uniforms` were packed - so a material could
//! not say "this glow is orange" once for all who wear it.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::renderer::material::UniformValue;

/// How many `vec4` parameters a graph material can read.
pub const MAX_PARAMS: usize = 4;

/// Four `vec4`s, as the batch uploads them.
pub type Params = [[f32; 4]; MAX_PARAMS];

#[derive(Default)]
struct Registry {
    /// Graph path -> its parameter names, in slot order.
    graphs: HashMap<String, Vec<String>>,
    /// Material id -> the graph it draws with.
    materials: HashMap<String, String>,
    /// Graphs with a `SceneColor` node.
    scene_readers: HashSet<String>,
    /// The material ids whose graph is in `scene_readers`, kept by
    /// [`Registry::refresh_readers`] whenever either side changes. The sprite
    /// pass asks [`reads_scene`] for every sprite in every pass, and two
    /// SipHash lookups of a string each time were 12% of a frame of ten
    /// thousand sprites (the showcase's Ten Thousand Lights, in wasm); a scene
    /// has a handful of these at most, so a scan of short strings is cheaper.
    reader_materials: Vec<String>,
    /// Material id -> its file's `uniforms`, the defaults under an entity's.
    defaults: HashMap<String, HashMap<String, UniformValue>>,
    /// The material ids in `defaults`, scanned by [`has_defaults`] for the
    /// same reason `reader_materials` is.
    default_materials: Vec<String>,
}

impl Registry {
    fn refresh_readers(&mut self) {
        self.reader_materials = self
            .materials
            .iter()
            .filter(|(_, graph)| self.scene_readers.contains(*graph))
            .map(|(id, _)| id.clone())
            .collect();
    }
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
}

/// A graph's parameter names in slot order: sorted, without repeats. The
/// rule `shader_codegen` places them by.
pub fn slot_order<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut v: Vec<String> = names.into_iter().map(str::to_string).collect();
    v.sort();
    v.dedup();
    v
}

/// Record a compiled graph's parameters.
pub fn note_graph(graph_path: &str, names: Vec<String>) {
    REGISTRY.with(|r| {
        r.borrow_mut().graphs.insert(graph_path.to_string(), names);
    });
}

/// Record which graph a material draws with.
pub fn note_material(material_id: &str, graph_path: &str) {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        r.materials.insert(material_id.to_string(), graph_path.to_string());
        r.refresh_readers();
    });
}

/// Record a material file's own `uniforms` as its defaults (an empty map
/// clears them).
pub fn note_material_defaults(material_id: &str, uniforms: &HashMap<String, UniformValue>) {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        if uniforms.is_empty() {
            r.defaults.remove(material_id);
        } else {
            r.defaults.insert(material_id.to_string(), uniforms.clone());
        }
        r.default_materials = r.defaults.keys().cloned().collect();
    });
}

/// Whether a material's file sets parameters of its own.
pub fn has_defaults(material_id: &str) -> bool {
    REGISTRY.with(|r| r.borrow().default_materials.iter().any(|m| m == material_id))
}

/// Record that a compiled graph reads the scene (a `SceneColor` node).
pub fn note_scene_reader(graph_path: &str) {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        r.scene_readers.insert(graph_path.to_string());
        r.refresh_readers();
    });
}

/// Whether a material draws with a graph that reads the scene - known once
/// the material has loaded and its graph has compiled.
///
/// The lit world passes ask this in core, where no pipeline cache is at
/// hand: such a sprite is drawn after the lighting composite, over a copy of
/// the lit image, and must leave no trace in the passes before it - not
/// even the black cutout an unlit sprite normally leaves in the scene
/// buffer, which the copy would then show (the showcase's shockwaves were
/// black boxes in a lit scene until this).
pub fn reads_scene(material_id: &str) -> bool {
    REGISTRY.with(|r| {
        r.borrow().reader_materials.iter().any(|m| m == material_id)
    })
}

/// A material's parameter names in slot order, once both its material and
/// its graph have been seen.
pub fn names_for(material_id: &str) -> Option<Vec<String>> {
    REGISTRY.with(|r| {
        let r = r.borrow();
        let graph = r.materials.get(material_id)?;
        r.graphs.get(graph).cloned()
    })
}

/// One value as a `vec4`: a float is `(v, 0, 0, 0)`, a vec2 `(x, y, 0, 0)`.
pub fn as_vec4(v: &UniformValue) -> [f32; 4] {
    match v {
        UniformValue::Float(f) => [*f, 0.0, 0.0, 0.0],
        UniformValue::Int(i) => [*i as f32, 0.0, 0.0, 0.0],
        UniformValue::Vec2(a) => [a[0], a[1], 0.0, 0.0],
        UniformValue::Vec3(a) => [a[0], a[1], a[2], 0.0],
        UniformValue::Vec4(a) => *a,
        #[allow(unreachable_patterns)]
        _ => [0.0; 4],
    }
}

/// Pack an instance's values into slots by `names`. A name the instance
/// does not set is zero.
pub fn pack(uniforms: &HashMap<String, UniformValue>, names: &[String]) -> Params {
    let mut out = [[0.0; 4]; MAX_PARAMS];
    for (slot, name) in names.iter().take(MAX_PARAMS).enumerate() {
        if let Some(v) = uniforms.get(name) {
            out[slot] = as_vec4(v);
        }
    }
    out
}

/// The packed parameters for a material instance - its material's defaults
/// with the instance's own values over them - or zeros when there are none
/// (or the graph has not been compiled yet - the first frame a material is
/// seen).
pub fn packed_for(material_id: &str, uniforms: &HashMap<String, UniformValue>) -> Params {
    let Some(names) = names_for(material_id) else {
        return [[0.0; 4]; MAX_PARAMS];
    };
    REGISTRY.with(|r| {
        let r = r.borrow();
        match r.defaults.get(material_id) {
            Some(defaults) if !defaults.is_empty() => {
                let mut merged = defaults.clone();
                for (k, v) in uniforms {
                    merged.insert(k.clone(), v.clone());
                }
                pack(&merged, &names)
            }
            _ if uniforms.is_empty() => [[0.0; 4]; MAX_PARAMS],
            _ => pack(uniforms, &names),
        }
    })
}

/// Parameters as bits, for sorting and grouping: two instances batch
/// together exactly when these are equal.
pub fn bits(p: &Params) -> [u32; 16] {
    let mut out = [0u32; 16];
    for (i, v) in p.iter().flatten().enumerate() {
        out[i] = v.to_bits();
    }
    out
}

pub fn from_bits(b: &[u32; 16]) -> Params {
    let mut out = [[0.0; 4]; MAX_PARAMS];
    for (i, v) in b.iter().enumerate() {
        out[i / 4][i % 4] = f32::from_bits(*v);
    }
    out
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_material_files_uniforms_are_its_defaults() {
        note_graph("g_defaults.shader", vec!["glow".into(), "tint".into()]);
        note_material("m_defaults", "g_defaults.shader");
        let mut file = HashMap::new();
        file.insert("tint".to_string(), UniformValue::Vec4([1.0, 0.5, 0.0, 1.0]));
        file.insert("glow".to_string(), UniformValue::Float(2.0));
        note_material_defaults("m_defaults", &file);
        assert!(has_defaults("m_defaults"));
        // An entity setting nothing draws with the file's values ...
        let none = HashMap::new();
        assert_eq!(packed_for("m_defaults", &none)[0], [2.0, 0.0, 0.0, 0.0]);
        assert_eq!(packed_for("m_defaults", &none)[1], [1.0, 0.5, 0.0, 1.0]);
        // ... and its own over them.
        let mut own = HashMap::new();
        own.insert("glow".to_string(), UniformValue::Float(5.0));
        let p = packed_for("m_defaults", &own);
        assert_eq!((p[0], p[1]), ([5.0, 0.0, 0.0, 0.0], [1.0, 0.5, 0.0, 1.0]));
        note_material_defaults("m_defaults", &HashMap::new());
        assert!(!has_defaults("m_defaults"));
    }
    use super::*;

    #[test]
    fn names_take_slots_in_sorted_order_whatever_an_instance_sets() {
        note_graph("g.shader", slot_order(["tint", "glow", "tint"]));
        note_material("m", "g.shader");
        assert_eq!(names_for("m").unwrap(), vec!["glow".to_string(), "tint".to_string()]);
        // An instance setting only `tint` still puts it in slot 1.
        let mut u = HashMap::new();
        u.insert("tint".to_string(), UniformValue::Vec4([1.0, 0.5, 0.0, 1.0]));
        let p = packed_for("m", &u);
        assert_eq!(p[0], [0.0; 4]);
        assert_eq!(p[1], [1.0, 0.5, 0.0, 1.0]);
        assert_eq!(from_bits(&bits(&p)), p);
    }

    #[test]
    fn a_material_whose_graph_is_unknown_packs_zeros() {
        let mut u = HashMap::new();
        u.insert("x".to_string(), UniformValue::Float(3.0));
        assert_eq!(packed_for("never-seen", &u), [[0.0; 4]; MAX_PARAMS]);
    }
}
