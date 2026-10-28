//! Networking for jEngine — transport, connection lifecycle, and a message
//! bus that game scripts can reach.
//!
//! This package deliberately has **no opinion about rooms, replication or
//! authority**. It is the layer underneath those: a [`Transport`] trait with
//! real WebSocket backends for native and browser, a [`NetConnection`] that
//! turns an event stream into a state machine plus bounded queues, and a
//! custom-component [`channel`] so Lua can send and receive without any new
//! script globals. A Colyseus client or a hand-rolled protocol both sit on
//! top of this unchanged, because both speak WebSocket.
//!
//! WebSocket is not one choice among several. A browser has no raw sockets,
//! so it is the only transport the editor viewport and web builds can ever
//! use, and anything that works everywhere has to go through it.
//!
//! # Using it from Rust
//!
//! ```no_run
//! # use jengine_net::{NetConnection, connect};
//! let mut conn = NetConnection::new();
//! connect(&mut conn, "wss://example.com/game");
//! // once per frame:
//! conn.pump();
//! for msg in conn.drain() {
//!     if let Some(text) = msg.as_text() { println!("{text}"); }
//! }
//! ```
//!
//! # Using it from a script
//!
//! See [`channel`] — an entity with a `"Net"` custom component gets `outbox`,
//! `inbox`, `state` and `dropped` fields.
//!
//! # What is not here yet
//!
//! - **No reconnection.** A failed connection stays failed until something
//!   calls [`connect`] again. Silent reconnects would replay game state the
//!   caller never asked to resend.
//! - **No server here.** This is a client; the engine does not listen for
//!   connections, and in a browser it could not. `crates/jengine-relay` is a
//!   small one for turn-based games (rooms, seats, one ordered log - `cargo
//!   run -p jengine-relay`), and a game's session layer speaks to it
//!   through this package's `Net` channel. `crates/jengine-server`
//!   is the general one: rooms a game writes, players that persist, and state
//!   sync by interest.
//! - **No replication.** No entity sync, no authority model, no interest
//!   management. That is the layer above, and it wants a game exercising it
//!   before its decisions get locked in.

pub mod channel;
pub mod connection;
pub mod event;
pub mod http;
pub mod transport;

pub use connection::{NetConnection, NetMessage, DEFAULT_CAPACITY};
pub use event::{NetError, NetEvent, NetState};
pub use http::{HttpBackend, HttpClient, HttpMethod, HttpRequest, HttpResponse, MockHttpBackend};
pub use transport::{MockTransport, Transport};

#[cfg(not(target_arch = "wasm32"))]
pub mod http_native;
#[cfg(not(target_arch = "wasm32"))]
pub mod ws_native;
#[cfg(not(target_arch = "wasm32"))]
pub use http_native::NativeHttpBackend as PlatformHttpBackend;
#[cfg(not(target_arch = "wasm32"))]
pub use ws_native::WebSocketTransport;

#[cfg(target_arch = "wasm32")]
pub mod http_web;
#[cfg(target_arch = "wasm32")]
pub mod js_transport;
#[cfg(target_arch = "wasm32")]
pub use js_transport::{JsTransport, DEFAULT_GLOBAL as JS_ADAPTER_GLOBAL};
#[cfg(target_arch = "wasm32")]
pub mod ws_web;
#[cfg(target_arch = "wasm32")]
pub use http_web::WebHttpBackend as PlatformHttpBackend;
#[cfg(target_arch = "wasm32")]
pub use ws_web::WebSocketTransport;

/// An HTTP client on whichever backend this platform has.
///
/// Native runs each request on a worker thread; the browser uses `fetch`.
/// Callers that want a different backend - a mock, a recorded session -
/// construct [`HttpClient`] directly.
pub fn http_client() -> HttpClient {
    HttpClient::new(Box::new(PlatformHttpBackend::new()))
}

