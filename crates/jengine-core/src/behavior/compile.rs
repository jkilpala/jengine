//! `.behavior` JSON → typed `CompiledGraph` lowering.
//!
//! Slice 7.2 implemented the runtime MVP subset of the v1 op vocabulary
//! (`OnTick` + `GetInputAxis` / `SetVelocity` / `AddVelocity` /
//! `BounceOnBounds` / `DespawnAfter`).
//!
//! Slice 7.3 generalises the compiler to **per-trigger event programs**:
//! - All [`TriggerKind`]s parse (`OnTick`, `OnSpawn`, `OnDespawn`,
//!   `OnCollision`, `OnTrigger`, `OnTimer`).
//! - Each trigger's reachable action subgraph is compiled into its own
//!   [`EventProgram`] keyed by `TriggerKind` in [`CompiledGraph::programs`].
//! - Per-trigger payload pins are allocated up-front (e.g.
//!   `OnCollision` reserves an `EntitySlot` for `other` and a
//!   `BoolSlot` for `enter`). These pins are *write-only* for now —
//!   no MVP op consumes them — but allocating them keeps the layout
//!   stable across slices.
//! - [`CompiledGraph::subscriptions`] is the union of trigger kinds
//!   that have non-empty programs, used by the system for O(1)
//!   dispatch pruning.
//!
//! See [`docs/step7-behavior-graph-spec.md`](../../../../docs/step7-behavior-graph-spec.md) §2 / §3.4 / §7.2 / §7.3.
//!
//! The compiler validates: unknown ops, missing node refs, missing
//! input bindings on direct-write ops, pin type mismatch (Float vs
//! Vec2), cycles in the data graph, and emits a
//! `setup_op_under_on_tick` warning when a setup op is reachable from
//! an `OnTick` trigger (the §2.6 footgun).

use crate::detmath::DetMath;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use glam::Vec2;
use thiserror::Error;

use crate::behavior::event::{EventMask, TriggerKind};
use crate::behavior::format::{BehaviorAsset, BehaviorNode, ControlPort, PinBinding};
use crate::behavior::pin::{BoolSlot, EntitySlot, FloatSlot, PinLayout, Vec2Slot};
use crate::behavior::state::{StateBoolSlot, StateCounterSlot, StateFloatSlot, StateLayout};

/// Compiled, ready-to-tick form of a `.behavior` asset. Cached behind
/// `Arc` in [`crate::behavior::cache::BehaviorGraphCache`].
#[derive(Debug, Clone)]
pub struct CompiledGraph {
    /// One program per subscribed trigger. Entry order is fixed by
    /// [`TriggerKind`]'s declaration order so iteration is deterministic.
    pub programs: BTreeMap<TriggerKind, EventProgram>,

    /// Pin layout shared across *all* programs in this graph; the
    /// system sizes one `PinFrame` once.
    pub pin_layout: PinLayout,

    /// Per-instance persistent state layout (filled by Slice 7.4A).
    pub state_layout: StateLayout,

    /// Bitset of [`TriggerKind`]s with non-empty programs.
    pub subscriptions: EventMask,

    /// Non-fatal warnings recorded at compile time.
    pub diagnostics: Vec<CompileDiagnostic>,
}

impl CompiledGraph {
    /// Convenience: ops for the OnTick program (empty if no `OnTick`
    /// trigger node exists). Kept for tests + backwards compat with
    /// Slice 7.2 callers.
    pub fn tick_program(&self) -> &[OpInstruction] {
        self.programs
            .get(&TriggerKind::OnTick)
            .map(|p| p.ops.as_slice())
            .unwrap_or(&[])
    }
}

/// One trigger's compiled program plus its payload-pin map.
#[derive(Debug, Clone, Default)]
pub struct EventProgram {
    pub ops: Vec<OpInstruction>,
    /// Source-map: `node_id` per entry of `ops`.
    pub source: Vec<String>,
    /// Where the dispatcher should write each payload field before
    /// running `ops`. Empty for triggers without payload
    /// (`OnTick`, `OnSpawn`, `OnDespawn`).
    pub payload_pins: PayloadPins,
    /// Static dispatch filter for [`crate::behavior::event::TriggerKind::OnKey`]
    /// programs. `Some` exactly when this program was compiled from
    /// an `OnKey` trigger node — the dispatcher
    /// (`BehaviorGraphSystem::dispatch_input_keys`) reads this to
    /// decide whether the per-frame `InputState` matches.
    pub key_filter: Option<KeyFilter>,
    /// Static event-name filter baked into an `OnEvent` program at
    /// compile time. The dispatcher (`dispatch_event`) only fires
    /// programs whose `name_filter` equals the incoming event name.
    pub name_filter: Option<String>,
    /// Static mouse-button filter baked into an `OnMouseClick`
    /// program at compile time. The dispatcher only fires programs
    /// whose `mouse_filter` matches (or `Any`).
    pub mouse_filter: Option<MouseButtonFilter>,
    /// `OnTimer`'s `interval_seconds`, baked in at compile time. `Some`
    /// exactly when this program was compiled from an `OnTimer` node;
    /// `tick_pre_simulation` advances each instance's clock against it.
    pub timer_interval: Option<f32>,
}

/// Which mouse button an [`crate::behavior::event::TriggerKind::OnMouseClick`]
/// node fires on. `Any` matches every button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButtonFilter {
    Left,
    Right,
    Any,
}

impl MouseButtonFilter {
    pub fn matches(self, button: u8) -> bool {
        match self {
            MouseButtonFilter::Any => true,
            MouseButtonFilter::Left => button == 0,
            MouseButtonFilter::Right => button == 1,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "left" => MouseButtonFilter::Left,
            "right" => MouseButtonFilter::Right,
            "any" => MouseButtonFilter::Any,
            _ => return None,
        })
    }
}

/// Static `(key, edge)` filter baked into an `OnKey` program at
/// compile time. Stored on [`EventProgram::key_filter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyFilter {
    pub key: crate::input::KeyCode,
    pub edge: crate::behavior::event::KeyEdge,
}

/// Where a trigger's payload fields land in the shared `PinFrame`.
/// The dispatcher writes these before stepping `ops`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PayloadPins {
    /// `OnCollision { other }` / `OnTrigger { other }`.
    pub other_entity: Option<EntitySlot>,
    /// `OnCollision { enter }`.
    pub enter: Option<BoolSlot>,
    /// `OnTimer { tick_index }`.
    pub tick_index: Option<FloatSlot>,
    /// `OnEvent { value }`.
    pub event_value: Option<FloatSlot>,
    /// `OnMouseClick { pos }`.
    pub mouse_pos: Option<Vec2Slot>,
    /// `OnMouseClick { button }` (0 = left, 1 = right).
    pub mouse_button: Option<FloatSlot>,
}

/// One executable instruction. Inputs are pre-resolved to typed pin
/// reads or baked literals; the interpreter never touches strings or
/// `serde_json::Value` on the hot path.
///
/// Slice 7.4A introduces three classes of *block* instructions
/// (`If`, `Cooldown`, `Once`). They carry the byte length of their
/// downstream subprogram(s); the interpreter advances the program
/// counter by that length to skip the suppressed body. Block bodies
/// are inlined immediately after the gate instruction, so the
/// program is one flat `Vec<OpInstruction>` with no separate jump
/// table.
#[derive(Debug, Clone, PartialEq)]
pub enum OpInstruction {
    /// Marker; the trigger itself emits no work.
    OnTick,
    /// Read self entity's `InputAxis2D.current` into a Vec2 pin.
    GetInputAxis { output: Vec2Slot },
    /// Read self entity's `Transform.position` into a Vec2 pin.
    /// Falls back to `Vec2::ZERO` (and bumps `missing_transform_reads`)
    /// if the entity has no `Transform`.
    GetPosition { output: Vec2Slot },
    /// Read self entity's `Velocity2D.{x,y}` into a Vec2 pin.
    /// Falls back to `Vec2::ZERO` (and bumps `missing_velocity_reads`)
    /// if the entity has no `Velocity2D`.
    GetVelocity { output: Vec2Slot },
    /// Comparison operator for the `Compare` op (§2.3).
    Compare {
        a: FloatInput,
        op: CompareOp,
        b: FloatInput,
        output: BoolSlot,
    },
    /// Boolean negation into a `Bool` pin.
    Not { input: BoolInput, output: BoolSlot },
    /// Pack two `Float` inputs into a `Vec2`.
    MakeVec2 { x: FloatInput, y: FloatInput, output: Vec2Slot },
    /// Extract `Vec2.x` into a `Float` pin.
    GetVec2X { input: Vec2Input, output: FloatSlot },
    /// Extract `Vec2.y` into a `Float` pin.
    GetVec2Y { input: Vec2Input, output: FloatSlot },
    /// Binary float arithmetic (`Add` / `Sub` / `Mul` / `Div` / `Min` / `Max` / `Mod` / `Pow`).
    /// Division and modulo by zero output `0.0`.
    MathFloat {
        op: BinaryFloatOp,
        a: FloatInput,
        b: FloatInput,
        output: FloatSlot,
    },
    /// Componentwise binary Vec2 arithmetic (`Add` / `Sub` / `Mul` / `Div` / `Min` / `Max`).
    /// Division by zero on a component outputs `0.0` for that component.
    MathVec2 {
        op: BinaryVec2Op,
        a: Vec2Input,
        b: Vec2Input,
        output: Vec2Slot,
    },
    /// Multiply a `Vec2` by a scalar `Float`.
    ScaleVec2 { v: Vec2Input, s: FloatInput, output: Vec2Slot },
    /// Length of a `Vec2`.
    Length { input: Vec2Input, output: FloatSlot },
    /// Unit-length copy of a `Vec2`. Zero-length input → `Vec2::ZERO`.
    Normalize { input: Vec2Input, output: Vec2Slot },
    /// Euclidean distance between two `Vec2`s.
    Distance { a: Vec2Input, b: Vec2Input, output: FloatSlot },
    /// Dot product of two `Vec2`s.
    Dot { a: Vec2Input, b: Vec2Input, output: FloatSlot },
    /// Linear interpolation `a + (b - a) * t`. `t` is not clamped.
    Lerp {
        a: FloatInput,
        b: FloatInput,
        t: FloatInput,
        output: FloatSlot,
    },
    /// Clamp `input` into `[min, max]`. If `min > max`, output is `min`.
    Clamp {
        input: FloatInput,
        min: FloatInput,
        max: FloatInput,
        output: FloatSlot,
    },
    /// Unary float math (`Abs` / `Sign` / `Neg` / `Sqrt` / `Sin` / `Cos` / `Tan` / `Floor` / `Ceil` / `Round`).
    /// `Sqrt` of negative values outputs `0.0`.
    MathUnary {
        op: UnaryFloatOp,
        input: FloatInput,
        output: FloatSlot,
    },
    /// `atan2(y, x)` in radians.
    Atan2 { y: FloatInput, x: FloatInput, output: FloatSlot },
    /// Binary bool op (`And` / `Or` / `Xor`).
    BoolOp {
        op: BoolBinaryOp,
        a: BoolInput,
        b: BoolInput,
        output: BoolSlot,
    },
    /// Ternary float select. `cond ? a : b`.
    SelectFloat {
        cond: BoolInput,
        a: FloatInput,
        b: FloatInput,
        output: FloatSlot,
    },
    /// Ternary Vec2 select.
    SelectVec2 {
        cond: BoolInput,
        a: Vec2Input,
        b: Vec2Input,
        output: Vec2Slot,
    },
    /// Read self entity's `Transform.rotation` (radians) into a Float pin.
    GetRotation { output: FloatSlot },
    /// Read self entity's `Transform.scale` into a Vec2 pin.
    GetScale { output: Vec2Slot },
    /// Read the current frame's `dt` (seconds) into a Float pin.
    GetDeltaTime { output: FloatSlot },
    /// Find the nearest entity carrying `tag` (Euclidean distance from
    /// this entity's `Transform.position`) and write its `EntityRef`
    /// into an Entity pin. When `exclude_self` is true, this entity is
    /// skipped (used for separation/flocking against same-tag peers).
    /// If no match exists, the pin is left empty (`None`).
    GetNearestByTag {
        tag: String,
        exclude_self: bool,
        output: EntitySlot,
    },
    /// Read an Entity pin and write that entity's `Transform.position`
    /// into a Vec2 pin. An empty ref (`None`) or a despawned/invalid
    /// entity resolves to *this* entity's own position, so downstream
    /// movement ops (`Translate` toward it) become a safe no-op.
    GetEntityPosition { entity: EntitySlot, output: Vec2Slot },
    /// Read a Float field from a custom component on this entity into a
    /// Float pin. Missing component / field (or non-numeric value)
    /// yields `0.0` and bumps `missing_custom_reads`.
    GetCustomField {
        component: String,
        field: String,
        output: FloatSlot,
    },
    /// Conditional gate. Layout in the program: `[If, then-block (then_len ops), else-block (else_len ops)]`.
    /// On true, run `then` block then advance pc past `else_len`.
    /// On false, advance pc past `then_len` then run `else` block.
    If {
        cond: BoolInput,
        then_len: u16,
        else_len: u16,
    },
    /// Suppress downstream until `seconds` elapse. State slot stores
    /// remaining cooldown time (decremented by `ctx.dt` each visit).
    /// Body is the `body_len` ops following this instruction; if the
    /// gate is closed (`remaining > 0`), pc advances past them.
    Cooldown {
        seconds: FloatInput,
        state_slot: StateFloatSlot,
        body_len: u16,
    },
    /// Fire downstream the first time only. State slot is a sticky
    /// `bool` flag; once set, the body is permanently skipped.
    Once {
        state_slot: StateBoolSlot,
        body_len: u16,
    },
    /// Suppress downstream until `seconds` have elapsed since the
    /// first visit, then fire body on every subsequent visit.
    /// State slot accumulates elapsed time (seconds).
    Delay {
        seconds: FloatInput,
        state_slot: StateFloatSlot,
        body_len: u16,
    },
    /// Fire body up to `count` times total per instance, then
    /// permanently skip. State slot is a u32 visit counter.
    ForCount {
        count: FloatInput,
        state_slot: StateCounterSlot,
        body_len: u16,
    },
    /// Pure passthrough action — no engine effect. Useful as an
    /// editor-side label / grouping anchor for downstream chains.
    Sequence,
    /// Push a `BehaviorCommand::PlayAnimation` for the self entity.
    /// `anim_id` is opaque in v1 — host wires it to the animator.
    PlayAnimation { anim_id: String },
    /// Push a `BehaviorCommand::StopAnimation` for the self entity.
    StopAnimation,
    /// Push a `BehaviorCommand::EmitEvent` with `name` and an
    /// optional numeric `value` for host-side routing.
    EmitEvent { name: String, value: Option<FloatInput> },
    /// Overwrite self entity's `Velocity2D.{x,y}` with a Vec2 input.
    SetVelocity { vel: Vec2Input },
    /// Add a Vec2 input to self entity's `Velocity2D.{x,y}`.
    AddVelocity { delta: Vec2Input },
    /// Add a Vec2 input to self entity's `Transform.position`.
    /// Per spec §5, missing `Transform` bumps `missing_transform_writes`.
    Translate { delta: Vec2Input },
    /// Add a Vec2 input to self entity's `Force2D.{x,y}` (additive).
    /// Missing `Force2D` bumps `missing_force_writes`.
    ApplyForce { force: Vec2Input },
    /// Idempotent: ensure self entity has a `BounceArea`. If `bounds`
    /// is `Some`, also ensure the area matches.
    BounceOnBounds { bounds: Option<BoundsLiteral> },
    /// Idempotent: ensure self entity has a `Lifetime` with the given
    /// initial seconds. **Does NOT reset** an existing `Lifetime`.
    DespawnAfter { seconds: FloatInput },
    /// Push a `BehaviorCommand::Spawn` onto the system command queue.
    /// `prefab_id` is opaque to v1 (stored on the spawned entity as
    /// a `Name` component); `at` is the spawn position; `with_velocity`
    /// optionally adds a `Velocity2D`.
    SpawnEntity {
        prefab_id: String,
        at: Vec2Input,
        rotation: Option<FloatInput>,
        scale: Option<Vec2Input>,
        with_velocity: Option<Vec2Input>,
    },
    /// Push a `BehaviorCommand::Despawn` for the self entity. Drained
    /// at the Phase 7 command barrier (does NOT remove immediately).
    Despawn,
    /// Push a `BehaviorCommand::DespawnByTag` resolved at the command
    /// barrier. The `tag` param is opaque to the compiler.
    DespawnByTag { tag: String },
    /// Push a `BehaviorCommand::ChangeScene { scene_id }` for the
    /// host to act on. Engine itself does not load scenes here.
    ChangeScene { scene_id: String },
    /// Push a `BehaviorCommand::PlaySound`. Volume is `[0, 1]`.
    PlaySound { sound_id: String, volume: FloatInput },
    /// Set self entity's `Transform.rotation` (radians) to face
    /// `target_pos` from the current position. Computed via
    /// `atan2(target.y - pos.y, target.x - pos.x)`. If the target
    /// coincides with the current position (within `1e-6`), rotation
    /// is left unchanged. Missing `Transform` bumps both
    /// `missing_transform_reads` and `missing_transform_writes`.
    LookAt { target_pos: Vec2Input },
    /// Set self entity's `Velocity2D.{x,y}` to a unit vector toward
    /// `target_pos` scaled by `speed`. If the target coincides with
    /// the current position (within `1e-6`), velocity is set to
    /// zero. Missing `Transform` bumps `missing_transform_reads`;
    /// missing `Velocity2D` bumps `missing_velocity_writes`.
    MoveToward { target_pos: Vec2Input, speed: FloatInput },
    /// Overwrite self entity's `Transform.position` with a Vec2 input.
    /// Missing `Transform` bumps `missing_transform_writes`.
    SetPosition { pos: Vec2Input },
    /// Overwrite self entity's `Transform.rotation` (radians) with a Float input.
    /// Missing `Transform` bumps `missing_transform_writes`.
    SetRotation { rot: FloatInput },
    /// Overwrite self entity's `Transform.scale` with a Vec2 input.
    /// Missing `Transform` bumps `missing_transform_writes`.
    SetScale { scale: Vec2Input },
    /// Overwrite self entity's `SpriteComponent.color` (RGBA) with four Float inputs.
    /// Each component is clamped to `[0, 1]`.
    /// Missing `SpriteComponent` bumps `missing_sprite_writes`.
    SetSpriteColor {
        r: FloatInput,
        g: FloatInput,
        b: FloatInput,
        a: FloatInput,
    },
    /// Overwrite only the alpha channel of self entity's `SpriteComponent.color`.
    /// Clamped to `[0, 1]`. Missing `SpriteComponent` bumps `missing_sprite_writes`.
    SetSpriteAlpha { alpha: FloatInput },
    /// Insert `tag` into self entity's `Tags`. Inserts a `Tags` component if absent.
    AddTag { tag: String },
    /// Remove `tag` from self entity's `Tags`. No-op if `Tags` absent or tag missing.
    RemoveTag { tag: String },
    /// Set a Float field on a custom component. `component` is the component
    /// name (e.g. `"Health"`); `field` is the field name within the component's
    /// JSON object. The numeric value is written via `serde_json::json!(value)`.
    /// Missing component or non-object field bumps `missing_custom_writes`.
    SetCustomField {
        component: String,
        field: String,
        value: FloatInput,
    },
    /// Print a labelled message to the engine log at `info` level.
    /// If `value` is `Some`, the format is `[Behavior] <label>: <value>`;
    /// otherwise `[Behavior] <label>`. Side-effect only (no pin output).
    Log { label: String, value: Option<FloatInput> },
}

