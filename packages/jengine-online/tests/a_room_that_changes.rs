//! **What a room may change while a player is in it** - and what a dropped
//! connection must remember to come back to it. One small room (`deck`)
//! over real sockets, played by a hosted client as a runtime hosts it:
//!
//! - it takes a code to join, and a reconnect must bring the code again (C11);
//! - its ship has a facing, sent as `Space::set_rotation` and turned on the
//!   slot (C15), and a script's scale on a bound slot stays (C12);
//! - it opens a wall mid-stay with a `netcode` frame, and the prediction
//!   walks through where the wall was (C14);
//! - it replaces the ship with a `you` frame, and the new one is ours (C19),
//!   bound with no trail left from the old.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hecs::{Entity, World};
use jengine_core::ecs::components::{CustomComponents, Name, Parent, Trail, TrailPoint, Transform};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::host::PackageHost;
use jengine_core::package::script_commands::handle_package_command;
use jengine_core::package::system_schedule::SystemPhase;
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost};
use jengine_net::NetPackage;
use jengine_online::{Online, OnlinePackage, OnlineSlot};
use jengine_server::netcode::{Input, InputQueue, Motion, TileMotion, Walls};
use jengine_server::sync::Space;
use jengine_server::{KindOptions, MemoryStore, Player, PlayerId, Players, Room, RoomCtx, Running, Server, Store};
use serde_json::{Value, json};

const STEP: f64 = 0.1;

/// A strip four cells long; the third is a wall until somebody opens it.
struct Deck {
    space: Space,
    field: TileMotion,
    ships: HashMap<PlayerId, (u64, InputQueue)>,
    /// Every join's options, as the room was asked.
    joins: Arc<Mutex<Vec<Value>>>,
    /// Until when the room applies no inputs and answers none - a long
    /// stall on the network, as far as the client can tell.
    stalled_until: f64,
    /// Helm messages heard.
    helms: u32,
}

impl Deck {
    fn new(joins: Arc<Mutex<Vec<Value>>>) -> Self {
        let walls = Walls::from_rows(&["..#."], 100.0, (0.0, 100.0));
        let field = TileMotion { speed: 300.0, radius: 10.0, min: (0.0, 0.0), max: (400.0, 100.0), walls };
        Deck { space: Space::new(1000.0), field, ships: HashMap::new(), joins, stalled_until: 0.0, helms: 0 }
    }

    fn netcode(&self) -> Value {
        let (f, w) = (&self.field, &self.field.walls);
        json!({
            "step": STEP, "speed": f.speed, "radius": f.radius, "min": [f.min.0, f.min.1], "max": [f.max.0, f.max.1],
            "walls": { "cols": w.cols, "rows": w.rows, "cell": w.cell, "origin": [w.origin.0, w.origin.1], "solid": w.packed() },
        })
    }
}

impl Room for Deck {
    fn on_join(&mut self, _ctx: &mut RoomCtx, player: &mut Player, options: &Value) -> Result<Value, String> {
        self.joins.lock().unwrap().push(options.clone());
        if options["code"] != "SEA" {
            return Err("wrong code".into());
        }
        let ship = self.space.spawn(50.0, 50.0, json!({ "kind": "ship" }));
        self.space.set_rotation(ship, 0.5);
        self.space.watch_from(&player.id, ship);
        self.ships.insert(player.id.clone(), (ship, InputQueue::new()));
        Ok(json!({ "you": ship, "netcode": self.netcode() }))
    }

