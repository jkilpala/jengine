//! Op catalog — machine-readable descriptions of every behavior op.
//!
//! Slice E.1 of the node-graph editor plan
//! ([`docs/archive/step7-node-graph-editor-plan.md`](../../../../docs/archive/step7-node-graph-editor-plan.md)).
//! The runtime is the source of truth for what ops exist and what
//! pins they expose; the editor consumes this catalog to render the
//! palette, inline literal editors, and connection-type checks.
//!
//! The catalog is hand-written rather than reflected: the compiler in
//! [`crate::behavior::compile`] hand-writes each op's pin handling,
//! and there is no single registry table to introspect. To prevent
//! drift, [`tests::catalog_matches_op_kind_set`] asserts that every
//! op the compiler accepts (via [`crate::behavior::compile`]'s
//! `OpKind::parse`) is described here, and vice-versa.

use serde::Serialize;

/// Effect kind classification, mirroring `BehaviorEffectKind` from
/// [`docs/step7-behavior-graph-spec.md`](../../../../docs/step7-behavior-graph-spec.md)
/// §2. Strings are stable — used directly as JSON discriminants and
/// for the editor's category grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum EffectKind {
    Trigger,
    Read,
    Pure,
    Gate,
    DirectWrite,
    SetupComponent,
    StructuralCommand,
    SideEffect,
}

/// Pin value type. v1 pins are scalar / Vec2 / Bool / EntityRef / Aabb.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum PinType {
    Float,
    Vec2,
    Bool,
    EntityRef,
    Aabb,
    /// Enum-like string parameter (e.g. `Compare.op` → `"Eq"|"Lt"|...`).
    /// The catalog reports the allowed variants via
    /// [`ParamDescriptor::enum_variants`].
    Enum,
    /// Free-form string (e.g. `SpawnEntity.prefab_id`,
    /// `PlaySound.sound_id`).
    String,
}

