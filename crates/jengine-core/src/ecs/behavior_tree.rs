//! Behaviour trees (`behavior_tree`, `BehaviorTree.set`; Oct 2026).
//!
//! A behaviour graph (`crate::behavior`) answers events - "on a hit, do
//! this". A behaviour tree answers the other question a creature asks every
//! frame: *what should I be doing now?* Its root is ticked once a frame;
//! **composites** decide which child runs (`sequence`: each in turn until one
//! fails, `selector`: each in turn until one succeeds, `parallel`, `random`),
//! **decorators** change one child's answer (`invert`, `succeed`, `repeat`,
//! `cooldown`, `timeout`), and **leaves** look (`check`, `near`, `chance`) or
//! act (`wait`, `move_to`, `set`, `write`, `event`, `call`). Every node
//! answers success, failure or running; a running leaf is picked up where it
//! left off next frame.
//!
//! A `reactive` sequence or selector looks again from its first child every
//! frame, so a guard who hears a noise drops its patrol mid-walk: the
//! higher-priority branch starting **aborts** the one that was running (an
//! aborted `move_to` stops its agent, an aborted `call` is told so).
//!
//! The **blackboard** is the entity's own `Blackboard` custom component, so a
//! script reads and writes it with `Entity.get_custom` / `set_custom` and
//! needs nothing new; the tree writes `_running` there (the labels of the
//! leaves running now) for a readout. A `call` leaf hands work to the
//! entity's script: it calls `on_<name>` and runs until the script writes
//! `"success"` or `"failure"` (or `true` / `false`) under that key.
//!
//! The tree is held inline in the component, as a timeline's clips are, so no
//! host delivers an asset, and [`tick_behavior_trees`] runs inside
//! `behaviors::tick_motion` - before navigation, so a `move_to` steering a
//! `nav_agent` is walked the same frame - so no host was wired.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ecs::components::{CustomComponents, Name, Tags, Transform, Velocity2D, WorldTransform};

/// What a node answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Success,
    Failure,
    Running,
}

fn yes() -> bool {
    true
}
fn default_op() -> String {
    "==".into()
}
fn default_speed() -> f32 {
    120.0
}
fn default_arrive() -> f32 {
    8.0
}

/// One node of a tree. Every node takes an optional `label`, which is what
/// the `_running` readout shows (its type when there is none).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BtNode {
    /// Each child in turn while they succeed; fails with the first that fails.
    Sequence {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        /// Look again from the first child every frame (put conditions first).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reactive: bool,
        #[serde(default)]
        children: Vec<BtNode>,
    },
    /// Each child in turn while they fail; succeeds with the first that does.
    Selector {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        /// Look again from the first child every frame: a higher-priority
        /// branch that starts aborts the one running.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reactive: bool,
        #[serde(default)]
        children: Vec<BtNode>,
    },
    /// Every child at once: succeeds when `succeed` of them have (0, all),
    /// fails as soon as that can no longer happen.
    Parallel {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        #[serde(default)]
        succeed: usize,
        #[serde(default)]
        children: Vec<BtNode>,
    },
    /// One child, picked at random each time it starts.
    Random {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        #[serde(default)]
        children: Vec<BtNode>,
    },
    /// Success for failure and failure for success.
    Invert {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        child: Box<BtNode>,
    },
    /// Success whatever the child answers (once it has finished).
    Succeed {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        child: Box<BtNode>,
    },
    /// The child again, once a frame at most: `times` times (0, forever),
    /// or until it fails (`until_failure`, which then succeeds).
    Repeat {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        child: Box<BtNode>,
        #[serde(default)]
        times: u32,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        until_failure: bool,
    },
    /// Fails without running the child for `seconds` after it last finished.
    Cooldown {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        child: Box<BtNode>,
        seconds: f32,
    },
    /// Aborts the child and fails if it runs longer than `seconds`.
    Timeout {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        child: Box<BtNode>,
        seconds: f32,
    },
    /// A blackboard key against a value: `==`, `!=`, `<`, `<=`, `>`, `>=`,
    /// `set` (present and not false, 0 or empty) or `unset`.
    Check {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        key: String,
        #[serde(default = "default_op")]
        op: String,
        #[serde(default)]
        value: Value,
    },
    /// Succeeds with probability `p`.
    Chance {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        p: f32,
    },
    /// Succeeds when the target is within `distance`; `store` keeps its
    /// position (`[x, y]`) under that key. A target is an entity's name, the
    /// nearest with a tag (`"tag:guard"`), or a blackboard key (`"@goal"`:
    /// a point, a name or an entity id).
    Near {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        target: String,
        distance: f32,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        store: String,
    },
    /// Runs for `seconds` (plus up to `jitter` more, at random).
    Wait {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        seconds: f32,
        #[serde(default)]
        jitter: f32,
    },
    /// Goes to within `arrive` of the target (or, `away`, until it is that
    /// far from it) at `speed`: through the entity's `nav_agent` when it has
    /// one, else straight - by its `velocity` when it has one, so a
    /// `face_velocity` turns it. The target is as `near`'s, or a point.
    MoveTo {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        target: Value,
        #[serde(default = "default_speed")]
        speed: f32,
        #[serde(default = "default_arrive")]
        arrive: f32,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        away: bool,
    },
    /// Writes a blackboard key (adds to it, with `add`).
    Set {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        key: String,
        value: Value,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        add: bool,
    },
    /// Writes any component field (`component`, a dotted `field`) on the
    /// entity or the named `target`, as a timeline track does.
    Write {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        target: String,
        component: String,
        field: String,
        value: Value,
    },
    /// Calls `on_<event>` on the entity's script and succeeds.
    Event {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        event: String,
    },
    /// Calls `on_<call>` on the entity's script and runs until the script
    /// writes `"success"` / `"failure"` (or `true` / `false`) to the
    /// blackboard key `<call>`. Aborted, the key reads `"aborted"`.
    Call {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
        call: String,
    },
}