use jengine_core::ecs::components::CustomComponents;
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::script_api::ScriptApiRegistry;
use jengine_core::package::script_commands::PackageCommandRegistry;
use jengine_core::package::system_schedule::{SystemContext, SystemPhase, SystemSchedule};
use jengine_core::package::{JEnginePackage, PackageMetadata};

/// Open a connection, replacing any existing one.
///
/// The one call that differs between native and browser, wrapped so callers
/// do not need the `cfg`. Failures are reported through the connection's
/// state rather than returned, so both backends behave identically: native
/// connects on a thread and cannot fail synchronously either.
/// URL prefix that routes a connection through a JS adapter on the page
/// rather than a raw WebSocket.
///
/// `js:` alone uses the default global; `js:myAdapter:wss://host/path` names
/// one. Everything after the global is handed to the adapter untouched,
/// because a library's "URL" is its own business - Colyseus takes a room name
/// where a WebSocket takes a path.
pub const JS_SCHEME: &str = "js:";

/// Split a `js:` URL into `(global, rest)`.
///
/// `js:wss://x` -> (default, "wss://x"); `js:room:lobby` -> ("room", "lobby").
/// A global name may not contain `:` or `/`, which is what keeps
/// `js:wss://host` from being read as a global called `wss`.
// Only *called* on wasm, but pure enough to test everywhere - and the
// scheme-versus-global ambiguity below is exactly the kind of thing worth
// pinning on a target that can run tests.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn parse_js_url(url: &str) -> (String, String) {
    let rest = &url[JS_SCHEME.len()..];
    match rest.split_once(':') {
        Some((head, tail))
            if !head.is_empty() && !head.contains('/') && !tail.starts_with('/') =>
        {
            (head.to_string(), tail.to_string())
        }
        _ => (String::new(), rest.to_string()),
    }
}

pub fn connect(conn: &mut NetConnection, url: &str) {
    conn.set_url(url);
    #[cfg(not(target_arch = "wasm32"))]
    {
        if url.starts_with(JS_SCHEME) {
            // There is no page and no JS engine here. Failing with the reason
            // beats handing "js:..." to a DNS resolver and reporting whatever
            // it makes of it.
            let mut mock = MockTransport::new();
            mock.push_event(NetEvent::Failed(format!(
                "'{url}' needs a browser: a {JS_SCHEME} adapter only exists in a web build"
            )));
            conn.attach(Box::new(mock));
            conn.pump();
            return;
        }
        conn.attach(Box::new(WebSocketTransport::connect(url)));
    }
    #[cfg(target_arch = "wasm32")]
    {
        if url.starts_with(JS_SCHEME) {
            let (global, target) = parse_js_url(url);
            let global = if global.is_empty() {
                JS_ADAPTER_GLOBAL.to_string()
            } else {
                global
            };
            match JsTransport::connect(&global, &target) {
                Ok(t) => conn.attach(Box::new(t)),
                Err(e) => {
                    let mut mock = MockTransport::new();
                    mock.push_event(NetEvent::Failed(e.to_string()));
                    conn.attach(Box::new(mock));
                    conn.pump();
                }
            }
            return;
        }
        match WebSocketTransport::connect(url) {
            Ok(t) => conn.attach(Box::new(t)),
            Err(e) => {
                // Mirror the native shape: the caller polls state, not a
                // Result, so a synchronous rejection has to land there too.
                let mut mock = MockTransport::new();
                mock.push_event(NetEvent::Failed(e.to_string()));
                conn.attach(Box::new(mock));
                conn.pump();
            }
        }
    }
}

