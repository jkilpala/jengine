//! Editor extension registry — schema-driven UI extensibility for packages.
//!
//! Packages describe their editor needs (component inspector fields,
//! menus, toolbar actions) via data descriptors rather than code.
//! The editor reads these descriptors and renders generic UI accordingly.

use serde::{Deserialize, Serialize};

// ──────────────────────────── Field Descriptors ────────────────────────────

/// Type of a component field — drives which widget the editor renders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FieldType {
    /// Floating-point number. Optional range for slider widget.
    Float {
        #[serde(skip_serializing_if = "Option::is_none")]
        min: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        max: Option<f32>,
    },
    /// Integer number. Optional range for slider widget.
    Int {
        #[serde(skip_serializing_if = "Option::is_none")]
        min: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        max: Option<i32>,
    },
    /// UTF-8 string.
    String,
    /// Boolean — rendered as checkbox.
    Bool,
    /// Two-component vector — rendered as X/Y sub-fields.
    Vec2,
    /// RGBA color — rendered as color picker.
    Color,
    /// RGB color, three numbers - most lights' `color`.
    Color3,
    /// A list of points (`[[x, y], ...]`) - a spline shape's control points.
    /// Rendered as rows of X/Y with add and remove.
    Points,
    /// A curve over 0..1 (`[[t, value], ...]`) - a line's width, a
    /// particle's size over life. Rendered as a graph with draggable keys.
    Curve,
    /// Colour stops over 0..1 (`[[t, r, g, b, a], ...]`, linear) - a line's
    /// gradient, a particle's colour over life. Rendered as a gradient bar.
    Gradient,
    /// Any JSON value, edited as text - a field with no widget of its own
    /// (an object, a map, a list of objects), so none is left out.
    Json,
    /// Dropdown with fixed choices.
    Enum {
        /// List of possible values.
        options: Vec<String>,
    },
    /// Asset reference — rendered as file picker with type filter.
    AssetRef {
        /// Accepted file extensions (e.g. ["png", "jpg"]).
        extensions: Vec<String>,
    },
}

/// Describes a single field of a package-provided component for the editor inspector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentFieldDescriptor {
    /// JSON key / struct field name (e.g. "mass", "gravity_scale").
    pub name: String,
    /// Human-readable label (e.g. "Mass", "Gravity Scale").
    pub display_name: String,
    /// Field data type — drives which widget to render.
    pub field_type: FieldType,
    /// Default value as JSON (used when adding the component).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<serde_json::Value>,
    /// Tooltip / description shown on hover.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// Editor metadata for a component type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentEditorInfo {
    /// JSON key matching the `ComponentDescriptor::name` in the component registry.
    pub component_name: String,
    /// Category in the "Add Component" menu (e.g. "Physics", "Audio").
    pub category: String,
    /// Ordered list of field descriptors for the inspector.
    pub fields: Vec<ComponentFieldDescriptor>,
    /// The whole component as added - fields the inspector cannot edit
    /// included, or adding it would drop them. Derived schemas carry it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
}

// ──────────────────────────── Menu / Toolbar ────────────────────────────

/// Inspector metadata for a registered component, derived from the component
/// itself: its default (what `{}` deserializes to) gives the fields and their
/// widgets, and serde gives an enum's options - so every component the
/// engine registers can be added and edited in the editor without a
/// hand-written section or a hand-kept list. `None` for a component that
/// cannot be built from `{}` (it has a required field): the editor keeps
/// showing its raw data.
///
/// Until Sep 2026 the editor knew components only from a hand-kept list
/// (`componentDefs.ts`) and from the few packages that wrote this metadata
/// by hand, so every component added since - `trail`, `toon_lighting`,
/// `directional_light`, `ambient_occlusion`, `mesh`, ... - could be neither
/// added nor edited there: it showed as "No schema - raw data attached".
pub fn derived_component_info(
    desc: &crate::package::component_registry::ComponentDescriptor,
) -> Option<ComponentEditorInfo> {
    let default = match desc.default_fn {
        Some(make) => make(),
        None => (desc.deserialize_fn)(&serde_json::json!({}))?,
    };
    let json = (desc.serialize_fn)(&*default);
    // A unit marker (`BulkEntity`) serializes as `null`: no fields, but it
    // can still be added.
    let empty = serde_json::Map::new();
    let obj = match &json {
        serde_json::Value::Null => &empty,
        other => other.as_object()?,
    };
    // Every field serde reads, in declaration order - the ones the default
    // leaves out when empty or `None` included - then anything the default
    // has that the list lacks (a flattened struct has no list).
    let mut names: Vec<String> = (desc.fields_fn)().unwrap_or(&[]).iter().map(|n| n.to_string()).collect();
    for key in obj.keys() {
        if !names.contains(key) {
            names.push(key.clone());
        }
    }
    let fields = names
        .iter()
        .filter_map(|name| match obj.get(name) {
            Some(value) => Some(derive_field(desc, obj, name, value)),
            None => derive_absent_field(desc, obj, name),
        })
        .collect();
    Some(ComponentEditorInfo {
        component_name: desc.name.clone(),
        category: desc.category.clone(),
        fields,
        default: Some(if json.is_null() { serde_json::json!({}) } else { json.clone() }),
    })
}

