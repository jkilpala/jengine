//! Package host — the per-runtime object that owns registered packages and
//! runs their systems.
//!
//! # Why this exists
//!
//! [`PackageRegistry`], [`SystemSchedule`], [`ResourceMap`] and the component /
//! script-API registries were all complete, and **nothing owned them**. The
//! `AppState` that used to has not existed since the glow renderer was cut, so
//! `register_systems` was called by no runtime: a package could declare a
//! system and it would never run. Physics, audio and particles appeared to
//! work only because each runtime calls them directly, hardcoded; parallax has
//! no such path and was therefore inert.
//!
//! `PackageHost` is that owner. A runtime creates one, adds its packages,
//! initializes once, then calls [`PackageHost::run_phase`] at the points in
//! its frame that correspond to each [`SystemPhase`].
//!
//! # Resources are how a system reaches engine state
//!
//! A system receives `&mut hecs::World`, `dt`, and the [`ResourceMap`]. Engine
//! subsystems a runtime chooses to share are inserted into that map; a system
//! whose resource is absent is expected to do nothing rather than panic — the
//! built-in physics and audio packages are already written that way, which is
//! what makes it safe for a runtime to register them while still driving
//! physics on its own hardcoded path.
//!
//! # Platform note
//!
//! Packages are Rust crates linked at compile time — there is no dynamic
//! loading. A package therefore runs wherever it was linked in: the native
//! runtime and a web build (both compiled per project), and the editor
//! viewport only when the viewport WASM is rebuilt with it.

use crate::package::component_registry::ComponentRegistry;
use crate::package::editor_registry::EditorRegistry;
use crate::package::script_api::ScriptApiRegistry;
use crate::package::script_commands::PackageCommandRegistry;
use crate::package::system_schedule::{
    ResourceMap, SystemContext, SystemPhase, SystemSchedule,
};
use crate::package::{JEnginePackage, PackageMetadata, PackageRegistry};

/// Owns every registered package plus the schedule, resources and API
/// registries they populate.
///
/// One per runtime. See the module docs for why it exists.
pub struct PackageHost {
    registry: PackageRegistry,
    schedule: SystemSchedule,
    resources: ResourceMap,
    script_api: ScriptApiRegistry,
    /// Script globals that work in every runtime, unlike `script_api`.
    script_commands: PackageCommandRegistry,
    editor: EditorRegistry,
    /// Guards against a second `initialize`, which would register every
    /// system twice and run it twice per frame.
    initialized: bool,
    /// What the host's `AssetManager` is swapped with while a phase lends it
    /// to the packages (see [`Self::run_phase_with`]).
    spare_assets: Option<crate::asset::AssetManager>,
}

impl PackageHost {
    /// Create an empty host. Add packages, then [`Self::initialize`].
    pub fn new() -> Self {
        Self {
            registry: PackageRegistry::new(),
            schedule: SystemSchedule::new(),
            resources: ResourceMap::new(),
            script_api: ScriptApiRegistry::new(),
            script_commands: PackageCommandRegistry::new(),
            editor: EditorRegistry::new(),
            initialized: false,
            spare_assets: None,
        }
    }

    /// Add a package. Must happen before [`Self::initialize`].
    ///
    /// Adding afterwards is a programming error rather than a recoverable
    /// one — the package's components would be missing from the registry the
    /// scene was already loaded against — so it logs loudly and ignores the
    /// package instead of registering it into an inconsistent world.
    /// Added twice, a package registers its systems twice and they run twice
    /// per frame — a silent double-step. That is easy to hit now that a
    /// project can declare a package the runtime also builds in, so a
    /// duplicate name is dropped with a warning rather than trusted.
    pub fn add_package(&mut self, package: Box<dyn JEnginePackage>) {
        let name = package.metadata().name;
        if self.initialized {
            log::error!(
                "package '{name}' added after PackageHost::initialize — ignored. \
                 Add every package before initializing."
            );
            return;
        }
        if self.registry.list().iter().any(|m| m.name == name) {
            log::warn!(
                "package '{name}' is already registered — ignoring the duplicate. \
                 A project usually hits this by declaring a package the runtime \
                 already builds in."
            );
            return;
        }
        self.registry.add(package);
    }

    /// Register every added package's components, systems, script API and
    /// editor extensions.
    ///
    /// Takes the runtime's `ComponentRegistry` because scene loading uses it:
    /// a package component has to be in the same registry the loader consults,
    /// not a private copy.
    pub fn initialize(&mut self, components: &mut ComponentRegistry) {
        if self.initialized {
            log::warn!("PackageHost::initialize called twice — ignoring the second call");
            return;
        }
        self.registry.initialize_all(
            components,
            &mut self.schedule,
            &mut self.script_api,
            &mut self.editor,
            &mut self.script_commands,
        );
        self.initialized = true;
        log::info!(
            "package host ready: {} package(s), {} system(s), {} script command(s)",
            self.registry.len(),
            self.schedule.len(),
            self.script_commands.len()
        );
    }

    /// Script commands contributed by packages.
    ///
    /// A host drains its command queue through
    /// [`handle_package_command`](crate::package::script_commands::handle_package_command)
    /// with this, and builds its script globals from
    /// [`PackageCommandRegistry::names`].
    pub fn script_commands(&self) -> &PackageCommandRegistry {
        &self.script_commands
    }

    /// Take ownership of the script-command registry.
    ///
    /// A host dispatches commands while holding `&mut self` as the
    /// `ScriptWorldHost`, so it cannot also borrow a registry this host owns.
    /// Moving it out after `initialize` sidesteps that entirely — nothing
    /// here reads it again.
    pub fn take_script_commands(&mut self) -> PackageCommandRegistry {
        std::mem::take(&mut self.script_commands)
    }

