//! One zone of a small world: avatars that walk where they are told, coins to
//! pick up, walls, monsters that hunt, and a door to the next zone.
//!
//! The server decides everything. A client says where it would like to go
//! (`{"go":[x,y]}`) and the zone walks the avatar there at its own speed -
//! or steers it directly, an input a step (`{"input":{"seq":7,"x":1,"y":0}}`),
//! which is the form a client can *predict*: the zone applies inputs with
//! the same step function the client has ([`TileMotion`], from the netcode
//! crate both link), says which it has reached, and so the client's guess is
//! the server's answer. Either way each player is told what is near them
//! through a [`Space`]. What a player carries - which zone, where in it, how
//! many coins - is in their state, so it is the same player on the far side
//! of the door, after a dropped connection, and after the server restarts.
//!
//! # The engine on the server
//!
//! The zone's map is an engine [`Tilemap`] - the same type a scene holds -
//! and its monsters find their way with the engine's own A*
//! (`jengine_core::tilemap::pathfinding`). A server is plain Rust in the
//! engine's workspace, so it links `jengine-core` headless
//! (`default-features = false`) and any gameplay package the same way, and
//! implements nothing of its own. The walls are then handed to the netcode
//! crate as [`Walls`] and sent to the client on `joined`, so a player is
//! predicted against the very walls the monsters path through.

use std::collections::BTreeMap;

use jengine_core::tilemap::pathfinding::{PathMode, find_path_on_layer};
use jengine_core::tilemap::{Tilemap, TilemapLayer};
use jengine_server::netcode::{Input, InputQueue, Motion, TileMotion, Walls};
use jengine_server::sync::{EntityId, Space};
use jengine_server::{JoinRequest, Player, PlayerId, Players, Room, RoomCtx};
use serde_json::{Value, json};

pub const WIDTH: f64 = 1000.0;
pub const HEIGHT: f64 = 600.0;
/// Units a second an avatar walks.
pub const SPEED: f64 = 300.0;
/// Half an avatar's width, for the walls.
pub const RADIUS: f64 = 10.0;
/// The length of one input's step, in seconds: what a client predicts with.
/// It is a number both ends know, not this room's tick length - a room
/// ticking faster simply reaches each input sooner.
pub const STEP: f64 = 0.1;
/// A wall cell's side.
pub const CELL: f64 = 50.0;
const VIEW: f64 = 400.0;
const REACH: f64 = 18.0;
const COIN_RESPAWN: f64 = 5.0;
/// Units a second a monster runs: slower than a player, so they can be outrun.
pub const MONSTER_SPEED: f64 = 120.0;
/// How close a monster must be to bite, and how long it rests after.
pub const BITE_REACH: f64 = 24.0;
const BITE_REST: f64 = 2.0;
/// How often a monster asks the pathfinder again.
const REPLAN: f64 = 0.5;
/// How far a monster can see: beyond it, it stands where it is.
pub const HUNT_RANGE: f64 = 300.0;

/// The zones, west to east. Walking off the east edge of one comes in at the
/// west edge of the next, and back.
pub const ZONES: [&str; 2] = ["meadow", "cave"];

fn coins_of(zone: &str) -> &'static [(f64, f64)] {
    match zone {
        "meadow" => &[(560.0, 300.0), (300.0, 150.0), (820.0, 480.0)],
        _ => &[(200.0, 300.0), (640.0, 120.0)],
    }
}

/// The walls, drawn: twenty cells by twelve, `#` solid, the top row at the
/// top of the zone. The middle rows are kept open so the door is reachable
/// along the middle.
fn walls_of(zone: &str) -> &'static [&'static str] {
    match zone {
        "meadow" => &[
            "....................",
            "....................",
            "...##.....#.........",
            "....#.....#....##...",
            "....#...............",
            "....................",
            "....................",
            "..............#.....",
            "...###........#.....",
            "..............#.....",
            "....................",
            "....................",
        ],
        _ => &[
            "....................",
            "....................",
            ".....#......#.......",
            ".....#......#.......",
            ".....#......#.......",
            "....................",
            "....................",
            ".....#......#.......",
            ".....#......#.......",
            ".....#......#.......",
            "....................",
            "....................",
        ],
    }
}

