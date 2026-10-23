//! Trigger kinds + payload + subscription mask.
//!
//! Step 7 graphs are *event-driven*: every executable program inside a
//! `CompiledGraph` is rooted at exactly one trigger node. The compiler
//! groups ops by trigger kind so the runtime can dispatch one program
//! at a time without scanning unrelated ops.
//!
//! Slice 7.3 introduces:
//! - [`TriggerKind`] — closed enum mirroring the script-side
//!   `EngineEvent` (see [`crate::scripting::bridge::metadata::EngineEvent`]).
//! - [`EventMask`] — compact bitset stored on `CompiledGraph` so the
//!   system can answer *"does this graph care about kind X"* in O(1).
//! - [`EventPayload`] — typed payload structs the dispatcher writes
//!   into the graph's payload pins right before running the program.
//!
//! See [`docs/step7-behavior-graph-spec.md`](../../../../docs/step7-behavior-graph-spec.md) §3.4 / §7.3.

/// Closed set of triggers a behavior node may subscribe to. The
/// discriminant doubles as a bit position in [`EventMask`], so DO NOT
/// reorder existing variants — append new ones at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TriggerKind {
    /// Fires once per frame at Phase 2 (after `update_input_actions`).
    OnTick = 0,
    /// Fires exactly once when the graph instance is first attached
    /// to an entity (analogous to `EngineEvent::Spawn` for scripts).
    OnSpawn = 1,
    /// Fires exactly once just before the instance is detached from
    /// its entity. The world may or may not still contain the entity.
    OnDespawn = 2,
    /// Fires when the entity's collider starts (`enter = true`) or
    /// stops (`enter = false`) touching another collider.
    OnCollision = 3,
    /// Fires when the entity overlaps a `TriggerArea2D`.
    OnTrigger = 4,
    /// Fires when a timer tick (driven by host scheduling) elapses.
    OnTimer = 5,
    /// Fires when a keyboard key transitions to / from / stays pressed.
    /// The exact edge (`Pressed` / `Released` / `Held`) and the key
    /// itself are stored on the [`crate::behavior::compile::EventProgram::key_filter`]
    /// the compiler attaches to the program. The dispatch driver
    /// (`BehaviorGraphSystem::dispatch_input_keys`) checks both
    /// against the per-frame `InputState` and only fires matching
    /// programs — graphs without an `OnKey` node pay zero cost.
    OnKey = 6,
    /// Fires when a host-emitted named event arrives. The event
    /// `name` filter is baked onto [`crate::behavior::compile::EventProgram::name_filter`]
    /// at compile time; the dispatcher only fires programs whose
    /// filter matches the incoming name. Programs receive a single
    /// `f32` payload value (defaults to `0.0` if the host omits one).
    OnEvent = 7,
    /// Fires when a mouse button transitions to pressed this frame.
    /// The expected button (`left` / `right` / `any`) is baked onto
    /// [`crate::behavior::compile::EventProgram::mouse_filter`]; the
    /// dispatcher only fires for matching buttons. Programs receive
    /// the click position (Vec2) and which button (Float, 0=left, 1=right).
    OnMouseClick = 8,
}

impl TriggerKind {
    /// All trigger kinds in declaration order. Drives the BTreeMap
    /// ordering in `CompiledGraph::programs`.
    pub const ALL: [TriggerKind; 9] = [
        TriggerKind::OnTick,
        TriggerKind::OnSpawn,
        TriggerKind::OnDespawn,
        TriggerKind::OnCollision,
        TriggerKind::OnTrigger,
        TriggerKind::OnTimer,
        TriggerKind::OnKey,
        TriggerKind::OnEvent,
        TriggerKind::OnMouseClick,
    ];

    /// Op-name as it appears in `.behavior` JSON.
    pub fn op_name(self) -> &'static str {
        match self {
            TriggerKind::OnTick => "OnTick",
            TriggerKind::OnSpawn => "OnSpawn",
            TriggerKind::OnDespawn => "OnDespawn",
            TriggerKind::OnCollision => "OnCollision",
            TriggerKind::OnTrigger => "OnTrigger",
            TriggerKind::OnTimer => "OnTimer",
            TriggerKind::OnKey => "OnKey",
            TriggerKind::OnEvent => "OnEvent",
            TriggerKind::OnMouseClick => "OnMouseClick",
        }
    }

    /// Inverse of [`Self::op_name`].
    pub fn from_op_name(name: &str) -> Option<Self> {
        Some(match name {
            "OnTick" => TriggerKind::OnTick,
            "OnSpawn" => TriggerKind::OnSpawn,
            "OnDespawn" => TriggerKind::OnDespawn,
            "OnCollision" => TriggerKind::OnCollision,
            "OnTrigger" => TriggerKind::OnTrigger,
            "OnTimer" => TriggerKind::OnTimer,
            "OnKey" => TriggerKind::OnKey,
            "OnEvent" => TriggerKind::OnEvent,
            "OnMouseClick" => TriggerKind::OnMouseClick,
            _ => return None,
        })
    }

    fn bit(self) -> u8 {
        self as u8
    }
}

