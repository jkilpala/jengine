//! `jengine psd-import`: a layered Photoshop document as sprites and a
//! prefab (Unity's PSD Importer). Each visible pixel layer is written as
//! `sprites/<doc>/<layer>.png` and becomes a part of `prefabs/<doc>.prefab`:
//! a sprite at the layer's place, in its draw order, with its opacity and
//! (for multiply, screen and linear dodge) its blend. Each group is a part
//! holding its layers, so the prefab's hierarchy is the document's. Dropped
//! in a scene, the prefab draws as the document does.
//!
//! The root carries a `sorting_group`, so the parts keep their order among
//! themselves and sort against the rest of a scene as one thing. One pixel
//! is one world unit; the root is the canvas's centre, or the middle of its
//! bottom edge (`Origin::Bottom`, for a character standing on its feet). A
//! group sits at the centre of its layers, which is where it turns until
//! someone moves it. Importing again rewrites the same files, so a layer
//! keeps its picture's name while it keeps its own.
//!
//! Hidden layers are left out (and named in the report): the engine has no
//! hidden sprite, and a hidden layer is usually a spare. `include_hidden`
//! brings them in as visible.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::psd::{PsdLayer, read_psd};

/// Where the prefab's root is on the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Origin {
    #[default]
    Centre,
    /// The middle of the canvas's bottom edge.
    Bottom,
}

#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    pub origin: Origin,
    pub include_hidden: bool,
}

/// What an import wrote, as paths under `assets/`.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ImportReport {
    pub prefab: String,
    pub sprites: Vec<String>,
    /// Layers left out, and why.
    pub skipped: Vec<String>,
}

/// The `assets` directory a file is under.
pub fn assets_root_of(path: &Path) -> Option<PathBuf> {
    path.ancestors().skip(1).find(|p| p.file_name().is_some_and(|n| n == "assets")).map(Path::to_path_buf)
}

/// A file name from a layer's name: letters, digits, `-` and `_` kept,
/// lower case, anything else an underscore.
fn file_stem(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '_' })
        .collect();
    let s = s.trim_matches('_').to_string();
    if s.is_empty() { "layer".into() } else { s }
}

fn unique(base: String, taken: &mut HashSet<String>) -> String {
    if taken.insert(base.clone()) {
        return base;
    }
    (2..).map(|n| format!("{base}_{n}")).find(|c| taken.insert(c.clone())).unwrap()
}

