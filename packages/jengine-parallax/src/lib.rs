//! Parallax scrolling background package for jEngine.
//!
//! Provides a `ParallaxLayer` component that offsets an entity's rendered
//! position based on the active camera's position, creating a depth-based
//! scrolling effect.  Layers with `scroll_factor` < 1.0 scroll slower than
//! the camera (background), layers with > 1.0 scroll faster (foreground).

use jengine_core::ecs::components::{Camera2DComponent, SpriteComponent, Transform};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::editor_registry::EditorRegistry;
use jengine_core::package::script_api::{ScriptApiRegistry, ScriptArg, ScriptValueType};
use jengine_core::package::script_commands::PackageCommandRegistry;
use jengine_core::package::system_schedule::{SystemPhase, SystemSchedule};
use jengine_core::package::{JEnginePackage, PackageMetadata};
use jengine_core::scripting::host_commands::{CommandArgs, ScriptWorldHost};

use serde::{Deserialize, Serialize};

// ──────────────────────────── Component ────────────────────────────

/// A parallax scrolling layer.
///
/// Attach to an entity with a `Transform` and `SpriteComponent`. During the
/// PreRender phase the system offsets the entity's position so it scrolls at
/// `scroll_factor` relative to the camera.
///
/// # Fields
/// * `scroll_factor_x` / `scroll_factor_y` — multiplier on camera displacement.
///   0.0 = locked in place (skybox), 0.5 = half-speed, 1.0 = normal, >1.0 = foreground.
/// * `repeat_x` / `repeat_y` — whether the layer tiles infinitely along that axis:
///   its position wraps by `repeat_width` / `repeat_height` to stay about the
///   camera. For a seamless strip, give N copies of a W-wide image offsets
///   0, W, 2W ... and a `repeat_width` of N x W (W at least the view's width).
///   0 uses the sprite's `size`.
/// * `offset_x` / `offset_y` — base position offset applied before parallax math.
/// * `auto_scroll_x` / `auto_scroll_y` — constant scroll speed (pixels/sec), independent of camera.
/// * `z_order_override` — if `Some`, overrides the sprite's `z_order` for draw sorting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallaxLayer {
    /// Horizontal scroll factor (0.0–1.0 for background, >1.0 for foreground).
    #[serde(default = "default_scroll_factor")]
    pub scroll_factor_x: f32,
    /// Vertical scroll factor.
    #[serde(default = "default_scroll_factor")]
    pub scroll_factor_y: f32,

    /// Whether the layer repeats horizontally.
    #[serde(default)]
    pub repeat_x: bool,
    /// Whether the layer repeats vertically.
    #[serde(default)]
    pub repeat_y: bool,
    /// The span it wraps by horizontally; 0 is the sprite's `size` x.
    #[serde(default)]
    pub repeat_width: f32,
    /// The span it wraps by vertically; 0 is the sprite's `size` y.
    #[serde(default)]
    pub repeat_height: f32,

    /// Static offset applied before parallax calculation.
    #[serde(default)]
    pub offset_x: f32,
    /// Static offset applied before parallax calculation.
    #[serde(default)]
    pub offset_y: f32,

    /// Automatic scroll speed in pixels/sec (X axis). Scrolls regardless of camera.
    #[serde(default)]
    pub auto_scroll_x: f32,
    /// Automatic scroll speed in pixels/sec (Y axis).
    #[serde(default)]
    pub auto_scroll_y: f32,

    /// Optional z-order override for draw-sorting (applied to SpriteComponent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z_order_override: Option<i32>,

    // ── internal runtime state (not serialized) ──
    /// Accumulated auto-scroll displacement (X).
    #[serde(skip)]
    pub(crate) auto_accum_x: f32,
    /// Accumulated auto-scroll displacement (Y).
    #[serde(skip)]
    pub(crate) auto_accum_y: f32,
}

fn default_scroll_factor() -> f32 {
    0.5
}

