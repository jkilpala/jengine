//! System schedule — phased execution of update, physics, render, and other systems.
//!
//! Replaces the manually-ordered, hardcoded system calls in the game loop with
//! a registry of systems organized into phases with priority ordering.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;

// ──────────────────────────── Resource Map ────────────────────────────

/// Type-erased resource container keyed by `TypeId`.
///
/// Used to pass engine subsystem state (physics, audio, etc.) to systems
/// through the `SystemContext`. Systems access resources via typed getters.
pub struct ResourceMap {
    map: HashMap<TypeId, Box<dyn Any>>,
}

impl ResourceMap {
    /// Create an empty resource map.
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }

    /// Insert a resource, replacing any existing value of the same type.
    pub fn insert<T: 'static>(&mut self, val: T) {
        self.map.insert(TypeId::of::<T>(), Box::new(val));
    }

    /// Get an immutable reference to a resource by type.
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.map.get(&TypeId::of::<T>())?.downcast_ref()
    }

    /// Get a mutable reference to a resource by type.
    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.map.get_mut(&TypeId::of::<T>())?.downcast_mut()
    }

    /// Remove a resource by type, returning ownership.
    pub fn remove<T: 'static>(&mut self) -> Option<T> {
        let boxed = self.map.remove(&TypeId::of::<T>())?;
        boxed.downcast().ok().map(|b| *b)
    }

    /// Check if a resource of the given type exists.
    pub fn contains<T: 'static>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<T>())
    }
}

impl Default for ResourceMap {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────── System Phases ────────────────────────────

/// Execution phases within a single frame, run in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SystemPhase {
    /// Early update — input processing, GUI focus, pre-script work.
    PreUpdate = 0,
    /// Script execution — call init() / update() on all scripted entities.
    ScriptExec = 1,
    /// Physics — sync kinematic bodies, step simulation, sync transforms back.
    Physics = 2,
    /// Post-physics — reactions to physics results (contact events, etc.).
    PostPhysics = 3,
    /// Animation — sprite animation ticks, tweens.
    Animation = 4,
    /// Pre-render — camera update, culling, batch preparation.
    PreRender = 5,
    /// Render — draw tilemaps, sprites, particles, UI, GUI, debug overlays.
    Render = 6,
    /// Post-render — swap buffers, profiler stats, cleanup.
    PostRender = 7,
}

impl fmt::Display for SystemPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

// ──────────────────────────── System Context ────────────────────────────

/// Resources passed to every system function during execution.
///
/// Bundles all mutable and immutable state a system might need.
/// Systems access only what they require; unused fields are ignored.
///
/// Use `split()` when you need simultaneous mutable access to both
/// `world` and `resources` (Rust's borrow checker requires this).
pub struct SystemContext<'a> {
    /// The ECS world containing all entities and components.
    pub world: &'a mut hecs::World,
    /// Delta time for this frame (seconds).
    pub dt: f32,
    /// Typed resource map — engine subsystems and packages store state here.
    pub resources: &'a mut ResourceMap,
}

impl<'a> SystemContext<'a> {
    /// Split into independent borrows for world and resources.
    ///
    /// This allows systems to borrow both `world` and `resources` mutably
    /// at the same time, which the borrow checker forbids through `&mut self`.
    pub fn split(&mut self) -> (&mut hecs::World, f32, &mut ResourceMap) {
        (&mut *self.world, self.dt, &mut *self.resources)
    }
}

// ──────────────────────────── System Function Trait ────────────────────────────

/// Trait for a system function that can be registered with the schedule.
pub trait SystemFn: Send + Sync {
    /// Execute this system.
    fn run(&self, ctx: &mut SystemContext<'_>);

    /// Human-readable name for logging and debugging.
    fn name(&self) -> &str;
}

/// Wrapper that turns a plain function into a `SystemFn`.
pub struct FnSystem<F: Fn(&mut SystemContext<'_>) + Send + Sync> {
    name: String,
    func: F,
}

impl<F: Fn(&mut SystemContext<'_>) + Send + Sync> SystemFn for FnSystem<F> {
    fn run(&self, ctx: &mut SystemContext<'_>) {
        (self.func)(ctx);
    }

    fn name(&self) -> &str {
        &self.name
    }
}

// ──────────────────────────── Registered System Entry ────────────────────────────

/// A system registration: the function, its phase, and priority within that phase.
struct SystemEntry {
    system: Box<dyn SystemFn>,
    phase: SystemPhase,
    /// Lower priority values run first within a phase.
    priority: i32,
}

// ──────────────────────────── System Schedule ────────────────────────────

/// Holds all registered systems, organized by phase and priority.
///
/// Created during engine initialization, populated by core + packages,
/// then executed each frame by the game loop.
pub struct SystemSchedule {
    entries: Vec<SystemEntry>,
    /// Set to `true` after the first `run_all` — triggers a sort.
    dirty: bool,
}

impl SystemSchedule {
    /// Create an empty schedule.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            dirty: true,
        }
    }

    /// Register a system function in the given phase with the given priority.
    ///
    /// Lower `priority` values run first within a phase.
    /// Use 0 for default priority, negative for "before default", positive for "after default".
    pub fn add_system<F>(
        &mut self,
        name: &str,
        phase: SystemPhase,
        priority: i32,
        func: F,
    ) where
        F: Fn(&mut SystemContext<'_>) + Send + Sync + 'static,
    {
        log::trace!("System registered: {} (phase={}, priority={})", name, phase, priority);
        self.entries.push(SystemEntry {
            system: Box::new(FnSystem {
                name: name.to_string(),
                func,
            }),
            phase,
            priority,
        });
        self.dirty = true;
    }

    /// Sort entries by phase then priority if needed.
    fn ensure_sorted(&mut self) {
        if self.dirty {
            self.entries.sort_by(|a, b| {
                a.phase.cmp(&b.phase).then(a.priority.cmp(&b.priority))
            });
            self.dirty = false;
        }
    }

    /// Run all systems in a specific phase.
    pub fn run_phase(&mut self, phase: SystemPhase, ctx: &mut SystemContext<'_>) {
        self.ensure_sorted();
        for entry in &self.entries {
            if entry.phase == phase {
                entry.system.run(ctx);
            }
        }
    }

    /// Run all systems across all phases in order.
    pub fn run_all(&mut self, ctx: &mut SystemContext<'_>) {
        self.ensure_sorted();
        for entry in &self.entries {
            entry.system.run(ctx);
        }
    }

    /// Number of registered systems.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the schedule is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// List all systems with their phase and priority (for debugging).
    pub fn list(&self) -> Vec<(&str, SystemPhase, i32)> {
        // Return in sorted order
        let mut list: Vec<_> = self
            .entries
            .iter()
            .map(|e| (e.system.name(), e.phase, e.priority))
            .collect();
        list.sort_by(|a, b| a.1.cmp(&b.1).then(a.2.cmp(&b.2)));
        list
    }
}

impl Default for SystemSchedule {
    fn default() -> Self {
        Self::new()
    }
}
