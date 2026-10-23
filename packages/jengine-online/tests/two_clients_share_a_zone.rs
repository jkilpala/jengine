//! **Two engine clients in one server zone** - each its own world and its own
//! `PackageHost` running `jengine-net` and `jengine-online`, exactly as a
//! runtime hosts them, against a real `jengine-server` running the `zones`
//! example over real sockets.
//!
//! Nothing reaches past the packages' public surface: input goes in through
//! `online_send` - the command a script calls - and what is checked is what a
//! game would draw and read: where the slot entities are, and the `online`
//! and `net_entity` bags.

#[path = "../../../crates/jengine-server/examples/zones/world.rs"]
mod world;

use std::sync::Arc;
use std::time::{Duration, Instant};

use hecs::{Entity, World};
use jengine_core::ecs::components::{CustomComponents, Transform};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::host::PackageHost;
use jengine_core::package::script_commands::handle_package_command;
use jengine_core::package::system_schedule::SystemPhase;
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost};
use jengine_net::NetPackage;
use jengine_online::{Online, OnlinePackage, OnlineSlot};
use jengine_server::{ClusterConfig, KindOptions, MemoryStore, Running, Server, Store};
use serde_json::{Value, json};

fn zones(addr: &str, store: Arc<dyn Store>) -> Running {
    Server::new()
        .store(store)
        .room("zone", KindOptions::ticking(20.0).max_players(100).persistent(), world::Zone::new)
        .open_channels()
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

/// A game: one connection entity and a pool of slots.
struct Game {
    host: PackageHost,
    world: World,
    conn: Entity,
    slots: Vec<Entity>,
    heard: Vec<Value>,
    last: Instant,
}

impl Game {
    fn new(server: &str, name: &str, players: usize, coins: usize) -> Self {
        Self::signed_in_as(server, name, "", players, coins)
    }

    /// A game whose save already holds `token` ("" for none).
    fn signed_in_as(server: &str, name: &str, token: &str, players: usize, coins: usize) -> Self {
        let mut registry = ComponentRegistry::new();
        jengine_core::package::core_registration::register_core_components(&mut registry);
        let mut host = PackageHost::new();
        host.add_package(Box::new(NetPackage));
        host.add_package(Box::new(OnlinePackage));
        host.initialize(&mut registry);
        let mut world = World::new();
        // As a scene would author it: the fields, and nothing else.
        let online: Online = serde_json::from_value(json!({
            "server": server, "name": name, "token": token, "kind": "zone", "room": "meadow", "retry_secs": 0.2,
        }))
        .unwrap();
        let conn = world.spawn((online,));
        let slot = |kind: &str| {
            let authored = Transform { scale: glam_vec(2.0, 2.0), ..Default::default() };
            let slot: OnlineSlot = serde_json::from_value(json!({ "kind": kind })).unwrap();
            (authored, slot)
        };
        let mut slots = Vec::new();
        for _ in 0..players {
            slots.push(world.spawn(slot("player")));
        }
        for _ in 0..coins {
            slots.push(world.spawn(slot("coin")));
        }
        // Enough for every wall and monster a zone has in view.
        for _ in 0..30 {
            slots.push(world.spawn(slot("wall")));
        }
        for _ in 0..3 {
            slots.push(world.spawn(slot("monster")));
        }
        Game { host, world, conn, slots, heard: Vec::new(), last: Instant::now() }
    }

    fn command(&mut self, name: &str, mut args: Value) {
        let tag = format!("pkg/jengine-online/{name}");
        args["e"] = json!(self.conn.id());
        assert!(
            handle_package_command(&mut TestHost(&mut self.world), &tag, &JsonArgs::new(args), self.host.script_commands()),
            "{tag} is not registered"
        );
    }

    /// A runtime frame: receive, then the script phase (this package, then
    /// the network send).
    fn frame(&mut self) {
        let dt = self.last.elapsed().as_secs_f32();
        self.last = Instant::now();
        self.host.run_phase(SystemPhase::PreUpdate, &mut self.world, dt);
        self.host.run_phase(SystemPhase::ScriptExec, &mut self.world, dt);
        if let Some(events) = self.bag()["events"].as_array() {
            self.heard.extend(events.iter().cloned());
        }
    }

    /// The `online` bag a script reads.
    fn bag(&self) -> Value {
        self.world.get::<&CustomComponents>(self.conn).ok().and_then(|c| c.0.get("online").cloned()).unwrap_or(Value::Null)
    }

    /// What is on screen: each bound slot's `net_entity` bag, with where the
    /// slot entity is and how big.
    fn shown(&self) -> Vec<Value> {
        self.slots
            .iter()
            .filter_map(|e| {
                let custom = self.world.get::<&CustomComponents>(*e).ok()?;
                let mut bag = custom.0.get("net_entity")?.clone();
                let t = self.world.get::<&Transform>(*e).ok()?;
                bag["at"] = json!([t.position.x, t.position.y]);
                bag["scale"] = json!(t.scale.x);
                Some(bag)
            })
            .collect()
    }

    fn of_kind(&self, kind: &str) -> Vec<Value> {
        self.shown().into_iter().filter(|b| b["kind"] == kind).collect()
    }

    /// Slots showing nothing must be invisible.
    fn hidden_slots(&self) -> usize {
        self.slots
            .iter()
            .filter(|e| self.world.get::<&Transform>(**e).is_ok_and(|t| t.scale.x == 0.0 && t.scale.y == 0.0))
            .count()
    }
}

fn glam_vec(x: f32, y: f32) -> glam::Vec2 {
    glam::Vec2::new(x, y)
}

fn frames_until(games: &mut [&mut Game], what: &str, done: impl Fn(&[&mut Game]) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        for game in games.iter_mut() {
            game.frame();
        }
        if done(games) {
            return;
        }
        let bags: Vec<String> = games
            .iter()
            .map(|g| {
                let kinds: Vec<String> = g.shown().iter().map(|b| b["kind"].to_string()).collect();
                format!("{} showing {} {kinds:?} overflow {}", g.bag()["status"], g.shown().len(), g.bag()["overflow"])
            })
            .collect();
        assert!(Instant::now() < deadline, "timed out waiting for {what}: {bags:?}");
        std::thread::sleep(Duration::from_millis(4));
    }
}