impl Default for ParallaxLayer {
    fn default() -> Self {
        Self {
            scroll_factor_x: 0.5,
            scroll_factor_y: 0.5,
            repeat_x: false,
            repeat_y: false,
            repeat_width: 0.0,
            repeat_height: 0.0,
            offset_x: 0.0,
            offset_y: 0.0,
            auto_scroll_x: 0.0,
            auto_scroll_y: 0.0,
            z_order_override: None,
            auto_accum_x: 0.0,
            auto_accum_y: 0.0,
        }
    }
}

// ──────────────────────────── System ────────────────────────────

/// Update parallax layer positions based on the active camera.
///
/// For each entity with `ParallaxLayer` + `Transform`, computes the visual
/// offset so the layer scrolls at the configured rate relative to camera motion.
fn parallax_system(ctx: &mut jengine_core::package::system_schedule::SystemContext) {
    let (world, dt, _resources) = ctx.split();

    // Find the active camera transform position
    let camera_pos: [f32; 2] = {
        let mut pos = [0.0f32, 0.0f32];
        for (_, (transform, cam)) in world
            .query::<(&Transform, &Camera2DComponent)>()
            .iter()
        {
            if cam.is_active {
                pos = [transform.position[0], transform.position[1]];
                break;
            }
        }
        pos
    };

    // Update each parallax layer entity
    for (_, (transform, layer, sprite)) in world
        .query_mut::<(&mut Transform, &mut ParallaxLayer, Option<&mut SpriteComponent>)>()
    {
        // Accumulate auto-scroll
        layer.auto_accum_x += layer.auto_scroll_x * dt;
        layer.auto_accum_y += layer.auto_scroll_y * dt;

        // Parallax offset: camera_pos * (1.0 - scroll_factor)
        // scroll_factor=0 → layer stays fixed (moves opposite to camera)
        // scroll_factor=1 → layer moves with scene (no parallax)
        let parallax_x = camera_pos[0] * (1.0 - layer.scroll_factor_x);
        let parallax_y = camera_pos[1] * (1.0 - layer.scroll_factor_y);

        let mut x = layer.offset_x + parallax_x + layer.auto_accum_x;
        let mut y = layer.offset_y + parallax_y + layer.auto_accum_y;
        let size = sprite.as_ref().and_then(|s| s.size).unwrap_or([0.0, 0.0]);
        if layer.repeat_x {
            x = wrap_about(x, camera_pos[0], if layer.repeat_width > 0.0 { layer.repeat_width } else { size[0] });
        }
        if layer.repeat_y {
            y = wrap_about(y, camera_pos[1], if layer.repeat_height > 0.0 { layer.repeat_height } else { size[1] });
        }
        transform.position[0] = x;
        transform.position[1] = y;
        if let (Some(z), Some(s)) = (layer.z_order_override, sprite) {
            s.z_order = z;
        }
    }
}

/// `v` moved by whole `span`s into `[centre - span / 2, centre + span / 2)`.
fn wrap_about(v: f32, centre: f32, span: f32) -> f32 {
    if span <= 0.0 {
        return v;
    }
    centre + (v - centre + span * 0.5).rem_euclid(span) - span * 0.5
}

#[cfg(test)]
mod wrap_tests {
    use super::*;

    #[test]
    fn wrapping_keeps_a_layer_about_the_camera() {
        assert_eq!(wrap_about(100.0, 0.0, 1000.0), 100.0);
        assert_eq!(wrap_about(-700.0, 0.0, 1000.0), 300.0);
        assert_eq!(wrap_about(2600.0, 3000.0, 1000.0), 2600.0);
        assert_eq!(wrap_about(1000.0, 3000.0, 1000.0), 3000.0);
        assert_eq!(wrap_about(42.0, 9999.0, 0.0), 42.0);
    }
}

// ──────────────────────────── Script commands ────────────────────────────