impl Default for BtNode {
    fn default() -> Self {
        BtNode::Sequence { label: String::new(), reactive: false, children: Vec::new() }
    }
}

impl BtNode {
    fn children(&self) -> &[BtNode] {
        match self {
            BtNode::Sequence { children, .. }
            | BtNode::Selector { children, .. }
            | BtNode::Parallel { children, .. }
            | BtNode::Random { children, .. } => children,
            BtNode::Invert { child, .. }
            | BtNode::Succeed { child, .. }
            | BtNode::Repeat { child, .. }
            | BtNode::Cooldown { child, .. }
            | BtNode::Timeout { child, .. } => std::slice::from_ref(child.as_ref()),
            _ => &[],
        }
    }

    /// What the `_running` readout calls it.
    fn shown(&self) -> String {
        let label = match self {
            BtNode::Sequence { label, .. }
            | BtNode::Selector { label, .. }
            | BtNode::Parallel { label, .. }
            | BtNode::Random { label, .. }
            | BtNode::Invert { label, .. }
            | BtNode::Succeed { label, .. }
            | BtNode::Repeat { label, .. }
            | BtNode::Cooldown { label, .. }
            | BtNode::Timeout { label, .. }
            | BtNode::Check { label, .. }
            | BtNode::Chance { label, .. }
            | BtNode::Near { label, .. }
            | BtNode::Wait { label, .. }
            | BtNode::MoveTo { label, .. }
            | BtNode::Set { label, .. }
            | BtNode::Write { label, .. }
            | BtNode::Event { label, .. }
            | BtNode::Call { label, .. } => label,
        };
        if !label.is_empty() {
            return label.clone();
        }
        match self {
            BtNode::Wait { .. } => "wait".into(),
            BtNode::MoveTo { away: true, .. } => "flee".into(),
            BtNode::MoveTo { .. } => "move_to".into(),
            BtNode::Call { call, .. } => call.clone(),
            _ => "node".into(),
        }
    }
}

/// How many nodes a subtree holds, itself included: its children are the
/// next ids in pre-order.
fn size_of(node: &BtNode, sizes: &mut Vec<usize>) -> usize {
    let at = sizes.len();
    sizes.push(0);
    let mut n = 1;
    for c in node.children() {
        n += size_of(c, sizes);
    }
    sizes[at] = n;
    n
}

