//! **Two machines play one game in fixed-step lockstep** (`steps`,
//! `lockstep_session`): each its own thread - its own world, `PackageHost`
//! (`jengine-net` + this package), fixed-step clock and input, as two
//! processes would be - against a real `jengine-server` lockstep room over
//! real sockets.
//!
//! The two run at different frame rates (165 fps, and an uneven 60 / 30 /
//! 144) and press their keys at their own times, and each records the game
//! after every step. Every step's record is the same on both, to the bit:
//! each step ran only once every player's input for it had come back from
//! the room (`ecs::fixed_step::set_gate`), so both ran it with the same
//! inputs. And a game made to differ on one machine is caught by the world
//! hashes and stopped.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hecs::{Entity, World};
use jengine_core::asset::AssetManager;
use jengine_core::detmath::DetMath;
use jengine_core::ecs::components::{CustomComponents, Transform};
use jengine_core::ecs::fixed_step::FixedStep;
use jengine_core::input::{InputAction, InputState, KeyCode};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::host::{FrameResources, PackageHost};
use jengine_core::package::system_schedule::SystemPhase;
use jengine_online::OnlinePackage;
use jengine_online::lockstep::StepSession;
use jengine_net::NetPackage;
use serde_json::{Value, json};

const STEPS: u64 = 240;
/// What a machine reports as its progress once it has finished.
const DONE: u64 = u64::MAX;

fn at(x: f32) -> Transform {
    let mut t = Transform::default();
    t.position.x = x;
    t
}

/// When this machine's player holds which key, in its own seconds.
struct Script {
    holds: Vec<(KeyCode, f64, f64)>,
}

struct Machine {
    host: PackageHost,
    world: World,
    session: Entity,
    players: [Entity; 2],
    input: InputState,
    assets: AssetManager,
    fixed: FixedStep,
    t: f64,
    /// After each step: the step, and the players' transforms as bits.
    record: Vec<(u64, Vec<u32>)>,
    /// A nudge to this machine's game at one step - to make it differ.
    nudge_at: Option<u64>,
}

impl Machine {
    fn new(url: &str, room: &str, nudge_at: Option<u64>, session: &Value) -> Self {
        let mut registry = ComponentRegistry::new();
        jengine_core::package::core_registration::register_core_components(&mut registry);
        let mut host = PackageHost::new();
        host.add_package(Box::new(NetPackage));
        host.add_package(Box::new(OnlinePackage));
        host.initialize(&mut registry);
        let mut world = World::new();
        let session = world.spawn((
            // As a scene authors it.
            serde_json::from_value::<StepSession>({
                let mut s = json!({ "url": url, "room": room, "seats": 2, "delay": 3, "hash_every": 30 });
                for (k, v) in session.as_object().into_iter().flatten() {
                    s[k] = v.clone();
                }
                s
            })
            .unwrap(),
            CustomComponents::default(),
        ));
        let players = [
            world.spawn((at(-100.0),)),
            world.spawn((at(100.0),)),
        ];
        let mut input = InputState::new();
        let action = |name: &str, key: &str| InputAction { name: name.into(), keys: vec![key.into()], dead_zone: 0.0 };
        input.load_actions(&[action("left", "A"), action("right", "D"), action("jump", "Space")]);
        let mut fixed = FixedStep::new();
        fixed.set_hz(60.0);
        fixed.enter_scene(None);
        Machine {
            host,
            world,
            session,
            players,
            input,
            assets: AssetManager::new(),
            fixed,
            t: 0.0,
            record: Vec::new(),
            nudge_at,
        }
    }

    fn lockstep(&self) -> Value {
        self.world.get::<&CustomComponents>(self.session).ok().and_then(|c| c.0.get("Lockstep").cloned()).unwrap_or(Value::Null)
    }

    fn phase(&mut self, phase: SystemPhase, dt: f32) {
        let camera = jengine_core::renderer::camera::Camera2D::new(1280.0, 720.0);
        self.host.run_phase_with(
            phase,
            &mut self.world,
            dt,
            FrameResources { input: &mut self.input, assets: &mut self.assets, camera },
        );
    }

