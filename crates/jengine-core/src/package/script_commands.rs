//! Script-facing commands contributed by packages — the one mechanism that
//! reaches every runtime.
//!
//! # Why this exists
//!
//! Before this, a package had two half-bridges and no way across:
//!
//! | Mechanism | Native | Editor viewport + web |
//! |---|---|---|
//! | [`ScriptApiRegistry`](super::script_api::ScriptApiRegistry) handlers | yes | **no** |
//! | [`handle_world_command`](crate::scripting::host_commands::handle_world_command) | **no** | yes |
//!
//! `install_script_api` is only wired into `ScriptSystem`, so a package's
//! globals were `nil` in the editor — the runtime this engine is developed in.
//! The shared world-command path is the mirror image: implemented once and
//! dispatched by both browser hosts, but never by native.
//!
//! This module joins them. A package registers **one** handler taking the
//! world and its arguments; every runtime turns a call into `(tag, args)` and
//! dispatches it through [`handle_package_command`]. One implementation, four
//! runtimes, and — because the handler takes [`CommandArgs`] — it is testable
//! natively with `JsonArgs` rather than only in a browser.
//!
//! # What it is not
//!
//! **Fire-and-forget.** A command cannot return a value to the script that
//! called it, because the browser hosts drain commands *after* the script has
//! finished running. That covers most of a gameplay API — send, spawn, apply,
//! set — but not `get_fps()`. Reads belong in a published mirror the script
//! reads directly, the way core's own synchronous reads work.
//! [`PackageCommandRegistry::register`] refuses a name that looks like a
//! getter rather than letting it fail silently at call time.

use std::collections::HashMap;
use std::sync::Arc;

use crate::scripting::host_commands::{CommandArgs, ScriptWorldHost};

/// A package command's implementation.
///
/// Takes the live world (through the host, which owns entity-id resolution)
/// and the call's arguments. Returns nothing: see the module docs.
pub type PackageCommandHandler =
    Arc<dyn Fn(&mut dyn ScriptWorldHost, &dyn CommandArgs) + Send + Sync>;

/// Everything a runtime needs to expose one package command to scripts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageCommandInfo {
    /// The global a script calls, e.g. `net_send`.
    pub name: String,
    /// The wire tag, e.g. `pkg/jengine-net/net_send`. Generated, never
    /// authored — see [`command_tag`].
    pub tag: String,
    /// Which package contributed it.
    pub package: String,
    /// Parameter names, in positional order. A browser preamble maps
    /// positional JS arguments onto these, so the handler reads them by name
    /// through [`CommandArgs`].
    pub args: Vec<String>,
}

/// Build the wire tag for a package command.
///
/// Bridge tags are a flat, undeclared namespace shared with core's own — a
/// collision is nearly silent, because the first `match` arm wins and the
/// older function quietly stops working. Core's tags are short lowercase
/// mnemonics (`pa`, `sas`, `ikt`), so the `pkg/` prefix cannot collide with
/// one, and including the package name means two packages can both offer a
/// `connect` without fighting.
pub fn command_tag(package: &str, name: &str) -> String {
    format!("pkg/{package}/{name}")
}

/// Names that would need a return value, which a command cannot give.
///
/// Refused at registration rather than at call time: a script calling a
/// getter that silently returns nil is the failure mode this whole module
/// exists to end, and it is far cheaper to catch when the package loads.
fn looks_like_a_getter(name: &str) -> bool {
    name.starts_with("get_")
        || name.starts_with("is_")
        || name.starts_with("has_")
        || name.starts_with("find_")
}

/// Every script command contributed by the loaded packages.
#[derive(Default)]
pub struct PackageCommandRegistry {
    by_tag: HashMap<String, PackageCommandHandler>,
    infos: Vec<PackageCommandInfo>,
}