/// Two connections in one world - a split screen - each with slots of its
/// own, named for it: each pool shows that session's view, with that
/// session's own avatar marked `mine`.
/// What the server sends is a script's on the frame it arrives: published by
/// the receiving half (`PreUpdate`, right after the socket is read), before
/// the scripts run - and the sending half after them leaves it be. Until Oct
/// 2026 one system did both after the scripts, so every answer reached a
/// script a frame late (the three MMOs measured it on every action).
#[test]
fn a_frame_is_published_before_the_scripts_of_the_frame_it_arrives() {
    let server = zones("127.0.0.1:0", Arc::new(MemoryStore::new()));
    let mut ada = Game::new(&server.url(), "Ada", 4, 4);
    let mut before_scripts: Vec<String> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !before_scripts.iter().any(|t| t == "joined") {
        assert!(Instant::now() < deadline, "never joined; heard before the scripts: {before_scripts:?}");
        let dt = ada.last.elapsed().as_secs_f32();
        ada.last = Instant::now();
        ada.host.run_phase(SystemPhase::PreUpdate, &mut ada.world, dt);
        let after_receive = ada.bag()["events"].clone();
        // (The scripts would run here, reading `after_receive`.)
        ada.host.run_phase(SystemPhase::ScriptExec, &mut ada.world, dt);
        assert_eq!(ada.bag()["events"], after_receive, "the sending half must not replace what the scripts read");
        for ev in after_receive.as_array().into_iter().flatten() {
            before_scripts.push(ev["t"].as_str().unwrap_or("").to_string());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(before_scripts.iter().any(|t| t == "welcome"), "{before_scripts:?}");
}

#[test]
fn two_sessions_in_one_world_each_have_their_own_slots() {
    use jengine_core::ecs::components::Name;
    let server = zones("127.0.0.1:0", Arc::new(MemoryStore::new()));
    let mut registry = ComponentRegistry::new();
    jengine_core::package::core_registration::register_core_components(&mut registry);
    let mut host = PackageHost::new();
    host.add_package(Box::new(NetPackage));
    host.add_package(Box::new(OnlinePackage));
    host.initialize(&mut registry);
    let mut world = World::new();
    let online = |name: &str| -> Online {
        serde_json::from_value(json!({ "server": server.url(), "name": name, "kind": "zone", "room": "meadow" })).unwrap()
    };
    let ada = world.spawn((online("Ada"), Name("Ada".into())));
    let bo = world.spawn((online("Bo"), Name("Bo".into())));
    let slot = |session: &str| -> (Transform, OnlineSlot) {
        let slot: OnlineSlot = serde_json::from_value(json!({ "kind": "player", "session": session })).unwrap();
        (Transform::default(), slot)
    };
    // Two player slots each; Bo's name his, Ada's unnamed (the first session).
    let ada_slots = [world.spawn(slot("")), world.spawn(slot(""))];
    let bo_slots = [world.spawn(slot("Bo")), world.spawn(slot("Bo"))];
    let bag = |world: &World, e: Entity, key: &str| -> Value {
        world.get::<&CustomComponents>(e).ok().and_then(|c| c.0.get(key).cloned()).unwrap_or(Value::Null)
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut last = Instant::now();
    loop {
        let dt = last.elapsed().as_secs_f32();
        last = Instant::now();
        host.run_phase(SystemPhase::PreUpdate, &mut world, dt);
        host.run_phase(SystemPhase::ScriptExec, &mut world, dt);
        let shown = |slots: &[Entity]| slots.iter().filter(|e| bag(&world, **e, "net_entity").is_object()).count();
        if shown(&ada_slots) == 2 && shown(&bo_slots) == 2 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "both pools filled: {} {} - {} / {}",
            shown(&ada_slots),
            shown(&bo_slots),
            bag(&world, ada, "online"),
            bag(&world, bo, "online")
        );
        std::thread::sleep(Duration::from_millis(4));
    }
    let mine_in = |slots: &[Entity]| -> Value {
        slots.iter().map(|e| bag(&world, *e, "net_entity")).find(|b| b["mine"] == true).map(|b| b["id"].clone()).unwrap()
    };
    assert_eq!(mine_in(&ada_slots), bag(&world, ada, "online")["you"], "Ada's pool marks Ada as mine");
    assert_eq!(mine_in(&bo_slots), bag(&world, bo, "online")["you"], "and Bo's marks Bo");
    assert_ne!(mine_in(&ada_slots), mine_in(&bo_slots));
    server.stop();
}

#[test]
fn two_games_see_each_other_and_a_door_leads_to_another_room() {
    let server = zones("127.0.0.1:0", Arc::new(MemoryStore::new()));
    let mut ada = Game::new(&server.url(), "Ada", 4, 4);
    let mut bo = Game::new(&server.url(), "Bo", 4, 4);

    // Authoring the room was enough: no command was needed to get here.
    frames_until(&mut [&mut ada, &mut bo], "both in the meadow, seeing both players and its coins", |g| {
        g.iter().all(|g| g.bag()["status"] == "joined" && g.of_kind("player").len() == 2 && g.of_kind("coin").len() == 3)
    });
    assert_eq!(ada.bag()["room"], json!({ "kind": "zone", "id": "meadow" }));
    assert!(ada.bag()["token"].as_str().is_some_and(|t| !t.is_empty()), "a script can save who the player is");
    assert_eq!(ada.hidden_slots(), ada.slots.len() - ada.shown().len(), "a slot showing nothing is scaled to nothing");
    assert!(!ada.of_kind("wall").is_empty(), "the walls near the start are in view (the monsters start out of it)");
    assert!(ada.shown().iter().all(|b| b["scale"] == 2.0), "and a bound one has the scale it was authored with");

    // Each game knows which entity is its own player.
    let mine: Vec<Value> = ada.of_kind("player").into_iter().filter(|b| b["mine"] == true).collect();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0]["name"], "Ada");
    let ada_id = mine[0]["id"].clone();
    assert_eq!(ada.bag()["you"], ada_id);

    // Ada walks; Bo's game shows her moving - smoothly, not in the server's steps.
    ada.command("online_send", json!({ "body": { "go": [500, 100] } }));
    let mut seen_at = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        ada.frame();
        bo.frame();
        let her = bo.of_kind("player").into_iter().find(|b| b["id"] == ada_id).expect("Bo still sees Ada");
        let y = her["at"][1].as_f64().unwrap();
        if seen_at.last() != Some(&y) {
            seen_at.push(y);
        }
        if y == 100.0 {
            break;
        }
        assert!(Instant::now() < deadline, "she never arrived: {seen_at:?}");
        std::thread::sleep(Duration::from_millis(4));
    }
    // 200 units at 300 a second is about thirteen server ticks; a game that
    // drew only those would have shown about thirteen positions.
    assert!(seen_at.len() > 30, "walked between ticks: {} positions shown", seen_at.len());
    assert!(seen_at.windows(2).all(|w| w[1] <= w[0]), "and never backwards: {seen_at:?}");

    // Through the door. Nothing arrives but a new `joined`, and the game is
    // in another room: no Bo, and of the cave's two coins only the one near
    // the door - the other is out of view, so the server never mentions it.
    // Over a coin first: a frame of the room's own, which the package does
    // not know, reaches the script as an event.
    ada.command("online_send", json!({ "body": { "go": [560, 300] } }));
    frames_until(&mut [&mut ada, &mut bo], "the coin", |g| {
        g[0].heard.iter().any(|e| e["t"] == "coins" && e["coins"] == 1.0)
    });

    ada.command("online_send", json!({ "body": { "go": [1010, 300] } }));
    frames_until(&mut [&mut ada, &mut bo], "Ada in the cave and gone from Bo's meadow", |g| {
        g[0].bag()["room"]["id"] == "cave"
            && g[0].of_kind("coin").len() == 1
            && g[0].of_kind("player").len() == 1
            && g[1].of_kind("player").len() == 1
    });
    assert_eq!(ada.bag()["joined"]["coins"], 1.0, "and the coin went through the door with her");
    assert_eq!(bo.of_kind("player")[0]["name"], "Bo");
    server.stop();
}