/// Import `psd` (a file under some project's `assets/`).
pub fn import_psd(psd: &Path, options: &ImportOptions) -> Result<ImportReport> {
    let assets = assets_root_of(psd).with_context(|| format!("{} is not under an assets directory", psd.display()))?;
    let data = std::fs::read(psd).with_context(|| format!("reading {}", psd.display()))?;
    let doc = read_psd(&data).with_context(|| format!("reading {}", psd.display()))?;
    let stem = psd.file_stem().and_then(|s| s.to_str()).unwrap_or("document").to_string();
    let sprite_dir = format!("sprites/{}", file_stem(&stem));
    std::fs::create_dir_all(assets.join(&sprite_dir))?;

    let mut report = ImportReport { prefab: format!("prefabs/{stem}.prefab"), ..Default::default() };
    let (ox, oy) = match options.origin {
        Origin::Centre => (doc.width as f32 / 2.0, doc.height as f32 / 2.0),
        Origin::Bottom => (doc.width as f32 / 2.0, doc.height as f32),
    };
    // A canvas point as the root's local position (y up).
    let local = |x: f32, y: f32| [x - ox, oy - y];

    let kept: Vec<&PsdLayer> = doc
        .layers
        .iter()
        .filter(|l| {
            let why = if l.width == 0 || l.height == 0 {
                Some("empty")
            } else if !l.visible && !options.include_hidden {
                Some("hidden")
            } else {
                None
            };
            if let Some(why) = why {
                report.skipped.push(format!("{} ({why})", path_of(l)));
            }
            why.is_none()
        })
        .collect();
    if kept.is_empty() {
        bail!("{} has no visible pixel layers", psd.display());
    }

    // The pictures.
    let mut taken = HashSet::new();
    let mut textures = Vec::with_capacity(kept.len());
    for layer in &kept {
        let parts: Vec<String> = layer.groups.iter().chain([&layer.name]).map(|n| file_stem(n)).collect();
        let name = unique(parts.join("_"), &mut taken);
        let rel = format!("{sprite_dir}/{name}.png");
        let img = image::RgbaImage::from_raw(layer.width, layer.height, layer.rgba.clone())
            .context("a layer's pixels do not fill its bounds")?;
        img.save(assets.join(&rel)).with_context(|| format!("writing {rel}"))?;
        report.sprites.push(rel.clone());
        textures.push(rel);
    }

    // The parts, as a tree of groups. Each node: a name, its layers' and
    // groups' children in draw order.
    #[derive(Default)]
    struct Node {
        name: String,
        children: Vec<Child>,
    }
    enum Child {
        Layer(usize),
        Group(Node),
    }
    let mut root = Node::default();
    for (i, layer) in kept.iter().enumerate() {
        let mut at = &mut root;
        for g in &layer.groups {
            // Join the group if it is the last child (a group's layers are
            // contiguous); else open it.
            let reuse = matches!(at.children.last(), Some(Child::Group(n)) if &n.name == g);
            if !reuse {
                at.children.push(Child::Group(Node { name: g.clone(), children: Vec::new() }));
            }
            let Some(Child::Group(n)) = at.children.last_mut() else { unreachable!() };
            at = n;
        }
        at.children.push(Child::Layer(i));
    }

    // A node's box on the canvas, for placing a group at its middle.
    fn bounds(node: &Node, kept: &[&PsdLayer]) -> (f32, f32, f32, f32) {
        let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for c in &node.children {
            let (x0, y0, x1, y1) = match c {
                Child::Layer(i) => {
                    let l = kept[*i];
                    (l.left as f32, l.top as f32, (l.left + l.width as i32) as f32, (l.top + l.height as i32) as f32)
                }
                Child::Group(n) => bounds(n, kept),
            };
            b = (b.0.min(x0), b.1.min(y0), b.2.max(x1), b.3.max(y1));
        }
        b
    }

    let transform = |p: [f32; 2]| json!({ "position": p, "rotation": 0.0, "scale": [1.0, 1.0] });
    let mut names_taken = HashSet::new();
    // Parts, with their positions relative to `parent` (a canvas point).
    fn parts(
        node: &Node,
        parent: [f32; 2],
        kept: &[&PsdLayer],
        textures: &[String],
        local: &dyn Fn(f32, f32) -> [f32; 2],
        transform: &dyn Fn([f32; 2]) -> Value,
        names: &mut HashSet<String>,
    ) -> Vec<Value> {
        let rel = |p: [f32; 2]| [p[0] - parent[0], p[1] - parent[1]];
        node.children
            .iter()
            .map(|c| match c {
                Child::Layer(i) => {
                    let l = kept[*i];
                    let centre = local(l.left as f32 + l.width as f32 / 2.0, l.top as f32 + l.height as f32 / 2.0);
                    let mut components = json!({
                        "transform": transform(rel(centre)),
                        "sprite": {
                            "texture_id": textures[*i],
                            "color": [1.0, 1.0, 1.0, (l.opacity as f64 / 255.0 * 1000.0).round() / 1000.0],
                            "z_order": *i as i32,
                            "size": [l.width as f32, l.height as f32],
                            "anchor": [0.5, 0.5],
                        },
                    });
                    let blend = match l.blend.as_str() {
                        "mul " => Some("Multiply"),
                        "scrn" => Some("Screen"),
                        "lddg" => Some("Additive"),
                        _ => None,
                    };
                    if let Some(b) = blend {
                        components["material"] = json!({ "material_id": "default_sprite", "blend_mode": b });
                    }
                    json!({ "name": unique(l.name.clone(), names), "components": components })
                }
                Child::Group(n) => {
                    let (x0, y0, x1, y1) = bounds(n, kept);
                    let centre = local((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                    json!({
                        "name": unique(n.name.clone(), names),
                        "components": { "transform": transform(rel(centre)) },
                        "children": parts(n, centre, kept, textures, local, transform, names),
                    })
                }
            })
            .collect()
    }
    let children = parts(&root, [0.0, 0.0], &kept, &textures, &local, &transform, &mut names_taken);
    let prefab = json!({
        "name": stem,
        "components": {
            "transform": transform([0.0, 0.0]),
            "sorting_group": { "z_order": 0 },
        },
        "children": children,
    });
    std::fs::create_dir_all(assets.join("prefabs"))?;
    std::fs::write(assets.join(&report.prefab), serde_json::to_string_pretty(&prefab)? + "\n")
        .with_context(|| format!("writing {}", report.prefab))?;
    Ok(report)
}

fn path_of(l: &PsdLayer) -> String {
    l.groups.iter().chain([&l.name]).cloned().collect::<Vec<_>>().join("/")
}