    fn on_message(&mut self, ctx: &mut RoomCtx, player: &mut Player, body: Value) {
        if let Some(blob) = body["blob"].as_str() {
            return ctx.send(&player.id, json!({ "t": "got", "len": blob.len() }));
        }
        if let Some(n) = body["big"].as_u64() {
            return ctx.send(&player.id, json!({ "t": "big", "data": "x".repeat(n as usize) }));
        }
        if body["when"] == true {
            return ctx.send(&player.id, json!({ "t": "when", "delay": ctx.delay() }));
        }
        if let Some(helm) = body["helm"].as_f64() {
            self.helms += 1;
            return ctx.send(&player.id, json!({ "t": "helms", "n": self.helms, "last": helm }));
        }
        let Some((ship, inputs)) = self.ships.get_mut(&player.id) else { return };
        if let Some(seq) = body["input"]["seq"].as_u64() {
            let (x, y) = (body["input"]["x"].as_f64().unwrap_or(0.0), body["input"]["y"].as_f64().unwrap_or(0.0));
            inputs.push(Input { seq: seq as u32, x, y });
        } else if let Some(turn) = body["turn"].as_f64() {
            self.space.set_rotation(*ship, turn);
        } else if body["open"] == true {
            self.field.walls.set_solid(2, 0, false);
            self.stalled_until = ctx.time() + 0.6;
            ctx.broadcast(json!({ "t": "netcode", "netcode": self.netcode() }));
        } else if body["respawn"] == true {
            let at = self.space.position(*ship).unwrap();
            self.space.despawn(*ship);
            *ship = self.space.spawn(at.0, at.1, json!({ "kind": "ship" }));
            self.space.watch_from(&player.id, *ship);
            ctx.send(&player.id, json!({ "t": "you", "you": *ship }));
        }
    }

    fn on_leave(&mut self, _ctx: &mut RoomCtx, player: &mut Player) {
        if let Some((ship, _)) = self.ships.remove(&player.id) {
            self.space.despawn(ship);
        }
        self.space.unwatch(&player.id);
    }

    fn on_tick(&mut self, ctx: &mut RoomCtx, _players: &mut Players, _dt: f64) {
        let stalled = ctx.time() < self.stalled_until;
        for (id, (ship, inputs)) in &mut self.ships {
            if stalled {
                continue;
            }
            let Some(at) = self.space.position(*ship) else { continue };
            if let Some(input) = inputs.take() {
                let to = self.field.step(at, &input, STEP);
                self.space.set_position(*ship, to.0, to.1);
            }
            if inputs.acked() > 0 {
                self.space.ack(id, inputs.acked());
            }
        }
        for (to, frame) in self.space.flush() {
            ctx.send_text(&to, frame);
        }
    }
}

fn deck(addr: &str, store: Arc<dyn Store>, joins: Arc<Mutex<Vec<Value>>>) -> Running {
    Server::new()
        .store(store)
        // Pinged often, so a test need not wait to have its round trip measured.
        .config(jengine_server::Config { ping_secs: 0.1, ..Default::default() })
        .room("deck", KindOptions::ticking(20.0).max_players(10), move |_| Deck::new(joins.clone()))
        // A route of the game's own, answering who the caller is.
        .get("/api/whoami", |_req, api| Ok(json!({ "you": api.caller()? })))
        .post("/api/echo", |req, _api| Ok(json!({ "got": req.body })))
        .start(addr)
        .unwrap()
}

struct TestHost<'a>(&'a mut World);

impl ScriptWorldHost for TestHost<'_> {
    fn world(&mut self) -> &mut World {
        self.0
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
    }
}

struct Game {
    host: PackageHost,
    world: World,
    conn: Entity,
    slots: Vec<Entity>,
    /// A child of each slot carrying a trail - a ship's wake.
    wakes: Vec<Entity>,
    /// A child of each slot named `Hull`, for a slot that turns it.
    hulls: Vec<Entity>,
    /// Every event the scripts were handed.
    heard: Vec<Value>,
    last: Instant,
}

impl Game {
    fn new(server: &str) -> Self {
        Self::turning(server, "")
    }