#[test]
fn a_dropped_connection_comes_back_as_the_same_player_in_the_same_room() {
    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let server = zones("127.0.0.1:0", store.clone());
    let addr = server.addr.to_string();
    let mut ada = Game::new(&server.url(), "Ada", 2, 4);
    frames_until(&mut [&mut ada], "the meadow", |g| g[0].bag()["status"] == "joined" && !g[0].of_kind("coin").is_empty());
    let player = ada.bag()["player"].clone();
    ada.command("online_send", json!({ "body": { "go": [560, 300] } }));
    frames_until(&mut [&mut ada], "a coin", |g| g[0].heard.iter().any(|e| e["t"] == "coins"));

    // She is in a channel, which belongs to the connection she is about to lose.
    ada.command("online_subscribe", json!({ "channel": "guild:7" }));
    frames_until(&mut [&mut ada], "to be in the channel", |g| g[0].heard.iter().any(|e| e["t"] == "subbed"));

    // The server goes away under her.
    server.stop();
    frames_until(&mut [&mut ada], "the game to notice", |g| g[0].bag()["status"] == "offline");
    assert!(ada.shown().is_empty() && ada.hidden_slots() == ada.slots.len(), "nothing from the old room is left on screen");
    // Raised by the sending half, so published by the next frame's receiving one.
    frames_until(&mut [&mut ada], "to hear it dropped", |g| g[0].heard.iter().any(|e| e["t"] == "dropped"));

    // And comes back. No command: the package retries, signs in with the
    // token it was given, and goes back to the room it was in.
    let server = zones(&addr, store);
    frames_until(&mut [&mut ada], "to be back in the meadow", |g| {
        g[0].bag()["status"] == "joined" && g[0].of_kind("player").len() == 1
    });
    assert_eq!(ada.bag()["player"], player, "the same player");
    assert_eq!(ada.bag()["joined"]["coins"], 1.0, "with what she had");
    assert_eq!(ada.bag()["room"]["id"], "meadow");
    // And in her channel again, without the script asking.
    ada.heard.clear();
    ada.command("online_say", json!({ "channel": "guild:7", "body": "still here" }));
    frames_until(&mut [&mut ada], "to hear herself in the channel", |g| {
        g[0].heard.iter().any(|e| e["t"] == "said" && e["body"] == "still here")
    });
    server.stop();
}