fn monsters_of(zone: &str) -> &'static [(f64, f64)] {
    match zone {
        "meadow" => &[(125.0, 525.0), (875.0, 125.0)],
        _ => &[(900.0, 300.0)],
    }
}

/// The engine's tilemap, built from the drawing: one layer, `walls`, with a
/// tile of any id in each solid cell - the pathfinder walks the empty ones.
fn tilemap_of(zone: &str) -> Tilemap {
    let rows = walls_of(zone);
    let (width, height) = (rows[0].len() as u32, rows.len() as u32);
    let mut layer = TilemapLayer {
        name: "walls".into(),
        tiles: vec![0; (width * height) as usize],
        width,
        height,
        ..Default::default()
    };
    for (y, row) in rows.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            if ch == '#' {
                layer.set_tile(x as u32, y as u32, Some(1));
            }
        }
    }
    Tilemap { tile_size: [CELL as f32, CELL as f32], layers: vec![layer], ..Default::default() }
}

/// The netcode crate's view of a tilemap layer: what is sent to the client.
/// The one place the two meet, so they cannot disagree about a cell.
fn walls_from(layer: &TilemapLayer) -> Walls {
    let mut walls = Walls::open(layer.width as usize, layer.height as usize, CELL, (0.0, HEIGHT));
    for y in 0..layer.height {
        for x in 0..layer.width {
            walls.set_solid(x as usize, y as usize, layer.get_tile(x, y).is_some());
        }
    }
    walls
}

/// How close a player must be to tag another.
pub const TAG_REACH: f64 = 60.0;
/// Ticks of positions kept for judging a tag against what the tagger saw.
const REMEMBER: usize = 30;

struct Avatar {
    entity: EntityId,
    target: (f64, f64),
    /// Direct inputs waiting their turn: one is applied a tick.
    inputs: InputQueue,
    /// Already written to the player's state as being somewhere else, so
    /// `on_leave` must not write this zone's position over it.
    departed: bool,
}

struct Monster {
    entity: EntityId,
    /// Cells still to walk, nearest first; empty when there is nobody to hunt.
    path: Vec<(i32, i32)>,
    replan_at: f64,
    rests_until: f64,
}

pub struct Zone {
    name: String,
    space: Space,
    /// The engine's own map: what the monsters path over.
    map: Tilemap,
    /// How a steered avatar moves - the walls as the client has them.
    field: TileMotion,
    avatars: BTreeMap<PlayerId, Avatar>,
    monsters: Vec<Monster>,
    /// (when, x, y) - coins waiting to come back.
    respawn: Vec<(f64, f64, f64)>,
    /// How long the last tick was: what a claimed tick is measured in.
    tick_secs: f64,
}

impl Zone {
    pub fn new(name: &str) -> Self {
        let mut space = Space::new(VIEW);
        // For `tag`: where everyone was, for the last second or so.
        space.remember(REMEMBER);
        for (x, y) in coins_of(name) {
            space.spawn(*x, *y, json!({ "kind": "coin" }));
        }
        let map = tilemap_of(name);
        let walls = walls_from(&map.layers[0]);
        // Every wall is an entity too, so a client can draw it.
        for (col, row) in walls.solid_cells() {
            let (x, y) = walls.centre(col as i64, row as i64);
            space.spawn(x, y, json!({ "kind": "wall" }));
        }
        let monsters = monsters_of(name)
            .iter()
            .map(|(x, y)| Monster {
                entity: space.spawn(*x, *y, json!({ "kind": "monster" })),
                path: Vec::new(),
                replan_at: 0.0,
                rests_until: 0.0,
            })
            .collect();
        // One step past the edge is allowed: that is how a door is walked through.
        let field =
            TileMotion { speed: SPEED, radius: RADIUS, min: (-10.0, 0.0), max: (WIDTH + 10.0, HEIGHT), walls };
        Zone { name: name.to_string(), space, map, field, avatars: BTreeMap::new(), monsters, respawn: Vec::new(), tick_secs: 0.1 }
    }

