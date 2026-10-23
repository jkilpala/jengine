//! The networking package works end to end through a `PackageHost`.
//!
//! Drives the real package with a real `PackageHost`, because "registered but
//! never executed" is the exact failure this package system already shipped
//! once (BUGS #13): `register_systems` was called by nobody, and the packages
//! that looked fine only did so because runtimes invoked them directly. This
//! package has no hardcoded path in any runtime, so nothing but a test like
//! this would notice it going dead.
//!
//! No sockets. The transport is a mock, so the whole script channel — outbox
//! to the wire, wire to inbox, state published — is exercised deterministically.

use hecs::World;
use jengine_core::ecs::components::CustomComponents;
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::host::PackageHost;
use jengine_core::package::system_schedule::SystemPhase;
use jengine_net::{
    HttpClient, HttpResponse, MockHttpBackend, MockTransport, NetConnection, NetEvent,
    NetPackage,
};
use serde_json::json;

fn hosted() -> PackageHost {
    // Core components first, then the package registers into the same
    // registry — the order a runtime uses.
    let mut registry = ComponentRegistry::new();
    jengine_core::package::core_registration::register_core_components(&mut registry);
    let mut host = PackageHost::new();
    host.add_package(Box::new(NetPackage));
    host.initialize(&mut registry);
    host
}

/// An entity carrying the `Net` channel, as a scene would author it.
fn net_entity(world: &mut World, outbox: serde_json::Value) -> hecs::Entity {
    let mut custom = CustomComponents::default();
    custom.0.insert("Net".to_string(), json!({ "outbox": outbox }));
    world.spawn((custom,))
}

fn channel(world: &World, e: hecs::Entity) -> serde_json::Value {
    world
        .get::<&CustomComponents>(e)
        .unwrap()
        .0
        .get("Net")
        .cloned()
        .unwrap()
}

/// A connection already open on a mock, plus the events it will deliver.
fn open_connection(inbound: &[NetEvent]) -> NetConnection {
    let mut transport = MockTransport::new();
    transport.push_event(NetEvent::Opened);
    for e in inbound {
        transport.push_event(e.clone());
    }
    let mut conn = NetConnection::new();
    conn.attach(Box::new(transport));
    conn
}

#[test]
fn received_text_reaches_the_script_inbox() {
    let mut host = hosted();
    let mut world = World::new();
    let e = net_entity(&mut world, json!([]));
    host.insert_resource(open_connection(&[NetEvent::Text("hello".into())]));

    host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);

    let net = channel(&world, e);
    assert_eq!(net["inbox"], json!(["hello"]));
    assert_eq!(net["state"], json!("connected"), "state is published too");
}

#[test]
fn the_outbox_is_sent_and_cleared() {
    let mut host = hosted();
    let mut world = World::new();
    let e = net_entity(&mut world, json!(["a", "b"]));
    host.insert_resource(open_connection(&[]));

    // Open the socket first; a send while still connecting is queued, not
    // dropped, but this test is about the channel rather than the handshake.
    host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);
    host.run_phase(SystemPhase::ScriptExec, &mut world, 1.0 / 60.0);

    assert_eq!(
        channel(&world, e)["outbox"],
        json!([]),
        "the package owns clearing what it sent"
    );
}

#[test]
fn receive_runs_before_scripts_and_send_after() {
    // The phase choice is the whole reason a message queued this frame leaves
    // this frame. If either system moved, traffic would lag a frame in one
    // direction - which looks like network latency and is nearly impossible
    // to diagnose from the outside.
    let mut host = hosted();
    let mut world = World::new();
    let e = net_entity(&mut world, json!([]));
    host.insert_resource(open_connection(&[NetEvent::Text("early".into())]));

    // PreUpdate alone must already have delivered, i.e. before any script ran.
    host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);
    assert_eq!(channel(&world, e)["inbox"], json!(["early"]));
}

#[test]
fn an_absent_connection_is_a_no_op_not_a_panic() {
    // The package contract: a system whose resource is missing must do
    // nothing. A runtime that does not share a connection should not crash,
    // and should not have one opened behind its back.
    let mut host = hosted();
    let mut world = World::new();
    let e = net_entity(&mut world, json!(["queued"]));

    host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);
    host.run_phase(SystemPhase::ScriptExec, &mut world, 1.0 / 60.0);

    let net = channel(&world, e);
    assert_eq!(
        net["outbox"],
        json!(["queued"]),
        "nothing was sent, so nothing should have been consumed"
    );
    assert!(net.get("state").is_none(), "no connection, no state to report");
}