impl PackageCommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register one command.
    ///
    /// `args` are the parameter names in positional order. Returns the tag it
    /// was registered under, or an error explaining why it was refused —
    /// refusing loudly at load beats a global that exists and does nothing.
    pub fn register<F>(
        &mut self,
        package: &str,
        name: &str,
        args: &[&str],
        handler: F,
    ) -> Result<String, String>
    where
        F: Fn(&mut dyn ScriptWorldHost, &dyn CommandArgs) + Send + Sync + 'static,
    {
        // A global has to be a valid identifier in *both* Lua and JS, which
        // means it cannot start with a digit - `2fast` is all alphanumeric and
        // still a syntax error in the generated preamble.
        let valid = !name.is_empty()
            && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err(format!(
                "'{name}' is not a usable script global: it must start with a letter or underscore, then letters, digits and underscores only"
            ));
        }
        if looks_like_a_getter(name) {
            return Err(format!(
                "'{name}' reads like a getter, and a package command cannot return a value \
                 (browser hosts drain commands after the script has finished). Publish the \
                 value into a mirror the script reads instead."
            ));
        }
        if self.infos.iter().any(|i| i.name == name) {
            return Err(format!(
                "a script global named '{name}' is already registered; \
                 the second one would shadow the first"
            ));
        }

        let tag = command_tag(package, name);
        self.by_tag.insert(tag.clone(), Arc::new(handler));
        self.infos.push(PackageCommandInfo {
            name: name.to_string(),
            tag: tag.clone(),
            package: package.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
        });
        Ok(tag)
    }

    /// The handler for a tag, if this registry owns it.
    pub fn handler(&self, tag: &str) -> Option<&PackageCommandHandler> {
        self.by_tag.get(tag)
    }

    /// Every registered command, in registration order.
    pub fn commands(&self) -> &[PackageCommandInfo] {
        &self.infos
    }

    /// The script-facing global names, for the hosts that install globals from
    /// an allowlist.
    pub fn names(&self) -> Vec<String> {
        self.infos.iter().map(|i| i.name.clone()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.infos.is_empty()
    }

    pub fn len(&self) -> usize {
        self.infos.len()
    }

    /// JavaScript declaring every command as a global that queues its tag.
    ///
    /// Spliced into the browser script wrapper, so package globals sit in the
    /// same scope as core's and are found by the same `lookup` closure that
    /// the Fengari bridge uses. Without this a package global is simply not
    /// defined in a browser, which is the bug this module exists to fix.
    ///
    /// Positional parameters are written out as named fields so the handler
    /// reads them with `args.text("message")` rather than by index.
    pub fn browser_globals_js(&self) -> String {
        let mut out = String::new();
        for info in &self.infos {
            let params: Vec<String> = (0..info.args.len()).map(|i| format!("a{i}")).collect();
            let fields: Vec<String> = info
                .args
                .iter()
                .enumerate()
                .map(|(i, name)| format!("{}: a{}", js_key(name), i))
                .collect();
            out.push_str(&format!(
                "  function {}({}) {{ window.__jengine_play.commands.push({{ t: '{}'{}{} }}); }}\n",
                info.name,
                params.join(", "),
                info.tag,
                if fields.is_empty() { "" } else { ", " },
                fields.join(", "),
            ));
        }
        out
    }
}

/// Quote an argument name for use as a JS object key when it needs it.
fn js_key(name: &str) -> String {
    let plain = !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if plain {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "\\'"))
    }
}

/// Dispatch a command if a package owns its tag.
///
/// Hosts call this after [`handle_world_command`] and before their own
/// `match`, so a package tag never reaches a host's unknown-command path.
/// Returns `true` when handled — including when the handler chose to do
/// nothing, because the tag *was* ours and the host must not try it again.
///
/// [`handle_world_command`]: crate::scripting::host_commands::handle_world_command
pub fn handle_package_command(
    host: &mut dyn ScriptWorldHost,
    tag: &str,
    args: &dyn CommandArgs,
    registry: &PackageCommandRegistry,
) -> bool {
    // Cheap reject: every package tag starts with the prefix, so a host
    // draining hundreds of core commands per frame does not pay for a hash
    // lookup on each one.
    if !tag.starts_with("pkg/") {
        return false;
    }
    match registry.handler(tag) {
        Some(handler) => {
            handler(host, args);
            true
        }
        None => {
            // A `pkg/` tag with no handler means a script was compiled against
            // a package this runtime did not load. Silence would look like the
            // call working.
            log::warn!(
                "[package] no handler for '{tag}' - is the package enabled in this runtime?"
            );
            true
        }
    }
}

