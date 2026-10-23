//! Typed pin storage for the behavior interpreter.
//!
//! Slice 7.2 introduced `Float` and `Vec2` registers (consumed by
//! `GetInputAxis`, `SetVelocity`, etc.). Slice 7.3 adds `Bool` and
//! `Entity` registers used by trigger payload pins
//! (`OnCollision { other: Entity, enter: Bool }` and friends). These
//! slots are *write-only* in 7.3 — no MVP op reads from them yet —
//! but allocating them now means later ops (Slice 7.4A `Compare`,
//! 7.4D entity-target `LookAt`, etc.) can plug in without churning
//! the layout struct.
//!
//! See [`docs/step7-behavior-graph-spec.md`](../../../../docs/step7-behavior-graph-spec.md) §4.3.

use glam::Vec2;

/// Per-graph register counts. Computed at compile time and stored on
/// the `CompiledGraph`. The owning system uses this to size a single
/// reusable `PinFrame` shared across all instances.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PinLayout {
    pub floats: u16,
    pub vec2s: u16,
    pub bools: u16,
    pub entities: u16,
}

/// Strongly-typed pin slot indices. Distinct types prevent crossing a
/// `Float` source into a `Vec2` sink at op build time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatSlot(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vec2Slot(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoolSlot(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntitySlot(pub u16);

/// Transient per-tick scratch. Owned by the system, sized once to the
/// max layout in the cache, cleared between graphs.
#[derive(Debug, Clone, Default)]
pub struct PinFrame {
    floats: Vec<f32>,
    vec2s: Vec<Vec2>,
    bools: Vec<bool>,
    entities: Vec<Option<hecs::Entity>>,
}

impl PinFrame {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resize the frame so all slots in `layout` are addressable, and
    /// reset register contents to zero. Cheap when capacity is already
    /// sufficient.
    pub fn reset(&mut self, layout: &PinLayout) {
        self.floats.clear();
        self.floats.resize(layout.floats as usize, 0.0);
        self.vec2s.clear();
        self.vec2s.resize(layout.vec2s as usize, Vec2::ZERO);
        self.bools.clear();
        self.bools.resize(layout.bools as usize, false);
        self.entities.clear();
        self.entities.resize(layout.entities as usize, None);
    }

    pub fn write_float(&mut self, slot: FloatSlot, value: f32) {
        self.floats[slot.0 as usize] = value;
    }

    pub fn write_vec2(&mut self, slot: Vec2Slot, value: Vec2) {
        self.vec2s[slot.0 as usize] = value;
    }

    pub fn write_bool(&mut self, slot: BoolSlot, value: bool) {
        self.bools[slot.0 as usize] = value;
    }

    pub fn write_entity(&mut self, slot: EntitySlot, value: hecs::Entity) {
        self.entities[slot.0 as usize] = Some(value);
    }

    pub fn read_float(&self, slot: FloatSlot) -> f32 {
        self.floats[slot.0 as usize]
    }

    pub fn read_vec2(&self, slot: Vec2Slot) -> Vec2 {
        self.vec2s[slot.0 as usize]
    }

    pub fn read_bool(&self, slot: BoolSlot) -> bool {
        self.bools[slot.0 as usize]
    }

    pub fn read_entity(&self, slot: EntitySlot) -> Option<hecs::Entity> {
        self.entities[slot.0 as usize]
    }
}