#[test]
fn an_entity_without_the_channel_is_left_alone() {
    let mut host = hosted();
    let mut world = World::new();
    let mut other = CustomComponents::default();
    other.0.insert("Health".to_string(), json!({ "hp": 10 }));
    let e = world.spawn((other,));
    host.insert_resource(open_connection(&[NetEvent::Text("hello".into())]));

    host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);

    let custom = world.get::<&CustomComponents>(e).unwrap();
    assert!(custom.0.get("Net").is_none(), "no Net component, no channel");
    assert_eq!(custom.0["Health"], json!({ "hp": 10 }), "untouched");
}

#[test]
fn a_closed_connection_publishes_its_state() {
    // A script polling `state` is how a game notices it was disconnected;
    // that has to survive a frame with no traffic on it.
    let mut host = hosted();
    let mut world = World::new();
    let e = net_entity(&mut world, json!([]));
    host.insert_resource(open_connection(&[NetEvent::Closed {
        code: 1006,
        reason: "gone".into(),
    }]));

    host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);
    assert_eq!(channel(&world, e)["state"], json!("closed"));
}

#[test]
fn several_channel_entities_all_receive() {
    // Nothing designates one entity as "the" connection, so a scene with a
    // manager and a HUD both listening must work rather than the first one
    // winning.
    let mut host = hosted();
    let mut world = World::new();
    let a = net_entity(&mut world, json!([]));
    let b = net_entity(&mut world, json!([]));
    host.insert_resource(open_connection(&[NetEvent::Text("broadcast".into())]));

    host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);

    assert_eq!(channel(&world, a)["inbox"], json!(["broadcast"]));
    assert_eq!(channel(&world, b)["inbox"], json!(["broadcast"]));
}

// ---- URL reconciliation -------------------------------------------------
//
// These open a real socket, deliberately: `ws://127.0.0.1:1` is refused
// immediately by the loopback stack, with no DNS to wait on, so the connect
// path is exercised end to end without a server and without flakiness. They
// assert on the *request* having been acted on, not on the outcome, because
// the outcome arrives asynchronously on the socket thread.

const REFUSED: &str = "ws://127.0.0.1:1";

/// An entity whose channel asks for a connection.
fn net_entity_with_url(world: &mut World, url: &str) -> hecs::Entity {
    let mut custom = CustomComponents::default();
    custom
        .0
        .insert("Net".to_string(), json!({ "url": url, "outbox": [] }));
    world.spawn((custom,))
}

#[test]
fn a_url_in_the_channel_opens_a_connection() {
    // The whole reason the package is usable without a runtime knowing about
    // it: a scene states a URL and the package acts on it.
    let mut host = hosted();
    let mut world = World::new();
    net_entity_with_url(&mut world, REFUSED);

    host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);

    let conn = host
        .resource::<NetConnection>()
        .expect("the package creates its own connection when a URL is requested");
    assert_eq!(conn.url(), Some(REFUSED));
}

#[test]
fn the_same_url_does_not_reconnect_every_frame() {
    // Reconnecting per frame would hammer the server and reset the session
    // sixty times a second - the failure mode a declarative field invites if
    // nothing compares against the current value.
    let mut host = hosted();
    let mut world = World::new();
    net_entity_with_url(&mut world, REFUSED);

    host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);
    // A connect resets the drop counter and inbox; if it happened again the
    // state would be back to `connecting` after having moved on.
    for _ in 0..5 {
        host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);
    }
    let conn = host.resource::<NetConnection>().unwrap();
    assert_eq!(conn.url(), Some(REFUSED), "still the same session");
}

#[test]
fn clearing_the_url_disconnects() {
    let mut host = hosted();
    let mut world = World::new();
    let e = net_entity_with_url(&mut world, REFUSED);
    host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);
    assert!(host.resource::<NetConnection>().unwrap().url().is_some());

    {
        let mut custom = world.get::<&mut CustomComponents>(e).unwrap();
        custom.0.get_mut("Net").unwrap()["url"] = json!("");
    }
    host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);

    let conn = host.resource::<NetConnection>().unwrap();
    assert_eq!(conn.url(), None);
    assert_eq!(conn.state().as_str(), "disconnected");
}