    /// Slots whose `turn` is `turn`.
    fn turning(server: &str, turn: &str) -> Self {
        let mut registry = ComponentRegistry::new();
        jengine_core::package::core_registration::register_core_components(&mut registry);
        let mut host = PackageHost::new();
        host.add_package(Box::new(NetPackage));
        host.add_package(Box::new(OnlinePackage));
        host.initialize(&mut registry);
        let mut world = World::new();
        // No room on the component: the script joins with a code.
        let online: Online =
            serde_json::from_value(json!({ "server": server, "name": "Ada", "retry_secs": 0.2 })).unwrap();
        let conn = world.spawn((online,));
        let (mut slots, mut wakes, mut hulls) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..2 {
            let slot: OnlineSlot = serde_json::from_value(json!({ "kind": "ship", "turn": turn })).unwrap();
            let e = world.spawn((Transform { scale: glam::Vec2::new(2.0, 2.0), ..Default::default() }, slot));
            wakes.push(world.spawn((Transform::default(), Parent(e), Trail::default())));
            // Under the wake, so "under the slot" is more than its children.
            hulls.push(world.spawn((Transform::default(), Parent(*wakes.last().unwrap()), Name("Hull".into()))));
            slots.push(e);
        }
        Game { host, world, conn, slots, wakes, hulls, heard: Vec::new(), last: Instant::now() }
    }

    fn command(&mut self, name: &str, mut args: Value) {
        let tag = format!("pkg/jengine-online/{name}");
        args["e"] = json!(self.conn.id());
        assert!(handle_package_command(&mut TestHost(&mut self.world), &tag, &JsonArgs::new(args), self.host.script_commands()));
    }

    fn frame(&mut self) {
        let dt = self.last.elapsed().as_secs_f32();
        self.last = Instant::now();
        self.host.run_phase(SystemPhase::PreUpdate, &mut self.world, dt);
        self.host.run_phase(SystemPhase::ScriptExec, &mut self.world, dt);
        if let Some(events) = self.bag()["events"].as_array() {
            self.heard.extend(events.iter().cloned());
        }
    }

    fn bag(&self) -> Value {
        self.world.get::<&CustomComponents>(self.conn).ok().and_then(|c| c.0.get("online").cloned()).unwrap_or(Value::Null)
    }

    /// Our own ship's slot, and its `net_entity` bag.
    fn mine(&self) -> Option<(Entity, Value)> {
        self.slots.iter().find_map(|e| {
            let custom = self.world.get::<&CustomComponents>(*e).ok()?;
            let bag = custom.0.get("net_entity")?.clone();
            (bag["mine"] == true).then_some((*e, bag))
        })
    }

    fn transform(&self, e: Entity) -> Transform {
        (*self.world.get::<&Transform>(e).unwrap()).clone()
    }

    fn until(&mut self, what: &str, done: impl Fn(&Game) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !done(self) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}: {}", self.bag());
            self.frame();
            std::thread::sleep(Duration::from_millis(4));
        }
    }
}

fn joined_with_a_ship(g: &Game) -> bool {
    g.bag()["status"] == "joined" && g.mine().is_some()
}

/// C11: the room wants a code. A reconnect used to rejoin with no options at
/// all, and was turned away from its own room.
#[test]
fn a_reconnect_brings_the_options_it_joined_with() {
    let (store, joins): (Arc<dyn Store>, _) = (Arc::new(MemoryStore::new()), Arc::new(Mutex::new(Vec::new())));
    let server = deck("127.0.0.1:0", store.clone(), joins.clone());
    let addr = server.addr.to_string();
    let mut ada = Game::new(&server.url());
    ada.command("online_join", json!({ "kind": "deck", "room": "harbour", "options": { "code": "SEA" } }));
    ada.until("the deck", joined_with_a_ship);

    server.stop();
    ada.until("the drop", |g| g.bag()["status"] == "offline");
    let server = deck(&addr, store, joins.clone());
    ada.until("to be back on deck", joined_with_a_ship);
    let asked = joins.lock().unwrap().clone();
    assert_eq!(asked, vec![json!({ "code": "SEA" }); 2], "both joins brought the code");
    assert_eq!(ada.bag()["room"]["id"], "harbour");
    server.stop();
}