/// Comparison operator for the `Compare` op (§2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CompareOp {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "Eq" | "==" => Self::Eq,
            "Ne" | "!=" => Self::Ne,
            "Lt" | "<" => Self::Lt,
            "Le" | "<=" => Self::Le,
            "Gt" | ">" => Self::Gt,
            "Ge" | ">=" => Self::Ge,
            _ => return None,
        })
    }

    pub fn apply(self, a: f32, b: f32) -> bool {
        match self {
            Self::Eq => a == b,
            Self::Ne => a != b,
            Self::Lt => a < b,
            Self::Le => a <= b,
            Self::Gt => a > b,
            Self::Ge => a >= b,
        }
    }
}

/// Binary float arithmetic op for [`OpInstruction::MathFloat`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryFloatOp {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    Mod,
    Pow,
}

impl BinaryFloatOp {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "Add" | "+" => Self::Add,
            "Sub" | "-" => Self::Sub,
            "Mul" | "*" => Self::Mul,
            "Div" | "/" => Self::Div,
            "Min" => Self::Min,
            "Max" => Self::Max,
            "Mod" | "%" => Self::Mod,
            "Pow" | "^" => Self::Pow,
            _ => return None,
        })
    }

    pub fn apply(self, a: f32, b: f32) -> f32 {
        match self {
            Self::Add => a + b,
            Self::Sub => a - b,
            Self::Mul => a * b,
            Self::Div => {
                if b == 0.0 {
                    0.0
                } else {
                    a / b
                }
            }
            Self::Min => a.min(b),
            Self::Max => a.max(b),
            Self::Mod => {
                if b == 0.0 {
                    0.0
                } else {
                    a % b
                }
            }
            Self::Pow => a.dpowf(b),
        }
    }
}

/// Componentwise binary Vec2 arithmetic for [`OpInstruction::MathVec2`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryVec2Op {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
}

impl BinaryVec2Op {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "Add" | "+" => Self::Add,
            "Sub" | "-" => Self::Sub,
            "Mul" | "*" => Self::Mul,
            "Div" | "/" => Self::Div,
            "Min" => Self::Min,
            "Max" => Self::Max,
            _ => return None,
        })
    }

    pub fn apply(self, a: Vec2, b: Vec2) -> Vec2 {
        match self {
            Self::Add => a + b,
            Self::Sub => a - b,
            Self::Mul => Vec2::new(a.x * b.x, a.y * b.y),
            Self::Div => Vec2::new(
                if b.x == 0.0 { 0.0 } else { a.x / b.x },
                if b.y == 0.0 { 0.0 } else { a.y / b.y },
            ),
            Self::Min => a.min(b),
            Self::Max => a.max(b),
        }
    }
}

/// Unary float math for [`OpInstruction::MathUnary`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryFloatOp {
    Abs,
    Sign,
    Neg,
    Sqrt,
    Sin,
    Cos,
    Tan,
    Floor,
    Ceil,
    Round,
}

impl UnaryFloatOp {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "Abs" => Self::Abs,
            "Sign" => Self::Sign,
            "Neg" => Self::Neg,
            "Sqrt" => Self::Sqrt,
            "Sin" => Self::Sin,
            "Cos" => Self::Cos,
            "Tan" => Self::Tan,
            "Floor" => Self::Floor,
            "Ceil" => Self::Ceil,
            "Round" => Self::Round,
            _ => return None,
        })
    }

    pub fn apply(self, a: f32) -> f32 {
        match self {
            Self::Abs => a.abs(),
            Self::Sign => a.signum(),
            Self::Neg => -a,
            Self::Sqrt => {
                if a < 0.0 {
                    0.0
                } else {
                    a.sqrt()
                }
            }
            Self::Sin => a.dsin(),
            Self::Cos => a.dcos(),
            Self::Tan => a.dtan(),
            Self::Floor => a.floor(),
            Self::Ceil => a.ceil(),
            Self::Round => a.round(),
        }
    }
}

/// Binary bool op for [`OpInstruction::BoolOp`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolBinaryOp {
    And,
    Or,
    Xor,
}

impl BoolBinaryOp {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "And" | "&&" => Self::And,
            "Or" | "||" => Self::Or,
            "Xor" | "^" => Self::Xor,
            _ => return None,
        })
    }

    pub fn apply(self, a: bool, b: bool) -> bool {
        match self {
            Self::And => a && b,
            Self::Or => a || b,
            Self::Xor => a ^ b,
        }
    }
}

/// A typed `Bool` operand: literal or read from a previously-written
/// `Bool` pin (e.g. produced by `Compare` / `Not`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolInput {
    Literal(bool),
    Pin { slot: BoolSlot },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundsLiteral {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
    pub damping: f32,
}

/// A typed `Vec2` operand: either a baked literal or a read from a
/// previously-written Vec2 pin (with optional scale + uniform offset).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Vec2Input {
    Literal(Vec2),
    Pin {
        slot: Vec2Slot,
        scale: f32,
        offset: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FloatInput {
    Literal(f32),
    Pin {
        slot: FloatSlot,
        scale: f32,
        offset: f32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompileDiagnostic {
    pub node_id: String,
    pub kind: CompileWarning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileWarning {
    /// Setup op (BounceOnBounds / DespawnAfter) reachable from an
    /// OnTick trigger. The op is idempotent so this is non-fatal, but
    /// it usually means the author meant to wire it from an OnSpawn
    /// trigger instead. (Spec §2.6.)
    SetupOpUnderOnTick { op: &'static str },
}

#[derive(Debug, Error, PartialEq)]
pub enum CompileError {
    #[error("node `{node_id}`: unknown op `{op}` (see catalog::op_catalog for the full op list)")]
    UnknownOp { node_id: String, op: String },

    #[error("node `{node_id}`: duplicate node id (must be unique within the graph)")]
    DuplicateNodeId { node_id: String },

    #[error("edge from `{from}` to `{to}`: referenced node id not found")]
    EdgeRefNotFound { from: String, to: String },

    #[error("node `{node_id}` input `{pin}`: missing binding (op `{op}` requires this input)")]
    MissingInput {
        node_id: String,
        op: String,
        pin: &'static str,
    },

    #[error("node `{node_id}` input `{pin}`: references unknown node `{from}`")]
    PinRefNotFound {
        node_id: String,
        pin: &'static str,
        from: String,
    },

    #[error(
        "node `{node_id}` input `{pin}`: type mismatch (expected `{expected}`, source op `{from_op}` produces `{actual}`)"
    )]
    PinTypeMismatch {
        node_id: String,
        pin: &'static str,
        expected: &'static str,
        actual: &'static str,
        from_op: String,
    },

    #[error(
        "node `{node_id}` input `{pin}`: source op `{from_op}` produces no pin output (cannot be referenced)"
    )]
    PinSourceNotProducer {
        node_id: String,
        pin: &'static str,
        from_op: String,
    },

    #[error("data graph contains a cycle involving node `{node_id}`")]
    DataCycle { node_id: String },

    #[error(
        "node `{node_id}` input `{pin}`: literal value type mismatch (expected `{expected}`)"
    )]
    LiteralTypeMismatch {
        node_id: String,
        pin: &'static str,
        expected: &'static str,
    },

    #[error("node `{node_id}` op `{op}` does not accept input `{pin}`")]
    UnknownInputPin {
        node_id: String,
        op: String,
        pin: String,
    },

    #[error("edge from `{from}` to `{to}`: invalid control port `{port:?}` on op `{from_op}`")]
    InvalidControlPort {
        from: String,
        to: String,
        from_op: String,
        port: ControlPort,
    },

    #[error("OnKey node `{node_id}`: missing or invalid `{param}` param ({reason})")]
    InvalidKeyTriggerParam {
        node_id: String,
        param: &'static str,
        reason: &'static str,
    },

    #[error("graph contains multiple `OnKey` trigger nodes (`{first}` and `{second}`); v1 supports at most one OnKey per graph")]
    DuplicateOnKeyTrigger { first: String, second: String },

    #[error("OnEvent node `{node_id}`: missing required `name` param")]
    OnEventMissingName { node_id: String },

    #[error("OnMouseClick node `{node_id}`: invalid `button` param `{value}` (expected one of \"left\", \"right\", \"any\")")]
    OnMouseClickInvalidButton { node_id: String, value: String },
}