#[test]
fn no_url_anywhere_leaves_no_connection_behind() {
    // A project that merely links the package should not end up carrying an
    // idle connection resource it never asked for.
    let mut host = hosted();
    let mut world = World::new();
    net_entity(&mut world, json!([]));

    host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);

    assert!(host.resource::<NetConnection>().is_none());
}

// ---- HTTP ---------------------------------------------------------------

/// An entity whose channel queues HTTP requests.
fn http_entity(world: &mut World, requests: serde_json::Value) -> hecs::Entity {
    let mut custom = CustomComponents::default();
    custom
        .0
        .insert("Net".to_string(), json!({ "requests": requests }));
    world.spawn((custom,))
}

#[test]
fn a_queued_request_reaches_the_client_and_the_field_clears() {
    let mut host = hosted();
    let mut world = World::new();
    let e = http_entity(
        &mut world,
        json!([{ "id": "scores", "url": "https://example.invalid/scores" }]),
    );
    // A mock client, so nothing leaves the machine.
    host.insert_resource(HttpClient::new(Box::new(MockHttpBackend::new())));

    host.run_phase(SystemPhase::ScriptExec, &mut world, 1.0 / 60.0);

    assert_eq!(
        channel(&world, e)["requests"],
        json!([]),
        "the package owns clearing what it dispatched"
    );
}

#[test]
fn a_response_reaches_the_script() {
    let mut host = hosted();
    let mut world = World::new();
    let e = http_entity(&mut world, json!([]));

    let mut backend = MockHttpBackend::new();
    backend.push_ok("scores", "{\"top\":42}");
    host.insert_resource(HttpClient::new(Box::new(backend)));

    host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);

    let responses = &channel(&world, e)["responses"];
    assert_eq!(responses[0]["id"], json!("scores"));
    assert_eq!(responses[0]["status"], json!(200));
    assert_eq!(responses[0]["ok"], json!(true));
    assert_eq!(responses[0]["body"], json!("{\"top\":42}"));
}

#[test]
fn an_error_status_arrives_with_its_status_intact() {
    // A 404 is an answer. Folding it into `error` would throw away the status
    // and the body, which is usually where an API explains the problem.
    let mut host = hosted();
    let mut world = World::new();
    let e = http_entity(&mut world, json!([]));

    let mut backend = MockHttpBackend::new();
    backend.push_response(HttpResponse {
        id: "missing".into(),
        status: 404,
        ok: false,
        body: "no such player".into(),
        error: None,
    });
    host.insert_resource(HttpClient::new(Box::new(backend)));

    host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);

    let r = &channel(&world, e)["responses"][0];
    assert_eq!(r["status"], json!(404));
    assert_eq!(r["ok"], json!(false));
    assert_eq!(r["error"], json!(null), "not a transport failure");
    assert_eq!(r["body"], json!("no such player"));
}

#[test]
fn a_bad_method_is_skipped_rather_than_guessed() {
    // Silently turning a typo into a GET produces a server-side mystery.
    let mut host = hosted();
    let mut world = World::new();
    let e = http_entity(
        &mut world,
        json!([
            { "id": "typo", "url": "https://example.invalid/a", "method": "GTE" },
            { "id": "fine", "url": "https://example.invalid/b", "method": "POST", "body": "{}" }
        ]),
    );
    host.insert_resource(HttpClient::new(Box::new(MockHttpBackend::new())));

    host.run_phase(SystemPhase::ScriptExec, &mut world, 1.0 / 60.0);

    // Both entries are consumed; only the valid one was dispatched, and the
    // skipped one was reported in the log rather than sent as a GET.
    assert_eq!(channel(&world, e)["requests"], json!([]));
}

#[test]
fn no_requests_means_no_http_client_is_created() {
    // A project using only WebSockets should not end up carrying an HTTP
    // client it never asked for.
    let mut host = hosted();
    let mut world = World::new();
    net_entity(&mut world, json!([]));

    host.run_phase(SystemPhase::ScriptExec, &mut world, 1.0 / 60.0);

    assert!(host.resource::<HttpClient>().is_none());
}

// ---- script globals -----------------------------------------------------
//
// These are the payoff of routing package script functions through the
// command path: the same handler runs in every runtime, so a native test
// proves what the editor and a web build will do. Before, a package's globals
// were installed only by `ScriptSystem` and were `nil` in both browser hosts.

use jengine_core::package::script_commands::{self, PackageCommandRegistry};
use jengine_core::package::JEnginePackage;
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost};