/// C15 and C12: the room turns the ship and the slot turns with it, walked
/// between ticks; a script's scale on the slot is left alone; and a slot
/// handed to a new ship has no wake from the old one.
#[test]
fn a_slot_turns_with_its_entity_keeps_its_scale_and_loses_its_wake() {
    let server = deck("127.0.0.1:0", Arc::new(MemoryStore::new()), Arc::new(Mutex::new(Vec::new())));
    let mut ada = Game::new(&server.url());
    ada.command("online_join", json!({ "kind": "deck", "room": "harbour", "options": { "code": "SEA" } }));
    ada.until("the deck", joined_with_a_ship);
    let (slot, bag) = ada.mine().unwrap();
    assert!((ada.transform(slot).rotation - 0.5).abs() < 1e-3, "it entered facing 0.5");
    assert!((bag["rot"].as_f64().unwrap() - 0.5).abs() < 1e-3, "and in the bag, walked, for a script that turns something itself");
    assert_eq!(ada.transform(slot).scale.x, 2.0, "bound with its authored scale");

    // A script grows it; the package must not put it back.
    ada.world.get::<&mut Transform>(slot).unwrap().scale = glam::Vec2::splat(3.0);
    ada.command("online_send", json!({ "body": { "turn": 2.0 } }));
    ada.until("the turn", |g| (g.transform(slot).rotation - 2.0).abs() < 1e-3);
    assert_eq!(ada.transform(slot).scale.x, 3.0, "the script's scale stays");

    // A wake on every slot - whichever the new ship takes - then a new ship.
    for wake in ada.wakes.clone() {
        ada.world.get::<&mut Trail>(wake).unwrap().points = vec![TrailPoint { position: glam::Vec2::ZERO, age: 0.0 }; 5];
    }
    let old = bag["id"].clone();
    ada.command("online_send", json!({ "body": { "respawn": true } }));
    ada.until("the new ship", |g| g.mine().is_some_and(|(_, b)| b["id"] != old));
    let (slot, _) = ada.mine().unwrap();
    let wake = ada.wakes[ada.slots.iter().position(|e| *e == slot).unwrap()];
    assert!(ada.world.get::<&Trail>(wake).unwrap().points.is_empty(), "no wake drawn from the old ship");
    assert_eq!(ada.transform(slot).scale.x, 2.0, "a new binding is shown at its authored scale");
    server.stop();
}

/// A slot's `turn` names what the facing turns - here a hull two levels
/// down, leaving the slot (and a label on it) level - and the facing is
/// written before the scripts, so a script turning it after the package
/// has the last word for the frame (a ship's bow answering the helm).
#[test]
fn a_slot_turns_the_child_it_names_and_a_script_may_turn_it_after() {
    let server = deck("127.0.0.1:0", Arc::new(MemoryStore::new()), Arc::new(Mutex::new(Vec::new())));
    let mut ada = Game::turning(&server.url(), "Hull");
    ada.command("online_join", json!({ "kind": "deck", "room": "harbour", "options": { "code": "SEA" } }));
    ada.until("the deck", joined_with_a_ship);
    let (slot, bag) = ada.mine().unwrap();
    let hull = ada.hulls[ada.slots.iter().position(|e| *e == slot).unwrap()];
    ada.until("the hull turned", |g| (g.transform(hull).rotation - 0.5).abs() < 1e-3);
    assert_eq!(ada.transform(slot).rotation, 0.0, "the slot itself stays level");
    assert!((bag["rot"].as_f64().unwrap() - 0.5).abs() < 1e-3, "and the facing is in the bag: {bag}");

    // A frame as a runtime runs it, with a script between the halves.
    let dt = 0.016;
    ada.host.run_phase(SystemPhase::PreUpdate, &mut ada.world, dt);
    ada.world.get::<&mut Transform>(hull).unwrap().rotation = 1.25;
    ada.host.run_phase(SystemPhase::ScriptExec, &mut ada.world, dt);
    assert_eq!(ada.transform(hull).rotation, 1.25, "the script's turn stands for the frame");
    ada.frame();
    assert!((ada.transform(hull).rotation - 0.5).abs() < 1e-3, "and the room's is back the next");
    server.stop();
}