/// One node's progress. Reset when it finishes or is aborted - all but
/// `until`, a cooldown's, which outlives both.
#[derive(Debug, Clone, Default)]
struct NodeRun {
    active: bool,
    cursor: usize,
    timer: f32,
    limit: f32,
    count: u32,
    until: f32,
    /// A parallel's children: `None` still running, else whether it succeeded.
    results: Vec<Option<bool>>,
    /// A `move_to` drove the agent or the velocity, which an abort undoes.
    drove: Drove,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Drove {
    #[default]
    Nothing,
    Agent,
    Velocity,
}

impl NodeRun {
    fn reset(&mut self) {
        let until = self.until;
        *self = NodeRun { until, ..Default::default() };
    }
}

/// The tree's progress. Never saved.
#[derive(Debug, Clone, Default)]
pub struct BtState {
    runs: Vec<NodeRun>,
    sizes: Vec<usize>,
    clock: f32,
    rng: u64,
    /// The `_running` readout last written.
    shown: String,
    /// A tree that does not loop has finished.
    done: bool,
}

/// `behavior_tree`: a tree ticked once a frame. See the module docs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorTree {
    #[serde(default)]
    pub root: BtNode,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Start again from the root when it finishes (otherwise it stops and
    /// calls `on_tree_end`).
    #[serde(default = "yes", rename = "loop")]
    pub looping: bool,
    #[serde(skip)]
    pub state: BtState,
}

impl Default for BehaviorTree {
    fn default() -> Self {
        Self { root: BtNode::default(), enabled: true, looping: true, state: BtState::default() }
    }
}

/// The key the tree's readout and a script's blackboard live under.
pub const BLACKBOARD: &str = "Blackboard";

/// A blackboard key of `entity`.
pub fn blackboard(world: &World, entity: Entity, key: &str) -> Option<Value> {
    world.get::<&CustomComponents>(entity).ok()?.0.get(BLACKBOARD)?.get(key).cloned()
}

/// Write a blackboard key of `entity`, making the bag if it has none.
pub fn set_blackboard(world: &mut World, entity: Entity, key: &str, value: Value) {
    if world.get::<&CustomComponents>(entity).is_err() {
        let _ = world.insert_one(entity, CustomComponents::default());
    }
    if let Ok(mut c) = world.get::<&mut CustomComponents>(entity) {
        let bb = c.0.entry(BLACKBOARD.to_string()).or_insert_with(|| json!({}));
        if !bb.is_object() {
            *bb = json!({});
        }
        if let Some(map) = bb.as_object_mut() {
            map.insert(key.to_string(), value);
        }
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn compare(have: Option<&Value>, op: &str, want: &Value) -> bool {
    let num = |v: &Value| match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => s.parse().ok(),
        _ => None,
    };
    match op {
        "set" => have.is_some_and(truthy),
        "unset" => !have.is_some_and(truthy),
        "==" | "!=" => {
            let same = match (have, want) {
                (None, Value::Null) => true,
                (None, _) => false,
                (Some(h), w) => match (num(h), num(w)) {
                    (Some(a), Some(b)) if !h.is_string() || !w.is_string() => (a - b).abs() < 1e-6,
                    _ => h == w,
                },
            };
            same == (op == "==")
        }
        _ => {
            let (Some(a), Some(b)) = (have.and_then(num), num(want)) else { return false };
            match op {
                "<" => a < b,
                "<=" => a <= b,
                ">" => a > b,
                ">=" => a >= b,
                _ => false,
            }
        }
    }
}

fn position_of(world: &World, e: Entity) -> Option<Vec2> {
    if let Ok(w) = world.get::<&WorldTransform>(e) {
        return Some(w.position);
    }
    world.get::<&Transform>(e).ok().map(|t| t.position)
}

/// The entity a target string names, nearest to `from` when several do.
fn find_entity(world: &World, me: Entity, from: Vec2, target: &str) -> Option<Entity> {
    if target.is_empty() {
        return Some(me);
    }
    let mut best: Option<(f32, Entity)> = None;
    let mut consider = |e: Entity| {
        if e == me {
            return;
        }
        if let Some(p) = position_of(world, e) {
            let d = p.distance_squared(from);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, e));
            }
        }
    };
    if let Some(tag) = target.strip_prefix("tag:") {
        for (e, t) in world.query::<&Tags>().iter() {
            if t.0.contains(tag) {
                consider(e);
            }
        }
    } else {
        for (e, n) in world.query::<&Name>().iter() {
            if n.0 == target {
                consider(e);
            }
        }
    }
    best.map(|(_, e)| e)
}