/// "shadow_softness" -> "Shadow Softness".
fn display_name(name: &str) -> String {
    name.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_colour(name: &str) -> bool {
    ["color", "colour", "tint"].iter().any(|k| name.contains(k))
}

/// The file types a string field names, by its name.
fn asset_extensions(name: &str) -> Option<Vec<String>> {
    let ext: &[&str] = if name.contains("texture") || name.contains("normal_map") || name.ends_with("image_id") {
        &["png", "jpg", "jpeg", "webp"]
    } else if name == "material_id" {
        &["material"]
    } else if name == "gui_id" {
        &["gui"]
    } else if name == "font_id" {
        &["font"]
    } else if name == "clip_id" {
        &["wav", "ogg", "mp3"]
    } else if name == "source_path" {
        &["lua", "js"]
    } else if name.ends_with("prefab") {
        &["prefab"]
    } else {
        return None;
    };
    Some(ext.iter().map(|e| e.to_string()).collect())
}

/// The component's default with one field replaced, checked by serde.
fn probe(
    desc: &crate::package::component_registry::ComponentDescriptor,
    obj: &serde_json::Map<String, serde_json::Value>,
    name: &str,
    value: serde_json::Value,
) -> Result<(), String> {
    let mut o = obj.clone();
    o.insert(name.to_string(), value);
    (desc.check_fn)(&serde_json::Value::Object(o))
}

/// An enum's variant names from serde's message for an unknown one:
/// "unknown variant `x`, expected one of `A`, `B`" or "expected `A` or `B`".
fn enum_options(message: &str) -> Option<Vec<String>> {
    let rest = &message[message.find("expected")?..];
    let rest = rest.split(" at line ").next().unwrap_or(rest);
    let options: Vec<String> = rest.split('`').skip(1).step_by(2).map(str::to_string).collect();
    (!options.is_empty()).then_some(options)
}

/// Whether the component, with `name` set to `value`, deserializes and
/// serializes back with `name` still in it - which a field skipped when empty
/// does once it is not empty, and an alias of another field never does.
fn keeps(
    desc: &crate::package::component_registry::ComponentDescriptor,
    obj: &serde_json::Map<String, serde_json::Value>,
    name: &str,
    value: serde_json::Value,
) -> bool {
    let mut o = obj.clone();
    o.insert(name.to_string(), value);
    (desc.deserialize_fn)(&serde_json::Value::Object(o))
        .map(|c| (desc.serialize_fn)(&*c))
        .is_some_and(|v| v.get(name).is_some())
}

fn string_type(name: &str) -> FieldType {
    asset_extensions(name).map_or(FieldType::String, |extensions| FieldType::AssetRef { extensions })
}

fn descriptor(name: &str, field_type: FieldType, default_value: Option<serde_json::Value>) -> ComponentFieldDescriptor {
    ComponentFieldDescriptor {
        name: name.to_string(),
        display_name: display_name(name),
        field_type,
        default_value,
        description: String::new(),
    }
}

/// A field the default has: typed by its value, and by serde where the value
/// says too little (a string that may be an enum, an empty list). Anything
/// with no widget of its own - an object, a map, a list of objects - is
/// `Json`, edited as text, so no field is left out of the Inspector.
fn derive_field(
    desc: &crate::package::component_registry::ComponentDescriptor,
    obj: &serde_json::Map<String, serde_json::Value>,
    name: &str,
    value: &serde_json::Value,
) -> ComponentFieldDescriptor {
    use serde_json::Value;
    let number_array = |a: &Vec<Value>| a.iter().all(Value::is_number);
    let rows_of = |a: &Vec<Value>, width: usize| {
        a.iter().all(|p| p.as_array().is_some_and(|p| p.len() == width && number_array(p)))
    };
    let curve_named = name.contains("curve");
    let gradient_named = curve_named || name.contains("gradient");
    let field_type = match value {
        Value::Bool(_) => FieldType::Bool,
        Value::Number(n) if n.is_f64() => FieldType::Float { min: None, max: None },
        Value::Number(_) => FieldType::Int { min: None, max: None },
        Value::String(_) => match probe(desc, obj, name, Value::String("\u{1}probe".into())) {
            Ok(()) => string_type(name),
            Err(e) => enum_options(&e).map_or(FieldType::Json, |options| FieldType::Enum { options }),
        },
        Value::Array(a) if a.len() == 2 && number_array(a) => FieldType::Vec2,
        Value::Array(a) if !a.is_empty() && rows_of(a, 2) => {
            if curve_named { FieldType::Curve } else { FieldType::Points }
        }
        Value::Array(a) if !a.is_empty() && rows_of(a, 5) && gradient_named => FieldType::Gradient,
        // An empty list says nothing about its rows, so ask serde which
        // rows it takes - and leave a list that takes anything as text.
        Value::Array(a) if a.is_empty() => {
            let takes = |v: Value| probe(desc, obj, name, Value::Array(vec![v])).is_ok();
            if takes(serde_json::json!(["\u{1}probe"])) {
                FieldType::Json
            } else if takes(serde_json::json!([0.0, 1.0])) {
                if curve_named { FieldType::Curve } else { FieldType::Points }
            } else if takes(serde_json::json!([0.0, 1.0, 1.0, 1.0, 1.0])) && gradient_named {
                FieldType::Gradient
            } else {
                FieldType::Json
            }
        }
        Value::Array(a) if a.len() == 3 && number_array(a) && is_colour(name) => FieldType::Color3,
        Value::Array(a) if a.len() == 4 && number_array(a) && is_colour(name) => FieldType::Color,
        // An optional value that is absent by default: a string if one is
        // accepted there (a region, a sort layer), else text.
        Value::Null => match probe(desc, obj, name, Value::String("\u{1}probe".into())) {
            Ok(()) => string_type(name),
            Err(_) => FieldType::Json,
        },
        _ => FieldType::Json,
    };
    descriptor(name, field_type, Some(value.clone()))
}

/// A field the default leaves out (`skip_serializing_if` an empty list or a
/// `None`): typed by asking serde which value it keeps there. It has no
/// default value - adding the component leaves it out, as the engine does.
/// `None` for an alias of another field (serde lists aliases beside names):
/// a value it accepts, it never keeps under its own name.
fn derive_absent_field(
    desc: &crate::package::component_registry::ComponentDescriptor,
    obj: &serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> Option<ComponentFieldDescriptor> {
    use serde_json::{Value, json};
    let text = Value::String("\u{1}probe".into());
    let curve_named = name.contains("curve");
    let gradient_named = curve_named || name.contains("gradient");
    let enum_options = probe(desc, obj, name, text.clone()).err().and_then(|e| enum_options(&e));
    let candidates: Vec<(Value, Option<Value>, FieldType)> = vec![
        // (a value, one it must *not* keep, the field type)
        (text.clone(), Some(json!(true)), string_type(name)),
        (text.clone(), None, FieldType::Json),
        (json!(true), None, FieldType::Bool),
        (json!(1.5), None, FieldType::Float { min: None, max: None }),
        (json!(1), None, FieldType::Int { min: None, max: None }),
        // A struct before any list: serde reads a struct from an array of
        // its fields too, so `[[0, 1]]` would pass for points.
        (json!({}), None, FieldType::Json),
        (json!([0.5, 1.0]), Some(json!([0.5, 1.0, 2.0])), FieldType::Vec2),
        (json!([1.0, 1.0, 1.0, 1.0]), Some(json!([1.0, 1.0, 1.0])), if is_colour(name) { FieldType::Color } else { FieldType::Json }),
        (json!([1.0, 1.0, 1.0]), Some(json!([1.0, 1.0])), if is_colour(name) { FieldType::Color3 } else { FieldType::Json }),
        (json!([[0.0, 1.0]]), Some(json!([["\u{1}probe"]])), if curve_named { FieldType::Curve } else { FieldType::Points }),
        (json!([[0.0, 1.0, 1.0, 1.0, 1.0]]), Some(json!([["\u{1}probe"]])), if gradient_named { FieldType::Gradient } else { FieldType::Json }),
        (json!([{}]), None, FieldType::Json),
    ];
    // Any option will do: the default one is usually skipped when saved.
    if let Some(options) = enum_options
        && options.iter().any(|o| keeps(desc, obj, name, Value::String(o.clone())))
    {
        return Some(descriptor(name, FieldType::Enum { options }, None));
    }
    for (value, not, field_type) in &candidates {
        if keeps(desc, obj, name, value.clone()) && !not.as_ref().is_some_and(|n| keeps(desc, obj, name, n.clone())) {
            return Some(descriptor(name, field_type.clone(), None));
        }
    }
    // Nothing round-trips. Accepted but not kept is an alias; accepted by
    // nothing is a value no probe guessed (an optional struct with required
    // fields), still editable as text.
    let accepted = candidates.iter().any(|(v, _, _)| probe(desc, obj, name, v.clone()).is_ok());
    (!accepted).then(|| descriptor(name, FieldType::Json, None))
}

/// A menu item contributed by a package to the editor toolbar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuItemDescriptor {
    /// Menu path (e.g. "Physics/Debug Colliders").
    pub path: String,
    /// Tauri command name to invoke.
    pub command: String,
    /// Optional keyboard shortcut (e.g. "Ctrl+Shift+P").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
}

/// A toolbar button contributed by a package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolbarAction {
    /// Button label / tooltip.
    pub label: String,
    /// Emoji or icon identifier.
    pub icon: String,
    /// Tauri command name to invoke.
    pub command: String,
}