/// S17 and S21: a message over the server's 64 KB frame goes up in parts and
/// arrives whole; one the room sends down is not limited at all; and a value
/// sawed back and forth at the frame rate - a helm - reaches the room at
/// the rate asked, the last one last, never told to slow down.
#[test]
fn big_messages_go_both_ways_and_the_latest_goes_at_a_rate() {
    let server = deck("127.0.0.1:0", Arc::new(MemoryStore::new()), Arc::new(Mutex::new(Vec::new())));
    let mut ada = Game::new(&server.url());
    ada.command("online_join", json!({ "kind": "deck", "room": "harbour", "options": { "code": "SEA" } }));
    ada.until("the deck", joined_with_a_ship);

    // Quotes in it, so the parts' escaping is in play too.
    let blob: String = "a ghost \"lap\" ".repeat(20_000);
    ada.command("online_send", json!({ "body": { "blob": blob } }));
    ada.until("the blob, whole", |g| g.heard.iter().any(|e| e["t"] == "got"));
    let got = ada.heard.iter().find(|e| e["t"] == "got").unwrap().clone();
    assert_eq!(got["len"].as_u64(), Some(blob.len() as u64), "{got}");

    ada.command("online_send", json!({ "body": { "big": 500_000 } }));
    ada.until("a big frame down", |g| g.heard.iter().any(|e| e["t"] == "big"));
    let big = ada.heard.iter().find(|e| e["t"] == "big").unwrap();
    assert_eq!(big["data"].as_str().map(str::len), Some(500_000));

    // A second of a helm sawed every frame, at ten a second.
    let start = Instant::now();
    let mut v = 0.0;
    while start.elapsed() < Duration::from_secs(1) {
        v = if v > 0.0 { -1.0 } else { 1.0 };
        ada.command("online_send_latest", json!({ "key": "helm", "body": { "helm": v }, "per_second": 10 }));
        ada.frame();
        std::thread::sleep(Duration::from_millis(4));
    }
    ada.until("the last helm", |g| g.heard.iter().rev().find(|e| e["t"] == "helms").is_some_and(|e| e["last"] == v));
    let n = ada.heard.iter().rev().find(|e| e["t"] == "helms").unwrap()["n"].as_u64().unwrap();
    assert!((9..=13).contains(&n), "ten a second for a second, give or take: {n}");
    assert!(ada.heard.iter().all(|e| e["why"] != "slow down"), "never told to slow down");
    server.stop();
}

/// C13: a script reaches the game server's own HTTP routes as its player -
/// the session's token is the bearer - and the answer is an event.
#[test]
fn a_script_fetches_from_the_servers_routes_as_its_player() {
    let server = deck("127.0.0.1:0", Arc::new(MemoryStore::new()), Arc::new(Mutex::new(Vec::new())));
    let mut ada = Game::new(&server.url());
    ada.command("online_join", json!({ "kind": "deck", "room": "harbour", "options": { "code": "SEA" } }));
    ada.until("the deck", joined_with_a_ship);
    ada.command("online_fetch", json!({ "id": "me", "path": "/api/whoami" }));
    ada.command("online_fetch", json!({ "id": "echo", "path": "api/echo", "body": { "lap": 61.5 } }));
    ada.until("both answers", |g| g.heard.iter().filter(|e| e["t"] == "fetched").count() == 2);
    let answer = |id: &str| ada.heard.iter().find(|e| e["t"] == "fetched" && e["id"] == id).unwrap().clone();
    let me = answer("me");
    assert_eq!((me["status"].as_u64(), me["ok"].as_bool()), (Some(200), Some(true)), "{me}");
    assert_eq!(me["body"]["you"], ada.bag()["player"], "signed in as herself");
    assert_eq!(answer("echo")["body"]["got"]["lap"], 61.5, "a table is a JSON POST");
    server.stop();
}