/// Where a target is: a point, an entity by name / tag, or a blackboard
/// key holding either (or an entity id).
fn target_point(world: &World, me: Entity, from: Vec2, target: &Value) -> Option<Vec2> {
    match target {
        Value::Array(a) if a.len() >= 2 => Some(Vec2::new(a[0].as_f64()? as f32, a[1].as_f64()? as f32)),
        Value::String(s) => {
            if let Some(key) = s.strip_prefix('@') {
                let v = blackboard(world, me, key)?;
                if let Some(id) = v.as_u64() {
                    let e = world.iter().map(|r| r.entity()).find(|e| e.id() as u64 == id)?;
                    return position_of(world, e);
                }
                return target_point(world, me, from, &v);
            }
            position_of(world, find_entity(world, me, from, s)?)
        }
        _ => None,
    }
}

struct Ctx<'a> {
    world: &'a mut World,
    me: Entity,
    dt: f32,
    clock: f32,
    rng: &'a mut u64,
    running: Vec<String>,
}

impl Ctx<'_> {
    fn random(&mut self) -> f32 {
        // xorshift64*
        let mut x = *self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        *self.rng = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / (1u64 << 24) as f32
    }
    fn me_pos(&self) -> Vec2 {
        position_of(self.world, self.me).unwrap_or(Vec2::ZERO)
    }
}

/// The ids of a node's children.
fn child_ids(id: usize, n: usize, sizes: &[usize]) -> Vec<usize> {
    let mut ids = Vec::with_capacity(n);
    let mut next = id + 1;
    for _ in 0..n {
        ids.push(next);
        next += sizes[next];
    }
    ids
}

/// Stop a subtree that was running: undo what its leaves had started, and
/// clear its progress.
fn abort(node: &BtNode, id: usize, runs: &mut [NodeRun], sizes: &[usize], ctx: &mut Ctx) {
    if runs[id].active {
        match node {
            BtNode::MoveTo { .. } => stop_moving(runs[id].drove, ctx),
            BtNode::Call { call, .. } => set_blackboard(ctx.world, ctx.me, call, json!("aborted")),
            _ => {}
        }
        let ids = child_ids(id, node.children().len(), sizes);
        for (c, cid) in node.children().iter().zip(ids) {
            abort(c, cid, runs, sizes, ctx);
        }
    }
    runs[id].reset();
}

fn stop_moving(drove: Drove, ctx: &mut Ctx) {
    match drove {
        Drove::Agent => {
            crate::feature!("navigation", {
                let _ = crate::navigation::set_nav_target(ctx.world, ctx.me, None);
            });
        }
        Drove::Velocity => {
            if let Ok(mut v) = ctx.world.get::<&mut Velocity2D>(ctx.me) {
                v.x = 0.0;
                v.y = 0.0;
            }
        }
        Drove::Nothing => {}
    }
}

fn finish(runs: &mut [NodeRun], id: usize, s: Status) -> Status {
    if s != Status::Running {
        runs[id].reset();
    }
    s
}