/// Two windows signed in as one player - two browser tabs share a save, so
/// do two copies run without `--instances` - and the server keeps one
/// connection per player. The window pushed off used to take that for a
/// dropped connection: it reconnected, which pushed the other off, which
/// reconnected - each showing "Connection lost" while it played on (Game
/// Night's Hill Gunners, Oct 2026). The one replaced stays off and says so.
#[test]
fn a_window_replaced_by_another_stays_off_instead_of_taking_the_player_back() {
    let server = zones("127.0.0.1:0", Arc::new(MemoryStore::new()));
    let mut first = Game::new(&server.url(), "Ada", 2, 4);
    frames_until(&mut [&mut first], "the meadow", |g| g[0].bag()["status"] == "joined");
    let token = first.bag()["token"].as_str().unwrap().to_string();

    let mut second = Game::signed_in_as(&server.url(), "Ada", &token, 2, 4);
    frames_until(&mut [&mut first, &mut second], "the second window to take the player", |g| {
        g[1].bag()["status"] == "joined" && g[0].heard.iter().any(|e| e["t"] == "replaced")
    });
    assert_eq!(second.bag()["player"], first.bag()["player"], "one player");

    // Two seconds - ten of the first window's retry intervals - and neither
    // moves: the first stays off, the second plays on undisturbed.
    let until = Instant::now() + Duration::from_secs(2);
    while Instant::now() < until {
        first.frame();
        second.frame();
        std::thread::sleep(Duration::from_millis(4));
    }
    assert_eq!(first.bag()["status"], "offline");
    assert!(!first.heard.iter().any(|e| e["t"] == "dropped"), "a replaced window is not a dropped one: {:?}", first.heard);
    assert_eq!(second.bag()["status"], "joined");
    assert!(
        !second.heard.iter().any(|e| e["t"] == "dropped" || e["t"] == "error"),
        "the window that took over was pushed off again: {:?}",
        second.heard
    );

    // Asked to, the first comes back - and takes the player, as asked.
    first.command("online_connect", json!({ "server": "", "name": "Ada", "token": token }));
    first.command("online_join", json!({ "kind": "zone", "room": "meadow" }));
    frames_until(&mut [&mut first, &mut second], "the first window to come back when asked", |g| {
        g[0].bag()["status"] == "joined" && g[1].heard.iter().any(|e| e["t"] == "replaced")
    });
    server.stop();
}