/// Lower a parsed `BehaviorAsset` to an executable `CompiledGraph`.
pub fn compile(asset: &BehaviorAsset) -> Result<CompiledGraph, CompileError> {
    // --- 1. Index nodes by id, classify ops ---
    let mut by_id: HashMap<&str, &BehaviorNode> = HashMap::new();
    for node in &asset.nodes {
        if by_id.insert(node.id.as_str(), node).is_some() {
            return Err(CompileError::DuplicateNodeId {
                node_id: node.id.clone(),
            });
        }
        // Reject unknown ops up-front so later phases can assume a
        // closed set.
        OpKind::parse(&node.op).map_err(|()| CompileError::UnknownOp {
            node_id: node.id.clone(),
            op: node.op.clone(),
        })?;
    }

    // --- 2. Validate edges + classify exec ports ---
    for edge in &asset.edges {
        let from = by_id
            .get(edge.from.as_str())
            .ok_or_else(|| CompileError::EdgeRefNotFound {
                from: edge.from.clone(),
                to: edge.to.clone(),
            })?;
        if !by_id.contains_key(edge.to.as_str()) {
            return Err(CompileError::EdgeRefNotFound {
                from: edge.from.clone(),
                to: edge.to.clone(),
            });
        }
        // `then`/`else` are reserved for `If` (Slice 7.4A). Every
        // other op must use `exec`. Unknown / future port names land
        // in `ControlPort::Other` — reject those with the same error.
        let from_kind = OpKind::parse(&from.op).unwrap();
        match edge.out {
            ControlPort::Exec => {}
            ControlPort::Then | ControlPort::Else => {
                if !matches!(from_kind, OpKind::If) {
                    return Err(CompileError::InvalidControlPort {
                        from: edge.from.clone(),
                        to: edge.to.clone(),
                        from_op: from.op.clone(),
                        port: edge.out.clone(),
                    });
                }
            }
            ControlPort::Other(_) => {
                return Err(CompileError::InvalidControlPort {
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                    from_op: from.op.clone(),
                    port: edge.out.clone(),
                });
            }
        }
    }

    // --- 3. Topo-sort data producers (only GetInputAxis in 7.2) ---
    let producer_ids: Vec<&str> = asset
        .nodes
        .iter()
        .filter(|n| OpKind::parse(&n.op).unwrap().is_data_producer())
        .map(|n| n.id.as_str())
        .collect();

    // Data dependencies: edges in the *pin* graph (a node's `inputs`
    // referencing another node's output pin).
    let mut data_deps: HashMap<&str, Vec<&str>> = HashMap::new();
    for node in &asset.nodes {
        let mut deps = Vec::new();
        for binding in node.inputs.values() {
            if let PinBinding::Reference { from, .. } = binding {
                if !by_id.contains_key(from.as_str()) {
                    // Reported precisely later in build_input.
                    continue;
                }
                deps.push(from.as_str());
            }
        }
        data_deps.insert(node.id.as_str(), deps);
    }

    let topo_producers = topo_sort(&producer_ids, &data_deps)?;

    // --- 4. Allocate pin slots for producers (shared across all programs) ---
    let mut pin_layout = PinLayout::default();
    let mut producer_pins: Producers = Producers::default();
    for node in &asset.nodes {
        if let Some(name) = primary_output_name(&node.op) {
            producer_pins.primary.insert(node.id.as_str(), name);
        }
    }

    // **Trigger outputs first**, so a pure or read op can take one. Payload
    // slots used to be allocated inside the per-trigger loop below, after
    // every producer had been compiled against a map that held no trigger -
    // so `MathFloat(OnEvent.value, 2)` or `Compare(OnTimer.tick_index, 3)`
    // failed with "unknown node", and a graph could *act* on what triggered
    // it but never compute with it. One slot set per trigger kind, as
    // before: two `OnEvent` nodes share `value`, which holds whichever fired.
    let mut payload_by_kind: BTreeMap<TriggerKind, PayloadPins> = BTreeMap::new();
    for node in asset.nodes.iter().filter(|n| OpKind::parse(&n.op).unwrap().is_trigger()) {
        let kind = OpKind::parse(&node.op).unwrap().trigger_kind().unwrap();
        let pins = *payload_by_kind
            .entry(kind)
            .or_insert_with(|| allocate_payload_pins(kind, &mut pin_layout));
        let id = node.id.as_str();
        match kind {
            TriggerKind::OnTimer => {
                if let Some(slot) = pins.tick_index {
                    producer_pins.insert(id, ProducerPin::Float(slot));
                }
            }
            TriggerKind::OnEvent => {
                if let Some(slot) = pins.event_value {
                    producer_pins.insert(id, ProducerPin::Float(slot));
                }
            }
            TriggerKind::OnMouseClick => {
                if let Some(slot) = pins.mouse_pos {
                    producer_pins.insert(id, ProducerPin::Vec2(slot));
                }
                if let Some(slot) = pins.mouse_button {
                    producer_pins.insert_named(id, "button", ProducerPin::Float(slot));
                }
            }
            TriggerKind::OnCollision | TriggerKind::OnTrigger => {
                if let Some(slot) = pins.other_entity {
                    producer_pins.insert(id, ProducerPin::Entity(slot));
                }
                if let Some(slot) = pins.enter {
                    producer_pins.insert_named(id, "enter", ProducerPin::Bool(slot));
                }
            }
            _ => {}
        }
    }
    let mut producer_ops: Vec<(String, OpInstruction)> = Vec::new();

    for id in &topo_producers {
        let node = by_id[id];
        let kind = OpKind::parse(&node.op).unwrap();
        match kind {
            OpKind::GetInputAxis => {
                let slot = Vec2Slot(pin_layout.vec2s);
                pin_layout.vec2s += 1;
                producer_pins.insert(id, ProducerPin::Vec2(slot));
                producer_ops.push((node.id.clone(), OpInstruction::GetInputAxis { output: slot }));
            }
            OpKind::GetPosition => {
                let slot = Vec2Slot(pin_layout.vec2s);
                pin_layout.vec2s += 1;
                reject_unknown_inputs(node, &[])?;
                producer_pins.insert(id, ProducerPin::Vec2(slot));
                producer_ops.push((node.id.clone(), OpInstruction::GetPosition { output: slot }));
            }
            OpKind::GetVelocity => {
                let slot = Vec2Slot(pin_layout.vec2s);
                pin_layout.vec2s += 1;
                reject_unknown_inputs(node, &[])?;
                producer_pins.insert(id, ProducerPin::Vec2(slot));
                producer_ops.push((node.id.clone(), OpInstruction::GetVelocity { output: slot }));
            }
            OpKind::GetNearestByTag => {
                let tag = node
                    .params
                    .get("tag")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CompileError::MissingInput {
                        node_id: node.id.clone(),
                        op: "GetNearestByTag".to_string(),
                        pin: "tag",
                    })?
                    .to_string();
                let exclude_self = node
                    .params
                    .get("exclude_self")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                reject_unknown_inputs(node, &[])?;
                let output = EntitySlot(pin_layout.entities);
                pin_layout.entities += 1;
                producer_pins.insert(id, ProducerPin::Entity(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::GetNearestByTag { tag, exclude_self, output },
                ));
            }
            OpKind::GetEntityPosition => {
                let entity = build_entity_input(node, "entity", "GetEntityPosition", &producer_pins)?;
                reject_unknown_inputs(node, &["entity"])?;
                let output = Vec2Slot(pin_layout.vec2s);
                pin_layout.vec2s += 1;
                producer_pins.insert(id, ProducerPin::Vec2(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::GetEntityPosition { entity, output },
                ));
            }
            OpKind::GetCustomField => {
                let component = node
                    .params
                    .get("component")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CompileError::MissingInput {
                        node_id: node.id.clone(),
                        op: "GetCustomField".to_string(),
                        pin: "component",
                    })?
                    .to_string();
                let field = node
                    .params
                    .get("field")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CompileError::MissingInput {
                        node_id: node.id.clone(),
                        op: "GetCustomField".to_string(),
                        pin: "field",
                    })?
                    .to_string();
                reject_unknown_inputs(node, &[])?;
                let output = FloatSlot(pin_layout.floats);
                pin_layout.floats += 1;
                producer_pins.insert(id, ProducerPin::Float(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::GetCustomField { component, field, output },
                ));
            }
            OpKind::Compare => {
                let a = build_float_input(node, "a", "Compare", &producer_pins)?;
                let b = build_float_input(node, "b", "Compare", &producer_pins)?;
                let op_str = node
                    .params
                    .get("op")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CompileError::MissingInput {
                        node_id: node.id.clone(),
                        op: "Compare".to_string(),
                        pin: "op",
                    })?;
                let op = CompareOp::parse(op_str).ok_or_else(|| {
                    CompileError::LiteralTypeMismatch {
                        node_id: node.id.clone(),
                        pin: "op",
                        expected: "Eq|Ne|Lt|Le|Gt|Ge",
                    }
                })?;
                reject_unknown_inputs(node, &["a", "b"])?;
                let output = BoolSlot(pin_layout.bools);
                pin_layout.bools += 1;
                producer_pins.insert(id, ProducerPin::Bool(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::Compare { a, op, b, output },
                ));
            }
            OpKind::Not => {
                let input = build_bool_input(node, "in", "Not", &producer_pins)?;
                reject_unknown_inputs(node, &["in"])?;
                let output = BoolSlot(pin_layout.bools);
                pin_layout.bools += 1;
                producer_pins.insert(id, ProducerPin::Bool(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::Not { input, output },
                ));
            }
            OpKind::MakeVec2 => {
                let x = build_float_input(node, "x", "MakeVec2", &producer_pins)?;
                let y = build_float_input(node, "y", "MakeVec2", &producer_pins)?;
                reject_unknown_inputs(node, &["x", "y"])?;
                let output = Vec2Slot(pin_layout.vec2s);
                pin_layout.vec2s += 1;
                producer_pins.insert(id, ProducerPin::Vec2(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::MakeVec2 { x, y, output },
                ));
            }
            OpKind::GetVec2X => {
                let input = build_vec2_input(node, "in", "GetVec2X", &producer_pins)?;
                reject_unknown_inputs(node, &["in"])?;
                let output = FloatSlot(pin_layout.floats);
                pin_layout.floats += 1;
                producer_pins.insert(id, ProducerPin::Float(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::GetVec2X { input, output },
                ));
            }
            OpKind::GetVec2Y => {
                let input = build_vec2_input(node, "in", "GetVec2Y", &producer_pins)?;
                reject_unknown_inputs(node, &["in"])?;
                let output = FloatSlot(pin_layout.floats);
                pin_layout.floats += 1;
                producer_pins.insert(id, ProducerPin::Float(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::GetVec2Y { input, output },
                ));
            }
            OpKind::MathFloat => {
                let a = build_float_input(node, "a", "MathFloat", &producer_pins)?;
                let b = build_float_input(node, "b", "MathFloat", &producer_pins)?;
                let op_str = node.params.get("op").and_then(|v| v.as_str()).ok_or_else(|| {
                    CompileError::MissingInput {
                        node_id: node.id.clone(),
                        op: "MathFloat".to_string(),
                        pin: "op",
                    }
                })?;
                let op = BinaryFloatOp::parse(op_str).ok_or_else(|| {
                    CompileError::LiteralTypeMismatch {
                        node_id: node.id.clone(),
                        pin: "op",
                        expected: "Add|Sub|Mul|Div|Min|Max|Mod|Pow",
                    }
                })?;
                reject_unknown_inputs(node, &["a", "b"])?;
                let output = FloatSlot(pin_layout.floats);
                pin_layout.floats += 1;
                producer_pins.insert(id, ProducerPin::Float(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::MathFloat { op, a, b, output },
                ));
            }
            OpKind::MathVec2 => {
                let a = build_vec2_input(node, "a", "MathVec2", &producer_pins)?;
                let b = build_vec2_input(node, "b", "MathVec2", &producer_pins)?;
                let op_str = node.params.get("op").and_then(|v| v.as_str()).ok_or_else(|| {
                    CompileError::MissingInput {
                        node_id: node.id.clone(),
                        op: "MathVec2".to_string(),
                        pin: "op",
                    }
                })?;
                let op = BinaryVec2Op::parse(op_str).ok_or_else(|| {
                    CompileError::LiteralTypeMismatch {
                        node_id: node.id.clone(),
                        pin: "op",
                        expected: "Add|Sub|Mul|Div|Min|Max",
                    }
                })?;
                reject_unknown_inputs(node, &["a", "b"])?;
                let output = Vec2Slot(pin_layout.vec2s);
                pin_layout.vec2s += 1;
                producer_pins.insert(id, ProducerPin::Vec2(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::MathVec2 { op, a, b, output },
                ));
            }
            OpKind::ScaleVec2 => {
                let v = build_vec2_input(node, "v", "ScaleVec2", &producer_pins)?;
                let s = build_float_input(node, "s", "ScaleVec2", &producer_pins)?;
                reject_unknown_inputs(node, &["v", "s"])?;
                let output = Vec2Slot(pin_layout.vec2s);
                pin_layout.vec2s += 1;
                producer_pins.insert(id, ProducerPin::Vec2(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::ScaleVec2 { v, s, output },
                ));
            }
            OpKind::Length => {
                let input = build_vec2_input(node, "in", "Length", &producer_pins)?;
                reject_unknown_inputs(node, &["in"])?;
                let output = FloatSlot(pin_layout.floats);
                pin_layout.floats += 1;
                producer_pins.insert(id, ProducerPin::Float(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::Length { input, output },
                ));
            }
            OpKind::Normalize => {
                let input = build_vec2_input(node, "in", "Normalize", &producer_pins)?;
                reject_unknown_inputs(node, &["in"])?;
                let output = Vec2Slot(pin_layout.vec2s);
                pin_layout.vec2s += 1;
                producer_pins.insert(id, ProducerPin::Vec2(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::Normalize { input, output },
                ));
            }
            OpKind::Distance => {
                let a = build_vec2_input(node, "a", "Distance", &producer_pins)?;
                let b = build_vec2_input(node, "b", "Distance", &producer_pins)?;
                reject_unknown_inputs(node, &["a", "b"])?;
                let output = FloatSlot(pin_layout.floats);
                pin_layout.floats += 1;
                producer_pins.insert(id, ProducerPin::Float(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::Distance { a, b, output },
                ));
            }
            OpKind::Dot => {
                let a = build_vec2_input(node, "a", "Dot", &producer_pins)?;
                let b = build_vec2_input(node, "b", "Dot", &producer_pins)?;
                reject_unknown_inputs(node, &["a", "b"])?;
                let output = FloatSlot(pin_layout.floats);
                pin_layout.floats += 1;
                producer_pins.insert(id, ProducerPin::Float(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::Dot { a, b, output },
                ));
            }
            OpKind::Lerp => {
                let a = build_float_input(node, "a", "Lerp", &producer_pins)?;
                let b = build_float_input(node, "b", "Lerp", &producer_pins)?;
                let t = build_float_input(node, "t", "Lerp", &producer_pins)?;
                reject_unknown_inputs(node, &["a", "b", "t"])?;
                let output = FloatSlot(pin_layout.floats);
                pin_layout.floats += 1;
                producer_pins.insert(id, ProducerPin::Float(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::Lerp { a, b, t, output },
                ));
            }
            OpKind::Clamp => {
                let input = build_float_input(node, "in", "Clamp", &producer_pins)?;
                let min = build_float_input(node, "min", "Clamp", &producer_pins)?;
                let max = build_float_input(node, "max", "Clamp", &producer_pins)?;
                reject_unknown_inputs(node, &["in", "min", "max"])?;
                let output = FloatSlot(pin_layout.floats);
                pin_layout.floats += 1;
                producer_pins.insert(id, ProducerPin::Float(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::Clamp { input, min, max, output },
                ));
            }
            OpKind::MathUnary => {
                let input = build_float_input(node, "in", "MathUnary", &producer_pins)?;
                let op_str = node.params.get("op").and_then(|v| v.as_str()).ok_or_else(|| {
                    CompileError::MissingInput {
                        node_id: node.id.clone(),
                        op: "MathUnary".to_string(),
                        pin: "op",
                    }
                })?;
                let op = UnaryFloatOp::parse(op_str).ok_or_else(|| {
                    CompileError::LiteralTypeMismatch {
                        node_id: node.id.clone(),
                        pin: "op",
                        expected: "Abs|Sign|Neg|Sqrt|Sin|Cos|Tan|Floor|Ceil|Round",
                    }
                })?;
                reject_unknown_inputs(node, &["in"])?;
                let output = FloatSlot(pin_layout.floats);
                pin_layout.floats += 1;
                producer_pins.insert(id, ProducerPin::Float(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::MathUnary { op, input, output },
                ));
            }
            OpKind::Atan2 => {
                let y = build_float_input(node, "y", "Atan2", &producer_pins)?;
                let x = build_float_input(node, "x", "Atan2", &producer_pins)?;
                reject_unknown_inputs(node, &["y", "x"])?;
                let output = FloatSlot(pin_layout.floats);
                pin_layout.floats += 1;
                producer_pins.insert(id, ProducerPin::Float(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::Atan2 { y, x, output },
                ));
            }
            OpKind::BoolOp => {
                let a = build_bool_input(node, "a", "BoolOp", &producer_pins)?;
                let b = build_bool_input(node, "b", "BoolOp", &producer_pins)?;
                let op_str = node.params.get("op").and_then(|v| v.as_str()).ok_or_else(|| {
                    CompileError::MissingInput {
                        node_id: node.id.clone(),
                        op: "BoolOp".to_string(),
                        pin: "op",
                    }
                })?;
                let op = BoolBinaryOp::parse(op_str).ok_or_else(|| {
                    CompileError::LiteralTypeMismatch {
                        node_id: node.id.clone(),
                        pin: "op",
                        expected: "And|Or|Xor",
                    }
                })?;
                reject_unknown_inputs(node, &["a", "b"])?;
                let output = BoolSlot(pin_layout.bools);
                pin_layout.bools += 1;
                producer_pins.insert(id, ProducerPin::Bool(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::BoolOp { op, a, b, output },
                ));
            }
            OpKind::SelectFloat => {
                let cond = build_bool_input(node, "cond", "SelectFloat", &producer_pins)?;
                let a = build_float_input(node, "a", "SelectFloat", &producer_pins)?;
                let b = build_float_input(node, "b", "SelectFloat", &producer_pins)?;
                reject_unknown_inputs(node, &["cond", "a", "b"])?;
                let output = FloatSlot(pin_layout.floats);
                pin_layout.floats += 1;
                producer_pins.insert(id, ProducerPin::Float(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::SelectFloat { cond, a, b, output },
                ));
            }
            OpKind::SelectVec2 => {
                let cond = build_bool_input(node, "cond", "SelectVec2", &producer_pins)?;
                let a = build_vec2_input(node, "a", "SelectVec2", &producer_pins)?;
                let b = build_vec2_input(node, "b", "SelectVec2", &producer_pins)?;
                reject_unknown_inputs(node, &["cond", "a", "b"])?;
                let output = Vec2Slot(pin_layout.vec2s);
                pin_layout.vec2s += 1;
                producer_pins.insert(id, ProducerPin::Vec2(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::SelectVec2 { cond, a, b, output },
                ));
            }
            OpKind::GetRotation => {
                reject_unknown_inputs(node, &[])?;
                let output = FloatSlot(pin_layout.floats);
                pin_layout.floats += 1;
                producer_pins.insert(id, ProducerPin::Float(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::GetRotation { output },
                ));
            }
            OpKind::GetScale => {
                reject_unknown_inputs(node, &[])?;
                let output = Vec2Slot(pin_layout.vec2s);
                pin_layout.vec2s += 1;
                producer_pins.insert(id, ProducerPin::Vec2(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::GetScale { output },
                ));
            }
            OpKind::GetDeltaTime => {
                reject_unknown_inputs(node, &[])?;
                let output = FloatSlot(pin_layout.floats);
                pin_layout.floats += 1;
                producer_pins.insert(id, ProducerPin::Float(output));
                producer_ops.push((
                    node.id.clone(),
                    OpInstruction::GetDeltaTime { output },
                ));
            }
            _ => unreachable!("classified as producer"),
        }
    }

    // --- 5. Build port-keyed control-edge adjacency for trigger DFS ---
    // Slice 7.4A's `If` op routes execution down `then`/`else`
    // separately, so we key by `(from_id, port)` rather than just
    // `from_id`. Order is preserved within each port (BTreeMap-ish via
    // insertion order from `asset.edges`).
    let mut control_out: HashMap<(&str, ControlPort), Vec<&str>> = HashMap::new();
    for edge in &asset.edges {
        control_out
            .entry((edge.from.as_str(), edge.out.clone()))
            .or_default()
            .push(edge.to.as_str());
    }

    // --- 6. For each trigger node, DFS the action subgraph ---
    let mut diagnostics: Vec<CompileDiagnostic> = Vec::new();
    let mut programs: BTreeMap<TriggerKind, EventProgram> = BTreeMap::new();
    let mut subscriptions = EventMask::empty();
    let mut state_layout = StateLayout::default();

    for trigger_node in asset.nodes.iter().filter(|n| OpKind::parse(&n.op).unwrap().is_trigger())
    {
        let trigger_kind = OpKind::parse(&trigger_node.op).unwrap().trigger_kind().unwrap();
        let payload_pins = payload_by_kind[&trigger_kind];

        // OnKey: parse the static (key, edge) filter from the
        // trigger node's params and reject duplicates within the
        // same graph (v1 supports one OnKey per graph; multi-key
        // setups should be modelled as multiple graph instances or
        // handled via scripts).
        let key_filter = if trigger_kind == TriggerKind::OnKey {
            let key_name = trigger_node
                .params
                .get("key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CompileError::InvalidKeyTriggerParam {
                    node_id: trigger_node.id.clone(),
                    param: "key",
                    reason: "missing or not a string",
                })?;
            let key = crate::input::KeyCode::from_name(key_name).ok_or_else(|| {
                CompileError::InvalidKeyTriggerParam {
                    node_id: trigger_node.id.clone(),
                    param: "key",
                    reason: "unknown key name (see KeyCode::from_name)",
                }
            })?;
            let edge_name = trigger_node
                .params
                .get("edge")
                .and_then(|v| v.as_str())
                .unwrap_or("pressed");
            let edge = crate::behavior::event::KeyEdge::parse(edge_name).ok_or_else(|| {
                CompileError::InvalidKeyTriggerParam {
                    node_id: trigger_node.id.clone(),
                    param: "edge",
                    reason: "must be one of \"pressed\", \"released\", \"held\"",
                }
            })?;
            if let Some(existing) = programs.get(&TriggerKind::OnKey)
                && existing.key_filter.is_some()
            {
                // Find the prior OnKey node id for the error message.
                let first = asset
                    .nodes
                    .iter()
                    .find(|n| {
                        n.op == "OnKey" && n.id != trigger_node.id
                    })
                    .map(|n| n.id.clone())
                    .unwrap_or_default();
                return Err(CompileError::DuplicateOnKeyTrigger {
                    first,
                    second: trigger_node.id.clone(),
                });
            }
            Some(KeyFilter { key, edge })
        } else {
            None
        };

        // OnEvent: parse the static name filter (required).
        let name_filter = if trigger_kind == TriggerKind::OnEvent {
            let name = trigger_node
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CompileError::OnEventMissingName {
                    node_id: trigger_node.id.clone(),
                })?
                .to_string();
            Some(name)
        } else {
            None
        };

        // OnMouseClick: parse the static button filter (defaults to "any").
        let mouse_filter = if trigger_kind == TriggerKind::OnMouseClick {
            let raw = trigger_node
                .params
                .get("button")
                .and_then(|v| v.as_str())
                .unwrap_or("any");
            let f = MouseButtonFilter::parse(raw).ok_or_else(|| {
                CompileError::OnMouseClickInvalidButton {
                    node_id: trigger_node.id.clone(),
                    value: raw.to_string(),
                }
            })?;
            Some(f)
        } else {
            None
        };

        // OnTimer: the interval, defaulting to the catalog's 1 second. A
        // non-positive interval would fire every frame forever; clamp it.
        let timer_interval = if trigger_kind == TriggerKind::OnTimer {
            let secs = trigger_node
                .params
                .get("interval_seconds")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0) as f32;
            Some(secs.max(0.01))
        } else {
            None
        };

        let entry = programs.entry(trigger_kind).or_default();
        entry.payload_pins = payload_pins;
        if let Some(ti) = timer_interval {
            entry.timer_interval = Some(ti);
        }
        if let Some(kf) = key_filter {
            entry.key_filter = Some(kf);
        }
        if let Some(nf) = name_filter {
            entry.name_filter = Some(nf);
        }
        if let Some(mf) = mouse_filter {
            entry.mouse_filter = Some(mf);
        }
        subscriptions.insert(trigger_kind);

        let mut producers_used: HashSet<&str> = HashSet::new();
        let mut visited_action: HashSet<&str> = HashSet::new();
        let mut local_actions: Vec<(String, OpInstruction)> = Vec::new();

        // Every trigger's outputs were registered with the producers above,
        // so the chain reads them from the same map (BUGS #93 made the
        // primary readable by actions; Sep 2026 made all of them readable by
        // anything).
        let chain_pins = producer_pins.clone();

        emit_chain(
            trigger_node.id.as_str(),
            ControlPort::Exec,
            &by_id,
            &control_out,
            &chain_pins,
            &mut producers_used,
            &mut state_layout,
            &mut visited_action,
            trigger_kind,
            &mut diagnostics,
            &mut local_actions,
        )?;

        // Transitive closure of producer references: a needed Compare
        // may reference another Compare/Not via its own Float/Bool
        // input pins.
        loop {
            let snapshot: Vec<&str> = producers_used.iter().copied().collect();
            let before = producers_used.len();
            for pid in snapshot {
                if let Some(node) = by_id.get(pid) {
                    for binding in node.inputs.values() {
                        if let PinBinding::Reference { from, .. } = binding
                            && producer_pins.contains_key(from.as_str())
                        {
                            producers_used.insert(from.as_str());
                        }
                    }
                }
            }
            if producers_used.len() == before {
                break;
            }
        }

        // Emit producer ops (in topo order) first, then action ops.
        for (node_id, op) in &producer_ops {
            if producers_used.contains(node_id.as_str()) {
                entry.ops.push(op.clone());
                entry.source.push(node_id.clone());
            }
        }
        for (node_id, op) in local_actions {
            entry.ops.push(op);
            entry.source.push(node_id);
        }
    }

    Ok(CompiledGraph {
        programs,
        pin_layout,
        state_layout,
        subscriptions,
        diagnostics,
    })
}