// ──────────────────────────── Panel (Future) ────────────────────────────

/// Descriptor for a custom editor panel shipped by a package.
///
/// Packages register panels via `EditorRegistry::add_panel()` during init.
/// The editor loads the web bundle HTML into a sandboxed iframe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelDescriptor {
    /// Panel name shown in tab.
    pub name: String,
    /// Emoji or icon identifier.
    pub icon: String,
    /// Default dock zone ("left", "right", "bottom", "center").
    pub default_zone: String,
    /// Relative path to the web bundle entry HTML file (within the package).
    pub web_bundle_path: String,
    /// Resolved absolute path to the package directory on disk.
    /// Set by the editor at load time — not by the package itself.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub package_path: String,
}

// ──────────────────────────── Editor Registry ────────────────────────────

/// Collects all editor extensions from packages.
///
/// Populated during engine init via `JEnginePackage::register_editor()`,
/// then serialized and sent to the Svelte editor frontend.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EditorRegistry {
    /// Component inspector metadata, keyed by component name.
    pub components: Vec<ComponentEditorInfo>,
    /// Extra menu items.
    pub menu_items: Vec<MenuItemDescriptor>,
    /// Extra toolbar buttons.
    pub toolbar_actions: Vec<ToolbarAction>,
    /// Custom editor panels contributed by packages.
    pub panels: Vec<PanelDescriptor>,
}

impl EditorRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register editor metadata for a component.
    pub fn add_component_info(&mut self, info: ComponentEditorInfo) {
        log::trace!(
            "Editor component registered: {} (category={})",
            info.component_name,
            info.category
        );
        self.components.push(info);
    }

    /// Register a menu item.
    pub fn add_menu_item(&mut self, item: MenuItemDescriptor) {
        self.menu_items.push(item);
    }

    /// Register a toolbar action.
    pub fn add_toolbar_action(&mut self, action: ToolbarAction) {
        self.toolbar_actions.push(action);
    }

    /// Register a panel descriptor (Phase 7).
    pub fn add_panel(&mut self, panel: PanelDescriptor) {
        self.panels.push(panel);
    }

    /// Get component editor info by component name.
    pub fn get_component_info(&self, component_name: &str) -> Option<&ComponentEditorInfo> {
        self.components
            .iter()
            .find(|c| c.component_name == component_name)
    }
}