/// "Sign in again" - a disconnect and a connect in one frame, with a new
/// name - is a new socket and a new `hello`, and the server takes the new
/// name. It used to be neither: `jengine-net` saw the URL go "" and back
/// within the frame, kept the old socket, and a second `hello` went down it,
/// which the server refuses ("already signed in"). Found by a
/// hub of online games, whose CONNECT button does exactly this.
#[test]
fn a_disconnect_and_a_connect_in_one_frame_sign_in_again() {
    let server = zones("127.0.0.1:0", Arc::new(MemoryStore::new()));
    let mut ada = Game::new(&server.url(), "Ada", 2, 4);
    frames_until(&mut [&mut ada], "the meadow", |g| g[0].bag()["status"] == "joined");
    let player = ada.bag()["player"].clone();
    let token = ada.bag()["token"].as_str().unwrap().to_string();
    ada.heard.clear();
    ada.command("online_disconnect", json!({}));
    ada.command("online_connect", json!({ "server": "", "name": "Adelaide", "token": token }));
    ada.command("online_join", json!({ "kind": "zone", "room": "meadow" }));
    frames_until(&mut [&mut ada], "to be signed in again and back in the meadow", |g| {
        g[0].bag()["status"] == "joined" && g[0].heard.iter().any(|e| e["t"] == "welcome")
    });
    assert!(!ada.heard.iter().any(|e| e["t"] == "error"), "refused: {:?}", ada.heard);
    // A disconnect the script asked for is not a dropped connection: the
    // hub's CONNECT button flashed "Connection lost" every time until the
    // package stopped sending a `hello` down the socket it was closing.
    assert!(!ada.heard.iter().any(|e| e["t"] == "dropped"), "a sign-in again read as a drop: {:?}", ada.heard);
    assert_eq!(ada.bag()["player"], player, "the same player");
    assert_eq!(ada.bag()["name"], "Adelaide", "under the new name");
    server.stop();
}