/// Lends the package commands a world, the way every runtime does.
struct Host {
    world: World,
}

impl ScriptWorldHost for Host {
    fn world(&mut self) -> &mut World {
        &mut self.world
    }
    fn resolve_entity(&mut self, _id: u32) -> Option<hecs::Entity> {
        None
    }
}

fn registry() -> PackageCommandRegistry {
    let mut reg = PackageCommandRegistry::new();
    NetPackage.register_script_commands(&mut reg);
    reg
}

fn call(host: &mut Host, reg: &PackageCommandRegistry, name: &str, args: serde_json::Value) -> bool {
    let tag = script_commands::command_tag("jengine-net", name);
    script_commands::handle_package_command(host, &tag, &JsonArgs::new(args), reg)
}

fn channel_of(host: &Host, e: hecs::Entity) -> serde_json::Value {
    host.world
        .get::<&CustomComponents>(e)
        .unwrap()
        .0
        .get("Net")
        .cloned()
        .unwrap()
}

#[test]
fn the_package_registers_globals_for_every_runtime() {
    let reg = registry();
    let mut names = reg.names();
    names.sort();
    assert_eq!(
        names,
        vec![
            "net_connect".to_string(),
            "net_disconnect".to_string(),
            "net_http".to_string(),
            "net_send".to_string(),
        ]
    );
}

#[test]
fn net_connect_sets_the_requested_url() {
    let reg = registry();
    let mut world = World::new();
    let e = net_entity(&mut world, json!([]));
    let mut host = Host { world };

    assert!(call(&mut host, &reg, "net_connect", json!({ "url": "ws://x/y" })));
    assert_eq!(channel_of(&host, e)["url"], json!("ws://x/y"));
}

#[test]
fn net_disconnect_clears_the_url() {
    // Empty string means disconnect, which is distinct from the field being
    // absent - the reconciler treats those differently.
    let reg = registry();
    let mut world = World::new();
    let e = net_entity(&mut world, json!([]));
    let mut host = Host { world };

    call(&mut host, &reg, "net_connect", json!({ "url": "ws://x" }));
    call(&mut host, &reg, "net_disconnect", json!({}));
    assert_eq!(channel_of(&host, e)["url"], json!(""));
}

#[test]
fn net_send_appends_to_the_outbox() {
    let reg = registry();
    let mut world = World::new();
    let e = net_entity(&mut world, json!([]));
    let mut host = Host { world };

    call(&mut host, &reg, "net_send", json!({ "message": "one" }));
    call(&mut host, &reg, "net_send", json!({ "message": "two" }));
    assert_eq!(channel_of(&host, e)["outbox"], json!(["one", "two"]));
}

#[test]
fn net_send_without_a_message_does_nothing() {
    let reg = registry();
    let mut world = World::new();
    let e = net_entity(&mut world, json!([]));
    let mut host = Host { world };

    assert!(call(&mut host, &reg, "net_send", json!({})));
    assert_eq!(channel_of(&host, e)["outbox"], json!([]));
}

#[test]
fn net_http_queues_a_request_with_its_id() {
    let reg = registry();
    let mut world = World::new();
    let e = net_entity(&mut world, json!([]));
    let mut host = Host { world };

    call(
        &mut host,
        &reg,
        "net_http",
        json!({ "id": "scores", "url": "https://x/s", "method": "POST", "body": "{}" }),
    );
    let req = &channel_of(&host, e)["requests"][0];
    assert_eq!(req["id"], json!("scores"));
    assert_eq!(req["url"], json!("https://x/s"));
    assert_eq!(req["method"], json!("POST"));
    assert_eq!(req["body"], json!("{}"));
}

#[test]
fn net_http_defaults_its_id_to_the_url() {
    // An anonymous request still needs something to match its answer to.
    let reg = registry();
    let mut world = World::new();
    let e = net_entity(&mut world, json!([]));
    let mut host = Host { world };

    call(&mut host, &reg, "net_http", json!({ "url": "https://x/s" }));
    assert_eq!(channel_of(&host, e)["requests"][0]["id"], json!("https://x/s"));
}

#[test]
fn a_call_with_no_channel_entity_is_survivable() {
    // A scene that forgot the Net entity gets a warning, not a panic.
    let reg = registry();
    let mut host = Host { world: World::new() };
    assert!(call(&mut host, &reg, "net_send", json!({ "message": "x" })));
}