    fn neighbour(&self, step: i64) -> Option<&'static str> {
        let here = ZONES.iter().position(|z| *z == self.name)? as i64;
        ZONES.get(usize::try_from(here + step).ok()?).copied()
    }


    /// What a predicting client needs to move as this room does: sent to it,
    /// rather than built into it - the walls included.
    fn netcode(&self) -> Value {
        let w = &self.field.walls;
        json!({
            "step": STEP, "speed": self.field.speed, "radius": self.field.radius,
            "min": [self.field.min.0, self.field.min.1], "max": [self.field.max.0, self.field.max.1],
            "walls": { "cols": w.cols, "rows": w.rows, "cell": w.cell, "origin": [w.origin.0, w.origin.1], "solid": w.packed() },
        })
    }

    /// Each monster: a fresh path to the nearest player every so often, a
    /// run along it, and a bite when it arrives. The path is the engine's
    /// A* over the zone's own tilemap layer; the monster walks cell centre to
    /// cell centre, which the pathfinder promises never clips a corner.
    fn hunt(&mut self, ctx: &mut RoomCtx, players: &mut Players, dt: f64) {
        let now = ctx.time();
        let prey: Vec<(PlayerId, (f64, f64))> = self
            .avatars
            .iter()
            .filter_map(|(id, a)| Some((id.clone(), self.space.position(a.entity)?)))
            .collect();
        let (field, map, space) = (&self.field, &self.map, &mut self.space);
        let cell = |at| cell_in(&field.walls, at);
        for monster in &mut self.monsters {
            let Some(at) = space.position(monster.entity) else { continue };
            let nearest = prey
                .iter()
                .min_by(|a, b| dist(a.1, at).partial_cmp(&dist(b.1, at)).unwrap())
                .cloned();
            let Some((id, target)) = nearest.filter(|(_, at_prey)| dist(*at_prey, at) <= HUNT_RANGE) else {
                monster.path.clear();
                continue;
            };
            if now >= monster.replan_at {
                monster.replan_at = now + REPLAN;
                let found =
                    find_path_on_layer(&map.layers[0], cell(at), cell(target), PathMode::Diagonal).unwrap_or_default();
                // The first cell is the one it stands in.
                monster.path = found.into_iter().skip(1).collect();
            }
            if now < monster.rests_until {
                continue;
            }
            // In the last cell, go for the player themselves; else the next centre.
            let goal = match monster.path.first() {
                Some(c) if monster.path.len() > 1 || cell(target) != *c => field.walls.centre(c.0 as i64, c.1 as i64),
                _ => target,
            };
            let far = dist(goal, at);
            let step = MONSTER_SPEED * dt;
            let next = if step >= far { goal } else { (at.0 + (goal.0 - at.0) / far * step, at.1 + (goal.1 - at.1) / far * step) };
            let next = field.resolve(at, next);
            space.set_position(monster.entity, next.0, next.1);
            if let Some(c) = monster.path.first()
                && cell(next) == *c
            {
                monster.path.remove(0);
            }
            if dist(next, target) <= BITE_REACH
                && let Some(player) = players.get_mut(&id)
            {
                let coins = (player.num("coins", 0.0) - 1.0).max(0.0);
                player.set("coins", coins);
                ctx.send(&id, json!({ "t": "bitten", "by": monster.entity, "coins": coins }));
                monster.rests_until = now + BITE_REST;
            }
        }
    }
}