#[test]
fn a_pool_too_small_says_how_much_is_not_shown_and_commands_steer_the_connection() {
    let server = zones("127.0.0.1:0", Arc::new(MemoryStore::new()));
    // One coin slot for a meadow of three coins.
    let mut ada = Game::new(&server.url(), "Ada", 1, 1);
    frames_until(&mut [&mut ada], "the meadow", |g| {
        g[0].bag()["status"] == "joined" && g[0].bag()["count"].as_u64().is_some_and(|c| c >= 4)
    });
    ada.frame();
    assert_eq!(ada.bag()["overflow"], 2, "two coins have no slot to be shown in; the walls all have one");
    assert_eq!(ada.of_kind("coin").len(), 1);

    // Walking onto the shown coin frees its slot for one that had none.
    let shown_coin = ada.of_kind("coin")[0]["id"].clone();
    let (x, y) = (ada.of_kind("coin")[0]["at"][0].clone(), ada.of_kind("coin")[0]["at"][1].clone());
    ada.command("online_send", json!({ "body": { "go": [x, y] } }));
    frames_until(&mut [&mut ada], "another coin to take the slot", |g| {
        g[0].of_kind("coin").first().is_some_and(|c| c["id"] != shown_coin)
    });

    // A channel, from the commands a script calls.
    ada.command("online_subscribe", json!({ "channel": "guild:7" }));
    ada.command("online_say", json!({ "channel": "guild:7", "body": { "text": "hello" } }));
    frames_until(&mut [&mut ada], "her own words back", |g| {
        g[0].heard.iter().any(|e| e["t"] == "said" && e["body"]["text"] == "hello" && e["name"] == "Ada")
    });

    // Leave, and join somewhere else by command.
    ada.command("online_leave", json!({}));
    frames_until(&mut [&mut ada], "to be in no room", |g| g[0].bag()["status"] == "signed_in" && g[0].shown().is_empty());
    ada.command("online_join", json!({ "kind": "zone", "room": "atlantis" }));
    frames_until(&mut [&mut ada], "the refusal", |g| g[0].bag()["error"] == "there is no zone called 'atlantis'");
    assert_eq!(ada.bag()["status"], "signed_in");
    ada.command("online_join", json!({ "kind": "zone", "room": "cave" }));
    frames_until(&mut [&mut ada], "the cave", |g| g[0].bag()["room"]["id"] == "cave");

    ada.command("online_disconnect", json!({}));
    frames_until(&mut [&mut ada], "to be offline", |g| g[0].bag()["status"] == "offline" && g[0].bag()["net"] != "connected");
    std::thread::sleep(Duration::from_millis(500));
    for _ in 0..20 {
        ada.frame();
    }
    assert_eq!(ada.bag()["status"], "offline", "disconnecting on purpose is not retried");
    server.stop();
}