/// DFS-emit the control chain rooted at `parent_id.port`.
///
/// Each visited node is appended to `out` in execution order. Gate
/// nodes (`If`, `Cooldown`, `Once`) are emitted as a header followed
/// by their inlined sub-block(s); the interpreter advances pc by the
/// recorded body length when the gate suppresses its body.
#[allow(clippy::too_many_arguments)]
fn emit_chain<'a>(
    parent_id: &'a str,
    port: ControlPort,
    by_id: &HashMap<&'a str, &'a BehaviorNode>,
    control_out: &HashMap<(&'a str, ControlPort), Vec<&'a str>>,
    producer_pins: &Producers<'a>,
    producers_used: &mut HashSet<&'a str>,
    state_layout: &mut StateLayout,
    visited: &mut HashSet<&'a str>,
    trigger_kind: TriggerKind,
    diagnostics: &mut Vec<CompileDiagnostic>,
    out: &mut Vec<(String, OpInstruction)>,
) -> Result<(), CompileError> {
    let targets: Vec<&'a str> = control_out
        .get(&(parent_id, port))
        .cloned()
        .unwrap_or_default();
    for target_id in targets {
        emit_node(
            target_id,
            by_id,
            control_out,
            producer_pins,
            producers_used,
            state_layout,
            visited,
            trigger_kind,
            diagnostics,
            out,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_node<'a>(
    id: &'a str,
    by_id: &HashMap<&'a str, &'a BehaviorNode>,
    control_out: &HashMap<(&'a str, ControlPort), Vec<&'a str>>,
    producer_pins: &Producers<'a>,
    producers_used: &mut HashSet<&'a str>,
    state_layout: &mut StateLayout,
    visited: &mut HashSet<&'a str>,
    trigger_kind: TriggerKind,
    diagnostics: &mut Vec<CompileDiagnostic>,
    out: &mut Vec<(String, OpInstruction)>,
) -> Result<(), CompileError> {
    if !visited.insert(id) {
        // A node reached via multiple control paths emits only on the
        // first visit (matches the BFS dedupe behavior of 7.2/7.3).
        return Ok(());
    }
    let node = by_id[id];
    let kind = OpKind::parse(&node.op).unwrap();

    // Track which producers this node references so the program head
    // can prepend only the producers transitively consumed.
    for binding in node.inputs.values() {
        if let PinBinding::Reference { from, .. } = binding
            && let Some(from_key) = producer_pins.get_key_value(from.as_str())
        {
            producers_used.insert(*from_key.0);
        }
    }

    if kind.is_gate() {
        match kind {
            OpKind::If => {
                let cond = build_bool_input(node, "cond", "If", producer_pins)?;
                reject_unknown_inputs(node, &["cond"])?;
                let mut then_block: Vec<(String, OpInstruction)> = Vec::new();
                emit_chain(
                    id, ControlPort::Then, by_id, control_out, producer_pins,
                    producers_used, state_layout, visited, trigger_kind, diagnostics,
                    &mut then_block,
                )?;
                let mut else_block: Vec<(String, OpInstruction)> = Vec::new();
                emit_chain(
                    id, ControlPort::Else, by_id, control_out, producer_pins,
                    producers_used, state_layout, visited, trigger_kind, diagnostics,
                    &mut else_block,
                )?;
                out.push((
                    node.id.clone(),
                    OpInstruction::If {
                        cond,
                        then_len: then_block.len() as u16,
                        else_len: else_block.len() as u16,
                    },
                ));
                out.extend(then_block);
                out.extend(else_block);
            }
            OpKind::Cooldown => {
                let seconds = build_float_input(node, "seconds", "Cooldown", producer_pins)?;
                reject_unknown_inputs(node, &["seconds"])?;
                let state_slot = StateFloatSlot(state_layout.floats);
                state_layout.floats += 1;
                let mut body: Vec<(String, OpInstruction)> = Vec::new();
                emit_chain(
                    id, ControlPort::Exec, by_id, control_out, producer_pins,
                    producers_used, state_layout, visited, trigger_kind, diagnostics,
                    &mut body,
                )?;
                out.push((
                    node.id.clone(),
                    OpInstruction::Cooldown {
                        seconds,
                        state_slot,
                        body_len: body.len() as u16,
                    },
                ));
                out.extend(body);
            }
            OpKind::Once => {
                reject_unknown_inputs(node, &[])?;
                let state_slot = StateBoolSlot(state_layout.bools);
                state_layout.bools += 1;
                let mut body: Vec<(String, OpInstruction)> = Vec::new();
                emit_chain(
                    id, ControlPort::Exec, by_id, control_out, producer_pins,
                    producers_used, state_layout, visited, trigger_kind, diagnostics,
                    &mut body,
                )?;
                out.push((
                    node.id.clone(),
                    OpInstruction::Once {
                        state_slot,
                        body_len: body.len() as u16,
                    },
                ));
                out.extend(body);
            }
            OpKind::Delay => {
                let seconds = build_float_input(node, "seconds", "Delay", producer_pins)?;
                reject_unknown_inputs(node, &["seconds"])?;
                let state_slot = StateFloatSlot(state_layout.floats);
                state_layout.floats += 1;
                let mut body: Vec<(String, OpInstruction)> = Vec::new();
                emit_chain(
                    id, ControlPort::Exec, by_id, control_out, producer_pins,
                    producers_used, state_layout, visited, trigger_kind, diagnostics,
                    &mut body,
                )?;
                out.push((
                    node.id.clone(),
                    OpInstruction::Delay {
                        seconds,
                        state_slot,
                        body_len: body.len() as u16,
                    },
                ));
                out.extend(body);
            }
            OpKind::ForCount => {
                let count = build_float_input(node, "count", "ForCount", producer_pins)?;
                reject_unknown_inputs(node, &["count"])?;
                let state_slot = StateCounterSlot(state_layout.counters);
                state_layout.counters += 1;
                let mut body: Vec<(String, OpInstruction)> = Vec::new();
                emit_chain(
                    id, ControlPort::Exec, by_id, control_out, producer_pins,
                    producers_used, state_layout, visited, trigger_kind, diagnostics,
                    &mut body,
                )?;
                out.push((
                    node.id.clone(),
                    OpInstruction::ForCount {
                        count,
                        state_slot,
                        body_len: body.len() as u16,
                    },
                ));
                out.extend(body);
            }
            _ => unreachable!("is_gate covered above"),
        }
        return Ok(());
    }

    if kind.is_data_producer() || kind.is_trigger() {
        // Producers are emitted at program head; triggers don't chain.
        return Ok(());
    }

    // Action ops: emit one instruction + chain through Exec.
    if matches!(trigger_kind, TriggerKind::OnTick)
        && matches!(kind, OpKind::BounceOnBounds | OpKind::DespawnAfter)
    {
        diagnostics.push(CompileDiagnostic {
            node_id: node.id.clone(),
            kind: CompileWarning::SetupOpUnderOnTick { op: kind.display_name() },
        });
    }
    let instr = build_action_op(node, kind, producer_pins)?;
    out.push((node.id.clone(), instr));
    emit_chain(
        id, ControlPort::Exec, by_id, control_out, producer_pins,
        producers_used, state_layout, visited, trigger_kind, diagnostics,
        out,
    )?;
    Ok(())
}

fn allocate_payload_pins(kind: TriggerKind, layout: &mut PinLayout) -> PayloadPins {
    let mut p = PayloadPins::default();
    match kind {
        TriggerKind::OnCollision => {
            p.other_entity = Some(EntitySlot(layout.entities));
            layout.entities += 1;
            p.enter = Some(BoolSlot(layout.bools));
            layout.bools += 1;
        }
        TriggerKind::OnTrigger => {
            p.other_entity = Some(EntitySlot(layout.entities));
            layout.entities += 1;
        }
        TriggerKind::OnTimer => {
            p.tick_index = Some(FloatSlot(layout.floats));
            layout.floats += 1;
        }
        TriggerKind::OnTick | TriggerKind::OnSpawn | TriggerKind::OnDespawn => {}
        // OnKey carries no dynamic payload pins — the (key, edge)
        // filter is baked into `EventProgram::key_filter` at compile
        // time; the dispatcher only fires when InputState matches, so
        // handlers don't need to test the key from inside the program.
        TriggerKind::OnKey => {}
        TriggerKind::OnEvent => {
            p.event_value = Some(FloatSlot(layout.floats));
            layout.floats += 1;
        }
        TriggerKind::OnMouseClick => {
            p.mouse_pos = Some(Vec2Slot(layout.vec2s));
            layout.vec2s += 1;
            p.mouse_button = Some(FloatSlot(layout.floats));
            layout.floats += 1;
        }
    }
    p
}

/// Every output a pin reference can name.
///
/// `{ "from": "hit" }` is a node's **primary** output; `{ "from": "hit",
/// "out": "enter" }` names one. Until Sep 2026 there was only the first:
/// the map was keyed by node id alone and `out` was parsed and ignored, so
/// the three triggers with more than one output could hand over only one of
/// them. `OnCollision.enter` was unreachable - a graph could not tell a
/// contact starting from one ending, and fired its "on hit" logic both times
/// --  and so was `OnMouseClick.button`, while the catalog and the editor's
/// palette advertised both. An `out` naming nothing is now an error rather
/// than a silent redirect to the primary.
#[derive(Debug, Clone, Default)]
struct Producers<'a> {
    /// `from` alone.
    pins: HashMap<&'a str, ProducerPin>,
    /// `from` + `out` for a trigger's non-primary outputs.
    named: Vec<(&'a str, &'static str, ProducerPin)>,
    /// Each node's primary output name, so an `out` spelling it resolves too.
    primary: HashMap<&'a str, &'static str>,
}

impl<'a> Producers<'a> {
    fn insert(&mut self, id: &'a str, pin: ProducerPin) {
        self.pins.insert(id, pin);
    }

    fn insert_named(&mut self, id: &'a str, out: &'static str, pin: ProducerPin) {
        self.named.push((id, out, pin));
    }

    fn contains_key(&self, id: &str) -> bool {
        self.pins.contains_key(id)
    }

    fn get_key_value(&self, id: &str) -> Option<(&&'a str, &ProducerPin)> {
        self.pins.get_key_value(id)
    }

    fn resolve(&self, from: &str, out: Option<&str>) -> Option<&ProducerPin> {
        let Some(out) = out else { return self.pins.get(from) };
        if let Some((_, _, pin)) = self.named.iter().find(|(f, o, _)| *f == from && *o == out) {
            return Some(pin);
        }
        if self.primary.get(from).is_some_and(|p| *p == out) {
            return self.pins.get(from);
        }
        None
    }
}

/// How a reference names its source in an error: `hit.enter` when it gave
/// an `out`, so a misspelt output is not reported as a missing node.
fn ref_name(from: &str, out: Option<&str>) -> String {
    match out {
        Some(o) => format!("{from}.{o}"),
        None => from.to_string(),
    }
}

/// The primary output of each op - the one `{ "from": id }` means.
///
/// For the triggers it is the payload the compiler has always exposed; for
/// everything else the op's only output. `None` for an op with no output.
fn primary_output_name(op: &str) -> Option<&'static str> {
    match op {
        "OnTimer" => Some("tick_index"),
        "OnEvent" => Some("value"),
        "OnMouseClick" => Some("pos"),
        "OnCollision" | "OnTrigger" => Some("other"),
        _ => crate::behavior::catalog::op_catalog()
            .into_iter()
            .find(|d| d.op == op)
            .filter(|d| d.outputs.len() == 1)
            .map(|d| d.outputs[0].name),
    }
}

#[derive(Debug, Clone, Copy)]
enum ProducerPin {
    Vec2(Vec2Slot),
    #[allow(dead_code)] // wired by 7.4B (GetVelocity \u2192 Vec2.x split, etc.)
    Float(FloatSlot),
    Bool(BoolSlot),
    Entity(EntitySlot),
}

#[derive(Debug, Clone, Copy)]
enum OpKind {
    OnTick,
    OnSpawn,
    OnDespawn,
    OnCollision,
    OnTrigger,
    OnTimer,
    OnKey,
    OnEvent,
    OnMouseClick,
    GetInputAxis,
    GetPosition,
    GetVelocity,
    Compare,
    Not,
    MakeVec2,
    GetVec2X,
    GetVec2Y,
    MathFloat,
    MathVec2,
    ScaleVec2,
    Length,
    Normalize,
    Distance,
    Dot,
    Lerp,
    Clamp,
    MathUnary,
    Atan2,
    BoolOp,
    SelectFloat,
    SelectVec2,
    GetRotation,
    GetScale,
    GetDeltaTime,
    GetNearestByTag,
    GetEntityPosition,
    GetCustomField,
    If,
    Cooldown,
    Once,
    Delay,
    ForCount,
    Sequence,
    PlayAnimation,
    StopAnimation,
    EmitEvent,
    SetVelocity,
    AddVelocity,
    Translate,
    ApplyForce,
    BounceOnBounds,
    DespawnAfter,
    SpawnEntity,
    Despawn,
    DespawnByTag,
    ChangeScene,
    PlaySound,
    LookAt,
    MoveToward,
    SetPosition,
    SetRotation,
    SetScale,
    SetSpriteColor,
    SetSpriteAlpha,
    AddTag,
    RemoveTag,
    SetCustomField,
    Log,
}

impl OpKind {
    fn parse(name: &str) -> Result<Self, ()> {
        Ok(match name {
            "OnTick" => Self::OnTick,
            "OnSpawn" => Self::OnSpawn,
            "OnDespawn" => Self::OnDespawn,
            "OnCollision" => Self::OnCollision,
            "OnTrigger" => Self::OnTrigger,
            "OnTimer" => Self::OnTimer,
            "OnKey" => Self::OnKey,
            "OnEvent" => Self::OnEvent,
            "OnMouseClick" => Self::OnMouseClick,
            "GetInputAxis" => Self::GetInputAxis,
            "GetPosition" => Self::GetPosition,
            "GetVelocity" => Self::GetVelocity,
            "Compare" => Self::Compare,
            "Not" => Self::Not,
            "MakeVec2" => Self::MakeVec2,
            "GetVec2X" => Self::GetVec2X,
            "GetVec2Y" => Self::GetVec2Y,
            "MathFloat" => Self::MathFloat,
            "MathVec2" => Self::MathVec2,
            "ScaleVec2" => Self::ScaleVec2,
            "Length" => Self::Length,
            "Normalize" => Self::Normalize,
            "Distance" => Self::Distance,
            "Dot" => Self::Dot,
            "Lerp" => Self::Lerp,
            "Clamp" => Self::Clamp,
            "MathUnary" => Self::MathUnary,
            "Atan2" => Self::Atan2,
            "BoolOp" => Self::BoolOp,
            "SelectFloat" => Self::SelectFloat,
            "SelectVec2" => Self::SelectVec2,
            "GetRotation" => Self::GetRotation,
            "GetScale" => Self::GetScale,
            "GetDeltaTime" => Self::GetDeltaTime,
            "GetNearestByTag" => Self::GetNearestByTag,
            "GetEntityPosition" => Self::GetEntityPosition,
            "GetCustomField" => Self::GetCustomField,
            "If" => Self::If,
            "Cooldown" => Self::Cooldown,
            "Once" => Self::Once,
            "Delay" => Self::Delay,
            "ForCount" => Self::ForCount,
            "Sequence" => Self::Sequence,
            "PlayAnimation" => Self::PlayAnimation,
            "StopAnimation" => Self::StopAnimation,
            "EmitEvent" => Self::EmitEvent,
            "SetVelocity" => Self::SetVelocity,
            "AddVelocity" => Self::AddVelocity,
            "Translate" => Self::Translate,
            "ApplyForce" => Self::ApplyForce,
            "BounceOnBounds" => Self::BounceOnBounds,
            "DespawnAfter" => Self::DespawnAfter,
            "SpawnEntity" => Self::SpawnEntity,
            "Despawn" => Self::Despawn,
            "DespawnByTag" => Self::DespawnByTag,
            "ChangeScene" => Self::ChangeScene,
            "PlaySound" => Self::PlaySound,
            "LookAt" => Self::LookAt,
            "MoveToward" => Self::MoveToward,
            "SetPosition" => Self::SetPosition,
            "SetRotation" => Self::SetRotation,
            "SetScale" => Self::SetScale,
            "SetSpriteColor" => Self::SetSpriteColor,
            "SetSpriteAlpha" => Self::SetSpriteAlpha,
            "AddTag" => Self::AddTag,
            "RemoveTag" => Self::RemoveTag,
            "SetCustomField" => Self::SetCustomField,
            "Log" => Self::Log,
            _ => return Err(()),
        })
    }

    fn is_data_producer(self) -> bool {
        matches!(
            self,
            Self::GetInputAxis
                | Self::GetPosition
                | Self::GetVelocity
                | Self::Compare
                | Self::Not
                | Self::MakeVec2
                | Self::GetVec2X
                | Self::GetVec2Y
                | Self::MathFloat
                | Self::MathVec2
                | Self::ScaleVec2
                | Self::Length
                | Self::Normalize
                | Self::Distance
                | Self::Dot
                | Self::Lerp
                | Self::Clamp
                | Self::MathUnary
                | Self::Atan2
                | Self::BoolOp
                | Self::SelectFloat
                | Self::SelectVec2
                | Self::GetRotation
                | Self::GetScale
                | Self::GetDeltaTime
                | Self::GetNearestByTag
                | Self::GetEntityPosition
                | Self::GetCustomField
        )
    }

    fn is_gate(self) -> bool {
        matches!(
            self,
            Self::If | Self::Cooldown | Self::Once | Self::Delay | Self::ForCount
        )
    }

    fn is_trigger(self) -> bool {
        self.trigger_kind().is_some()
    }

    fn trigger_kind(self) -> Option<TriggerKind> {
        Some(match self {
            Self::OnTick => TriggerKind::OnTick,
            Self::OnSpawn => TriggerKind::OnSpawn,
            Self::OnDespawn => TriggerKind::OnDespawn,
            Self::OnCollision => TriggerKind::OnCollision,
            Self::OnTrigger => TriggerKind::OnTrigger,
            Self::OnTimer => TriggerKind::OnTimer,
            Self::OnKey => TriggerKind::OnKey,
            Self::OnEvent => TriggerKind::OnEvent,
            Self::OnMouseClick => TriggerKind::OnMouseClick,
            _ => return None,
        })
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::OnTick => "OnTick",
            Self::OnSpawn => "OnSpawn",
            Self::OnDespawn => "OnDespawn",
            Self::OnCollision => "OnCollision",
            Self::OnTrigger => "OnTrigger",
            Self::OnTimer => "OnTimer",
            Self::OnKey => "OnKey",
            Self::OnEvent => "OnEvent",
            Self::OnMouseClick => "OnMouseClick",
            Self::GetInputAxis => "GetInputAxis",
            Self::GetPosition => "GetPosition",
            Self::GetVelocity => "GetVelocity",
            Self::Compare => "Compare",
            Self::Not => "Not",
            Self::MakeVec2 => "MakeVec2",
            Self::GetVec2X => "GetVec2X",
            Self::GetVec2Y => "GetVec2Y",
            Self::MathFloat => "MathFloat",
            Self::MathVec2 => "MathVec2",
            Self::ScaleVec2 => "ScaleVec2",
            Self::Length => "Length",
            Self::Normalize => "Normalize",
            Self::Distance => "Distance",
            Self::Dot => "Dot",
            Self::Lerp => "Lerp",
            Self::Clamp => "Clamp",
            Self::MathUnary => "MathUnary",
            Self::Atan2 => "Atan2",
            Self::BoolOp => "BoolOp",
            Self::SelectFloat => "SelectFloat",
            Self::SelectVec2 => "SelectVec2",
            Self::GetRotation => "GetRotation",
            Self::GetScale => "GetScale",
            Self::GetDeltaTime => "GetDeltaTime",
            Self::GetNearestByTag => "GetNearestByTag",
            Self::GetEntityPosition => "GetEntityPosition",
            Self::GetCustomField => "GetCustomField",
            Self::If => "If",
            Self::Cooldown => "Cooldown",
            Self::Once => "Once",
            Self::Delay => "Delay",
            Self::ForCount => "ForCount",
            Self::Sequence => "Sequence",
            Self::PlayAnimation => "PlayAnimation",
            Self::StopAnimation => "StopAnimation",
            Self::EmitEvent => "EmitEvent",
            Self::SetVelocity => "SetVelocity",
            Self::AddVelocity => "AddVelocity",
            Self::Translate => "Translate",
            Self::ApplyForce => "ApplyForce",
            Self::BounceOnBounds => "BounceOnBounds",
            Self::DespawnAfter => "DespawnAfter",
            Self::SpawnEntity => "SpawnEntity",
            Self::Despawn => "Despawn",
            Self::DespawnByTag => "DespawnByTag",
            Self::ChangeScene => "ChangeScene",
            Self::PlaySound => "PlaySound",
            Self::LookAt => "LookAt",
            Self::MoveToward => "MoveToward",
            Self::SetPosition => "SetPosition",
            Self::SetRotation => "SetRotation",
            Self::SetScale => "SetScale",
            Self::SetSpriteColor => "SetSpriteColor",
            Self::SetSpriteAlpha => "SetSpriteAlpha",
            Self::AddTag => "AddTag",
            Self::RemoveTag => "RemoveTag",
            Self::SetCustomField => "SetCustomField",
            Self::Log => "Log",
        }
    }
}