fn tick(node: &BtNode, id: usize, runs: &mut [NodeRun], sizes: &[usize], ctx: &mut Ctx) -> Status {
    use Status::*;
    let kids = node.children();
    let ids = child_ids(id, kids.len(), sizes);
    match node {
        BtNode::Sequence { reactive, .. } | BtNode::Selector { reactive, .. } => {
            let is_seq = matches!(node, BtNode::Sequence { .. });
            // The answer that moves a sequence on is success; a selector's,
            // failure. The other ends it.
            let (go_on, ends) = if is_seq { (Success, Failure) } else { (Failure, Success) };
            let was = runs[id].active.then_some(runs[id].cursor);
            runs[id].active = true;
            let start = if *reactive { 0 } else { was.unwrap_or(0) };
            for i in start..kids.len() {
                let s = tick(&kids[i], ids[i], runs, sizes, ctx);
                if s == go_on {
                    continue;
                }
                // A child before the running one answered otherwise: the
                // running one is abandoned.
                if let Some(cur) = was
                    && i < cur
                    && cur < kids.len()
                {
                    abort(&kids[cur], ids[cur], runs, sizes, ctx);
                }
                if s == Running {
                    runs[id].cursor = i;
                    return Running;
                }
                return finish(runs, id, ends);
            }
            finish(runs, id, go_on)
        }
        BtNode::Parallel { succeed, .. } => {
            let n = kids.len();
            if !runs[id].active {
                runs[id].active = true;
                runs[id].results = vec![None; n];
            }
            for i in 0..n {
                if runs[id].results[i].is_none() {
                    match tick(&kids[i], ids[i], runs, sizes, ctx) {
                        Success => runs[id].results[i] = Some(true),
                        Failure => runs[id].results[i] = Some(false),
                        Running => {}
                    }
                }
            }
            let need = if *succeed == 0 { n } else { (*succeed).min(n) };
            let won = runs[id].results.iter().filter(|r| **r == Some(true)).count();
            let lost = runs[id].results.iter().filter(|r| **r == Some(false)).count();
            let verdict = if won >= need {
                Success
            } else if lost > n - need {
                Failure
            } else {
                return Running;
            };
            for i in 0..n {
                if runs[id].results[i].is_none() {
                    abort(&kids[i], ids[i], runs, sizes, ctx);
                }
            }
            finish(runs, id, verdict)
        }
        BtNode::Random { .. } => {
            if kids.is_empty() {
                return Failure;
            }
            if !runs[id].active {
                runs[id].active = true;
                runs[id].cursor = ((ctx.random() * kids.len() as f32) as usize).min(kids.len() - 1);
            }
            let i = runs[id].cursor;
            let s = tick(&kids[i], ids[i], runs, sizes, ctx);
            finish(runs, id, s)
        }
        BtNode::Invert { .. } | BtNode::Succeed { .. } => {
            runs[id].active = true;
            let s = tick(&kids[0], ids[0], runs, sizes, ctx);
            let s = match (node, s) {
                (_, Running) => Running,
                (BtNode::Invert { .. }, Success) => Failure,
                (BtNode::Invert { .. }, Failure) => Success,
                _ => Success,
            };
            finish(runs, id, s)
        }
        BtNode::Repeat { times, until_failure, .. } => {
            runs[id].active = true;
            match tick(&kids[0], ids[0], runs, sizes, ctx) {
                Running => Running,
                Failure if *until_failure => finish(runs, id, Success),
                Failure => finish(runs, id, Failure),
                // Once more - next frame, so a child that succeeds at once
                // cannot spin a frame forever.
                Success => {
                    runs[id].count += 1;
                    if !*until_failure && *times > 0 && runs[id].count >= *times {
                        finish(runs, id, Success)
                    } else {
                        Running
                    }
                }
            }
        }
        BtNode::Cooldown { seconds, .. } => {
            if !runs[id].active && ctx.clock < runs[id].until {
                return Failure;
            }
            runs[id].active = true;
            let s = tick(&kids[0], ids[0], runs, sizes, ctx);
            if s != Running {
                runs[id].until = ctx.clock + seconds;
            }
            finish(runs, id, s)
        }
        BtNode::Timeout { seconds, .. } => {
            runs[id].active = true;
            runs[id].timer += ctx.dt;
            if runs[id].timer > *seconds {
                abort(&kids[0], ids[0], runs, sizes, ctx);
                return finish(runs, id, Failure);
            }
            let s = tick(&kids[0], ids[0], runs, sizes, ctx);
            finish(runs, id, s)
        }
        BtNode::Check { key, op, value, .. } => {
            let have = blackboard(ctx.world, ctx.me, key);
            if compare(have.as_ref(), op, value) { Success } else { Failure }
        }
        BtNode::Chance { p, .. } => {
            if ctx.random() < *p { Success } else { Failure }
        }
        BtNode::Near { target, distance, store, .. } => {
            let from = ctx.me_pos();
            match target_point(ctx.world, ctx.me, from, &Value::String(target.clone())) {
                Some(p) if p.distance(from) <= *distance => {
                    if !store.is_empty() {
                        set_blackboard(ctx.world, ctx.me, store, json!([p.x, p.y]));
                    }
                    Success
                }
                _ => Failure,
            }
        }
        BtNode::Wait { seconds, jitter, .. } => {
            if !runs[id].active {
                runs[id].active = true;
                runs[id].timer = 0.0;
                runs[id].limit = seconds + jitter.max(0.0) * ctx.random();
            }
            runs[id].timer += ctx.dt;
            if runs[id].timer >= runs[id].limit {
                return finish(runs, id, Success);
            }
            ctx.running.push(node.shown());
            Running
        }
        BtNode::MoveTo { target, speed, arrive, away, .. } => move_to(node, id, runs, ctx, target, *speed, *arrive, *away),
        BtNode::Set { key, value, add, .. } => {
            let v = if *add {
                let have = blackboard(ctx.world, ctx.me, key).and_then(|v| v.as_f64()).unwrap_or(0.0);
                json!(have + value.as_f64().unwrap_or(0.0))
            } else {
                value.clone()
            };
            set_blackboard(ctx.world, ctx.me, key, v);
            Success
        }
        BtNode::Write { target, component, field, value, .. } => {
            let from = ctx.me_pos();
            let Some(e) = find_entity(ctx.world, ctx.me, from, target) else { return Failure };
            match crate::package::component_registry::write_field(ctx.world, e, component, field, value) {
                Ok(()) => Success,
                Err(err) => {
                    log::warn!("behavior_tree write: {err}");
                    Failure
                }
            }
        }
        BtNode::Event { event, .. } => {
            crate::ecs::callbacks::push(ctx.me, crate::ecs::callbacks::handler_for(event));
            Success
        }
        BtNode::Call { call, .. } => {
            if !runs[id].active {
                runs[id].active = true;
                set_blackboard(ctx.world, ctx.me, call, json!("running"));
                crate::ecs::callbacks::push(ctx.me, crate::ecs::callbacks::handler_for(call));
            }
            let s = match blackboard(ctx.world, ctx.me, call) {
                Some(Value::Bool(true)) => Success,
                Some(Value::Bool(false)) => Failure,
                Some(Value::String(s)) if s == "success" => Success,
                Some(Value::String(s)) if s == "failure" => Failure,
                _ => Running,
            };
            if s == Running {
                ctx.running.push(node.shown());
            }
            finish(runs, id, s)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn move_to(node: &BtNode, id: usize, runs: &mut [NodeRun], ctx: &mut Ctx, target: &Value, speed: f32, arrive: f32, away: bool) -> Status {
    let from = ctx.me_pos();
    let Some(goal) = target_point(ctx.world, ctx.me, from, target) else {
        stop_moving(runs[id].drove, ctx);
        return finish(runs, id, Status::Failure);
    };
    let d = goal.distance(from);
    if (!away && d <= arrive) || (away && d >= arrive) {
        stop_moving(runs[id].drove, ctx);
        return finish(runs, id, Status::Success);
    }
    let started = runs[id].active;
    runs[id].active = true;
    // A `nav_agent` walks the way there (`navigation`); without one - or in
    // a build without navigation - the leaf steers its velocity straight.
    #[cfg(feature = "navigation")]
    let has_agent = ctx.world.get::<&crate::navigation::NavAgent>(ctx.me).is_ok();
    #[cfg(not(feature = "navigation"))]
    let has_agent = false;
    let dir = if away {
        let v = from - goal;
        if v.length_squared() > 1e-6 { v.normalize() } else { Vec2::X }
    } else {
        (goal - from) / d.max(1e-6)
    };
    if has_agent {
        // Fleeing walks to a point past the distance wanted.
        let aim = if away { from + dir * (arrive - d + arrive * 0.5) } else { goal };
        #[cfg(feature = "navigation")]
        let current = ctx.world.get::<&crate::navigation::NavAgent>(ctx.me).ok().and_then(|a| a.target);
        #[cfg(not(feature = "navigation"))]
        let current: Option<[f32; 2]> = None;
        // The agent gave up short of the goal (it cannot get closer).
        if started && runs[id].drove == Drove::Agent && current.is_none() && runs[id].timer > 0.25 {
            return finish(runs, id, Status::Failure);
        }
        runs[id].timer += ctx.dt;
        let moved = current.is_none_or(|c| Vec2::from(c).distance(aim) > 4.0);
        if moved {
            crate::feature!("navigation", {
                let _ = crate::navigation::set_nav_target(ctx.world, ctx.me, Some([aim.x, aim.y]));
                if let Ok(mut a) = ctx.world.get::<&mut crate::navigation::NavAgent>(ctx.me) {
                    a.speed = speed;
                }
            });
            runs[id].timer = 0.0;
        }
        runs[id].drove = Drove::Agent;
    } else if let Ok(mut v) = ctx.world.get::<&mut Velocity2D>(ctx.me) {
        // Not past the goal in the next frame.
        let step = if away { speed } else { speed.min((d - arrive * 0.5).max(0.0) / ctx.dt.max(1e-4)) };
        v.x = dir.x * step;
        v.y = dir.y * step;
        runs[id].drove = Drove::Velocity;
    } else if let Ok(mut t) = ctx.world.get::<&mut Transform>(ctx.me) {
        let step = if away { speed * ctx.dt } else { (speed * ctx.dt).min(d) };
        t.position += dir * step;
    }
    ctx.running.push(node.shown());
    Status::Running
}

/// Tick every enabled tree once. See the module docs.
pub fn tick_behavior_trees(world: &mut World, dt: f32) {
    let owners: Vec<Entity> = world
        .query::<&BehaviorTree>()
        .iter()
        .filter(|(_, t)| t.enabled && !t.state.done)
        .map(|(e, _)| e)
        .collect();
    for e in owners {
        let (root, mut state, looping) = {
            let Ok(mut bt) = world.get::<&mut BehaviorTree>(e) else { continue };
            (std::mem::take(&mut bt.root), std::mem::take(&mut bt.state), bt.looping)
        };
        let mut sizes = Vec::new();
        size_of(&root, &mut sizes);
        if sizes != state.sizes {
            state.runs = vec![NodeRun::default(); sizes.len()];
            state.sizes = sizes;
        }
        if state.rng == 0 {
            state.rng = e.to_bits().get().wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        }
        state.clock += dt;
        let BtState { runs, sizes, clock, rng, .. } = &mut state;
        let mut ctx = Ctx { world, me: e, dt, clock: *clock, rng, running: Vec::new() };
        let s = tick(&root, 0, runs, sizes, &mut ctx);
        let shown = ctx.running.join(" + ");
        if s != Status::Running && !looping {
            state.done = true;
            crate::ecs::callbacks::push(e, "on_tree_end");
        }
        if shown != state.shown {
            set_blackboard(world, e, "_running", json!(shown));
            state.shown = shown;
        }
        if let Ok(mut bt) = world.get::<&mut BehaviorTree>(e) {
            // A `write` leaf may have replaced the component; keep what it
            // wrote unless it left the default root.
            if bt.root == BtNode::default() {
                bt.root = root;
            }
            bt.state = state;
        }
    }
}

/// `BehaviorTree.set(e, field, value)`: `root` (a new tree, started from
/// its beginning), `enabled`, `loop`, or `restart` (any value) to start the
/// tree again.
pub fn set_behavior_tree_field(world: &World, entity: Entity, field: &str, value: &Value) -> Result<(), String> {
    let Ok(mut bt) = world.get::<&mut BehaviorTree>(entity) else {
        return Err("no behavior_tree on that entity".into());
    };
    match field {
        "restart" => {
            bt.state = BtState::default();
            return Ok(());
        }
        "root" => {
            bt.root = serde_json::from_value(value.clone()).map_err(|e| format!("root: {e}"))?;
            bt.state = BtState::default();
        }
        "enabled" => bt.enabled = truthy(value),
        "loop" => {
            bt.looping = truthy(value);
            bt.state.done = false;
        }
        _ => return Err(format!("'{field}' is not a behavior_tree field (root, enabled, loop, restart)")),
    }
    Ok(())
}