/// The server is two machines. The game is pointed at one, its room is on
/// the other, and it ends up there without a script doing anything.
#[test]
fn a_game_follows_a_redirect_to_the_node_that_has_its_room() {
    let coord =
        jengine_server::cluster::coordinator("127.0.0.1:0", Arc::new(MemoryStore::new()), "cluster-secret", 2.0).unwrap();
    let node = |name: &str| {
        let mut config = ClusterConfig::new(&coord.url(), "cluster-secret", name);
        config.heartbeat_secs = 0.2;
        Server::new()
            .cluster(config)
            .room("zone", KindOptions::ticking(20.0).max_players(100).persistent(), world::Zone::new)
            .start("127.0.0.1:0")
            .unwrap()
    };
    let (a, b) = (node("a"), node("b"));

    // Bo's game is on A and opens the meadow there.
    let mut bo = Game::new(&a.url(), "Bo", 4, 4);
    frames_until(&mut [&mut bo], "Bo in the meadow", |g| g[0].bag()["status"] == "joined");

    // Ada's game is pointed at B, and asks for the meadow.
    let mut ada = Game::new(&b.url(), "Ada", 4, 4);
    frames_until(&mut [&mut ada, &mut bo], "Ada to be sent to A and both to see each other", |g| {
        g[0].bag()["status"] == "joined" && g[0].of_kind("player").len() == 2 && g[1].of_kind("player").len() == 2
    });
    assert!(ada.heard.iter().any(|e| e["t"] == "redirect" && e["url"] == a.url()), "she was redirected, not let in on B");
    assert_eq!(ada.bag()["room"], json!({ "kind": "zone", "id": "meadow" }));
    assert_eq!(ada.heard.iter().filter(|e| e["t"] == "welcome").count(), 2, "one sign-in on each node, as the same player");
    let players: Vec<&Value> = ada.heard.iter().filter(|e| e["t"] == "welcome").map(|e| &e["player"]).collect();
    assert_eq!(players[0], players[1]);
    a.stop();
    b.stop();
    coord.stop();
}