fn build_action_op(
    node: &BehaviorNode,
    kind: OpKind,
    producer_pins: &Producers<'_>,
) -> Result<OpInstruction, CompileError> {
    match kind {
        OpKind::SetVelocity => {
            let vel = build_vec2_input(node, "vel", "SetVelocity", producer_pins)?;
            reject_unknown_inputs(node, &["vel"])?;
            Ok(OpInstruction::SetVelocity { vel })
        }
        OpKind::AddVelocity => {
            let delta = build_vec2_input(node, "delta", "AddVelocity", producer_pins)?;
            reject_unknown_inputs(node, &["delta"])?;
            Ok(OpInstruction::AddVelocity { delta })
        }
        OpKind::Translate => {
            let delta = build_vec2_input(node, "delta", "Translate", producer_pins)?;
            reject_unknown_inputs(node, &["delta"])?;
            Ok(OpInstruction::Translate { delta })
        }
        OpKind::ApplyForce => {
            let force = build_vec2_input(node, "force", "ApplyForce", producer_pins)?;
            reject_unknown_inputs(node, &["force"])?;
            Ok(OpInstruction::ApplyForce { force })
        }
        OpKind::BounceOnBounds => {
            // Optional baked literal `bounds` from `params`.
            let bounds = node.params.get("bounds").map(parse_bounds_literal).transpose()
                .map_err(|()| CompileError::LiteralTypeMismatch {
                    node_id: node.id.clone(),
                    pin: "bounds",
                    expected: "Aabb {min_x,min_y,max_x,max_y[,damping]}",
                })?;
            reject_unknown_inputs(node, &[])?;
            Ok(OpInstruction::BounceOnBounds { bounds })
        }
        OpKind::DespawnAfter => {
            let seconds = build_float_input(node, "seconds", "DespawnAfter", producer_pins)?;
            reject_unknown_inputs(node, &["seconds"])?;
            Ok(OpInstruction::DespawnAfter { seconds })
        }
        OpKind::SpawnEntity => {
            let prefab_id = node
                .params
                .get("prefab_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CompileError::MissingInput {
                    node_id: node.id.clone(),
                    op: "SpawnEntity".to_string(),
                    pin: "prefab_id",
                })?
                .to_string();
            let at = build_vec2_input(node, "at", "SpawnEntity", producer_pins)?;
            let rotation = if node.inputs.contains_key("rotation") {
                Some(build_float_input(node, "rotation", "SpawnEntity", producer_pins)?)
            } else {
                None
            };
            let scale = if node.inputs.contains_key("scale") {
                Some(build_vec2_input(node, "scale", "SpawnEntity", producer_pins)?)
            } else {
                None
            };
            let with_velocity = if node.inputs.contains_key("with_velocity") {
                Some(build_vec2_input(node, "with_velocity", "SpawnEntity", producer_pins)?)
            } else {
                None
            };
            reject_unknown_inputs(node, &["at", "rotation", "scale", "with_velocity"])?;
            Ok(OpInstruction::SpawnEntity {
                prefab_id,
                at,
                rotation,
                scale,
                with_velocity,
            })
        }
        OpKind::Despawn => {
            reject_unknown_inputs(node, &[])?;
            Ok(OpInstruction::Despawn)
        }
        OpKind::DespawnByTag => {
            let tag = node
                .params
                .get("tag")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CompileError::MissingInput {
                    node_id: node.id.clone(),
                    op: "DespawnByTag".to_string(),
                    pin: "tag",
                })?
                .to_string();
            reject_unknown_inputs(node, &[])?;
            Ok(OpInstruction::DespawnByTag { tag })
        }
        OpKind::ChangeScene => {
            let scene_id = node
                .params
                .get("scene_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CompileError::MissingInput {
                    node_id: node.id.clone(),
                    op: "ChangeScene".to_string(),
                    pin: "scene_id",
                })?
                .to_string();
            reject_unknown_inputs(node, &[])?;
            Ok(OpInstruction::ChangeScene { scene_id })
        }
        OpKind::PlaySound => {
            let sound_id = node
                .params
                .get("sound_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CompileError::MissingInput {
                    node_id: node.id.clone(),
                    op: "PlaySound".to_string(),
                    pin: "sound_id",
                })?
                .to_string();
            let volume = if node.inputs.contains_key("volume") {
                build_float_input(node, "volume", "PlaySound", producer_pins)?
            } else {
                FloatInput::Literal(1.0)
            };
            reject_unknown_inputs(node, &["volume"])?;
            Ok(OpInstruction::PlaySound { sound_id, volume })
        }
        OpKind::LookAt => {
            let target_pos = build_vec2_input(node, "target_pos", "LookAt", producer_pins)?;
            reject_unknown_inputs(node, &["target_pos"])?;
            Ok(OpInstruction::LookAt { target_pos })
        }
        OpKind::MoveToward => {
            let target_pos = build_vec2_input(node, "target_pos", "MoveToward", producer_pins)?;
            let speed = build_float_input(node, "speed", "MoveToward", producer_pins)?;
            reject_unknown_inputs(node, &["target_pos", "speed"])?;
            Ok(OpInstruction::MoveToward { target_pos, speed })
        }
        OpKind::SetPosition => {
            let pos = build_vec2_input(node, "pos", "SetPosition", producer_pins)?;
            reject_unknown_inputs(node, &["pos"])?;
            Ok(OpInstruction::SetPosition { pos })
        }
        OpKind::SetRotation => {
            let rot = build_float_input(node, "rot", "SetRotation", producer_pins)?;
            reject_unknown_inputs(node, &["rot"])?;
            Ok(OpInstruction::SetRotation { rot })
        }
        OpKind::SetScale => {
            let scale = build_vec2_input(node, "scale", "SetScale", producer_pins)?;
            reject_unknown_inputs(node, &["scale"])?;
            Ok(OpInstruction::SetScale { scale })
        }
        OpKind::SetSpriteColor => {
            let r = build_float_input(node, "r", "SetSpriteColor", producer_pins)?;
            let g = build_float_input(node, "g", "SetSpriteColor", producer_pins)?;
            let b = build_float_input(node, "b", "SetSpriteColor", producer_pins)?;
            let a = if node.inputs.contains_key("a") {
                build_float_input(node, "a", "SetSpriteColor", producer_pins)?
            } else {
                FloatInput::Literal(1.0)
            };
            reject_unknown_inputs(node, &["r", "g", "b", "a"])?;
            Ok(OpInstruction::SetSpriteColor { r, g, b, a })
        }
        OpKind::SetSpriteAlpha => {
            let alpha = build_float_input(node, "alpha", "SetSpriteAlpha", producer_pins)?;
            reject_unknown_inputs(node, &["alpha"])?;
            Ok(OpInstruction::SetSpriteAlpha { alpha })
        }
        OpKind::AddTag => {
            let tag = node
                .params
                .get("tag")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CompileError::MissingInput {
                    node_id: node.id.clone(),
                    op: "AddTag".to_string(),
                    pin: "tag",
                })?
                .to_string();
            reject_unknown_inputs(node, &[])?;
            Ok(OpInstruction::AddTag { tag })
        }
        OpKind::RemoveTag => {
            let tag = node
                .params
                .get("tag")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CompileError::MissingInput {
                    node_id: node.id.clone(),
                    op: "RemoveTag".to_string(),
                    pin: "tag",
                })?
                .to_string();
            reject_unknown_inputs(node, &[])?;
            Ok(OpInstruction::RemoveTag { tag })
        }
        OpKind::SetCustomField => {
            let component = node
                .params
                .get("component")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CompileError::MissingInput {
                    node_id: node.id.clone(),
                    op: "SetCustomField".to_string(),
                    pin: "component",
                })?
                .to_string();
            let field = node
                .params
                .get("field")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CompileError::MissingInput {
                    node_id: node.id.clone(),
                    op: "SetCustomField".to_string(),
                    pin: "field",
                })?
                .to_string();
            let value = build_float_input(node, "value", "SetCustomField", producer_pins)?;
            reject_unknown_inputs(node, &["value"])?;
            Ok(OpInstruction::SetCustomField { component, field, value })
        }
        OpKind::Log => {
            let label = node
                .params
                .get("label")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CompileError::MissingInput {
                    node_id: node.id.clone(),
                    op: "Log".to_string(),
                    pin: "label",
                })?
                .to_string();
            let value = if node.inputs.contains_key("value") {
                Some(build_float_input(node, "value", "Log", producer_pins)?)
            } else {
                None
            };
            reject_unknown_inputs(node, &["value"])?;
            Ok(OpInstruction::Log { label, value })
        }
        OpKind::Sequence => {
            reject_unknown_inputs(node, &[])?;
            Ok(OpInstruction::Sequence)
        }
        OpKind::PlayAnimation => {
            let anim_id = node
                .params
                .get("anim_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CompileError::MissingInput {
                    node_id: node.id.clone(),
                    op: "PlayAnimation".to_string(),
                    pin: "anim_id",
                })?
                .to_string();
            reject_unknown_inputs(node, &[])?;
            Ok(OpInstruction::PlayAnimation { anim_id })
        }
        OpKind::StopAnimation => {
            reject_unknown_inputs(node, &[])?;
            Ok(OpInstruction::StopAnimation)
        }
        OpKind::EmitEvent => {
            let name = node
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CompileError::MissingInput {
                    node_id: node.id.clone(),
                    op: "EmitEvent".to_string(),
                    pin: "name",
                })?
                .to_string();
            let value = if node.inputs.contains_key("value") {
                Some(build_float_input(node, "value", "EmitEvent", producer_pins)?)
            } else {
                None
            };
            reject_unknown_inputs(node, &["value"])?;
            Ok(OpInstruction::EmitEvent { name, value })
        }
        // Triggers, producers, and gates are never emitted via this
        // path — the outer compile() phases handle them directly.
        OpKind::OnTick
        | OpKind::OnSpawn
        | OpKind::OnDespawn
        | OpKind::OnCollision
        | OpKind::OnTrigger
        | OpKind::OnTimer
        | OpKind::OnKey
        | OpKind::OnEvent
        | OpKind::OnMouseClick
        | OpKind::GetInputAxis
        | OpKind::GetPosition
        | OpKind::GetVelocity
        | OpKind::Compare
        | OpKind::Not
        | OpKind::MakeVec2
        | OpKind::GetVec2X
        | OpKind::GetVec2Y
        | OpKind::MathFloat
        | OpKind::MathVec2
        | OpKind::ScaleVec2
        | OpKind::Length
        | OpKind::Normalize
        | OpKind::Distance
        | OpKind::Dot
        | OpKind::Lerp
        | OpKind::Clamp
        | OpKind::MathUnary
        | OpKind::Atan2
        | OpKind::BoolOp
        | OpKind::SelectFloat
        | OpKind::SelectVec2
        | OpKind::GetRotation
        | OpKind::GetScale
        | OpKind::GetDeltaTime
        | OpKind::GetNearestByTag
        | OpKind::GetEntityPosition
        | OpKind::GetCustomField
        | OpKind::If
        | OpKind::Cooldown
        | OpKind::Once
        | OpKind::Delay
        | OpKind::ForCount => Ok(OpInstruction::OnTick),
    }
}