    /// The game: each seat moves by what it holds, jumps on a press - read
    /// from the session, never from this machine's own input.
    fn game_step(&mut self) {
        let lock = self.lockstep();
        let step = lock["step"].as_u64().unwrap();
        for seat in 0..2 {
            let held = |name: &str| lock["held"][seat].as_array().is_some_and(|a| a.iter().any(|v| v == name));
            let pressed = lock["pressed"][seat].as_array().is_some_and(|a| a.iter().any(|v| v == "jump"));
            let mut t = self.world.get::<&mut Transform>(self.players[seat]).unwrap();
            if held("right") {
                t.position.x += 2.5;
            }
            if held("left") {
                t.position.x -= 2.5;
            }
            if pressed {
                t.rotation += 0.7;
            }
            t.position.y = (t.position.x * 0.031).dsin() * 40.0 + t.rotation.dcos();
        }
        if self.nudge_at == Some(step) {
            self.world.get::<&mut Transform>(self.players[0]).unwrap().position.x += 0.001;
        }
        let bits = self
            .players
            .iter()
            .flat_map(|p| {
                let t = self.world.get::<&Transform>(*p).unwrap();
                [t.position.x.to_bits(), t.position.y.to_bits(), t.rotation.to_bits()]
            })
            .collect();
        self.record.push((step, bits));
    }

    /// One display frame of `dt`, as a host runs one.
    fn frame(&mut self, dt: f32, script: &Script) {
        self.t += dt as f64;
        for (key, from, to) in &script.holds {
            let down = self.t >= *from && self.t < *to;
            if down && !self.input.is_pressed(*key) {
                self.input.key_down(*key);
            } else if !down && self.input.is_pressed(*key) {
                self.input.key_up(*key);
            }
        }
        self.phase(SystemPhase::Frame, dt);
        let plan = self.fixed.plan(dt, 1.0);
        for s in 0..plan.steps {
            if s > 0 {
                self.input.clear_edges();
            }
            self.fixed.begin_step(&self.world);
            self.phase(SystemPhase::PreUpdate, plan.dt);
            self.phase(SystemPhase::ScriptExec, plan.dt);
            self.game_step();
        }
        if plan.steps == 0 {
            self.input.hold_edges();
        }
        self.input.begin_frame();
    }
}

/// Run one machine in a thread of its own until it has run `STEPS` steps or
/// its session stops; what it recorded, and how its session ended.
fn machine(url: String, room: String, frames: Vec<f32>, script: Script, nudge_at: Option<u64>)
-> std::thread::JoinHandle<(Vec<(u64, Vec<u32>)>, Value, Value)> {
    machine_with(url, room, frames, script, nudge_at, Life::default())
}

/// What else happens to a machine in its life.
#[derive(Default)]
struct Life {
    /// Its `lockstep_session`'s fields beside the room's.
    session: Value,
    /// Start once another machine has run this many steps.
    after: Option<(Arc<AtomicU64>, u64)>,
    /// Stop (the player closes the game) after this many steps.
    quit_at: Option<u64>,
    /// Hang - run no frame at all - for a while at this step.
    hang: Option<(u64, Duration)>,
    /// Lose the connection at this step (the session is pointed at a server
    /// that is not there), and get it back after a while.
    drop_at: Option<(u64, Duration)>,
    /// Keep playing after its own steps until another machine has run this
    /// many (`DONE`: until it has finished) - so a match is not over before
    /// the other is back.
    until: Option<(Arc<AtomicU64>, u64)>,
    /// Play on this long after it is otherwise done.
    linger: Duration,
    /// Where this machine reports the steps it has run.
    progress: Option<Arc<AtomicU64>>,
    /// Every `Lockstep` state this machine passed through.
    seen: Option<Arc<Mutex<Vec<Value>>>>,
}