/// The player's own avatar moves when the key is pressed, not a round trip
/// later - and ends exactly where the server puts it, because both ran the
/// same step function.
/// The server's walls are the client's: steered into one, the predicted
/// avatar stops where the server stops it, and nothing is pulled back.
#[test]
fn a_predicted_avatar_is_stopped_by_the_servers_walls() {
    let server = zones("127.0.0.1:0", Arc::new(MemoryStore::new()));
    let mut ada = Game::new(&server.url(), "Ada", 4, 4);
    frames_until(&mut [&mut ada], "in the meadow, walls in view", |g| {
        g[0].bag()["status"] == "joined" && !g[0].of_kind("wall").is_empty()
    });
    let mine = |g: &Game| g.of_kind("player").into_iter().find(|b| b["mine"] == true).unwrap();
    let start = mine(&ada)["at"][1].as_f64().unwrap();
    assert_eq!(start, 300.0);
    // North from (500, 300): the wall at column 10 spans y 400..500.
    ada.command("online_input", json!({ "x": 0, "y": 1 }));
    let until = Instant::now() + Duration::from_millis(900);
    let (mut highest, mut never_backwards, mut last) = (start, true, start);
    while Instant::now() < until {
        ada.frame();
        let y = mine(&ada)["at"][1].as_f64().unwrap();
        never_backwards &= y >= last - 1e-6;
        highest = highest.max(y);
        last = y;
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(never_backwards, "the server never corrected her: both ends stopped at the wall");
    assert!((highest - 390.0).abs() < 1e-6, "held at the wall's face less her radius, not {highest}");
    ada.command("online_input", json!({ "x": 0, "y": 0 }));
    frames_until(&mut [&mut ada], "the prediction handed back", |g| g[0].bag()["predicting"] == false);
    let me = mine(&ada);
    assert_eq!((me["at"][1].as_f64().unwrap(), me["server_y"].as_f64().unwrap()), (390.0, 390.0));
    server.stop();
}

#[test]
fn steering_is_predicted_and_agrees_with_the_server() {
    let server = zones("127.0.0.1:0", Arc::new(MemoryStore::new()));
    let mut ada = Game::new(&server.url(), "Ada", 4, 4);
    let mut bo = Game::new(&server.url(), "Bo", 4, 4);
    frames_until(&mut [&mut ada, &mut bo], "both in the meadow", |g| {
        g.iter().all(|g| g.bag()["status"] == "joined" && g.of_kind("player").len() == 2)
    });
    let mine = |g: &Game| g.of_kind("player").into_iter().find(|b| b["mine"] == true).unwrap();
    let start = mine(&ada)["at"][0].as_f64().unwrap();
    let id = mine(&ada)["id"].clone();
    assert_eq!(mine(&ada)["predicted"], false, "nothing is predicted until she steers");

    // East. Watch for the first frame her own slot moves.
    ada.command("online_input", json!({ "x": 1, "y": 0 }));
    let pressed = Instant::now();
    let mut led = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    while led.is_none() {
        ada.frame();
        bo.frame();
        let me = mine(&ada);
        if me["at"][0].as_f64().unwrap() > start {
            // The slot has moved; has the server said anything yet?
            led = Some(me["server_x"].as_f64().unwrap() == start);
        }
        assert!(Instant::now() < deadline, "she never moved");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(led, Some(true), "her avatar moved before the server had said it did");
    // At once, not a step later: the first input is taken on the frame the
    // stick moves (it waited a whole step, 100 ms, until Oct 2026).
    let took = pressed.elapsed();
    assert!(took < Duration::from_millis(50), "she started {} ms after the key", took.as_millis());
    assert_eq!(ada.bag()["predicting"], true);

    // Hold it for a while, then let go.
    let until = Instant::now() + Duration::from_millis(700);
    let mut never_backwards = true;
    let mut last = start;
    while Instant::now() < until {
        ada.frame();
        bo.frame();
        let x = mine(&ada)["at"][0].as_f64().unwrap();
        never_backwards &= x >= last - 1e-6;
        last = x;
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(never_backwards, "the server's answers never pulled her back: they agreed");
    ada.command("online_input", json!({ "x": 0, "y": 0 }));
    frames_until(&mut [&mut ada, &mut bo], "everything acknowledged and the prediction handed back", |g| {
        g[0].bag()["predicting"] == false
    });

    // Where she is drawn, where the server says, and where Bo sees her: one place.
    let me = mine(&ada);
    let (drawn, served) = (me["at"][0].as_f64().unwrap(), me["server_x"].as_f64().unwrap());
    assert!((drawn - served).abs() < 0.02, "drawn {drawn}, server {served}");
    let steps = (served - start) / 30.0;
    assert!(steps >= 4.0 && (steps - steps.round()).abs() < 1e-6, "whole steps of thirty: {steps}");
    frames_until(&mut [&mut ada, &mut bo], "Bo to see her there", |g| {
        g[1].of_kind("player").iter().any(|b| b["id"] == id && (b["at"][0].as_f64().unwrap() - served).abs() < 0.02)
    });

    // A second walk, after the prediction was handed back. Its inputs must
    // be numbered on from the first walk's: numbered from 1 again, the
    // room's queue dropped every one as old, so she walked alone on her own
    // screen and Bo never saw her move (found by a multiplayer arena).
    ada.command("online_input", json!({ "x": 1, "y": 0 }));
    let until = Instant::now() + Duration::from_millis(400);
    while Instant::now() < until {
        ada.frame();
        bo.frame();
        std::thread::sleep(Duration::from_millis(2));
    }
    ada.command("online_input", json!({ "x": 0, "y": 0 }));
    frames_until(&mut [&mut ada, &mut bo], "Bo to see her second walk", |g| {
        g[1].of_kind("player").iter().any(|b| b["id"] == id && b["at"][0].as_f64().unwrap() > served + 29.0)
    });
    server.stop();
}