// ---------------------------------------------------------------------------
// The process-wide registry
// ---------------------------------------------------------------------------

/// The commands the packages loaded in this process contributed.
///
/// A process runs exactly one set of packages — they are linked at compile
/// time and registered once at startup — so this is genuinely process state
/// rather than a shortcut. The alternative was threading a registry through
/// the native command-apply loop, which is a hot path taking tens of
/// thousands of commands a frame, to serve the handful that are package
/// calls.
static GLOBAL: std::sync::RwLock<Option<PackageCommandRegistry>> =
    std::sync::RwLock::new(None);

/// Publish the registry for every runtime in this process.
///
/// Call once, after `PackageHost::initialize`. A second call replaces the
/// first, which is what a test or an editor reloading a project wants.
pub fn install_global(registry: PackageCommandRegistry) {
    match GLOBAL.write() {
        Ok(mut slot) => *slot = Some(registry),
        Err(e) => log::error!("[package] could not publish script commands: {e}"),
    }
}

/// Dispatch through the process-wide registry.
///
/// Returns `false` for anything that is not a package tag, so a host falls
/// through to its own handling exactly as it does for
/// [`handle_package_command`].
pub fn dispatch_global(
    host: &mut dyn ScriptWorldHost,
    tag: &str,
    args: &dyn CommandArgs,
) -> bool {
    // Checked before taking the lock: a host draining thousands of core
    // commands a frame must not pay for a lock on each one.
    if !tag.starts_with("pkg/") {
        return false;
    }
    match GLOBAL.read() {
        Ok(slot) => match slot.as_ref() {
            Some(registry) => handle_package_command(host, tag, args, registry),
            None => {
                log::warn!("[package] '{tag}' called but no packages are registered");
                true
            }
        },
        Err(e) => {
            log::error!("[package] script command registry unreadable: {e}");
            true
        }
    }
}