/// Run `f` on the `parallax_layer` of the command's `e`, with its `x` / `y`.
fn with_layer(
    host: &mut dyn ScriptWorldHost,
    args: &dyn CommandArgs,
    command: &str,
    f: impl FnOnce(&mut ParallaxLayer, f32, f32),
) {
    let Some(e) = args.number("e").filter(|v| *v >= 0.0).and_then(|v| host.resolve_entity(v as u32)) else {
        log::warn!("[parallax] {command}: no such entity");
        return;
    };
    let x = args.number("x").unwrap_or(0.0) as f32;
    let y = args.number("y").unwrap_or(0.0) as f32;
    match host.world().get::<&mut ParallaxLayer>(e) {
        Ok(mut layer) => f(&mut layer, x, y),
        Err(_) => log::warn!("[parallax] {command}: entity has no `parallax_layer`"),
    }
}

// ──────────────────────────── Package ────────────────────────────

/// The parallax scrolling package.
pub struct ParallaxPackage;

impl JEnginePackage for ParallaxPackage {
    fn metadata(&self) -> PackageMetadata {
        PackageMetadata {
            name: "jengine-parallax".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "Multi-layer parallax scrolling backgrounds".to_string(),
            author: String::new(),
            engine_version: None,
            dependencies: Vec::new(),
        }
    }

    fn register_components(&self, registry: &mut ComponentRegistry) {
        registry.register::<ParallaxLayer>(
            "parallax_layer",
            "Parallax Layer",
            "Rendering",
        );
    }

    fn register_systems(&self, schedule: &mut SystemSchedule) {
        schedule.add_system(
            "parallax_scroll",
            SystemPhase::PreRender,
            0,
            parallax_system,
        );
    }