/// Write a field onto every `Net` channel entity.
///
/// The channel is the package's own state, carried on a component rather than
/// in a resource, so a command reaches it through the world like anything
/// else. Writing to *every* channel entity matches how the systems read them:
/// nothing designates one entity as "the" connection.
fn set_channel_field(
    host: &mut dyn jengine_core::scripting::host_commands::ScriptWorldHost,
    field: &str,
    value: serde_json::Value,
) {
    let mut wrote = false;
    for (_, custom) in host.world().query_mut::<&mut CustomComponents>() {
        if let Some(component) = custom.0.get_mut(channel::COMPONENT)
            && let Some(obj) = component.as_object_mut()
        {
            obj.insert(field.to_string(), value.clone());
            wrote = true;
        }
    }
    if !wrote {
        log::warn!(
            "[net] no entity carries a '{}' custom component - the call had nowhere to go",
            channel::COMPONENT
        );
    }
}

/// Append to an array field on every `Net` channel entity.
fn push_channel_array(
    host: &mut dyn jengine_core::scripting::host_commands::ScriptWorldHost,
    field: &str,
    value: serde_json::Value,
) {
    let mut wrote = false;
    for (_, custom) in host.world().query_mut::<&mut CustomComponents>() {
        if let Some(component) = custom.0.get_mut(channel::COMPONENT)
            && let Some(obj) = component.as_object_mut()
        {
            let slot = obj
                .entry(field.to_string())
                .or_insert_with(|| serde_json::Value::Array(Vec::new()));
            if !slot.is_array() {
                *slot = serde_json::Value::Array(Vec::new());
            }
            if let Some(items) = slot.as_array_mut() {
                items.push(value.clone());
                wrote = true;
            }
        }
    }
    if !wrote {
        log::warn!(
            "[net] no entity carries a '{}' custom component - the call had nowhere to go",
            channel::COMPONENT
        );
    }
}

pub struct NetPackage;

/// The connections of entities that asked for one of their own
/// (`channel::FIELD_OWN`), beside the shared [`NetConnection`] resource.
#[derive(Default)]
pub struct OwnConnections(pub std::collections::HashMap<hecs::Entity, NetConnection>);

/// The text messages among what a connection received; binary has no clean
/// representation in a Lua table, so it stays available to Rust only.
fn texts(messages: Vec<NetMessage>) -> Vec<String> {
    messages
        .into_iter()
        .filter_map(|m| match m {
            NetMessage::Text(s) => Some(s),
            NetMessage::Binary(b) => {
                log::debug!("[net] {} binary bytes not mirrored to scripts", b.len());
                None
            }
        })
        .collect()
}

impl JEnginePackage for NetPackage {
    fn metadata(&self) -> PackageMetadata {
        PackageMetadata {
            name: "jengine-net".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description:
                "WebSocket transport, connection lifecycle and a script-reachable message bus"
                    .into(),
            author: String::new(),
            engine_version: Some("0.1.0".into()),
            dependencies: vec![],
        }
    }

    fn register_components(&self, _registry: &mut ComponentRegistry) {
        // Nothing: the channel is a custom component (`Net`), and what an
        // entity is on the network is `jengine-online`'s `online_slot`.
    }