/// Drop the process-wide registry. For tests that must not leak into each
/// other, and for a runtime tearing a project down.
pub fn clear_global() {
    if let Ok(mut slot) = GLOBAL.write() {
        *slot = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripting::host_commands::JsonArgs;
    use hecs::{Entity, World};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A minimal host: enough to satisfy the trait without a runtime.
    struct TestHost {
        world: World,
    }

    impl ScriptWorldHost for TestHost {
        fn world(&mut self) -> &mut World {
            &mut self.world
        }
        fn resolve_entity(&mut self, _id: u32) -> Option<Entity> {
            None
        }
    }

    fn host() -> TestHost {
        TestHost { world: World::new() }
    }

    #[test]
    fn a_tag_is_namespaced_by_package() {
        assert_eq!(command_tag("jengine-net", "net_send"), "pkg/jengine-net/net_send");
        // Two packages can both offer `connect` without colliding - bridge
        // tags are a flat namespace and a collision is nearly silent.
        assert_ne!(command_tag("a", "connect"), command_tag("b", "connect"));
    }

    #[test]
    fn a_package_tag_cannot_collide_with_a_core_tag() {
        // Core's are short lowercase mnemonics; none contains a slash.
        for core_tag in crate::scripting::host_commands::SHARED_WORLD_TAGS {
            assert!(!core_tag.starts_with("pkg/"), "{core_tag}");
        }
    }

    #[test]
    fn registering_returns_the_tag_and_the_handler_runs() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let mut reg = PackageCommandRegistry::new();
        let tag = reg
            .register("p", "do_thing", &["amount"], move |_host, args| {
                assert_eq!(args.number("amount"), Some(7.0));
                seen.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();

        let mut h = host();
        assert!(handle_package_command(
            &mut h,
            &tag,
            &JsonArgs::new(json!({ "amount": 7 })),
            &reg
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_core_tag_is_not_claimed() {
        // The prefix check must let every core command fall through to the
        // host's own dispatch untouched.
        let reg = PackageCommandRegistry::new();
        let mut h = host();
        for tag in ["pa", "sa", "st", "log", "de"] {
            assert!(
                !handle_package_command(&mut h, tag, &JsonArgs::new(json!({})), &reg),
                "{tag} was claimed by the package dispatcher"
            );
        }
    }

    #[test]
    fn an_unknown_package_tag_is_claimed_and_warned_about() {
        // Falling through would reach the host's unknown-command path and
        // look like a typo in the engine rather than a package that is off.
        let reg = PackageCommandRegistry::new();
        let mut h = host();
        assert!(handle_package_command(
            &mut h,
            "pkg/absent/thing",
            &JsonArgs::new(json!({})),
            &reg
        ));
    }

    #[test]
    fn a_getter_name_is_refused_at_registration() {
        // The whole point: fail when the package loads, not when a script
        // calls it and silently receives nil.
        let mut reg = PackageCommandRegistry::new();
        for name in ["get_fps", "is_ready", "has_room", "find_peer"] {
            let err = reg.register("p", name, &[], |_, _| {}).unwrap_err();
            assert!(err.contains("cannot return a value"), "{name}: {err}");
        }
        assert!(reg.is_empty());
    }

    #[test]
    fn a_duplicate_name_is_refused() {
        let mut reg = PackageCommandRegistry::new();
        reg.register("a", "send", &[], |_, _| {}).unwrap();
        let err = reg.register("b", "send", &[], |_, _| {}).unwrap_err();
        assert!(err.contains("already registered"), "{err}");
        assert_eq!(reg.len(), 1, "the shadowing registration is dropped");
    }

    #[test]
    fn an_unusable_global_name_is_refused() {
        let mut reg = PackageCommandRegistry::new();
        for bad in ["", "net send", "net-send", "net.send", "2fast"] {
            assert!(reg.register("p", bad, &[], |_, _| {}).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn generated_js_maps_positional_arguments_onto_named_fields() {
        let mut reg = PackageCommandRegistry::new();
        reg.register("jengine-net", "net_send", &["message"], |_, _| {})
            .unwrap();
        let js = reg.browser_globals_js();
        assert!(js.contains("function net_send(a0)"), "{js}");
        assert!(js.contains("t: 'pkg/jengine-net/net_send'"), "{js}");
        assert!(js.contains("message: a0"), "{js}");
    }

    #[test]
    fn a_command_with_no_arguments_generates_valid_js() {
        // The trailing-comma case: `{ t: 'x', }` is legal JS but `{ t: 'x' }`
        // is what should be emitted, and an empty parameter list must not
        // produce `function f(,)`.
        let mut reg = PackageCommandRegistry::new();
        reg.register("p", "ping", &[], |_, _| {}).unwrap();
        let js = reg.browser_globals_js();
        assert!(js.contains("function ping()"), "{js}");
        assert!(js.contains("commands.push({ t: 'pkg/p/ping' })"), "{js}");
    }

    #[test]
    fn an_entity_argument_named_e_reaches_the_shared_target_helper() {
        // `host_commands::target` reads the conventional `"e"` field, so a
        // package command taking an entity has to spell it that way to get
        // entity resolution for free.
        let mut reg = PackageCommandRegistry::new();
        reg.register("p", "poke", &["e", "force"], |_, _| {}).unwrap();
        let js = reg.browser_globals_js();
        assert!(js.contains("e: a0"), "{js}");
        assert!(js.contains("force: a1"), "{js}");
    }

    #[test]
    fn names_lists_every_global_for_the_allowlist_hosts() {
        let mut reg = PackageCommandRegistry::new();
        reg.register("p", "alpha", &[], |_, _| {}).unwrap();
        reg.register("p", "beta", &[], |_, _| {}).unwrap();
        assert_eq!(reg.names(), vec!["alpha".to_string(), "beta".to_string()]);
    }
}