fn machine_with(
    url: String,
    room: String,
    frames: Vec<f32>,
    script: Script,
    nudge_at: Option<u64>,
    mut life: Life,
) -> std::thread::JoinHandle<(Vec<(u64, Vec<u32>)>, Value, Value)> {
    std::thread::spawn(move || {
        if let Some((other, steps)) = &life.after {
            let deadline = Instant::now() + Duration::from_secs(30);
            while other.load(Ordering::SeqCst) < *steps {
                assert!(Instant::now() < deadline, "the other machine never reached step {steps}");
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        let mut m = Machine::new(&url, &room, nudge_at, &life.session);
        let deadline = Instant::now() + Duration::from_secs(40);
        let mut i = 0;
        let mut restore: Option<Instant> = None;
        let wanted = |m: &Machine| {
            // Still replaying the match is not done either.
            (m.record.len() as u64) < STEPS
                || m.lockstep()["state"] == "catching_up"
                || life.until.as_ref().is_some_and(|(o, n)| o.load(Ordering::SeqCst) < *n)
        };
        let linger = life.linger;
        let mut level: Option<Instant> = None;
        loop {
            if wanted(&m) {
                level = None;
            } else if Instant::now() >= *level.get_or_insert_with(|| Instant::now() + linger) {
                break;
            }
            let state = m.lockstep()["state"].as_str().unwrap_or("").to_string();
            if state == "desync" || state == "ended" {
                break;
            }
            let ran = m.record.len() as u64;
            if life.quit_at.is_some_and(|q| ran >= q) {
                break;
            }
            if let Some((at, long)) = life.hang
                && ran >= at
            {
                std::thread::sleep(long);
                life.hang = None;
            }
            if let Some((at, long)) = life.drop_at
                && ran >= at
            {
                m.world.get::<&mut StepSession>(m.session).unwrap().url = "ws://127.0.0.1:9/nobody".into();
                restore = Some(Instant::now() + long);
                life.drop_at = None;
            }
            if restore.is_some_and(|r| Instant::now() >= r) {
                m.world.get::<&mut StepSession>(m.session).unwrap().url = url.clone();
                restore = None;
            }
            if let Some(p) = &life.progress {
                p.store(ran, Ordering::SeqCst);
            }
            if let Some(seen) = &life.seen {
                seen.lock().unwrap().push(m.lockstep());
            }
            assert!(Instant::now() < deadline, "stuck at step {} ({state})", m.record.len());
            m.frame(frames[i % frames.len()], &script);
            i += 1;
            std::thread::sleep(Duration::from_millis(1));
        }
        if let Some(p) = &life.progress {
            p.store(DONE, Ordering::SeqCst);
        }
        // As it stood on its last step - and a little later, so a desync
        // reported after it arrives (by then the other machine may have
        // finished and left, which ends the match).
        let done = m.lockstep();
        for _ in 0..50 {
            m.phase(SystemPhase::Frame, 0.0);
            std::thread::sleep(Duration::from_millis(2));
        }
        let later = m.lockstep();
        // A frame running two steps can carry a machine one past the count.
        m.record.truncate(STEPS as usize);
        (m.record, done, later)
    })
}

fn server() -> (jengine_server::Running, String) {
    let server = jengine_server::Server::new().lockstep().start("127.0.0.1:0").unwrap();
    let url = server.url();
    (server, url)
}

#[test]
fn two_machines_at_different_frame_rates_play_the_same_game() {
    let (_server, url) = server();
    let a = machine(
        url.clone(),
        "duel".into(),
        vec![1.0 / 165.0],
        Script { holds: vec![(KeyCode::D, 0.3, 1.6), (KeyCode::Space, 0.8, 0.85), (KeyCode::Space, 1.2, 1.3)] },
        None,
    );
    let b = machine(
        url,
        "duel".into(),
        vec![1.0 / 60.0, 1.0 / 30.0, 1.0 / 144.0],
        Script { holds: vec![(KeyCode::A, 0.5, 2.5), (KeyCode::Space, 1.4, 1.42), (KeyCode::D, 2.6, 3.4)] },
        None,
    );
    let (ra, la, _) = a.join().unwrap();
    let (rb, lb, _) = b.join().unwrap();
    assert_eq!(la["state"], "running", "{la}");
    assert_eq!(lb["state"], "running", "{lb}");
    assert_eq!(ra.len() as u64, STEPS);
    assert_eq!(rb.len() as u64, STEPS);
    assert_eq!(ra.iter().map(|r| r.0).collect::<Vec<_>>(), (1..=STEPS).collect::<Vec<_>>(), "every step, in order, once");
    for (x, y) in ra.iter().zip(rb.iter()) {
        assert_eq!(x, y, "step {} differs", x.0);
    }
    // Both players' inputs moved both machines' game.
    let first = &ra[0].1;
    let last = &ra[ra.len() - 1].1;
    assert_ne!(first[0], last[0], "seat 0 moved");
    assert_ne!(first[3], last[3], "seat 1 moved");
    assert_ne!(la["seat"], lb["seat"], "two seats");
}

#[test]
fn a_game_that_comes_apart_is_stopped() {
    let (_server, url) = server();
    let a = machine(url.clone(), "apart".into(), vec![1.0 / 120.0], Script { holds: vec![(KeyCode::D, 0.1, 3.0)] }, None);
    // The other machine's game differs from step 40: the hash at step 60 disagrees.
    let b = machine(url, "apart".into(), vec![1.0 / 60.0], Script { holds: vec![] }, Some(40));
    let (ra, _, la) = a.join().unwrap();
    let (_, _, lb) = b.join().unwrap();
    assert_eq!(la["state"], "desync", "{la}");
    assert_eq!(lb["state"], "desync", "{lb}");
    assert!((ra.len() as u64) < STEPS, "the match stopped at the desync, at step {}", ra.len());
}

/// Every step both machines ran is the same on both, to the bit.
fn same_game(a: &[(u64, Vec<u32>)], b: &[(u64, Vec<u32>)]) {
    let ours: std::collections::BTreeMap<u64, &Vec<u32>> = a.iter().map(|(s, r)| (*s, r)).collect();
    let mut compared = 0;
    for (step, record) in b {
        if let Some(theirs) = ours.get(step) {
            assert_eq!(*theirs, record, "step {step} differs");
            compared += 1;
        }
    }
    assert!(compared > 0, "no step in common");
}

/// A player who closes the game mid-match is played as "nothing held" - the
/// other plays on - and comes back by loading the scene again: the room hands
/// over the match so far, the new machine replays it fast and then plays on
/// with everyone, the same game to the bit.
#[test]
fn a_player_who_leaves_comes_back_and_catches_up() {
    let (_server, url) = server();
    let a_steps = Arc::new(AtomicU64::new(0));
    let b2_steps = Arc::new(AtomicU64::new(0));
    let a = machine_with(
        url.clone(),
        "back".into(),
        vec![1.0 / 60.0],
        Script { holds: vec![(KeyCode::D, 0.2, 3.0)] },
        None,
        Life {
            session: json!({ "name": "a", "silent_after": 1.0 }),
            progress: Some(a_steps.clone()),
            until: Some((b2_steps.clone(), DONE)),
            ..Life::default()
        },
    );
    let b_script = || Script { holds: vec![(KeyCode::A, 0.1, 0.6), (KeyCode::Space, 0.3, 0.35)] };
    let b = machine_with(
        url.clone(),
        "back".into(),
        vec![1.0 / 90.0],
        b_script(),
        None,
        Life { session: json!({ "name": "b", "silent_after": 1.0 }), quit_at: Some(60), ..Life::default() },
    );
    // The same player, back once the match is well past where they left.
    let seen = Arc::new(Mutex::new(Vec::new()));
    let b2 = machine_with(
        url,
        "back".into(),
        vec![1.0 / 60.0],
        b_script(),
        None,
        Life {
            session: json!({ "name": "b" }),
            after: Some((a_steps.clone(), 150)),
            seen: Some(seen.clone()),
            progress: Some(b2_steps),
            ..Life::default()
        },
    );
    let (rb, lb, _) = b.join().unwrap();
    let (ra, la, _) = a.join().unwrap();
    let (rb2, lb2, _) = b2.join().unwrap();
    assert_eq!(la["state"], "running", "{la}");
    assert_eq!(lb2["state"], "running", "{lb2}");
    assert_eq!(rb2.iter().map(|r| r.0).collect::<Vec<_>>(), (1..=STEPS).collect::<Vec<_>>(), "replayed from step 1");
    same_game(&ra, &rb);
    same_game(&ra, &rb2);
    assert_eq!(lb2["seat"], lb["seat"], "back in its own seat");
    let seen = seen.lock().unwrap();
    assert!(seen.iter().any(|l| l["state"] == "catching_up"), "it replayed the match so far");
    // While it was away it held nothing: its player stood still.
    let at = |step: u64| &ra[step as usize - 1].1;
    let x = 3 * lb["seat"].as_u64().unwrap() as usize;
    assert_eq!(at(100)[x], at(140)[x], "an away seat holds nothing");
}

/// A player whose machine hangs holds the others up for `silent_after`, not
/// for ever: then it is played as "nothing held" until it speaks again.
#[test]
fn a_silent_player_holds_nobody_up_for_long() {
    let (_server, url) = server();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let b_steps = Arc::new(AtomicU64::new(0));
    let a = machine_with(
        url.clone(),
        "quiet".into(),
        vec![1.0 / 120.0],
        Script { holds: vec![(KeyCode::D, 0.1, 3.0)] },
        None,
        Life {
            session: json!({ "silent_after": 0.5 }),
            seen: Some(seen.clone()),
            until: Some((b_steps.clone(), DONE)),
            ..Life::default()
        },
    );
    let b = machine_with(
        url,
        "quiet".into(),
        vec![1.0 / 60.0],
        Script { holds: vec![(KeyCode::A, 0.1, 3.0)] },
        None,
        Life {
            session: json!({ "silent_after": 0.5 }),
            hang: Some((50, Duration::from_millis(2500))),
            progress: Some(b_steps),
            linger: Duration::from_millis(500),
            ..Life::default()
        },
    );
    let (ra, la, _) = a.join().unwrap();
    let (rb, lb, _) = b.join().unwrap();
    assert_eq!(la["state"], "running", "{la}");
    assert_eq!(lb["state"], "running", "{lb}");
    same_game(&ra, &rb);
    let seen = seen.lock().unwrap();
    let k = lb["seat"].as_u64().unwrap() as usize;
    let silent = seen.iter().position(|l| l["silent"][k] == true).expect("the hung seat was called silent");
    assert!(seen[silent..].iter().any(|l| l["silent"][k] == false), "and back once it spoke");
}

/// A dropped connection is not the end: the session reconnects by itself and
/// takes its seat back from where it was.
#[test]
fn a_dropped_connection_reconnects_and_plays_on() {
    let (_server, url) = server();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let b_steps = Arc::new(AtomicU64::new(0));
    let a = machine_with(
        url.clone(),
        "drop".into(),
        vec![1.0 / 120.0],
        Script { holds: vec![(KeyCode::D, 0.1, 3.0)] },
        None,
        Life {
            session: json!({ "name": "a", "silent_after": 0.5 }),
            until: Some((b_steps.clone(), DONE)),
            ..Life::default()
        },
    );
    let b = machine_with(
        url,
        "drop".into(),
        vec![1.0 / 60.0],
        Script { holds: vec![(KeyCode::A, 0.1, 3.0)] },
        None,
        Life {
            session: json!({ "name": "b", "reconnect_every": 0.2, "silent_after": 0.5 }),
            drop_at: Some((60, Duration::from_millis(1500))),
            seen: Some(seen.clone()),
            progress: Some(b_steps),
            ..Life::default()
        },
    );
    let (ra, la, _) = a.join().unwrap();
    let (rb, lb, _) = b.join().unwrap();
    assert_eq!(la["state"], "running", "{la}");
    assert_eq!(lb["state"], "running", "{lb}");
    assert_eq!(rb.iter().map(|r| r.0).collect::<Vec<_>>(), (1..=STEPS).collect::<Vec<_>>());
    same_game(&ra, &rb);
    assert!(seen.lock().unwrap().iter().any(|l| l["state"] == "reconnecting"), "it lost the connection");
}

/// A spectator joins mid-match, replays it and watches the same game.
#[test]
fn a_spectator_watches_the_same_game() {
    let (_server, url) = server();
    let a_steps = Arc::new(AtomicU64::new(0));
    let w_steps = Arc::new(AtomicU64::new(0));
    let a = machine_with(
        url.clone(),
        "watched".into(),
        vec![1.0 / 60.0],
        Script { holds: vec![(KeyCode::D, 0.2, 3.0), (KeyCode::Space, 1.0, 1.1)] },
        None,
        Life { progress: Some(a_steps.clone()), until: Some((w_steps.clone(), DONE)), ..Life::default() },
    );
    let b = machine_with(
        url.clone(),
        "watched".into(),
        vec![1.0 / 144.0],
        Script { holds: vec![(KeyCode::A, 0.4, 2.0)] },
        None,
        Life { until: Some((w_steps.clone(), DONE)), ..Life::default() },
    );
    let w = machine_with(
        url,
        "watched".into(),
        vec![1.0 / 60.0],
        Script { holds: vec![(KeyCode::D, 0.0, 9.0)] },
        None,
        Life {
            session: json!({ "spectate": true }),
            after: Some((a_steps, 100)),
            progress: Some(w_steps),
            ..Life::default()
        },
    );
    let (ra, _, _) = a.join().unwrap();
    let (rb, _, _) = b.join().unwrap();
    let (rw, lw, _) = w.join().unwrap();
    assert_eq!(lw["seat"], Value::Null, "{lw}");
    assert_eq!(rw.len() as u64, STEPS, "{lw}");
    same_game(&ra, &rb);
    same_game(&ra, &rw);
}