/// S19: the server measures the round trip of a client that answers its
/// pings - which `jengine-online` does, unseen by the scripts - and a room
/// is told how long ago each message was sent.
#[test]
fn a_message_says_how_long_ago_it_was_sent() {
    let server = deck("127.0.0.1:0", Arc::new(MemoryStore::new()), Arc::new(Mutex::new(Vec::new())));
    let mut ada = Game::new(&server.url());
    ada.command("online_join", json!({ "kind": "deck", "room": "harbour", "options": { "code": "SEA" } }));
    ada.until("the deck", joined_with_a_ship);
    let until = Instant::now() + Duration::from_millis(600);
    while Instant::now() < until {
        ada.frame();
        std::thread::sleep(Duration::from_millis(4));
    }
    assert!(ada.heard.iter().all(|e| e["t"] != "ping"), "a ping is the package's to answer, not a script's");
    ada.command("online_send", json!({ "body": { "when": true } }));
    ada.until("the answer", |g| g.heard.iter().any(|e| e["t"] == "when"));
    let delay = ada.heard.iter().find(|e| e["t"] == "when").unwrap()["delay"].as_f64().unwrap();
    assert!(delay > 0.0 && delay < 0.1, "half a loopback round trip and a frame: {delay}");
    server.stop();
}

/// C14 and C19: steer into the wall, open it mid-stay, steer through; then
/// the ship is replaced and the new one is ours, and predicted.
#[test]
fn a_wall_opened_mid_stay_and_a_new_avatar_are_predicted() {
    let server = deck("127.0.0.1:0", Arc::new(MemoryStore::new()), Arc::new(Mutex::new(Vec::new())));
    let mut ada = Game::new(&server.url());
    ada.command("online_join", json!({ "kind": "deck", "room": "harbour", "options": { "code": "SEA" } }));
    ada.until("the deck", joined_with_a_ship);
    let at = |g: &Game| {
        let (_, b) = g.mine().unwrap();
        (b["server_x"].as_f64().unwrap(), g.transform(g.mine().unwrap().0).position.x as f64)
    };

    ada.command("online_input", json!({ "x": 1, "y": 0 }));
    ada.until("the wall", |g| at(g).0 == 190.0);
    ada.command("online_send", json!({ "body": { "open": true } }));
    // Through where the wall was. The room opens it and then says nothing
    // for a while, so what the player sees is the prediction alone: by the
    // new walls it walks on through; by the old ones it stands at the face.
    ada.until("well past where the wall was, on the prediction alone", |g| {
        let (server, drawn) = at(g);
        assert_eq!(server, 190.0, "the server has not moved her yet");
        drawn > 260.0
    });
    // And the server, when it answers, agrees.
    ada.until("the server through too", |g| at(g).0 > 300.0);
    ada.command("online_input", json!({ "x": 0, "y": 0 }));
    ada.until("at rest", |g| g.bag()["predicting"] == false);

    // A new ship: the `you` frame makes it ours.
    let old = ada.mine().unwrap().1["id"].clone();
    ada.command("online_send", json!({ "body": { "respawn": true } }));
    ada.until("the new ship to be ours", |g| g.mine().is_some_and(|(_, b)| b["id"] != old));
    assert_eq!(ada.bag()["you"], ada.mine().unwrap().1["id"]);
    // Steering it is predicted: drawn ahead of the server's word.
    ada.command("online_input", json!({ "x": -1, "y": 0 }));
    ada.until("the new ship predicted", |g| {
        let (server, drawn) = at(g);
        g.bag()["predicting"] == true && drawn < server - 1.0
    });
    server.stop();
}