fn build_vec2_input(
    node: &BehaviorNode,
    pin: &'static str,
    op_name: &'static str,
    producer_pins: &Producers<'_>,
) -> Result<Vec2Input, CompileError> {
    let binding = node.inputs.get(pin).ok_or_else(|| CompileError::MissingInput {
        node_id: node.id.clone(),
        op: op_name.to_string(),
        pin,
    })?;
    match binding {
        PinBinding::Literal { value } => Ok(Vec2Input::Literal(parse_vec2_literal(value).map_err(
            |()| CompileError::LiteralTypeMismatch {
                node_id: node.id.clone(),
                pin,
                expected: "Vec2 [x,y]",
            },
        )?)),
        PinBinding::Reference {
            from, out, scale, offset,
        } => {
            let producer = producer_pins.resolve(from.as_str(), out.as_deref()).ok_or_else(|| {
                CompileError::PinRefNotFound {
                    node_id: node.id.clone(),
                    pin,
                    from: ref_name(from, out.as_deref()),
                }
            })?;
            match producer {
                ProducerPin::Vec2(slot) => Ok(Vec2Input::Pin {
                    slot: *slot,
                    scale: scale.unwrap_or(1.0),
                    offset: offset.unwrap_or(0.0),
                }),
                ProducerPin::Float(_) => Err(CompileError::PinTypeMismatch {
                    node_id: node.id.clone(),
                    pin,
                    expected: "Vec2",
                    actual: "Float",
                    from_op: from.clone(),
                }),
                ProducerPin::Bool(_) => Err(CompileError::PinTypeMismatch {
                    node_id: node.id.clone(),
                    pin,
                    expected: "Vec2",
                    actual: "Bool",
                    from_op: from.clone(),
                }),
                ProducerPin::Entity(_) => Err(CompileError::PinTypeMismatch {
                    node_id: node.id.clone(),
                    pin,
                    expected: "Vec2",
                    actual: "EntityRef",
                    from_op: from.clone(),
                }),
            }
        }
    }
}

fn build_float_input(
    node: &BehaviorNode,
    pin: &'static str,
    op_name: &'static str,
    producer_pins: &Producers<'_>,
) -> Result<FloatInput, CompileError> {
    let binding = node.inputs.get(pin).ok_or_else(|| CompileError::MissingInput {
        node_id: node.id.clone(),
        op: op_name.to_string(),
        pin,
    })?;
    match binding {
        PinBinding::Literal { value } => {
            let v = value.as_f64().ok_or(CompileError::LiteralTypeMismatch {
                node_id: node.id.clone(),
                pin,
                expected: "Float",
            })?;
            Ok(FloatInput::Literal(v as f32))
        }
        PinBinding::Reference {
            from, out, scale, offset,
        } => {
            let producer = producer_pins.resolve(from.as_str(), out.as_deref()).ok_or_else(|| {
                CompileError::PinRefNotFound {
                    node_id: node.id.clone(),
                    pin,
                    from: ref_name(from, out.as_deref()),
                }
            })?;
            match producer {
                ProducerPin::Float(slot) => Ok(FloatInput::Pin {
                    slot: *slot,
                    scale: scale.unwrap_or(1.0),
                    offset: offset.unwrap_or(0.0),
                }),
                ProducerPin::Vec2(_) => Err(CompileError::PinTypeMismatch {
                    node_id: node.id.clone(),
                    pin,
                    expected: "Float",
                    actual: "Vec2",
                    from_op: from.clone(),
                }),
                ProducerPin::Bool(_) => Err(CompileError::PinTypeMismatch {
                    node_id: node.id.clone(),
                    pin,
                    expected: "Float",
                    actual: "Bool",
                    from_op: from.clone(),
                }),
                ProducerPin::Entity(_) => Err(CompileError::PinTypeMismatch {
                    node_id: node.id.clone(),
                    pin,
                    expected: "Float",
                    actual: "EntityRef",
                    from_op: from.clone(),
                }),
            }
        }
    }
}

/// Resolve a `Bool` input pin to either a baked literal or a read of a
/// producer-emitted `Bool` slot.
fn build_bool_input(
    node: &BehaviorNode,
    pin: &'static str,
    op_name: &'static str,
    producer_pins: &Producers<'_>,
) -> Result<BoolInput, CompileError> {
    let binding = node.inputs.get(pin).ok_or_else(|| CompileError::MissingInput {
        node_id: node.id.clone(),
        op: op_name.to_string(),
        pin,
    })?;
    match binding {
        PinBinding::Literal { value } => {
            let v = value.as_bool().ok_or(CompileError::LiteralTypeMismatch {
                node_id: node.id.clone(),
                pin,
                expected: "Bool",
            })?;
            Ok(BoolInput::Literal(v))
        }
        PinBinding::Reference { from, out, .. } => {
            let producer = producer_pins.resolve(from.as_str(), out.as_deref()).ok_or_else(|| {
                CompileError::PinRefNotFound {
                    node_id: node.id.clone(),
                    pin,
                    from: ref_name(from, out.as_deref()),
                }
            })?;
            match producer {
                ProducerPin::Bool(slot) => Ok(BoolInput::Pin { slot: *slot }),
                ProducerPin::Float(_) => Err(CompileError::PinTypeMismatch {
                    node_id: node.id.clone(),
                    pin,
                    expected: "Bool",
                    actual: "Float",
                    from_op: from.clone(),
                }),
                ProducerPin::Vec2(_) => Err(CompileError::PinTypeMismatch {
                    node_id: node.id.clone(),
                    pin,
                    expected: "Bool",
                    actual: "Vec2",
                    from_op: from.clone(),
                }),
                ProducerPin::Entity(_) => Err(CompileError::PinTypeMismatch {
                    node_id: node.id.clone(),
                    pin,
                    expected: "Bool",
                    actual: "EntityRef",
                    from_op: from.clone(),
                }),
            }
        }
    }
}

/// Resolve an `EntityRef` input pin. Entity pins have no scale/offset
/// and cannot be baked literals — they must be wired from an
/// entity-producing op (`GetNearestByTag`).
fn build_entity_input(
    node: &BehaviorNode,
    pin: &'static str,
    op_name: &'static str,
    producer_pins: &Producers<'_>,
) -> Result<EntitySlot, CompileError> {
    let binding = node.inputs.get(pin).ok_or_else(|| CompileError::MissingInput {
        node_id: node.id.clone(),
        op: op_name.to_string(),
        pin,
    })?;
    match binding {
        PinBinding::Literal { .. } => Err(CompileError::LiteralTypeMismatch {
            node_id: node.id.clone(),
            pin,
            expected: "EntityRef (wire from GetNearestByTag; literals unsupported)",
        }),
        PinBinding::Reference { from, out, .. } => {
            let producer = producer_pins.resolve(from.as_str(), out.as_deref()).ok_or_else(|| {
                CompileError::PinRefNotFound {
                    node_id: node.id.clone(),
                    pin,
                    from: ref_name(from, out.as_deref()),
                }
            })?;
            match producer {
                ProducerPin::Entity(slot) => Ok(*slot),
                ProducerPin::Vec2(_) => Err(CompileError::PinTypeMismatch {
                    node_id: node.id.clone(),
                    pin,
                    expected: "EntityRef",
                    actual: "Vec2",
                    from_op: from.clone(),
                }),
                ProducerPin::Float(_) => Err(CompileError::PinTypeMismatch {
                    node_id: node.id.clone(),
                    pin,
                    expected: "EntityRef",
                    actual: "Float",
                    from_op: from.clone(),
                }),
                ProducerPin::Bool(_) => Err(CompileError::PinTypeMismatch {
                    node_id: node.id.clone(),
                    pin,
                    expected: "EntityRef",
                    actual: "Bool",
                    from_op: from.clone(),
                }),
            }
        }
    }
}