/// The map cell a point is in, for the pathfinder.
fn cell_in(walls: &Walls, at: (f64, f64)) -> (i32, i32) {
    let (col, row) = walls.cell_of(at);
    (col as i32, row as i32)
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

impl Room for Zone {
    fn on_join(&mut self, _ctx: &mut RoomCtx, player: &mut Player, _options: &Value)
    -> Result<Value, String> {
        if !ZONES.contains(&self.name.as_str()) {
            return Err(format!("there is no zone called '{}'", self.name));
        }
        // Somebody arriving from elsewhere - or for the first time - comes
        // in at the middle; somebody who was last here, where they stood.
        let (x, y) = if player.text("zone", "") == self.name {
            (player.num("x", WIDTH / 2.0), player.num("y", HEIGHT / 2.0))
        } else {
            (WIDTH / 2.0, HEIGHT / 2.0)
        };
        player.set("zone", self.name.clone());
        player.set("x", x);
        player.set("y", y);
        let entity = self.space.spawn(x, y, json!({ "kind": "player", "name": player.name }));
        self.space.watch_from(&player.id, entity);
        self.avatars.insert(
            player.id.clone(),
            Avatar { entity, target: (x, y), inputs: InputQueue::new(), departed: false },
        );
        Ok(json!({
            "you": entity, "width": WIDTH, "height": HEIGHT,
            "coins": player.num("coins", 0.0),
            "netcode": self.netcode(),
        }))
    }

    /// Back on a new connection, which has seen nothing: watch again from
    /// the avatar, so the whole view is sent afresh.
    fn on_rejoin(&mut self, _ctx: &mut RoomCtx, player: &mut Player) -> Value {
        let Some(avatar) = self.avatars.get(&player.id) else { return Value::Null };
        self.space.watch_from(&player.id, avatar.entity);
        json!({
            "you": avatar.entity, "width": WIDTH, "height": HEIGHT,
            "coins": player.num("coins", 0.0),
            "netcode": self.netcode(),
        })
    }

    fn on_message(&mut self, ctx: &mut RoomCtx, player: &mut Player, body: Value) {
        // A tag: `{"tag":{"target":entity,"tick":n}}` - "I touched them, and
        // this is the tick I was looking at". Everyone else is drawn a tick
        // or more in the past on the tagger's screen, so the fair question
        // is whether the target was in reach **at that tick**, not where the
        // zone has moved them since. That is lag compensation, whole: the
        // tagger stands where the server has them now; only the target is
        // rewound, and no further than the zone remembers - nor further than
        // the tagger's own measured lag (and a tick of drawing behind) lets
        // them have been looking: a claim from longer ago is judged at the
        // oldest tick that allows, so lying about the tick buys nothing.
        if let Some(target) = body["tag"]["target"].as_u64() {
            let Some(me) = self.avatars.get(&player.id).map(|a| a.entity) else { return };
            let claimed = body["tag"]["tick"].as_u64().unwrap_or(u64::MAX);
            let tick = ctx.clamp_rewind(claimed, self.space.tick(), self.tick_secs, 2);
            let tagged = self.avatars.iter().find(|(_, a)| a.entity == target && a.entity != me).map(|(id, _)| id);
            let hit = match (tagged, self.space.position(me), self.space.position_at(target, tick)) {
                (Some(_), Some(mine), Some(theirs)) => (mine.0 - theirs.0).hypot(mine.1 - theirs.1) <= TAG_REACH,
                _ => false,
            };
            ctx.send(&player.id, json!({ "t": "tag", "target": target, "hit": hit }));
            if let (true, Some(tagged)) = (hit, tagged) {
                ctx.send(tagged, json!({ "t": "tagged", "by": player.name }));
            }
            return;
        }
        let Some(avatar) = self.avatars.get_mut(&player.id) else { return };
        // Steering: an input for one step, numbered. Queued, not applied -
        // the tick takes one at a time, so a burst cannot buy speed.
        if let Some(seq) = body["input"]["seq"].as_u64() {
            let (x, y) = (body["input"]["x"].as_f64().unwrap_or(0.0), body["input"]["y"].as_f64().unwrap_or(0.0));
            avatar.inputs.push(Input { seq: seq as u32, x, y });
            return;
        }
        let (Some(x), Some(y)) = (body["go"][0].as_f64(), body["go"][1].as_f64()) else { return };
        // Past the edge is allowed by one step: that is how a door is walked through.
        avatar.target = (x.clamp(-10.0, WIDTH + 10.0), y.clamp(0.0, HEIGHT));
    }

    fn on_leave(&mut self, _ctx: &mut RoomCtx, player: &mut Player) {
        let Some(avatar) = self.avatars.remove(&player.id) else { return };
        if let (false, Some((x, y))) = (avatar.departed, self.space.position(avatar.entity)) {
            player.set("x", x);
            player.set("y", y);
        }
        self.space.despawn(avatar.entity);
        self.space.unwatch(&player.id);
    }

    fn describe(&self) -> Value {
        json!({ "entities": self.space.len(), "coins_coming_back": self.respawn.len(), "monsters": self.monsters.len() })
    }

    /// The zone's own commands, from an operator or from one of the game's
    /// HTTP routes: `{"coin":[x,y]}` drops a coin, `{"grant":player,"coins":n}`
    /// gives somebody who is here coins.
    fn on_admin(&mut self, ctx: &mut RoomCtx, players: &mut Players, command: Value) -> Value {
        if let Some(id) = command["grant"].as_str() {
            let Some(player) = players.get_mut(id) else { return json!({ "error": "not in this zone" }) };
            let coins = player.num("coins", 0.0) + command["coins"].as_f64().unwrap_or(1.0);
            player.set("coins", coins);
            ctx.send(id, json!({ "t": "coins", "coins": coins }));
            return json!({ "ok": true, "coins": coins });
        }
        match (command["coin"][0].as_f64(), command["coin"][1].as_f64()) {
            (Some(x), Some(y)) => {
                let id = self.space.spawn(x, y, json!({ "kind": "coin" }));
                json!({ "ok": true, "id": id })
            }
            _ => json!({ "error": "try {\"coin\":[x,y]}" }),
        }
    }

    fn on_tick(&mut self, ctx: &mut RoomCtx, players: &mut Players, dt: f64) {
        let now = ctx.time();
        if dt > 0.0 {
            self.tick_secs = dt;
        }
        let mut back = Vec::new();
        self.respawn.retain(|(when, x, y)| {
            let due = *when <= now;
            if due {
                back.push((*x, *y));
            }
            !due
        });
        for (x, y) in back {
            self.space.spawn(x, y, json!({ "kind": "coin" }));
        }

        let (east, west) = (self.neighbour(1), self.neighbour(-1));
        for (id, avatar) in &mut self.avatars {
            let Some((x, y)) = self.space.position(avatar.entity) else { continue };
            // An input, if one is waiting: one fixed step of it, whatever
            // this tick's own length - which is what lets the client have
            // made exactly the same move already. It also ends any walk.
            let steered = avatar.inputs.take().map(|input| self.field.step((x, y), &input, STEP));
            if let Some(to) = steered {
                self.space.set_position(avatar.entity, to.0, to.1);
                avatar.target = to;
            }
            if avatar.inputs.acked() > 0 {
                self.space.ack(id, avatar.inputs.acked());
            }
            let (dx, dy) = (avatar.target.0 - x, avatar.target.1 - y);
            let far = if steered.is_some() { 1.0 } else { dx.hypot(dy) };
            if far > 0.0 && steered.is_none() {
                // The last step lands *on* the target. Stopping "near enough"
                // leaves an avatar a hair short for ever, and a client that
                // waits for it to arrive waits for ever with it.
                let step = SPEED * dt;
                let (nx, ny) = if step >= far { avatar.target } else { (x + dx / far * step, y + dy / far * step) };
                // Through the same walls a steered avatar meets.
                let (nx, ny) = self.field.resolve((x, y), (nx, ny));
                self.space.set_position(avatar.entity, nx, ny);
            }
            let Some((x, y)) = self.space.position(avatar.entity) else { continue };
            let Some(player) = players.get_mut(id) else { continue };
            // Where they stand is theirs to keep, so it goes in their state
            // as they go - which is what an autosave saves, and what they
            // are told if they come back to a held seat.
            if far > 0.0 {
                player.set("x", x);
                player.set("y", y);
            }

            for coin in self.space.near(x, y, REACH) {
                if self.space.get(coin, "kind") == Some(&json!("coin")) {
                    let (cx, cy) = self.space.position(coin).unwrap_or((x, y));
                    self.space.despawn(coin);
                    self.respawn.push((now + COIN_RESPAWN, cx, cy));
                    let coins = player.num("coins", 0.0) + 1.0;
                    player.set("coins", coins);
                    ctx.send(id, json!({ "t": "coins", "coins": coins }));
                }
            }

            let door = match (x >= WIDTH, x <= 0.0) {
                (true, _) => east.map(|zone| (zone, 40.0)),
                (_, true) => west.map(|zone| (zone, WIDTH - 40.0)),
                _ => None,
            };
            if let Some((zone, arrive_x)) = door {
                // Written as already being there: if the move fails half way
                // the player is still somewhere that exists.
                player.set("zone", zone);
                player.set("x", arrive_x);
                player.set("y", y);
                avatar.departed = true;
                ctx.transfer(id, JoinRequest::named("zone", zone));
            }
        }

        self.hunt(ctx, players, dt);

        for (to, frame) in self.space.flush() {
            ctx.send_text(&to, frame);
        }
    }
}