/// Bitset of [`TriggerKind`]s a graph subscribes to. Stored on
/// `CompiledGraph` so the system can prune dispatch per kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EventMask(pub u16);

impl EventMask {
    pub const fn empty() -> Self {
        EventMask(0)
    }

    pub fn insert(&mut self, kind: TriggerKind) {
        self.0 |= 1 << kind.bit();
    }

    pub fn contains(self, kind: TriggerKind) -> bool {
        (self.0 & (1 << kind.bit())) != 0
    }

    pub fn iter(self) -> impl Iterator<Item = TriggerKind> {
        TriggerKind::ALL
            .into_iter()
            .filter(move |k| self.contains(*k))
    }
}

/// Typed payload delivered to a non-tick trigger. Each variant maps
/// 1:1 onto a `TriggerKind`. The interpreter writes payload fields
/// into the graph's payload pin slots (allocated at compile time) and
/// then runs the program.
#[derive(Debug, Clone, Copy)]
pub enum EventPayload {
    /// `OnTick` carries `dt` only. The interpreter does not write
    /// anything pin-side for tick (no payload pins are allocated).
    Tick { dt: f32 },
    /// `OnSpawn` / `OnDespawn` have no extra payload — `self` is
    /// implicit (it's the instance entity).
    Lifecycle,
    /// `OnCollision { other, enter }`.
    Collision { other: hecs::Entity, enter: bool },
    /// `OnTrigger { other }` — `area` overload lands when
    /// `TriggerShape` payload pins gain a representation (Slice 7.4D).
    TriggerOverlap { other: hecs::Entity },
    /// `OnTimer { tick_index }` — monotonic tick counter from the host.
    Timer { tick_index: f32 },
    /// `OnKey { key, edge }` — a keyboard key transitioned to the
    /// matching edge this frame, or (for `Held`) is currently down.
    /// The compiler bakes the *expected* `(key, edge)` filter into
    /// the program; the dispatcher only constructs this payload for
    /// matching presses, so handlers can treat both fields as a
    /// confirmation rather than a discriminator.
    Key { key: crate::input::KeyCode, edge: KeyEdge },
    /// `OnEvent { value }` — the dispatcher pre-filters by event
    /// name (baked into the program), so handlers can treat `value`
    /// as the only payload field they need to read.
    Event { value: f32 },
    /// `OnMouseClick { pos, button }` — click world/screen position
    /// and which mouse button (0 = left, 1 = right). Filtered by
    /// the program's `mouse_filter`.
    MouseClick { pos: glam::Vec2, button: u8 },
}

/// Which keyboard transition an [`TriggerKind::OnKey`] node fires on.
///
/// `Pressed` and `Released` are edge-triggered (one event per
/// transition), driven by [`crate::input::InputState::just_pressed_iter`]
/// and [`crate::input::InputState::just_released_iter`]. `Held` is
/// level-triggered (one event per frame the key is down), driven by
/// [`crate::input::InputState::pressed_iter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyEdge {
    Pressed,
    Released,
    Held,
}

impl KeyEdge {
    pub fn as_str(self) -> &'static str {
        match self {
            KeyEdge::Pressed => "pressed",
            KeyEdge::Released => "released",
            KeyEdge::Held => "held",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pressed" => KeyEdge::Pressed,
            "released" => KeyEdge::Released,
            "held" => KeyEdge::Held,
            _ => return None,
        })
    }
}

impl EventPayload {
    pub fn kind(&self) -> TriggerKind {
        match self {
            EventPayload::Tick { .. } => TriggerKind::OnTick,
            EventPayload::Lifecycle => {
                // Caller disambiguates — see `dispatch_lifecycle`.
                TriggerKind::OnSpawn
            }
            EventPayload::Collision { .. } => TriggerKind::OnCollision,
            EventPayload::TriggerOverlap { .. } => TriggerKind::OnTrigger,
            EventPayload::Timer { .. } => TriggerKind::OnTimer,
            EventPayload::Key { .. } => TriggerKind::OnKey,
            EventPayload::Event { .. } => TriggerKind::OnEvent,
            EventPayload::MouseClick { .. } => TriggerKind::OnMouseClick,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_mask_round_trip() {
        let mut m = EventMask::empty();
        m.insert(TriggerKind::OnTick);
        m.insert(TriggerKind::OnCollision);
        assert!(m.contains(TriggerKind::OnTick));
        assert!(m.contains(TriggerKind::OnCollision));
        assert!(!m.contains(TriggerKind::OnSpawn));
        let collected: Vec<_> = m.iter().collect();
        assert_eq!(collected, vec![TriggerKind::OnTick, TriggerKind::OnCollision]);
    }

    #[test]
    fn op_name_round_trip() {
        for k in TriggerKind::ALL {
            assert_eq!(TriggerKind::from_op_name(k.op_name()), Some(k));
        }
        assert_eq!(TriggerKind::from_op_name("Frobnicate"), None);
    }
}