    fn register_systems(&self, schedule: &mut SystemSchedule) {
        // Receive before scripts run, so a script sees this frame's traffic
        // rather than last frame's.
        schedule.add_system("net_receive", SystemPhase::PreUpdate, 0, net_receive);
        // And once a frame before any step: with fixed-step gameplay a frame
        // that runs no step - one waiting on the network, say - still reads it.
        schedule.add_system("net_receive_frame", SystemPhase::Frame, 0, net_receive);

        // Send after scripts have run, so a message queued this frame leaves
        // this frame instead of waiting for the next one.
        schedule.add_system("net_send", SystemPhase::ScriptExec, 0, net_send);
        // And at the end of the frame's own phase, after what a `Frame`
        // system queued (a lockstep session's inputs) - none of which waits
        // for a step that may not come.
        schedule.add_system("net_send_frame", SystemPhase::Frame, 20, net_send);

        // HTTP answers land in the same pass, and before scripts, so a
        // response is readable on the frame it arrived rather than the next.
        schedule.add_system("http_receive", SystemPhase::PreUpdate, 1, |ctx| {
            let (world, _dt, resources) = ctx.split();
            let Some(client) = resources.get_mut::<HttpClient>() else {
                return;
            };
            client.pump();
            let responses = client.drain();
            if responses.is_empty() {
                return;
            }
            for (_, custom) in world.query_mut::<&mut CustomComponents>() {
                if let Some(component) = custom.0.get_mut(channel::COMPONENT) {
                    channel::publish_responses(component, &responses);
                }
            }
        });

        // Requests go out after scripts, alongside outgoing socket traffic.
        schedule.add_system("http_send", SystemPhase::ScriptExec, 1, |ctx| {
            let (world, _dt, resources) = ctx.split();

            // Collect first: creating the client needs `&mut resources`, and
            // reading the requests needs the world, so the two cannot overlap.
            let mut requests = Vec::new();
            for (_, custom) in world.query_mut::<&mut CustomComponents>() {
                if let Some(component) = custom.0.get_mut(channel::COMPONENT) {
                    requests.extend(channel::take_requests(component));
                }
            }
            if requests.is_empty() {
                return;
            }
            // Created on first use, so a project that only uses WebSockets -
            // or neither - never carries an HTTP client it did not ask for.
            if !resources.contains::<HttpClient>() {
                resources.insert(http_client());
            }
            let client = resources
                .get_mut::<HttpClient>()
                .expect("just inserted if absent");
            for request in requests {
                client.send(request);
            }
        });

    }

    fn register_script_api(&self, _api: &mut ScriptApiRegistry) {
        // Nothing here on purpose: `ScriptApiRegistry` handlers are installed
        // only by `ScriptSystem`, so they are native-only. Everything
        // script-facing is a *command* instead - see below.
    }