/// Const-friendly default value, used for pin and param defaults
/// declared in `&'static` slices. Serialises as a JSON value of the
/// matching shape so the editor can drop it straight into the
/// `BehaviorDoc` JSON.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(untagged)]
pub enum DefaultValue {
    Float(f64),
    Vec2([f64; 2]),
    Bool(bool),
    Str(&'static str),
}

/// Description of one input or output data pin.
#[derive(Debug, Clone, Serialize)]
pub struct PinDescriptor {
    pub name: &'static str,
    pub ty: PinType,
    /// Default applied when the editor first creates a node and no
    /// source binding is supplied. `None` means the pin is required
    /// (compile error if unbound).
    pub default: Option<DefaultValue>,
    /// Whether the editor should expose `× scale` / `+ offset`
    /// modifiers when this input is wired to a producer pin. Always
    /// false for outputs.
    pub accepts_scale_offset: bool,
}

/// Description of a baked literal parameter (lives under
/// `node.params` in the on-disk JSON, not `node.in`).
#[derive(Debug, Clone, Serialize)]
pub struct ParamDescriptor {
    pub name: &'static str,
    pub ty: PinType,
    pub default: Option<DefaultValue>,
    /// For `PinType::Enum`, the allowed string values. Empty for
    /// non-enum params.
    pub enum_variants: Vec<&'static str>,
    /// Whether the param must be set before the graph compiles.
    pub required: bool,
}

/// Full description of one op. Stable JSON shape — the editor
/// caches this for the session.
#[derive(Debug, Clone, Serialize)]
pub struct OpDescriptor {
    pub op: &'static str,
    pub category: EffectKind,
    pub inputs: Vec<PinDescriptor>,
    pub outputs: Vec<PinDescriptor>,
    /// Whether the op accepts an inbound `exec` edge. Triggers have
    /// no input exec port; everything else does.
    pub exec_in: bool,
    /// Names of output exec ports. Most ops emit `["exec"]`; `If`
    /// emits `["then", "else"]`; producers, gates that have no body
    /// (`Cooldown`/`Once` route through `exec`), and pure data ops
    /// emit `[]`.
    pub exec_out: Vec<&'static str>,
    pub params: Vec<ParamDescriptor>,
    /// Short human-facing summary for tooltips. Stable text.
    pub summary: &'static str,
}

// ─── Helper constructors used inside the giant static catalog ────

fn pin(name: &'static str, ty: PinType) -> PinDescriptor {
    PinDescriptor {
        name,
        ty,
        default: None,
        accepts_scale_offset: matches!(ty, PinType::Float | PinType::Vec2),
    }
}

fn req_param(
    name: &'static str,
    ty: PinType,
    enum_variants: Vec<&'static str>,
) -> ParamDescriptor {
    ParamDescriptor {
        name,
        ty,
        default: None,
        enum_variants,
        required: true,
    }
}

// ─── The catalog itself. One entry per op in OpKind. ────────────

/// All v1 ops, in stable order grouped by [`EffectKind`].
///
/// Adding an op to [`crate::behavior::compile::OpKind`] without
/// adding a matching entry here will fail
/// [`tests::catalog_matches_op_kind_set`].
pub fn op_catalog() -> Vec<OpDescriptor> {
    vec![
        // ── Triggers (§2.1) ────────────────────────────────────────
        OpDescriptor {
            op: "OnTick",
            category: EffectKind::Trigger,
            inputs: vec![],
            // No `dt` output: it was advertised here and never given a slot,
            // so wiring it failed with "unknown node". `GetDeltaTime` reads it.
            outputs: vec![],
            exec_in: false,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Fires once per frame for every entity carrying the graph. Read the frame's dt with GetDeltaTime.",
        },
        OpDescriptor {
            op: "OnSpawn",
            category: EffectKind::Trigger,
            inputs: vec![],
            // No `self` output: every read op already reads this entity,
            // and nothing ever gave `self` a pin slot, so the palette offered
            // a pin that could not be wired.
            outputs: vec![],
            exec_in: false,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Fires the first frame the entity exists.",
        },
        OpDescriptor {
            op: "OnDespawn",
            category: EffectKind::Trigger,
            inputs: vec![],
            // No `self` output: every read op already reads this entity,
            // and nothing ever gave `self` a pin slot, so the palette offered
            // a pin that could not be wired.
            outputs: vec![],
            exec_in: false,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Fires the frame the entity is despawned. Position, rotation and scale read as they were last seen; anything else on the entity is already gone.",
        },
        OpDescriptor {
            op: "OnCollision",
            category: EffectKind::Trigger,
            inputs: vec![],
            outputs: vec![
                PinDescriptor { name: "other", ty: PinType::EntityRef, default: None, accepts_scale_offset: false },
                PinDescriptor { name: "enter", ty: PinType::Bool,      default: None, accepts_scale_offset: false },
            ],
            exec_in: false,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Fires on collision enter (`enter = true`) or exit (`enter = false`).",
        },
        OpDescriptor {
            op: "OnTrigger",
            category: EffectKind::Trigger,
            inputs: vec![],
            outputs: vec![
                PinDescriptor { name: "other", ty: PinType::EntityRef, default: None, accepts_scale_offset: false },
            ],
            exec_in: false,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Fires when another entity enters this entity's TriggerArea.",
        },
        OpDescriptor {
            op: "OnTimer",
            category: EffectKind::Trigger,
            inputs: vec![],
            outputs: vec![PinDescriptor {
                name: "tick_index",
                ty: PinType::Float,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec!["exec"],
            params: vec![ParamDescriptor {
                name: "interval_seconds",
                ty: PinType::Float,
                default: Some(DefaultValue::Float(1.0)),
                enum_variants: vec![],
                required: true,
            }],
            summary: "Fires every `interval_seconds`, persistently per-instance.",
        },
        OpDescriptor {
            op: "OnKey",
            category: EffectKind::Trigger,
            inputs: vec![],
            outputs: vec![],
            exec_in: false,
            exec_out: vec!["exec"],
            params: vec![
                ParamDescriptor {
                    name: "key",
                    ty: PinType::String,
                    default: None,
                    enum_variants: vec![],
                    required: true,
                },
                ParamDescriptor {
                    name: "edge",
                    ty: PinType::Enum,
                    default: Some(DefaultValue::Str("pressed")),
                    enum_variants: vec!["pressed", "released", "held"],
                    required: false,
                },
            ],
            summary: "Fires when a key transitions (pressed/released) or every frame while held. Only one OnKey per graph.",
        },
        OpDescriptor {
            op: "OnEvent",
            category: EffectKind::Trigger,
            inputs: vec![],
            outputs: vec![pin("value", PinType::Float)],
            exec_in: false,
            exec_out: vec!["exec"],
            params: vec![req_param("name", PinType::String, vec![])],
            summary: "Fires when a named event arrives - from another graph's EmitEvent, or a script's send_event (a number, a numeric string, or a table's `value`; else 0).",
        },
        OpDescriptor {
            op: "OnMouseClick",
            category: EffectKind::Trigger,
            inputs: vec![],
            outputs: vec![pin("pos", PinType::Vec2), pin("button", PinType::Float)],
            exec_in: false,
            exec_out: vec!["exec"],
            params: vec![ParamDescriptor {
                name: "button",
                ty: PinType::Enum,
                default: Some(DefaultValue::Str("any")),
                enum_variants: vec!["left", "right", "any"],
                required: false,
            }],
            summary: "Fires when the chosen mouse button transitions to pressed. Outputs click position and button index (0=left, 1=right).",
        },
        // ── Reads (§2.2) ───────────────────────────────────────────
        OpDescriptor {
            op: "GetInputAxis",
            category: EffectKind::Read,
            inputs: vec![],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Vec2,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Reads the current 2D input axis (e.g. WASD / left stick).",
        },
        OpDescriptor {
            op: "GetPosition",
            category: EffectKind::Read,
            inputs: vec![],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Vec2,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Reads the entity's Transform.position.",
        },
        OpDescriptor {
            op: "GetVelocity",
            category: EffectKind::Read,
            inputs: vec![],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Vec2,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Reads the entity's Velocity2D.",
        },
        OpDescriptor {
            op: "GetNearestByTag",
            category: EffectKind::Read,
            inputs: vec![],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::EntityRef,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![
                req_param("tag", PinType::String, vec![]),
                ParamDescriptor {
                    name: "exclude_self",
                    ty: PinType::Bool,
                    default: Some(DefaultValue::Bool(false)),
                    enum_variants: vec![],
                    required: false,
                },
            ],
            summary: "Finds the nearest entity carrying `tag` and outputs its EntityRef (empty if none). Set `exclude_self` to skip this entity (separation/flocking).",
        },
        OpDescriptor {
            op: "GetEntityPosition",
            category: EffectKind::Read,
            inputs: vec![PinDescriptor {
                name: "entity",
                ty: PinType::EntityRef,
                default: None,
                accepts_scale_offset: false,
            }],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Vec2,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Reads the Transform.position of the referenced entity. An empty or invalid ref resolves to this entity's own position (so movement toward it is a no-op).",
        },
        OpDescriptor {
            op: "GetCustomField",
            category: EffectKind::Read,
            inputs: vec![],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Float,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![
                req_param("component", PinType::String, vec![]),
                req_param("field", PinType::String, vec![]),
            ],
            summary: "Reads a numeric field from a custom component on this entity (0.0 if missing).",
        },
        OpDescriptor {
            op: "GetRotation",
            category: EffectKind::Read,
            inputs: vec![],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Float,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Reads the entity's Transform.rotation (radians).",
        },
        OpDescriptor {
            op: "GetScale",
            category: EffectKind::Read,
            inputs: vec![],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Vec2,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Reads the entity's Transform.scale.",
        },
        OpDescriptor {
            op: "GetDeltaTime",
            category: EffectKind::Read,
            inputs: vec![],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Float,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Reads the current frame's `dt` (seconds).",
        },
        // ── Pure logic (§2.3) ──────────────────────────────────────
        OpDescriptor {
            op: "Compare",
            category: EffectKind::Pure,
            inputs: vec![pin("a", PinType::Float), pin("b", PinType::Float)],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Bool,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![req_param(
                "op",
                PinType::Enum,
                vec!["Eq", "Ne", "Lt", "Le", "Gt", "Ge"],
            )],
            summary: "Compares two scalars and produces a Bool.",
        },
        OpDescriptor {
            op: "Not",
            category: EffectKind::Pure,
            inputs: vec![pin("in", PinType::Bool)],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Bool,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Inverts a Bool.",
        },
        OpDescriptor {
            op: "BoolOp",
            category: EffectKind::Pure,
            inputs: vec![pin("a", PinType::Bool), pin("b", PinType::Bool)],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Bool,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![req_param("op", PinType::Enum, vec!["And", "Or", "Xor"])],
            summary: "Binary boolean op (And/Or/Xor) on two Bools.",
        },
        OpDescriptor {
            op: "SelectFloat",
            category: EffectKind::Pure,
            inputs: vec![
                pin("cond", PinType::Bool),
                pin("a", PinType::Float),
                pin("b", PinType::Float),
            ],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Float,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Ternary float select: `cond ? a : b`.",
        },
        OpDescriptor {
            op: "SelectVec2",
            category: EffectKind::Pure,
            inputs: vec![
                pin("cond", PinType::Bool),
                pin("a", PinType::Vec2),
                pin("b", PinType::Vec2),
            ],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Vec2,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Ternary Vec2 select: `cond ? a : b`.",
        },
        OpDescriptor {
            op: "MakeVec2",
            category: EffectKind::Pure,
            inputs: vec![pin("x", PinType::Float), pin("y", PinType::Float)],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Vec2,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Pack two Floats into a Vec2.",
        },
        OpDescriptor {
            op: "GetVec2X",
            category: EffectKind::Pure,
            inputs: vec![pin("in", PinType::Vec2)],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Float,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Extract Vec2.x as a Float.",
        },
        OpDescriptor {
            op: "GetVec2Y",
            category: EffectKind::Pure,
            inputs: vec![pin("in", PinType::Vec2)],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Float,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Extract Vec2.y as a Float.",
        },
        OpDescriptor {
            op: "MathFloat",
            category: EffectKind::Pure,
            inputs: vec![pin("a", PinType::Float), pin("b", PinType::Float)],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Float,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![req_param(
                "op",
                PinType::Enum,
                vec!["Add", "Sub", "Mul", "Div", "Min", "Max", "Mod", "Pow"],
            )],
            summary: "Binary float arithmetic. Div/Mod by zero outputs 0.",
        },
        OpDescriptor {
            op: "MathVec2",
            category: EffectKind::Pure,
            inputs: vec![pin("a", PinType::Vec2), pin("b", PinType::Vec2)],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Vec2,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![req_param(
                "op",
                PinType::Enum,
                vec!["Add", "Sub", "Mul", "Div", "Min", "Max"],
            )],
            summary: "Componentwise binary Vec2 arithmetic.",
        },
        OpDescriptor {
            op: "ScaleVec2",
            category: EffectKind::Pure,
            inputs: vec![pin("v", PinType::Vec2), pin("s", PinType::Float)],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Vec2,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Multiply a Vec2 by a scalar.",
        },
        OpDescriptor {
            op: "Length",
            category: EffectKind::Pure,
            inputs: vec![pin("in", PinType::Vec2)],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Float,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Length of a Vec2.",
        },
        OpDescriptor {
            op: "Normalize",
            category: EffectKind::Pure,
            inputs: vec![pin("in", PinType::Vec2)],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Vec2,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Unit-length copy of a Vec2 (zero-length → ZERO).",
        },
        OpDescriptor {
            op: "Distance",
            category: EffectKind::Pure,
            inputs: vec![pin("a", PinType::Vec2), pin("b", PinType::Vec2)],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Float,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Euclidean distance between two Vec2s.",
        },
        OpDescriptor {
            op: "Dot",
            category: EffectKind::Pure,
            inputs: vec![pin("a", PinType::Vec2), pin("b", PinType::Vec2)],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Float,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Dot product of two Vec2s.",
        },
        OpDescriptor {
            op: "Lerp",
            category: EffectKind::Pure,
            inputs: vec![
                pin("a", PinType::Float),
                pin("b", PinType::Float),
                pin("t", PinType::Float),
            ],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Float,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Linear interpolation a + (b - a) * t. t is not clamped.",
        },
        OpDescriptor {
            op: "Clamp",
            category: EffectKind::Pure,
            inputs: vec![
                pin("in", PinType::Float),
                pin("min", PinType::Float),
                pin("max", PinType::Float),
            ],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Float,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "Clamp `in` into `[min, max]`.",
        },
        OpDescriptor {
            op: "MathUnary",
            category: EffectKind::Pure,
            inputs: vec![pin("in", PinType::Float)],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Float,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![req_param(
                "op",
                PinType::Enum,
                vec![
                    "Abs", "Sign", "Neg", "Sqrt", "Sin", "Cos", "Tan", "Floor", "Ceil", "Round",
                ],
            )],
            summary: "Unary float math. Sqrt of negatives outputs 0.",
        },
        OpDescriptor {
            op: "Atan2",
            category: EffectKind::Pure,
            inputs: vec![pin("y", PinType::Float), pin("x", PinType::Float)],
            outputs: vec![PinDescriptor {
                name: "value",
                ty: PinType::Float,
                default: None,
                accepts_scale_offset: false,
            }],
            exec_in: false,
            exec_out: vec![],
            params: vec![],
            summary: "atan2(y, x) in radians.",
        },
        // ── Gates (§2.4) ───────────────────────────────────────────
        OpDescriptor {
            op: "If",
            category: EffectKind::Gate,
            inputs: vec![pin("cond", PinType::Bool)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["then", "else"],
            params: vec![],
            summary: "Routes execution down `then` or `else` based on `cond`.",
        },
        OpDescriptor {
            op: "Cooldown",
            category: EffectKind::Gate,
            inputs: vec![pin("seconds", PinType::Float)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Suppresses downstream execution until `seconds` elapse since last fire.",
        },
        OpDescriptor {
            op: "Once",
            category: EffectKind::Gate,
            inputs: vec![],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Fires downstream the first time only; latches per-instance.",
        },
        OpDescriptor {
            op: "Delay",
            category: EffectKind::Gate,
            inputs: vec![pin("seconds", PinType::Float)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Suppresses downstream until `seconds` have elapsed since first visit, then fires every tick.",
        },
        OpDescriptor {
            op: "ForCount",
            category: EffectKind::Gate,
            inputs: vec![pin("count", PinType::Float)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Fires downstream up to `count` times per instance, then latches off.",
        },
        OpDescriptor {
            op: "Sequence",
            category: EffectKind::SideEffect,
            inputs: vec![],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "No-op passthrough; useful as an editor-side label/grouping anchor.",
        },
        // ── Direct writes (§2.5) ───────────────────────────────────
        OpDescriptor {
            op: "SetVelocity",
            category: EffectKind::DirectWrite,
            inputs: vec![pin("vel", PinType::Vec2)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Overwrites Velocity2D.",
        },
        OpDescriptor {
            op: "AddVelocity",
            category: EffectKind::DirectWrite,
            inputs: vec![pin("delta", PinType::Vec2)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Adds to Velocity2D.",
        },
        OpDescriptor {
            op: "Translate",
            category: EffectKind::DirectWrite,
            inputs: vec![pin("delta", PinType::Vec2)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Adds to Transform.position.",
        },
        OpDescriptor {
            op: "ApplyForce",
            category: EffectKind::DirectWrite,
            inputs: vec![pin("force", PinType::Vec2)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Adds to Force2D.",
        },
        // ── Setup components (§2.6) ────────────────────────────────
        OpDescriptor {
            op: "BounceOnBounds",
            category: EffectKind::SetupComponent,
            inputs: vec![],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![ParamDescriptor {
                name: "bounds",
                ty: PinType::Aabb,
                default: None,
                enum_variants: vec![],
                required: false,
            }],
            summary: "Idempotently adds/updates a BounceArea component. Wire from a one-shot trigger, not OnTick.",
        },
        OpDescriptor {
            op: "DespawnAfter",
            category: EffectKind::SetupComponent,
            inputs: vec![pin("seconds", PinType::Float)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Adds a Lifetime component that despawns the entity after `seconds`. Does not reset if already present.",
        },
        // ── Structural commands (§2.7) ─────────────────────────────
        OpDescriptor {
            op: "SpawnEntity",
            category: EffectKind::StructuralCommand,
            inputs: vec![
                pin("at", PinType::Vec2),
                pin("rotation", PinType::Float),
                pin("scale", PinType::Vec2),
                pin("with_velocity", PinType::Vec2),
            ],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![req_param("prefab_id", PinType::String, vec![])],
            summary: "Queues a deferred spawn of `prefab_id` at `at`. Optional rotation (radians), scale (Vec2, defaults to (1,1)), and initial velocity.",
        },
        OpDescriptor {
            op: "Despawn",
            category: EffectKind::StructuralCommand,
            inputs: vec![],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Queues a deferred despawn of the current entity.",
        },
        OpDescriptor {
            op: "DespawnByTag",
            category: EffectKind::StructuralCommand,
            inputs: vec![],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![req_param("tag", PinType::String, vec![])],
            summary: "Queues despawn of every entity whose Tags component contains `tag`. Resolved at the command barrier.",
        },
        OpDescriptor {
            op: "ChangeScene",
            category: EffectKind::StructuralCommand,
            inputs: vec![],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![req_param("scene_id", PinType::String, vec![])],
            summary: "Surfaces a scene-change request to the host (runtime wires the swap).",
        },
        // ── Side effects (§2.7) ────────────────────────────────────
        OpDescriptor {
            op: "PlaySound",
            category: EffectKind::SideEffect,
            inputs: vec![PinDescriptor {
                name: "volume",
                ty: PinType::Float,
                default: Some(DefaultValue::Float(1.0)),
                accepts_scale_offset: true,
            }],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![req_param("sound_id", PinType::String, vec![])],
            summary: "Queues a deferred sound playback.",
        },
        OpDescriptor {
            op: "PlayAnimation",
            category: EffectKind::SideEffect,
            inputs: vec![],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![req_param("anim_id", PinType::String, vec![])],
            summary: "Queues a request to start `anim_id` on the self entity (host wires to animator).",
        },
        OpDescriptor {
            op: "StopAnimation",
            category: EffectKind::SideEffect,
            inputs: vec![],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Queues a request to halt the animation playing on the self entity.",
        },
        OpDescriptor {
            op: "EmitEvent",
            category: EffectKind::SideEffect,
            inputs: vec![pin("value", PinType::Float)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![req_param("name", PinType::String, vec![])],
            summary: "Sends a named event with an optional `value`: to every graph's OnEvent, and to scripts' `on_<name>(entity, data)` as `{ value, x, y }` - where this entity was.",
        },
        // ── Higher-level gameplay ops, Vec2-only in v1 (§2.8) ──────
        OpDescriptor {
            op: "LookAt",
            category: EffectKind::DirectWrite,
            inputs: vec![pin("target_pos", PinType::Vec2)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Rotates Transform to face `target_pos` (Vec2-only in v1).",
        },
        OpDescriptor {
            op: "MoveToward",
            category: EffectKind::DirectWrite,
            inputs: vec![pin("target_pos", PinType::Vec2), pin("speed", PinType::Float)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Moves Transform.position toward `target_pos` at `speed` units/sec (Vec2-only in v1).",
        },
        OpDescriptor {
            op: "SetPosition",
            category: EffectKind::DirectWrite,
            inputs: vec![pin("pos", PinType::Vec2)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Overwrites Transform.position with `pos`.",
        },
        OpDescriptor {
            op: "SetRotation",
            category: EffectKind::DirectWrite,
            inputs: vec![pin("rot", PinType::Float)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Overwrites Transform.rotation (radians) with `rot`.",
        },
        OpDescriptor {
            op: "SetScale",
            category: EffectKind::DirectWrite,
            inputs: vec![pin("scale", PinType::Vec2)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Overwrites Transform.scale with `scale`.",
        },
        OpDescriptor {
            op: "SetSpriteColor",
            category: EffectKind::DirectWrite,
            inputs: vec![
                pin("r", PinType::Float),
                pin("g", PinType::Float),
                pin("b", PinType::Float),
                pin("a", PinType::Float),
            ],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Overwrites SpriteComponent.color (RGBA, each clamped to [0,1]). `a` defaults to 1.",
        },
        OpDescriptor {
            op: "SetSpriteAlpha",
            category: EffectKind::DirectWrite,
            inputs: vec![pin("alpha", PinType::Float)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![],
            summary: "Overwrites only the alpha channel of SpriteComponent.color (clamped to [0,1]).",
        },
        OpDescriptor {
            op: "AddTag",
            category: EffectKind::DirectWrite,
            inputs: vec![],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![req_param("tag", PinType::String, vec![])],
            summary: "Inserts `tag` into the entity's Tags (creates Tags if absent).",
        },
        OpDescriptor {
            op: "RemoveTag",
            category: EffectKind::DirectWrite,
            inputs: vec![],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![req_param("tag", PinType::String, vec![])],
            summary: "Removes `tag` from the entity's Tags. No-op if absent.",
        },
        OpDescriptor {
            op: "SetCustomField",
            category: EffectKind::DirectWrite,
            inputs: vec![pin("value", PinType::Float)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![
                req_param("component", PinType::String, vec![]),
                req_param("field", PinType::String, vec![]),
            ],
            summary: "Sets `component.field = value` on the entity's CustomComponents, creating the component entry if needed (as set_custom does).",
        },
        OpDescriptor {
            op: "Log",
            category: EffectKind::SideEffect,
            inputs: vec![pin("value", PinType::Float)],
            outputs: vec![],
            exec_in: true,
            exec_out: vec!["exec"],
            params: vec![req_param("label", PinType::String, vec![])],
            summary: "Prints `[Behavior] label` (or `label: value` if `value` is connected) to the engine log.",
        },
    ]
}

// ─── The op reference in `docs/scripting-api.md` ─────────────────

/// Marks the generated op reference in `docs/scripting-api.md`.
pub const OP_REFERENCE_BEGIN: &str = "<!-- BEGIN GENERATED BEHAVIOR OPS -->";
/// End marker; see [`OP_REFERENCE_BEGIN`].
pub const OP_REFERENCE_END: &str = "<!-- END GENERATED BEHAVIOR OPS -->";

fn pin_type_name(ty: PinType) -> &'static str {
    match ty {
        PinType::Float => "Float",
        PinType::Vec2 => "Vec2",
        PinType::Bool => "Bool",
        PinType::EntityRef => "Entity",
        PinType::Aabb => "Aabb",
        PinType::Enum => "Enum",
        PinType::String => "String",
    }
}

fn default_text(d: &DefaultValue) -> String {
    match d {
        DefaultValue::Float(v) => format!("{v}"),
        DefaultValue::Vec2([x, y]) => format!("[{x}, {y}]"),
        DefaultValue::Bool(b) => format!("{b}"),
        DefaultValue::Str(s) => format!("\"{s}\""),
    }
}

fn pins_text(pins: &[PinDescriptor]) -> String {
    if pins.is_empty() {
        return "-".into();
    }
    pins.iter()
        .map(|p| {
            let mut t = format!("`{}`: {}", p.name, pin_type_name(p.ty));
            if let Some(d) = &p.default {
                t.push_str(&format!(" = {}", default_text(d)));
            }
            t
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn params_text(params: &[ParamDescriptor]) -> String {
    if params.is_empty() {
        return "-".into();
    }
    params
        .iter()
        .map(|p| {
            let mut t = format!("`{}`", p.name);
            if !p.enum_variants.is_empty() {
                t.push_str(&format!(" ({})", p.enum_variants.join(" / ")));
            } else {
                t.push_str(&format!(": {}", pin_type_name(p.ty)));
            }
            if let Some(d) = &p.default {
                t.push_str(&format!(" = {}", default_text(d)));
            } else if p.required {
                t.push_str(" (required)");
            }
            t
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The op reference, rendered from [`op_catalog`].
///
/// **Generated because the hand-written one was wrong in the way that
/// costs most.** It listed 23 ops when there were 68, and its example graph
/// used an edge syntax (`"from": "tick.dt"`) and parameters that do not
/// exist - so a graph written from the documentation did not compile. That
/// file is upstream of the website and the machine-readable AI spec, so the
/// error travelled. The catalog is already checked against the compiler by
/// `catalog_matches_op_kind_set`; this makes the documentation the catalog.
///
/// Regenerate with `cargo run -p jengine-core --example behavior_op_reference`;
/// `behavior_op_reference_is_current` fails when it is stale.
pub fn render_op_reference() -> String {
    let groups: [(EffectKind, &str, &str); 8] = [
        (EffectKind::Trigger, "Triggers", "roots: each starts a program when its event happens"),
        (EffectKind::Read, "Reads", "component values into pins; no writes"),
        (EffectKind::Pure, "Pure", "arithmetic and logic on pins"),
        (EffectKind::Gate, "Gates", "control flow, some with per-instance state"),
        (EffectKind::DirectWrite, "Direct writes", "applied immediately, in the tick"),
        (
            EffectKind::SetupComponent,
            "Setup components",
            "idempotent - wire them from a one-shot trigger, not `OnTick`",
        ),
        (
            EffectKind::StructuralCommand,
            "Structural commands",
            "deferred to the barrier after the tick, like a script's",
        ),
        (EffectKind::SideEffect, "Side effects", "deferred to the barrier"),
    ];
    let ops = op_catalog();
    let mut out = String::new();
    out.push_str(OP_REFERENCE_BEGIN);
    out.push_str("\n\n");
    out.push_str(&format!(
        "{} ops, generated from `behavior::catalog::op_catalog` - the same \
         catalog the editor's node palette reads.\n",
        ops.len()
    ));
    for (kind, title, note) in groups {
        let in_group: Vec<&OpDescriptor> = ops.iter().filter(|o| o.category == kind).collect();
        if in_group.is_empty() {
            continue;
        }
        out.push_str(&format!("\n**{title}** - {note}\n\n"));
        out.push_str("| Op | Inputs | Outputs | Params | Does |\n");
        out.push_str("|---|---|---|---|---|\n");
        for o in in_group {
            let mut outputs = pins_text(&o.outputs);
            let branches: Vec<&str> =
                o.exec_out.iter().copied().filter(|p| *p != "exec").collect();
            if !branches.is_empty() {
                outputs.push_str(&format!(" (branches: {})", branches.join(" / ")));
            }
            out.push_str(&format!(
                "| `{}` | {} | {} | {} | {} |\n",
                o.op,
                pins_text(&o.inputs),
                outputs,
                params_text(&o.params),
                o.summary.replace('|', "\\|"),
            ));
        }
    }
    out.push('\n');
    out.push_str(OP_REFERENCE_END);
    out
}

/// Swap the generated block in `doc` for `block`. `None` when either marker
/// is missing - the caller decides whether that is an error.
pub fn replace_op_reference(doc: &str, block: &str) -> Option<String> {
    let start = doc.find(OP_REFERENCE_BEGIN)?;
    let end = doc[start..].find(OP_REFERENCE_END)? + start + OP_REFERENCE_END.len();
    let mut out = String::with_capacity(doc.len());
    out.push_str(&doc[..start]);
    out.push_str(block);
    out.push_str(&doc[end..]);
    Some(out)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::compile;

    /// Every op the compiler parses must have a catalog entry, and
    /// every catalog entry must parse back through the compiler.
    /// This is the drift-detector for Slice E.1.
    #[test]
    fn catalog_matches_op_kind_set() {
        let catalog = op_catalog();

        // Forward direction: every catalog op compiles a minimal
        // single-node graph (or fails for a structured reason that
        // is not "UnknownOp").
        for op in &catalog {
            let json = format!(
                r#"{{ "name": "Probe", "version": 1,
                       "nodes": [{{ "id": "n0", "op": "{}" }}],
                       "edges": [] }}"#,
                op.op,
            );
            let asset: crate::behavior::format::BehaviorAsset =
                serde_json::from_str(&json).expect("schema parse");
            // We don't care if compile fails — just that it doesn't
            // fail with UnknownOp.
            if let Err(compile::CompileError::UnknownOp { op: name, .. }) = compile::compile(&asset)
            {
                panic!(
                    "catalog lists op `{}` but compiler rejects it as UnknownOp ({})",
                    op.op, name
                );
            }
        }

        // Reverse direction: enumerate every op the spec §2 names
        // and ensure the catalog covers it. This list is the
        // authoritative cross-check against the catalog.
        const SPEC_OPS: &[&str] = &[
            // Triggers
            "OnTick", "OnSpawn", "OnDespawn", "OnCollision", "OnTrigger", "OnTimer", "OnKey",
            "OnEvent", "OnMouseClick",
            // Reads
            "GetInputAxis", "GetPosition", "GetVelocity", "GetRotation", "GetScale",
            "GetDeltaTime", "GetNearestByTag", "GetEntityPosition", "GetCustomField",
            // Pure
            "Compare", "Not", "BoolOp", "SelectFloat", "SelectVec2", "MakeVec2", "GetVec2X",
            "GetVec2Y", "MathFloat", "MathVec2", "ScaleVec2", "Length", "Normalize", "Distance",
            "Dot", "Lerp", "Clamp", "MathUnary", "Atan2",
            // Gates
            "If", "Cooldown", "Once", "Delay", "ForCount",
            // DirectWrite
            "SetVelocity", "AddVelocity", "Translate", "ApplyForce",
            "SetPosition", "SetRotation", "SetScale",
            "SetSpriteColor", "SetSpriteAlpha",
            "AddTag", "RemoveTag", "SetCustomField",
            // SetupComponent
            "BounceOnBounds", "DespawnAfter",
            // StructuralCommand
            "SpawnEntity", "Despawn", "DespawnByTag", "ChangeScene",
            // SideEffect
            "PlaySound", "Log", "Sequence", "PlayAnimation", "StopAnimation", "EmitEvent",
            // Higher-level Vec2-only (§2.8)
            "LookAt", "MoveToward",
        ];

        let catalog_names: std::collections::HashSet<&'static str> =
            catalog.iter().map(|d| d.op).collect();
        for op in SPEC_OPS {
            assert!(
                catalog_names.contains(op),
                "op `{}` named in spec §2 but missing from op_catalog()",
                op,
            );
        }

        // No extras either — anything in the catalog must be in the
        // spec list. Catches typos.
        let spec_set: std::collections::HashSet<&'static str> =
            SPEC_OPS.iter().copied().collect();
        for op in &catalog {
            assert!(
                spec_set.contains(op.op),
                "op `{}` listed in catalog but not in spec §2",
                op.op,
            );
        }
    }

    #[test]
    fn catalog_serialises_stably() {
        // The editor caches this JSON; if the shape changes, it
        // should be a deliberate breaking change. Smoke-test that
        // serialisation succeeds for every op and re-parses as a
        // generic JSON array of objects keyed by `op`.
        let catalog = op_catalog();
        let json = serde_json::to_string(&catalog).expect("serialise");
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        let arr = v.as_array().expect("array");
        assert_eq!(arr.len(), catalog.len());
        for (entry, expected) in arr.iter().zip(catalog.iter()) {
            assert_eq!(entry["op"].as_str(), Some(expected.op));
        }
    }

    #[test]
    fn triggers_have_no_exec_in() {
        for op in op_catalog() {
            if op.category == EffectKind::Trigger {
                assert!(!op.exec_in, "trigger {} should not accept exec_in", op.op);
            }
        }
    }

    #[test]
    fn if_has_then_else_ports() {
        let catalog = op_catalog();
        let if_op = catalog.iter().find(|d| d.op == "If").unwrap();
        assert_eq!(if_op.exec_out, &["then", "else"]);
    }
}
