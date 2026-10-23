//! Package system — trait, registry, and extension infrastructure for jEngine.
//!
//! Packages extend the engine with new components, systems, script API functions,
//! editor UI extensions, assets, and shaders. They are resolved at build-time
//! via Cargo (Rust crates) or at runtime (script/asset packages).

/// Renders the API Index in `docs/scripting-api.md` from the registration
/// sites, so it cannot drift from what scripts can actually call.
pub mod api_index;
pub mod component_registry;
pub mod core_registration;
pub mod core_script_api;
pub mod editor_registry;
pub mod field_names;
pub mod host;
pub mod manifest;
pub mod script_api;
pub mod script_commands;
pub mod system_schedule;

use serde::{Deserialize, Serialize};

use crate::package::component_registry::ComponentRegistry;
use crate::package::editor_registry::EditorRegistry;
use crate::package::script_api::ScriptApiRegistry;
use crate::package::system_schedule::SystemSchedule;

// ──────────────────────────── Global Component Registry Handle ────────────────────────────

use std::sync::{Arc, OnceLock, RwLock};

/// Process-wide handle to the live [`ComponentRegistry`]. Set once by the
/// host (runtime / editor / tests) after all packages have registered their
/// components, and read by code paths that need to dispatch on the dynamic
/// component set without threading the registry through their signature —
/// notably [`crate::scripting::bridge::command::BridgeCommand::SetComponentField`]
/// and the matching [`crate::scripting::api::ScriptCommand::SetComponentField`]
/// applier.
///
/// Wrapped in [`RwLock`] so tests can swap registries between cases without
/// leaking; production code only ever calls [`set_global_component_registry`]
/// once.
static GLOBAL_COMPONENT_REGISTRY: OnceLock<RwLock<Arc<ComponentRegistry>>> = OnceLock::new();

/// Install (or replace) the process-wide [`ComponentRegistry`] handle.
pub fn set_global_component_registry(registry: Arc<ComponentRegistry>) {
    let cell = GLOBAL_COMPONENT_REGISTRY.get_or_init(|| RwLock::new(registry.clone()));
    *cell.write().expect("component registry lock poisoned") = registry;
}

/// Borrow the process-wide [`ComponentRegistry`] handle, if one has been
/// installed via [`set_global_component_registry`]. Returns `None` for hosts
/// that never wired up a registry (e.g. minimal unit tests).
pub fn global_component_registry() -> Option<Arc<ComponentRegistry>> {
    GLOBAL_COMPONENT_REGISTRY
        .get()
        .map(|cell| cell.read().expect("component registry lock poisoned").clone())
}

// ──────────────────────────── Frame Resource Wrappers ────────────────────────────


/// Contact events produced by the physics system during the current frame.
///
/// Inserted into the `ResourceMap` by the physics system after stepping.
/// The game loop reads these back to feed into the scripting API.
pub struct FrameContactEvents(pub Vec<crate::physics::ContactEvent>);

/// Audio state results from the audio system, fed back to the scripting API.
///
/// Carries the last sound handle and current music handle after audio commands are applied.
pub struct AudioResults {
    pub last_sound_handle: u64,
    pub music_handle: Option<u64>,
}

// ──────────────────────────── Package Trait ────────────────────────────

/// Trait implemented by every jEngine package (plugin).
///
/// Packages register their components, systems, script API functions,
/// and editor extensions during engine initialization.
pub trait JEnginePackage: Send + Sync {
    /// Metadata describing this package.
    fn metadata(&self) -> PackageMetadata;

    /// Register ECS components with the engine's component registry.
    fn register_components(&self, registry: &mut ComponentRegistry);

    /// Register systems (update, render, physics, etc.) with the system schedule.
    fn register_systems(&self, schedule: &mut SystemSchedule);

    /// Register script API functions available to Lua/JS.
    fn register_script_api(&self, api_registry: &mut ScriptApiRegistry);

    /// Register editor UI extensions (inspector fields, menus, toolbar actions).
    /// Default impl does nothing — packages without editor extensions skip this.
    fn register_editor(&self, _editor: &mut EditorRegistry) {}

    /// Contribute script-facing commands that work in **every** runtime.
    ///
    /// Prefer this over [`Self::register_script_api`] for anything a game
    /// script should be able to call. `register_script_api` handlers are
    /// installed only by `ScriptSystem` (native Luau / QuickJS), so they are
    /// `nil` in the editor viewport and web builds; commands registered here
    /// are dispatched by all four runtimes through
    /// [`handle_package_command`](script_commands::handle_package_command).
    ///
    /// The trade is that a command cannot return a value — see the
    /// [`script_commands`] module docs. Registration refuses a getter-shaped
    /// name rather than letting it fail silently at call time.
    fn register_script_commands(&self, _commands: &mut script_commands::PackageCommandRegistry) {}

    /// Contribute to audio: a clip format the package renders to PCM when
    /// it loads (`jengine-tracker` plays `.song` this way). The registration
    /// is the process's - both audio backends ask it - so it reaches every
    /// runtime. Default does nothing.
    fn register_audio(&self, _audio: &mut crate::audio::transcode::AudioRegistrar) {}

    // Future: register_assets, register_shaders — reserved for Phase 6+.
}

// ──────────────────────────── Package Metadata ────────────────────────────

/// Describes a package's identity and compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    /// Unique package name (e.g. "jengine-physics").
    pub name: String,
    /// Semver version string (e.g. "0.1.0").
    pub version: String,
    /// Short description of what the package provides.
    pub description: String,
    /// Author name or organization.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
    /// Minimum compatible jengine-core version (semver).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,
    /// Names of packages this package depends on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
}

// ──────────────────────────── Package Registry ────────────────────────────

/// Central registry holding all loaded packages.
///
/// Created at engine initialization, packages are registered in order,
/// then their components/systems/APIs are initialized.
pub struct PackageRegistry {
    packages: Vec<Box<dyn JEnginePackage>>,
}

impl PackageRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            packages: Vec::new(),
        }
    }

    /// Add a package to the registry.
    pub fn add(&mut self, package: Box<dyn JEnginePackage>) {
        log::info!(
            "Package registered: {} v{}",
            package.metadata().name,
            package.metadata().version
        );
        self.packages.push(package);
    }

    /// Initialize all packages: register their components, systems, script APIs, and editor extensions.
    pub fn initialize_all(
        &self,
        components: &mut ComponentRegistry,
        schedule: &mut SystemSchedule,
        script_api: &mut ScriptApiRegistry,
        editor: &mut EditorRegistry,
        script_commands: &mut script_commands::PackageCommandRegistry,
    ) {
        let mut audio = crate::audio::transcode::AudioRegistrar::new();
        for pkg in &self.packages {
            let meta = pkg.metadata();
            log::info!("Initializing package: {} v{}", meta.name, meta.version);
            pkg.register_components(components);
            pkg.register_systems(schedule);
            pkg.register_script_api(script_api);
            pkg.register_script_commands(script_commands);
            pkg.register_editor(editor);
            pkg.register_audio(&mut audio);
        }
    }

    /// Get metadata for all registered packages.
    pub fn list(&self) -> Vec<PackageMetadata> {
        self.packages.iter().map(|p| p.metadata()).collect()
    }

    /// Number of registered packages.
    pub fn len(&self) -> usize {
        self.packages.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }
}

impl Default for PackageRegistry {
    fn default() -> Self {
        Self::new()
    }
}