fn reject_unknown_inputs(
    node: &BehaviorNode,
    accepted: &[&'static str],
) -> Result<(), CompileError> {
    for pin in node.inputs.keys() {
        if !accepted.contains(&pin.as_str()) {
            return Err(CompileError::UnknownInputPin {
                node_id: node.id.clone(),
                op: node.op.clone(),
                pin: pin.clone(),
            });
        }
    }
    Ok(())
}

fn parse_vec2_literal(value: &serde_json::Value) -> Result<Vec2, ()> {
    let arr = value.as_array().ok_or(())?;
    if arr.len() != 2 {
        return Err(());
    }
    let x = arr[0].as_f64().ok_or(())? as f32;
    let y = arr[1].as_f64().ok_or(())? as f32;
    Ok(Vec2::new(x, y))
}

fn parse_bounds_literal(value: &serde_json::Value) -> Result<BoundsLiteral, ()> {
    let obj = value.as_object().ok_or(())?;
    let g = |k: &str| -> Result<f32, ()> {
        Ok(obj.get(k).and_then(|v| v.as_f64()).ok_or(())? as f32)
    };
    Ok(BoundsLiteral {
        min_x: g("min_x")?,
        min_y: g("min_y")?,
        max_x: g("max_x")?,
        max_y: g("max_y")?,
        damping: obj.get("damping").and_then(|v| v.as_f64()).map(|v| v as f32).unwrap_or(0.9),
    })
}

/// Kahn-style topo sort over a slice of producer ids. Returns ids in
/// dependency-respecting order. Errors on a cycle that involves any
/// producer.
fn topo_sort<'a>(
    ids: &[&'a str],
    deps: &HashMap<&'a str, Vec<&'a str>>,
) -> Result<Vec<&'a str>, CompileError> {
    let id_set: HashSet<&str> = ids.iter().copied().collect();
    // For each producer, count incoming edges from *other producers*
    // only — non-producer dependencies don't gate ordering.
    let mut in_degree: BTreeMap<&str, usize> = ids.iter().map(|id| (*id, 0)).collect();
    for id in ids {
        if let Some(d) = deps.get(id) {
            for src in d {
                if id_set.contains(src) {
                    *in_degree.entry(*id).or_insert(0) += 1;
                }
            }
        }
    }
    let mut ready: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(id, _)| *id)
        .collect();
    let mut out: Vec<&str> = Vec::with_capacity(ids.len());
    while let Some(id) = ready.pop_front() {
        out.push(id);
        // Visit downstream producers (those that depend on `id`).
        for other in ids {
            let Some(d) = deps.get(other) else { continue };
            if !d.contains(&id) {
                continue;
            }
            let Some(n) = in_degree.get_mut(other) else { continue };
            if *n == 0 {
                continue;
            }
            *n -= 1;
            if *n == 0 {
                ready.push_back(*other);
            }
        }
    }
    if out.len() != ids.len() {
        // Find any node still with in_degree > 0.
        let stuck = in_degree
            .iter()
            .find(|(_, n)| **n > 0)
            .map(|(id, _)| (*id).to_string())
            .unwrap_or_else(|| "?".to_string());
        return Err(CompileError::DataCycle { node_id: stuck });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::format::BehaviorAsset;

    fn parse(json: &str) -> BehaviorAsset {
        BehaviorAsset::from_json(json).expect("parse")
    }

    #[test]
    fn compiles_player_mover_example() {
        let asset = parse(
            r#"{
                "name": "PlayerMover",
                "version": 1,
                "nodes": [
                    { "id": "n0", "op": "OnTick" },
                    { "id": "n1", "op": "GetInputAxis" },
                    { "id": "n2", "op": "SetVelocity",
                      "in": { "vel": { "from": "n1", "scale": 200.0 } } }
                ],
                "edges": [
                    { "from": "n0", "to": "n2" }
                ]
            }"#,
        );
        let g = compile(&asset).expect("compile");
        assert_eq!(g.pin_layout.vec2s, 1);
        let tick = g.tick_program();
        assert_eq!(tick.len(), 2);
        assert!(matches!(tick[0], OpInstruction::GetInputAxis { .. }));
        match &tick[1] {
            OpInstruction::SetVelocity {
                vel: Vec2Input::Pin { slot, scale, .. },
            } => {
                assert_eq!(*slot, Vec2Slot(0));
                assert!((*scale - 200.0).abs() < 1e-6);
            }
            other => panic!("expected SetVelocity{{Pin}}, got {other:?}"),
        }
        assert!(g.diagnostics.is_empty());
        assert!(g.subscriptions.contains(TriggerKind::OnTick));
        assert!(!g.subscriptions.contains(TriggerKind::OnSpawn));
    }

    #[test]
    fn compiles_chase_and_separation_graph() {
        // Exercises the three targeting ops (GetNearestByTag /
        // GetEntityPosition / GetCustomField) wired into a chaser +
        // lightweight-separation behavior. Mirrors the authored
        // `chaser.behavior` asset a demo game shipped.
        let asset = parse(
            r#"{
                "name": "Chaser",
                "version": 1,
                "nodes": [
                    { "id": "tick", "op": "OnTick" },
                    { "id": "speed", "op": "GetCustomField",
                      "params": { "component": "Enemy", "field": "speed" } },
                    { "id": "target", "op": "GetNearestByTag",
                      "params": { "tag": "player" } },
                    { "id": "tpos", "op": "GetEntityPosition",
                      "in": { "entity": { "from": "target" } } },
                    { "id": "spos", "op": "GetPosition" },
                    { "id": "toTarget", "op": "MathVec2", "params": { "op": "Sub" },
                      "in": { "a": { "from": "tpos" }, "b": { "from": "spos" } } },
                    { "id": "dir", "op": "Normalize",
                      "in": { "in": { "from": "toTarget" } } },
                    { "id": "dt", "op": "GetDeltaTime" },
                    { "id": "step", "op": "MathFloat", "params": { "op": "Mul" },
                      "in": { "a": { "from": "speed" }, "b": { "from": "dt" } } },
                    { "id": "moveVec", "op": "ScaleVec2",
                      "in": { "v": { "from": "dir" }, "s": { "from": "step" } } },
                    { "id": "moveAct", "op": "Translate",
                      "in": { "delta": { "from": "moveVec" } } },

                    { "id": "nbr", "op": "GetNearestByTag",
                      "params": { "tag": "enemy", "exclude_self": true } },
                    { "id": "npos", "op": "GetEntityPosition",
                      "in": { "entity": { "from": "nbr" } } },
                    { "id": "fromNbr", "op": "MathVec2", "params": { "op": "Sub" },
                      "in": { "a": { "from": "spos" }, "b": { "from": "npos" } } },
                    { "id": "ndist", "op": "Length",
                      "in": { "in": { "from": "fromNbr" } } },
                    { "id": "gap", "op": "MathFloat", "params": { "op": "Sub" },
                      "in": { "a": { "value": 34.0 }, "b": { "from": "ndist" } } },
                    { "id": "gapPos", "op": "MathFloat", "params": { "op": "Max" },
                      "in": { "a": { "from": "gap" }, "b": { "value": 0.0 } } },
                    { "id": "awayDir", "op": "Normalize",
                      "in": { "in": { "from": "fromNbr" } } },
                    { "id": "sepMag", "op": "MathFloat", "params": { "op": "Mul" },
                      "in": { "a": { "from": "gapPos" }, "b": { "value": 6.0 } } },
                    { "id": "sepStep", "op": "MathFloat", "params": { "op": "Mul" },
                      "in": { "a": { "from": "sepMag" }, "b": { "from": "dt" } } },
                    { "id": "sepVec", "op": "ScaleVec2",
                      "in": { "v": { "from": "awayDir" }, "s": { "from": "sepStep" } } },
                    { "id": "sepAct", "op": "Translate",
                      "in": { "delta": { "from": "sepVec" } } }
                ],
                "edges": [
                    { "from": "tick", "to": "moveAct" },
                    { "from": "tick", "to": "sepAct" }
                ]
            }"#,
        );
        let g = compile(&asset).expect("chase+separation graph compiles");
        assert!(g.diagnostics.is_empty(), "diagnostics: {:?}", g.diagnostics);
        assert!(g.subscriptions.contains(TriggerKind::OnTick));
        let tick = g.tick_program();
        let translates = tick
            .iter()
            .filter(|op| matches!(op, OpInstruction::Translate { .. }))
            .count();
        assert_eq!(translates, 2, "expected chase + separation Translate ops");
    }

    #[test]
    fn compiles_kiter_spacing_graph() {
        // Kiter archetype: maintains a desired distance from the player
        // using Compare + SelectVec2 (approach when far, retreat when
        // close), plus the shared separation subgraph. Mirrors the
        // authored `kiter.behavior` asset.
        let asset = parse(
            r#"{
                "name": "Kiter",
                "version": 1,
                "nodes": [
                    { "id": "tick", "op": "OnTick" },
                    { "id": "speed", "op": "GetCustomField",
                      "params": { "component": "Enemy", "field": "speed" } },
                    { "id": "target", "op": "GetNearestByTag",
                      "params": { "tag": "player" } },
                    { "id": "tpos", "op": "GetEntityPosition",
                      "in": { "entity": { "from": "target" } } },
                    { "id": "spos", "op": "GetPosition" },
                    { "id": "toTarget", "op": "MathVec2", "params": { "op": "Sub" },
                      "in": { "a": { "from": "tpos" }, "b": { "from": "spos" } } },
                    { "id": "dist", "op": "Length",
                      "in": { "in": { "from": "toTarget" } } },
                    { "id": "dirToward", "op": "Normalize",
                      "in": { "in": { "from": "toTarget" } } },
                    { "id": "awayVec", "op": "MathVec2", "params": { "op": "Sub" },
                      "in": { "a": { "from": "spos" }, "b": { "from": "tpos" } } },
                    { "id": "dirAway", "op": "Normalize",
                      "in": { "in": { "from": "awayVec" } } },
                    { "id": "farEnough", "op": "Compare", "params": { "op": "Gt" },
                      "in": { "a": { "from": "dist" }, "b": { "value": 220.0 } } },
                    { "id": "moveDir", "op": "SelectVec2",
                      "in": { "cond": { "from": "farEnough" },
                              "a": { "from": "dirToward" },
                              "b": { "from": "dirAway" } } },
                    { "id": "dt", "op": "GetDeltaTime" },
                    { "id": "step", "op": "MathFloat", "params": { "op": "Mul" },
                      "in": { "a": { "from": "speed" }, "b": { "from": "dt" } } },
                    { "id": "moveVec", "op": "ScaleVec2",
                      "in": { "v": { "from": "moveDir" }, "s": { "from": "step" } } },
                    { "id": "moveAct", "op": "Translate",
                      "in": { "delta": { "from": "moveVec" } } }
                ],
                "edges": [
                    { "from": "tick", "to": "moveAct" }
                ]
            }"#,
        );
        let g = compile(&asset).expect("kiter spacing graph compiles");
        assert!(g.diagnostics.is_empty(), "diagnostics: {:?}", g.diagnostics);
        assert!(g.subscriptions.contains(TriggerKind::OnTick));
    }

    #[test]
    fn rejects_unknown_op() {
        let asset = parse(
            r#"{
                "name": "Bad", "version": 1,
                "nodes": [{ "id": "x", "op": "Frobnicate" }],
                "edges": []
            }"#,
        );
        let err = compile(&asset).unwrap_err();
        assert!(matches!(err, CompileError::UnknownOp { .. }));
    }

    #[test]
    fn rejects_missing_input_binding() {
        let asset = parse(
            r#"{
                "name": "Bad", "version": 1,
                "nodes": [
                    { "id": "n0", "op": "OnTick" },
                    { "id": "n1", "op": "SetVelocity" }
                ],
                "edges": [{ "from": "n0", "to": "n1" }]
            }"#,
        );
        let err = compile(&asset).unwrap_err();
        assert!(matches!(err, CompileError::MissingInput { pin: "vel", .. }));
    }

    #[test]
    fn rejects_pin_type_mismatch_when_float_input_takes_vec2_source() {
        // GetInputAxis (Vec2) wired into DespawnAfter.seconds (Float).
        let asset = parse(
            r#"{
                "name": "Bad", "version": 1,
                "nodes": [
                    { "id": "n0", "op": "OnTick" },
                    { "id": "n1", "op": "GetInputAxis" },
                    { "id": "n2", "op": "DespawnAfter",
                      "in": { "seconds": { "from": "n1" } } }
                ],
                "edges": [{ "from": "n0", "to": "n2" }]
            }"#,
        );
        let err = compile(&asset).unwrap_err();
        assert!(matches!(
            err,
            CompileError::PinTypeMismatch {
                expected: "Float",
                actual: "Vec2",
                ..
            }
        ));
    }

    #[test]
    fn warns_on_setup_op_under_on_tick() {
        let asset = parse(
            r#"{
                "name": "Footgun", "version": 1,
                "nodes": [
                    { "id": "n0", "op": "OnTick" },
                    { "id": "n1", "op": "DespawnAfter",
                      "in": { "seconds": { "value": 2.0 } } }
                ],
                "edges": [{ "from": "n0", "to": "n1" }]
            }"#,
        );
        let g = compile(&asset).expect("compile");
        assert_eq!(g.diagnostics.len(), 1);
        assert!(matches!(
            g.diagnostics[0].kind,
            CompileWarning::SetupOpUnderOnTick { op: "DespawnAfter" }
        ));
    }

    #[test]
    fn rejects_unknown_input_pin() {
        let asset = parse(
            r#"{
                "name": "Bad", "version": 1,
                "nodes": [
                    { "id": "n0", "op": "OnTick" },
                    { "id": "n1", "op": "AddVelocity",
                      "in": { "delta": { "value": [1.0, 0.0] },
                              "extra": { "value": [0.0, 0.0] } } }
                ],
                "edges": [{ "from": "n0", "to": "n1" }]
            }"#,
        );
        let err = compile(&asset).unwrap_err();
        assert!(matches!(err, CompileError::UnknownInputPin { .. }));
    }

    // ── Slice 7.3 ── per-trigger event programs ──

    #[test]
    fn splits_programs_by_trigger_kind() {
        // OnTick + OnSpawn in the same graph: distinct programs, only
        // their reachable subgraph counts toward each.
        let asset = parse(
            r#"{
                "name": "Mixed", "version": 1,
                "nodes": [
                    { "id": "tick", "op": "OnTick" },
                    { "id": "spawn", "op": "OnSpawn" },
                    { "id": "av", "op": "AddVelocity",
                      "in": { "delta": { "value": [1.0, 0.0] } } },
                    { "id": "sv", "op": "SetVelocity",
                      "in": { "vel": { "value": [10.0, 0.0] } } }
                ],
                "edges": [
                    { "from": "tick", "to": "av" },
                    { "from": "spawn", "to": "sv" }
                ]
            }"#,
        );
        let g = compile(&asset).expect("compile");
        assert!(g.subscriptions.contains(TriggerKind::OnTick));
        assert!(g.subscriptions.contains(TriggerKind::OnSpawn));
        assert!(!g.subscriptions.contains(TriggerKind::OnDespawn));
        let tick = g.programs.get(&TriggerKind::OnTick).unwrap();
        let spawn = g.programs.get(&TriggerKind::OnSpawn).unwrap();
        assert_eq!(tick.ops.len(), 1);
        assert_eq!(spawn.ops.len(), 1);
        assert!(matches!(tick.ops[0], OpInstruction::AddVelocity { .. }));
        assert!(matches!(spawn.ops[0], OpInstruction::SetVelocity { .. }));
    }

    #[test]
    fn allocates_payload_pins_for_collision_and_trigger_and_timer() {
        let asset = parse(
            r#"{
                "name": "Payloads", "version": 1,
                "nodes": [
                    { "id": "c", "op": "OnCollision" },
                    { "id": "t", "op": "OnTrigger" },
                    { "id": "m", "op": "OnTimer" }
                ],
                "edges": []
            }"#,
        );
        let g = compile(&asset).expect("compile");
        // OnCollision: 1 entity slot + 1 bool slot.
        // OnTrigger:   1 entity slot.
        // OnTimer:     1 float slot.
        assert_eq!(g.pin_layout.entities, 2);
        assert_eq!(g.pin_layout.bools, 1);
        assert_eq!(g.pin_layout.floats, 1);
        let c = g.programs.get(&TriggerKind::OnCollision).unwrap();
        assert!(c.payload_pins.other_entity.is_some());
        assert!(c.payload_pins.enter.is_some());
        let t = g.programs.get(&TriggerKind::OnTrigger).unwrap();
        assert!(t.payload_pins.other_entity.is_some());
        assert!(t.payload_pins.enter.is_none());
        let m = g.programs.get(&TriggerKind::OnTimer).unwrap();
        assert!(m.payload_pins.tick_index.is_some());
    }

    #[test]
    fn setup_op_warning_only_fires_for_on_tick_not_on_spawn() {
        // Setup op under OnSpawn is the *idiomatic* placement \u2014 no
        // warning should fire there.
        let asset = parse(
            r#"{
                "name": "SpawnSetup", "version": 1,
                "nodes": [
                    { "id": "s", "op": "OnSpawn" },
                    { "id": "d", "op": "DespawnAfter",
                      "in": { "seconds": { "value": 2.0 } } }
                ],
                "edges": [{ "from": "s", "to": "d" }]
            }"#,
        );
        let g = compile(&asset).expect("compile");
        assert!(g.diagnostics.is_empty(), "diagnostics: {:?}", g.diagnostics);
        assert!(g.subscriptions.contains(TriggerKind::OnSpawn));
    }

    // ── Slice 7.4A ── gate / control ops ──

    #[test]
    fn compare_op_writes_bool_pin() {
        let asset = parse(
            r#"{
                "name": "Cmp", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "c", "op": "Compare",
                      "params": { "op": "Gt" },
                      "in": { "a": { "value": 1.0 }, "b": { "value": 0.0 } } },
                    { "id": "i", "op": "If", "in": { "cond": { "from": "c" } } },
                    { "id": "sv", "op": "SetVelocity",
                      "in": { "vel": { "value": [10.0, 0.0] } } }
                ],
                "edges": [
                    { "from": "t", "to": "i" },
                    { "from": "i", "out": "then", "to": "sv" }
                ]
            }"#,
        );
        let g = compile(&asset).expect("compile");
        assert_eq!(g.pin_layout.bools, 1);
        let tick = g.tick_program();
        // Producer (Compare) is prepended, then If with then_len=1, else_len=0,
        // then the SetVelocity in the then-branch.
        assert!(matches!(
            tick[0],
            OpInstruction::Compare {
                op: CompareOp::Gt,
                output: BoolSlot(0),
                ..
            }
        ));
        match tick[1] {
            OpInstruction::If {
                then_len,
                else_len,
                ..
            } => {
                assert_eq!(then_len, 1);
                assert_eq!(else_len, 0);
            }
            ref other => panic!("expected If, got {other:?}"),
        }
        assert!(matches!(tick[2], OpInstruction::SetVelocity { .. }));
        assert_eq!(tick.len(), 3);
    }

    #[test]
    fn not_op_writes_bool_pin_and_chains_with_compare() {
        let asset = parse(
            r#"{
                "name": "NotChain", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "c", "op": "Compare",
                      "params": { "op": "Lt" },
                      "in": { "a": { "value": 0.0 }, "b": { "value": 1.0 } } },
                    { "id": "n", "op": "Not", "in": { "in": { "from": "c" } } },
                    { "id": "i", "op": "If", "in": { "cond": { "from": "n" } } }
                ],
                "edges": [{ "from": "t", "to": "i" }]
            }"#,
        );
        let g = compile(&asset).expect("compile");
        assert_eq!(g.pin_layout.bools, 2); // Compare output + Not output
        let tick = g.tick_program();
        // Order: Compare, Not, If (closure pulls Compare via Not chain).
        assert!(matches!(tick[0], OpInstruction::Compare { .. }));
        assert!(matches!(tick[1], OpInstruction::Not { .. }));
        assert!(matches!(tick[2], OpInstruction::If { .. }));
    }

    #[test]
    fn if_with_then_and_else_records_block_lengths() {
        let asset = parse(
            r#"{
                "name": "IfBoth", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "i", "op": "If", "in": { "cond": { "value": true } } },
                    { "id": "a", "op": "AddVelocity",
                      "in": { "delta": { "value": [1.0, 0.0] } } },
                    { "id": "b", "op": "SetVelocity",
                      "in": { "vel": { "value": [0.0, 0.0] } } }
                ],
                "edges": [
                    { "from": "t", "to": "i" },
                    { "from": "i", "out": "then", "to": "a" },
                    { "from": "i", "out": "else", "to": "b" }
                ]
            }"#,
        );
        let g = compile(&asset).expect("compile");
        let tick = g.tick_program();
        match tick[0] {
            OpInstruction::If {
                then_len,
                else_len,
                ..
            } => {
                assert_eq!(then_len, 1);
                assert_eq!(else_len, 1);
            }
            ref other => panic!("expected If, got {other:?}"),
        }
        assert!(matches!(tick[1], OpInstruction::AddVelocity { .. }));
        assert!(matches!(tick[2], OpInstruction::SetVelocity { .. }));
        assert_eq!(tick.len(), 3);
    }

    #[test]
    fn cooldown_allocates_state_float_slot() {
        let asset = parse(
            r#"{
                "name": "CD", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "cd", "op": "Cooldown",
                      "in": { "seconds": { "value": 0.5 } } },
                    { "id": "sv", "op": "SetVelocity",
                      "in": { "vel": { "value": [10.0, 0.0] } } }
                ],
                "edges": [
                    { "from": "t", "to": "cd" },
                    { "from": "cd", "to": "sv" }
                ]
            }"#,
        );
        let g = compile(&asset).expect("compile");
        assert_eq!(g.state_layout.floats, 1);
        assert_eq!(g.state_layout.bools, 0);
        let tick = g.tick_program();
        match tick[0] {
            OpInstruction::Cooldown {
                state_slot,
                body_len,
                ..
            } => {
                assert_eq!(state_slot, StateFloatSlot(0));
                assert_eq!(body_len, 1);
            }
            ref other => panic!("expected Cooldown, got {other:?}"),
        }
    }

    #[test]
    fn once_allocates_state_bool_slot() {
        let asset = parse(
            r#"{
                "name": "Once1", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "o", "op": "Once" },
                    { "id": "sv", "op": "SetVelocity",
                      "in": { "vel": { "value": [10.0, 0.0] } } }
                ],
                "edges": [
                    { "from": "t", "to": "o" },
                    { "from": "o", "to": "sv" }
                ]
            }"#,
        );
        let g = compile(&asset).expect("compile");
        assert_eq!(g.state_layout.bools, 1);
        assert_eq!(g.state_layout.floats, 0);
        let tick = g.tick_program();
        match tick[0] {
            OpInstruction::Once {
                state_slot,
                body_len,
            } => {
                assert_eq!(state_slot, StateBoolSlot(0));
                assert_eq!(body_len, 1);
            }
            ref other => panic!("expected Once, got {other:?}"),
        }
    }

    #[test]
    fn then_else_port_on_non_if_op_is_rejected() {
        let asset = parse(
            r#"{
                "name": "Bad", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "a", "op": "AddVelocity",
                      "in": { "delta": { "value": [1.0, 0.0] } } },
                    { "id": "b", "op": "SetVelocity",
                      "in": { "vel": { "value": [0.0, 0.0] } } }
                ],
                "edges": [
                    { "from": "t", "to": "a" },
                    { "from": "a", "out": "then", "to": "b" }
                ]
            }"#,
        );
        let err = compile(&asset).unwrap_err();
        assert!(matches!(err, CompileError::InvalidControlPort { .. }));
    }

    // \u2500\u2500 Slice 7.4B \u2500\u2500 motion ops (Translate / ApplyForce / GetPosition / GetVelocity) \u2500\u2500

    #[test]
    fn translate_compiles_with_literal_delta() {
        let asset = parse(
            r#"{
                "name": "Trans", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "tr", "op": "Translate",
                      "in": { "delta": { "value": [1.0, 0.0] } } }
                ],
                "edges": [{ "from": "t", "to": "tr" }]
            }"#,
        );
        let g = compile(&asset).expect("compile");
        let tick = g.tick_program();
        assert_eq!(tick.len(), 1);
        assert!(matches!(tick[0], OpInstruction::Translate { .. }));
        assert!(g.diagnostics.is_empty());
    }

    #[test]
    fn apply_force_compiles_and_rejects_unknown_input_pin() {
        let ok = parse(
            r#"{
                "name": "AF", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "f", "op": "ApplyForce",
                      "in": { "force": { "value": [10.0, -2.0] } } }
                ],
                "edges": [{ "from": "t", "to": "f" }]
            }"#,
        );
        let g = compile(&ok).expect("compile");
        assert!(matches!(g.tick_program()[0], OpInstruction::ApplyForce { .. }));

        let bad = parse(
            r#"{
                "name": "AFBad", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "f", "op": "ApplyForce",
                      "in": { "delta": { "value": [10.0, -2.0] } } }
                ],
                "edges": [{ "from": "t", "to": "f" }]
            }"#,
        );
        let err = compile(&bad).unwrap_err();
        assert!(matches!(err, CompileError::MissingInput { pin: "force", .. }));
    }

    #[test]
    fn get_position_and_get_velocity_allocate_vec2_pins() {
        let asset = parse(
            r#"{
                "name": "Reads", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "gp", "op": "GetPosition" },
                    { "id": "gv", "op": "GetVelocity" },
                    { "id": "tr", "op": "Translate",
                      "in": { "delta": { "from": "gv" } } },
                    { "id": "sv", "op": "SetVelocity",
                      "in": { "vel": { "from": "gp" } } }
                ],
                "edges": [
                    { "from": "t", "to": "tr" },
                    { "from": "t", "to": "sv" }
                ]
            }"#,
        );
        let g = compile(&asset).expect("compile");
        // Two Vec2 producers \u2192 2 vec2 pins.
        assert_eq!(g.pin_layout.vec2s, 2);
        let tick = g.tick_program();
        // Producers (in topo order: GetPosition, GetVelocity), then actions.
        assert!(matches!(tick[0], OpInstruction::GetPosition { .. }));
        assert!(matches!(tick[1], OpInstruction::GetVelocity { .. }));
        assert!(matches!(tick[2], OpInstruction::Translate { .. }));
        assert!(matches!(tick[3], OpInstruction::SetVelocity { .. }));
    }

    #[test]
    fn translate_rejects_missing_delta() {
        let asset = parse(
            r#"{
                "name": "Bad", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "tr", "op": "Translate" }
                ],
                "edges": [{ "from": "t", "to": "tr" }]
            }"#,
        );
        let err = compile(&asset).unwrap_err();
        assert!(matches!(err, CompileError::MissingInput { pin: "delta", .. }));
    }

    // ── Slice 7.4C ── structural commands (SpawnEntity / Despawn / PlaySound) ──

    #[test]
    fn spawn_entity_compiles_with_required_params() {
        let asset = parse(
            r#"{
                "name": "Spawner", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "s", "op": "SpawnEntity",
                      "params": { "prefab_id": "bullet" },
                      "in": {
                          "at": { "value": [10.0, 20.0] },
                          "with_velocity": { "value": [100.0, 0.0] }
                      } }
                ],
                "edges": [{ "from": "t", "to": "s" }]
            }"#,
        );
        let g = compile(&asset).expect("compile");
        let tick = g.tick_program();
        assert_eq!(tick.len(), 1);
        match &tick[0] {
            OpInstruction::SpawnEntity { prefab_id, with_velocity, .. } => {
                assert_eq!(prefab_id, "bullet");
                assert!(with_velocity.is_some());
            }
            other => panic!("expected SpawnEntity, got {other:?}"),
        }
    }

    #[test]
    fn spawn_entity_rejects_missing_prefab_id() {
        let asset = parse(
            r#"{
                "name": "Bad", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "s", "op": "SpawnEntity",
                      "in": { "at": { "value": [0.0, 0.0] } } }
                ],
                "edges": [{ "from": "t", "to": "s" }]
            }"#,
        );
        let err = compile(&asset).unwrap_err();
        assert!(matches!(err, CompileError::MissingInput { pin: "prefab_id", .. }));
    }

    #[test]
    fn despawn_compiles_with_no_inputs() {
        let asset = parse(
            r#"{
                "name": "Killer", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "d", "op": "Despawn" }
                ],
                "edges": [{ "from": "t", "to": "d" }]
            }"#,
        );
        let g = compile(&asset).expect("compile");
        assert!(matches!(g.tick_program()[0], OpInstruction::Despawn));
    }

    #[test]
    fn play_sound_uses_default_volume_when_unspecified() {
        let asset = parse(
            r#"{
                "name": "Boom", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "p", "op": "PlaySound",
                      "params": { "sound_id": "boom.wav" } }
                ],
                "edges": [{ "from": "t", "to": "p" }]
            }"#,
        );
        let g = compile(&asset).expect("compile");
        match &g.tick_program()[0] {
            OpInstruction::PlaySound { sound_id, volume } => {
                assert_eq!(sound_id, "boom.wav");
                assert!(matches!(volume, FloatInput::Literal(v) if (*v - 1.0).abs() < 1e-6));
            }
            other => panic!("expected PlaySound, got {other:?}"),
        }
    }

    #[test]
    fn play_sound_rejects_missing_sound_id() {
        let asset = parse(
            r#"{
                "name": "Bad", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "p", "op": "PlaySound" }
                ],
                "edges": [{ "from": "t", "to": "p" }]
            }"#,
        );
        let err = compile(&asset).unwrap_err();
        assert!(matches!(err, CompileError::MissingInput { pin: "sound_id", .. }));
    }

    // ── Slice 7.4D ── higher-level gameplay ops (LookAt / MoveToward) ──

    #[test]
    fn look_at_compiles_with_literal_target_pos() {
        let asset = parse(
            r#"{
                "name": "Look", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "l", "op": "LookAt",
                      "in": { "target_pos": { "value": [10.0, 5.0] } } }
                ],
                "edges": [{ "from": "t", "to": "l" }]
            }"#,
        );
        let g = compile(&asset).expect("compile");
        assert!(matches!(g.tick_program()[0], OpInstruction::LookAt { .. }));
    }

    #[test]
    fn look_at_rejects_missing_target_pos() {
        let asset = parse(
            r#"{
                "name": "Bad", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "l", "op": "LookAt" }
                ],
                "edges": [{ "from": "t", "to": "l" }]
            }"#,
        );
        let err = compile(&asset).unwrap_err();
        assert!(matches!(err, CompileError::MissingInput { pin: "target_pos", .. }));
    }

    #[test]
    fn move_toward_compiles_with_target_and_speed() {
        let asset = parse(
            r#"{
                "name": "Move", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "m", "op": "MoveToward",
                      "in": {
                          "target_pos": { "value": [100.0, 0.0] },
                          "speed": { "value": 50.0 }
                      } }
                ],
                "edges": [{ "from": "t", "to": "m" }]
            }"#,
        );
        let g = compile(&asset).expect("compile");
        assert!(matches!(g.tick_program()[0], OpInstruction::MoveToward { .. }));
    }

    #[test]
    fn move_toward_rejects_missing_speed() {
        let asset = parse(
            r#"{
                "name": "Bad", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTick" },
                    { "id": "m", "op": "MoveToward",
                      "in": { "target_pos": { "value": [0.0, 0.0] } } }
                ],
                "edges": [{ "from": "t", "to": "m" }]
            }"#,
        );
        let err = compile(&asset).unwrap_err();
        assert!(matches!(err, CompileError::MissingInput { pin: "speed", .. }));
    }
}