    fn register_script_commands(&self, commands: &mut PackageCommandRegistry) {
        // These reach every runtime: the global queues a `PackageCall` and
        // this handler runs at apply time, in the editor viewport and a web
        // build as well as natively.
        //
        // The `Net` custom component is still the channel for *reads* -
        // `state`, `inbox`, `responses` - because a command cannot return a
        // value. Writes are much nicer as calls.
        if let Err(e) = commands.register(
            "jengine-net",
            "net_connect",
            &["url"],
            |host, args| {
                let url = args.text("url").unwrap_or_default();
                set_channel_field(host, channel::FIELD_URL, serde_json::Value::String(url));
            },
        ) {
            log::error!("[net] {e}");
        }

        if let Err(e) = commands.register("jengine-net", "net_disconnect", &[], |host, _args| {
            set_channel_field(host, channel::FIELD_URL, serde_json::Value::String(String::new()));
        }) {
            log::error!("[net] {e}");
        }

        if let Err(e) = commands.register(
            "jengine-net",
            "net_send",
            &["message"],
            |host, args| {
                let Some(message) = args.text("message") else {
                    log::warn!("[net] net_send called without a message");
                    return;
                };
                push_channel_array(host, channel::FIELD_OUTBOX, serde_json::Value::String(message));
            },
        ) {
            log::error!("[net] {e}");
        }

        if let Err(e) = commands.register(
            "jengine-net",
            "net_http",
            &["id", "url", "method", "body"],
            |host, args| {
                let Some(url) = args.text("url") else {
                    log::warn!("[net] net_http called without a url");
                    return;
                };
                let mut request = serde_json::Map::new();
                request.insert(
                    "id".into(),
                    serde_json::Value::String(args.text("id").unwrap_or_else(|| url.clone())),
                );
                request.insert("url".into(), serde_json::Value::String(url));
                if let Some(method) = args.text("method") {
                    request.insert("method".into(), serde_json::Value::String(method));
                }
                if let Some(body) = args.text("body") {
                    request.insert("body".into(), serde_json::Value::String(body));
                }
                push_channel_array(
                    host,
                    channel::FIELD_REQUESTS,
                    serde_json::Value::Object(request),
                );
            },
        ) {
            log::error!("[net] {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_js_url_without_a_global_uses_the_default() {
        let (global, target) = parse_js_url("js:wss://example.com/game");
        assert!(global.is_empty(), "empty means 'use the default'");
        assert_eq!(target, "wss://example.com/game");
    }

    #[test]
    fn a_js_url_can_name_its_adapter() {
        let (global, target) = parse_js_url("js:colyseus:my_room");
        assert_eq!(global, "colyseus");
        assert_eq!(target, "my_room");
    }

    #[test]
    fn a_scheme_in_the_target_is_not_mistaken_for_a_global() {
        // The trap: `js:wss://host` must not read `wss` as the adapter name.
        // A `//` immediately after the colon is what distinguishes them.
        for url in ["js:wss://h/p", "js:ws://h", "js:https://h"] {
            let (global, _) = parse_js_url(url);
            assert!(global.is_empty(), "{url} parsed a global it should not have");
        }
    }

    #[test]
    fn a_named_adapter_keeps_a_target_containing_colons() {
        // Only the first colon separates; the rest belongs to the adapter.
        let (global, target) = parse_js_url("js:room:lobby:eu-west");
        assert_eq!(global, "room");
        assert_eq!(target, "lobby:eu-west");
    }

    #[test]
    fn an_empty_js_url_is_handled() {
        let (global, target) = parse_js_url("js:");
        assert!(global.is_empty());
        assert!(target.is_empty());
    }

    #[test]
    fn metadata_names_the_package_the_loader_expects() {
        let m = NetPackage.metadata();
        assert_eq!(m.name, "jengine-net");
        assert!(!m.version.is_empty());
    }

    #[test]
    fn the_package_registers_its_systems() {
        let mut registry = ComponentRegistry::new();
        NetPackage.register_components(&mut registry);
        assert!(registry.get("network_identity").is_none(), "it registers no component: `online_slot` is that job");

        let mut schedule = SystemSchedule::new();
        NetPackage.register_systems(&mut schedule);
        // Six: WebSocket receive/send and HTTP receive/send. Receive runs in
        // PreUpdate and send in ScriptExec for both, so traffic queued on a
        // frame leaves on that frame and an answer is readable on the frame it
        // arrived - and the WebSocket pair runs again in `Frame`, once a
        // frame before any step, so a fixed-step frame that runs no step
        // (one waiting on a lockstep session) still reads and sends.
        assert_eq!(schedule.len(), 6);
    }
}

fn net_receive(ctx: &mut SystemContext<'_>) {
    let (world, _dt, resources) = ctx.split();

    // Who wants what: every `Net` component's requested URL, and
    // whether it wants a connection of its own. Read before any
    // connection is borrowed, because both need the world.
    let asks: Vec<(hecs::Entity, Option<String>, bool)> = world
        .query::<&CustomComponents>()
        .iter()
        .filter_map(|(e, custom)| {
            let component = custom.0.get(channel::COMPONENT)?;
            let url = channel::requested_url(component).map(str::to_string);
            Some((e, url, channel::owns_connection(component)))
        })
        .collect();
    // The shared connection: what the first component asking for
    // one (and not for its own) wants.
    let requested: Option<String> =
        asks.iter().filter(|(_, url, own)| url.is_some() && !own).find_map(|(_, url, _)| url.clone());
    // What each `Net` component is told this frame: (inbox, state, dropped).
    let mut told: Vec<(hecs::Entity, Vec<String>, &'static str, u64)> = Vec::new();

    // Connections of their own, one per entity that asked. Kept
    // apart from the shared one so a scene with one session is
    // exactly as it was.
    let owners: Vec<(hecs::Entity, Option<String>)> =
        asks.iter().filter(|(_, _, own)| *own).map(|(e, url, _)| (*e, url.clone())).collect();
    if !owners.is_empty() || resources.contains::<OwnConnections>() {
        if !resources.contains::<OwnConnections>() {
            resources.insert(OwnConnections::default());
        }
        let own = resources.get_mut::<OwnConnections>().expect("just inserted if absent");
        // An entity that is gone takes its socket with it.
        own.0.retain(|e, _| owners.iter().any(|(o, _)| o == e));
        for (e, url) in &owners {
            let conn = own.0.entry(*e).or_insert_with(NetConnection::new);
            match url.as_deref() {
                Some("") if conn.url().is_some() => conn.disconnect(),
                Some(url) if !url.is_empty() && conn.url() != Some(url) => {
                    log::info!("[net] connecting to {url} (an entity's own connection)");
                    connect(conn, url);
                }
                _ => {}
            }
            conn.pump();
            let incoming = texts(conn.drain());
            told.push((*e, incoming, conn.state().as_str(), conn.dropped_count()));
        }
    }

    // Nothing is asking for the shared connection and none exists:
    // stay out of the way entirely rather than parking an idle
    // resource on every project that merely links this package.
    let shared = if requested.is_none() && !resources.contains::<NetConnection>() {
        None
    } else {
        // The connection is this package's own state, not something
        // the runtime hands down - unlike `PhysicsWorld`, nothing
        // else can create it, so creating it here is not stepping on
        // an owner.
        if !resources.contains::<NetConnection>() {
            resources.insert(NetConnection::new());
        }
        let conn = resources
            .get_mut::<NetConnection>()
            .expect("just inserted if absent");

        // Reconcile: a script states the URL it wants, this closes the gap.
        match requested.as_deref() {
            Some("") if conn.url().is_some() => conn.disconnect(),
            Some(url) if !url.is_empty() && conn.url() != Some(url) => {
                log::info!("[net] connecting to {url}");
                connect(conn, url);
            }
            _ => {}
        }

        conn.pump();
        Some((texts(conn.drain()), conn.state().as_str(), conn.dropped_count()))
    };

    // Each component with a connection of its own hears that one;
    // every other hears the shared one.
    for (e, custom) in world.query_mut::<&mut CustomComponents>() {
        let Some(component) = custom.0.get_mut(channel::COMPONENT) else {
            continue;
        };
        if let Some(i) = told.iter().position(|(owner, ..)| *owner == e) {
            let (_, incoming, state, dropped) = told.swap_remove(i);
            channel::publish(component, &incoming, state, dropped);
        } else if let Some((incoming, state, dropped)) = &shared {
            channel::publish(component, incoming, state, *dropped);
        }
    }
}

fn net_send(ctx: &mut SystemContext<'_>) {
    let (world, _dt, resources) = ctx.split();
    // A message is taken only where there is a connection to carry
    // it; otherwise it waits in the outbox.
    let has_shared = resources.contains::<NetConnection>();
    let owned: Vec<hecs::Entity> =
        resources.get::<OwnConnections>().map(|own| own.0.keys().copied().collect()).unwrap_or_default();
    // (entity, its messages, whether it has a connection of its own)
    let mut outgoing: Vec<(hecs::Entity, Vec<String>, bool)> = Vec::new();
    for (e, custom) in world.query_mut::<&mut CustomComponents>() {
        let Some(component) = custom.0.get_mut(channel::COMPONENT) else {
            continue;
        };
        let own = channel::owns_connection(component);
        if if own { !owned.contains(&e) } else { !has_shared } {
            continue;
        }
        let msgs = channel::take_outbox(component);
        if !msgs.is_empty() {
            outgoing.push((e, msgs, own));
        }
    }
    for (e, msgs, own) in outgoing {
        let conn = if own {
            resources.get_mut::<OwnConnections>().and_then(|own| own.0.get_mut(&e))
        } else {
            resources.get_mut::<NetConnection>()
        };
        let Some(conn) = conn else { continue };
        for msg in msgs {
            if let Err(e) = conn.send_text(&msg) {
                log::warn!("[net] send failed: {e}");
            }
        }
    }
}