    fn register_script_api(&self, api: &mut ScriptApiRegistry) {
        use jengine_core::scripting::bridge::command::BridgeCommand;
        use jengine_core::scripting::bridge::value::Value;
        use std::sync::Arc;

        // Helper: build the SetComponentField command for one named field on
        // ParallaxLayer. Field names are interned via Arc<str> at construction
        // and cloned per call (one Arc bump, no allocation).
        fn set_field(commands: &mut Vec<BridgeCommand>, entity_id: u64, field: &Arc<str>, v: f64) {
            commands.push(BridgeCommand::SetComponentField {
                entity_id,
                component_name: COMPONENT_NAME.with(Arc::clone),
                field_name: Arc::clone(field),
                value: serde_json::json!(v),
            });
        }

        thread_local! {
            static COMPONENT_NAME: Arc<str> = Arc::<str>::from("parallax_layer");
            static SCROLL_FACTOR_X: Arc<str> = Arc::<str>::from("scroll_factor_x");
            static SCROLL_FACTOR_Y: Arc<str> = Arc::<str>::from("scroll_factor_y");
            static AUTO_SCROLL_X: Arc<str> = Arc::<str>::from("auto_scroll_x");
            static AUTO_SCROLL_Y: Arc<str> = Arc::<str>::from("auto_scroll_y");
        }

        api.add_function_with_handler(
            "set_parallax",
            "jengine-parallax",
            "Set parallax scroll factors on an entity",
            vec![
                ScriptArg { name: "entity_id".to_string(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "factor_x".to_string(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "factor_y".to_string(), arg_type: ScriptValueType::Float },
            ],
            ScriptValueType::Nil,
            |ctx, args| {
                let entity_id = match args.first() {
                    Some(Value::EntityId(id)) => *id,
                    Some(Value::Number(n)) => *n as u64,
                    _ => return Ok(Value::Nil),
                };
                let fx = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0);
                let fy = args.get(2).and_then(|v| v.as_number()).unwrap_or(0.0);
                SCROLL_FACTOR_X.with(|f| set_field(ctx.commands, entity_id, f, fx));
                SCROLL_FACTOR_Y.with(|f| set_field(ctx.commands, entity_id, f, fy));
                Ok(Value::Nil)
            },
        );

        api.add_function_with_handler(
            "set_parallax_auto_scroll",
            "jengine-parallax",
            "Set auto-scroll speed on a parallax layer entity",
            vec![
                ScriptArg { name: "entity_id".to_string(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "speed_x".to_string(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "speed_y".to_string(), arg_type: ScriptValueType::Float },
            ],
            ScriptValueType::Nil,
            |ctx, args| {
                let entity_id = match args.first() {
                    Some(Value::EntityId(id)) => *id,
                    Some(Value::Number(n)) => *n as u64,
                    _ => return Ok(Value::Nil),
                };
                let sx = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0);
                let sy = args.get(2).and_then(|v| v.as_number()).unwrap_or(0.0);
                AUTO_SCROLL_X.with(|f| set_field(ctx.commands, entity_id, f, sx));
                AUTO_SCROLL_Y.with(|f| set_field(ctx.commands, entity_id, f, sy));
                Ok(Value::Nil)
            },
        );
    }

    /// `Parallax.set(e, x, y)` and `Parallax.set_auto_scroll(e, x, y)`, in
    /// every runtime. The `set_parallax` handlers above are installed by the
    /// native `ScriptSystem` only, so they were `nil` in the editor and on
    /// the web; they stay for native scripts that already call them.
    fn register_script_commands(&self, commands: &mut PackageCommandRegistry) {
        let registered = commands
            .register("jengine-parallax", "parallax_set", &["e", "x", "y"], |host, args| {
                with_layer(host, args, "parallax_set", |layer, x, y| {
                    layer.scroll_factor_x = x;
                    layer.scroll_factor_y = y;
                });
            })
            .and_then(|_| {
                commands.register("jengine-parallax", "parallax_set_auto_scroll", &["e", "x", "y"], |host, args| {
                    with_layer(host, args, "parallax_set_auto_scroll", |layer, x, y| {
                        layer.auto_scroll_x = x;
                        layer.auto_scroll_y = y;
                    });
                })
            });
        if let Err(err) = registered {
            log::error!("[parallax] script commands not registered: {err}");
        }
    }

    fn register_editor(&self, editor: &mut EditorRegistry) {
        use jengine_core::package::editor_registry::{
            ComponentEditorInfo, ComponentFieldDescriptor, FieldType,
        };

        editor.add_component_info(ComponentEditorInfo {
            component_name: "parallax_layer".to_string(),
            category: "Rendering".to_string(),
            fields: vec![
                ComponentFieldDescriptor {
                    name: "scroll_factor_x".to_string(),
                    display_name: "Scroll Factor X".to_string(),
                    field_type: FieldType::Float {
                        min: Some(-10.0),
                        max: Some(10.0),
                    },
                    default_value: Some(serde_json::json!(0.5)),
                    description: "Horizontal scroll rate relative to camera (0=fixed, 1=normal)".to_string(),
                },
                ComponentFieldDescriptor {
                    name: "scroll_factor_y".to_string(),
                    display_name: "Scroll Factor Y".to_string(),
                    field_type: FieldType::Float {
                        min: Some(-10.0),
                        max: Some(10.0),
                    },
                    default_value: Some(serde_json::json!(0.5)),
                    description: "Vertical scroll rate relative to camera".to_string(),
                },
                ComponentFieldDescriptor {
                    name: "repeat_x".to_string(),
                    display_name: "Repeat X".to_string(),
                    field_type: FieldType::Bool,
                    default_value: Some(serde_json::json!(false)),
                    description: "Tile infinitely along X axis".to_string(),
                },
                ComponentFieldDescriptor {
                    name: "repeat_y".to_string(),
                    display_name: "Repeat Y".to_string(),
                    field_type: FieldType::Bool,
                    default_value: Some(serde_json::json!(false)),
                    description: "Tile infinitely along Y axis".to_string(),
                },
                ComponentFieldDescriptor {
                    name: "offset_x".to_string(),
                    display_name: "Offset X".to_string(),
                    field_type: FieldType::Float { min: None, max: None },
                    default_value: Some(serde_json::json!(0.0)),
                    description: "Base X offset before parallax".to_string(),
                },
                ComponentFieldDescriptor {
                    name: "offset_y".to_string(),
                    display_name: "Offset Y".to_string(),
                    field_type: FieldType::Float { min: None, max: None },
                    default_value: Some(serde_json::json!(0.0)),
                    description: "Base Y offset before parallax".to_string(),
                },
                ComponentFieldDescriptor {
                    name: "auto_scroll_x".to_string(),
                    display_name: "Auto Scroll X".to_string(),
                    field_type: FieldType::Float { min: None, max: None },
                    default_value: Some(serde_json::json!(0.0)),
                    description: "Constant horizontal scroll speed (px/sec)".to_string(),
                },
                ComponentFieldDescriptor {
                    name: "auto_scroll_y".to_string(),
                    display_name: "Auto Scroll Y".to_string(),
                    field_type: FieldType::Float { min: None, max: None },
                    default_value: Some(serde_json::json!(0.0)),
                    description: "Constant vertical scroll speed (px/sec)".to_string(),
                },
            ],
            default: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallax_layer_default() {
        let layer = ParallaxLayer::default();
        assert_eq!(layer.scroll_factor_x, 0.5);
        assert_eq!(layer.scroll_factor_y, 0.5);
        assert!(!layer.repeat_x);
        assert!(!layer.repeat_y);
        assert_eq!(layer.auto_scroll_x, 0.0);
    }

    #[test]
    fn parallax_layer_serialize_roundtrip() {
        let layer = ParallaxLayer {
            scroll_factor_x: 0.3,
            scroll_factor_y: 0.1,
            repeat_x: true,
            offset_x: 100.0,
            auto_scroll_x: 20.0,
            z_order_override: Some(-10),
            ..Default::default()
        };
        let json = serde_json::to_string(&layer).unwrap();
        let back: ParallaxLayer = serde_json::from_str(&json).unwrap();
        assert_eq!(back.scroll_factor_x, 0.3);
        assert_eq!(back.scroll_factor_y, 0.1);
        assert!(back.repeat_x);
        assert_eq!(back.offset_x, 100.0);
        assert_eq!(back.auto_scroll_x, 20.0);
        assert_eq!(back.z_order_override, Some(-10));
        // auto_accum should be 0 (skip-serialized)
        assert_eq!(back.auto_accum_x, 0.0);
    }

    #[test]
    fn package_metadata() {
        let pkg = ParallaxPackage;
        let meta = pkg.metadata();
        assert_eq!(meta.name, "jengine-parallax");
        assert!(!meta.description.is_empty());
    }

    #[test]
    fn component_registration() {
        let pkg = ParallaxPackage;
        let mut registry = ComponentRegistry::new();
        pkg.register_components(&mut registry);
        assert!(registry.get("parallax_layer").is_some());
    }

    #[test]
    fn deserialize_from_scene_json() {
        let pkg = ParallaxPackage;
        let mut registry = ComponentRegistry::new();
        pkg.register_components(&mut registry);

        let json = serde_json::json!({
            "scroll_factor_x": 0.2,
            "scroll_factor_y": 0.8,
            "repeat_x": true,
            "auto_scroll_x": 15.0
        });
        let desc = registry.get("parallax_layer").unwrap();
        let boxed = (desc.deserialize_fn)(&json);
        assert!(boxed.is_some(), "deserialize_fn should succeed");
        let any = boxed.unwrap();
        let layer = any.downcast_ref::<ParallaxLayer>().unwrap();
        assert_eq!(layer.scroll_factor_x, 0.2);
        assert_eq!(layer.scroll_factor_y, 0.8);
        assert!(layer.repeat_x);
        assert_eq!(layer.auto_scroll_x, 15.0);
    }
}