    /// Insert a resource for systems to reach — physics world, audio backend,
    /// whatever the runtime chooses to share.
    ///
    /// A runtime that drives a subsystem on its own hardcoded path should
    /// simply not share it: the built-in packages no-op when their resource is
    /// absent, so this is the switch that decides who owns the work.
    pub fn insert_resource<T: 'static>(&mut self, value: T) {
        self.resources.insert(value);
    }

    /// Read a resource back — how a runtime collects what a system produced.
    pub fn resource<T: 'static>(&self) -> Option<&T> {
        self.resources.get::<T>()
    }

    /// Mutable access to a resource.
    pub fn resource_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.resources.get_mut::<T>()
    }

    /// Take a resource back out of the map.
    pub fn take_resource<T: 'static>(&mut self) -> Option<T> {
        self.resources.remove::<T>()
    }

    /// Run every system registered in `phase`, in priority order.
    ///
    /// Cheap when nothing is registered for the phase, so a runtime can call
    /// it at every phase boundary without checking first.
    ///
    /// A runtime hosting a game calls [`Self::run_phase_with`] instead, so
    /// packages get the frame's input and camera; this bare form is for tests
    /// and tools that have neither.
    pub fn run_phase(&mut self, phase: SystemPhase, world: &mut hecs::World, dt: f32) {
        if self.schedule.is_empty() {
            return;
        }
        let mut ctx = SystemContext {
            world,
            dt,
            resources: &mut self.resources,
        };
        self.schedule.run_phase(phase, &mut ctx);
    }

    /// [`Self::run_phase`] with the frame lent to the packages: the host's
    /// [`InputState`] and camera are in the resource map for the length of
    /// the phase, and the input is handed back afterwards.
    ///
    /// Until Sep 2026 a package got a world and a delta time and nothing
    /// else, so it could not read a key: a platformer package had to be fed
    /// the player's input by a script command every frame. The input is
    /// **moved** in and out rather than copied - `InputState` owns maps that
    /// would allocate to clone - which is why a package must read it and
    /// never `remove` it; a package that takes it loses the frame's input
    /// for everything after, and is told so in the log.
    ///
    /// Both hosts that run packages go through here, and
    /// `packages_get_the_frame` is the test that says neither calls the
    /// bare form.
    pub fn run_phase_with(
        &mut self,
        phase: SystemPhase,
        world: &mut hecs::World,
        dt: f32,
        frame: FrameResources<'_>,
    ) {
        if self.schedule.is_empty() {
            return;
        }
        self.resources.insert(std::mem::take(frame.input));
        // Swapped with a spare kept here, not `take`n: `take` builds a new
        // `AssetManager` to leave behind, six phases a frame - 1.5% of a web
        // frame of Ten Thousand Lights, building a manager nothing used.
        let spare = self.spare_assets.take().unwrap_or_default();
        self.resources.insert(std::mem::replace(frame.assets, spare));
        self.resources.insert(FrameCamera(frame.camera));
        self.run_phase(phase, world, dt);
        match self.resources.remove::<InputState>() {
            Some(input) => *frame.input = input,
            None => log::error!(
                "a package system removed the `InputState` resource during {phase:?}; \
                 input is lost for the rest of this frame - read it, do not take it"
            ),
        }
        match self.resources.remove::<crate::asset::AssetManager>() {
            Some(assets) => self.spare_assets = Some(std::mem::replace(frame.assets, assets)),
            None => log::error!(
                "a package system removed the `AssetManager` resource during {phase:?}; \
                 every asset is gone for the rest of this frame - read it, do not take it"
            ),
        }
    }

    /// Metadata for every registered package — for `--list-packages`, the
    /// editor's package panel, and diagnostics.
    pub fn packages(&self) -> Vec<PackageMetadata> {
        self.registry.list()
    }

    /// Every registered system as `(name, phase, priority)`, in run order.
    ///
    /// The answer to "is my package's system actually registered?", which was
    /// previously unanswerable.
    pub fn systems(&self) -> Vec<(&str, SystemPhase, i32)> {
        self.schedule.list()
    }

    /// The script API surface packages declared, for editor tooltips and the
    /// generated API index.
    pub fn script_api(&self) -> &ScriptApiRegistry {
        &self.script_api
    }

    /// Editor extensions packages declared.
    pub fn editor(&self) -> &EditorRegistry {
        &self.editor
    }

    /// Whether [`Self::initialize`] has run.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

impl Default for PackageHost {
    fn default() -> Self {
        Self::new()
    }
}

use crate::input::InputState;

/// What a host lends packages for one phase, beside the world and `dt`.
/// See [`PackageHost::run_phase_with`].
pub struct FrameResources<'a> {
    /// The host's input for this frame. Moved into the resource map for the
    /// phase and back out after it, so a package reads it in place.
    pub input: &'a mut InputState,
    /// The host's assets: atlases and their regions, fonts, materials, data.
    /// Moved in and out like the input; read it, never `remove` it.
    pub assets: &'a mut crate::asset::AssetManager,
    /// The camera the frame is drawn through: position, zoom, viewport,
    /// rotation. A package that needs the world under the mouse converts
    /// through `Camera2D::screen_to_world` with the input's mouse position.
    pub camera: crate::renderer::camera::Camera2D,
}

/// The frame's camera, as a resource. Published by every host that runs
/// packages, before each phase.
#[derive(Debug, Clone, Copy)]
pub struct FrameCamera(pub crate::renderer::camera::Camera2D);
