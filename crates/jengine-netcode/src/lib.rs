//! **Prediction-ready netcode**: the pieces a server and a client must agree
//! on, in one small crate that both link - so they cannot disagree.
//!
//! A server that decides everything is correct and feels late: press a key,
//! and the avatar moves when the server says so, a round trip and a tick
//! afterwards. The cure is old and exact:
//!
//! 1. The client sends **inputs**, numbered, one per fixed step.
//! 2. It applies each one **itself, at once**, with the same step function
//!    the server will use ([`Motion`]). That is *prediction*.
//! 3. The server applies them in order, one per tick ([`InputQueue`]), and
//!    says which it has got to (the *ack*) with the position that gave.
//! 4. The client takes that position as the truth, throws away the inputs
//!    it covers, and **replays the rest** on top ([`Reconciler`]). If the
//!    prediction was right - and with a shared step function it is, unless
//!    the server knew something the client did not - nothing moves. If it
//!    was wrong, the avatar is where the server says it must be, and the
//!    difference is eased out over a few frames rather than snapped.
//!
//! Everybody else is *not* predicted: their positions arrive late and are
//! drawn a little in the past, which is what the client package already does.
//!
//! And because every client sees the others in the past, a server judging
//! "did that shot hit" at its own present would miss what the shooter was
//! plainly aiming at. [`History`] keeps where things were, so the server can
//! ask the question at the tick the shooter saw (*lag compensation*).
//!
//! # What has to be shared
//!
//! The step function, its numbers, and the length of a step. That is why
//! this is a crate of its own with no dependencies: `jengine-server` rooms
//! and the `jengine-online` client package both link it, and a room tells
//! the client its numbers (`"netcode"` on the `joined` frame) rather than
//! the client being built with a copy.
//!
//! Two models are built in. [`TopDown`] is a direction and a speed, inside
//! bounds - a town, a twin-stick shooter. [`TileMotion`] adds [`Walls`]: a
//! grid of solid cells the avatar is stopped by and slides along, which the
//! server sends with its numbers so the client predicts against the same
//! walls - and which the server can build from the engine's own `Tilemap`,
//! so its monsters path (with the engine's A*) through the very walls the
//! players are predicted against. A game with rules of its own implements
//! [`Motion`] in a crate its server and its client package both use.

use std::collections::VecDeque;

/// One step's worth of what a player wants: a direction, and its number.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Input {
    /// Counts up from 1. The server says which it has reached; the client
    /// replays the ones after.
    pub seq: u32,
    /// The direction, each component -1 to 1. Longer than 1 is shortened to
    /// 1, so a diagonal is not faster than a straight line - and a client
    /// that claims to push harder gains nothing.
    pub x: f64,
    pub y: f64,
}

/// A position. Everything here is two numbers.
pub type Point = (f64, f64);

/// The step function both ends run. **It must be a pure function of its
/// arguments**: no clocks, no randomness, no state of its own - or the
/// client's replay and the server's tick give different answers and the
/// avatar twitches for ever.
pub trait Motion {
    fn step(&self, at: Point, input: &Input, dt: f64) -> Point;
}

/// Move in a direction at a speed, and stay inside a rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TopDown {
    /// Units a second at full tilt.
    pub speed: f64,
    pub min: Point,
    pub max: Point,
}

impl Motion for TopDown {
    fn step(&self, at: Point, input: &Input, dt: f64) -> Point {
        let (mut x, mut y) = (input.x, input.y);
        if !x.is_finite() || !y.is_finite() {
            return at;
        }
        let length = x.hypot(y);
        if length > 1.0 {
            (x, y) = (x / length, y / length);
        }
        (
            (at.0 + x * self.speed * dt).clamp(self.min.0, self.max.0),
            (at.1 + y * self.speed * dt).clamp(self.min.1, self.max.1),
        )
    }
}

// ---- a world with walls --------------------------------------------------------------

/// A grid of cells, some solid: the walls both ends must agree on.
///
/// A server builds one from whatever holds its map - the engine's `Tilemap`,
/// an ASCII drawing - and sends it with the room's numbers ([`Walls::packed`]
/// is the wire form, one character a cell); a client rebuilds it
/// ([`Walls::unpack`]) and predicts against the same walls. Nothing here
/// knows what a tilemap is, which is what lets a server link the engine's
/// `jengine-core` for its pathfinding while this crate stays free of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Walls {
    pub cols: usize,
    pub rows: usize,
    /// A cell's side, in world units.
    pub cell: f64,
    /// The world position of the top-left corner of cell `(0, 0)`; rows run
    /// **down** from it (`y` decreasing), as a tilemap's do.
    pub origin: Point,
    /// Row-major, `rows * cols` long.
    pub solid: Vec<bool>,
}

impl Walls {
    /// No walls at all: a `cols` x `rows` grid of open cells.
    pub fn open(cols: usize, rows: usize, cell: f64, origin: Point) -> Self {
        Walls { cols, rows, cell, origin, solid: vec![false; cols * rows] }
    }

    /// From a drawing, one string a row, top row first: `#` is solid, anything
    /// else open. Rows are padded to the longest.
    pub fn from_rows(rows: &[&str], cell: f64, origin: Point) -> Self {
        let cols = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
        let mut walls = Walls::open(cols, rows.len(), cell, origin);
        for (r, row) in rows.iter().enumerate() {
            for (c, ch) in row.chars().enumerate() {
                walls.solid[r * cols + c] = ch == '#';
            }
        }
        walls
    }

    pub fn is_solid(&self, col: i64, row: i64) -> bool {
        if col < 0 || row < 0 || col as usize >= self.cols || row as usize >= self.rows {
            return false;
        }
        self.solid[row as usize * self.cols + col as usize]
    }

    pub fn set_solid(&mut self, col: usize, row: usize, solid: bool) {
        if col < self.cols && row < self.rows {
            self.solid[row * self.cols + col] = solid;
        }
    }

    /// The cell a point is in (it may be outside the grid).
    pub fn cell_of(&self, at: Point) -> (i64, i64) {
        (((at.0 - self.origin.0) / self.cell).floor() as i64, ((self.origin.1 - at.1) / self.cell).floor() as i64)
    }

    /// The middle of a cell.
    pub fn centre(&self, col: i64, row: i64) -> Point {
        (self.origin.0 + (col as f64 + 0.5) * self.cell, self.origin.1 - (row as f64 + 0.5) * self.cell)
    }

    /// The wire form: `0` open, `1` solid, row by row.
    pub fn packed(&self) -> String {
        self.solid.iter().map(|s| if *s { '1' } else { '0' }).collect()
    }

    /// Back from [`Walls::packed`]. `None` if the string is not `cols * rows` long.
    pub fn unpack(cols: usize, rows: usize, cell: f64, origin: Point, packed: &str) -> Option<Self> {
        if packed.len() != cols * rows || cell <= 0.0 {
            return None;
        }
        Some(Walls { cols, rows, cell, origin, solid: packed.chars().map(|c| c == '1').collect() })
    }

    /// The cells that are solid, as `(col, row)` - for a server spawning a
    /// block entity per wall so clients can draw them.
    pub fn solid_cells(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        (0..self.rows)
            .flat_map(move |r| (0..self.cols).filter(move |c| self.solid[r * self.cols + c]).map(move |c| (c, r)))
    }
}

/// A square avatar of `radius` half-width that moves at `speed` and is
/// stopped by [`Walls`] - and stays inside a rectangle, as [`TopDown`] does.
///
/// The move is resolved one axis at a time: the avatar is moved along x and
/// pushed back to the face of any solid cell it overlaps, then the same
/// along y. That is how a character controller slides along a wall rather
/// than sticking to it, and it is a pure function of its arguments, so both
/// ends get the same answer from it.
#[derive(Debug, Clone, PartialEq)]
pub struct TileMotion {
    pub speed: f64,
    /// Half the avatar's width.
    pub radius: f64,
    pub min: Point,
    pub max: Point,
    pub walls: Walls,
}

impl TileMotion {
    /// The solid cells a square at `at` overlaps.
    fn overlapped(&self, at: Point) -> Vec<(i64, i64)> {
        let r = self.radius - 1e-9;
        let (c0, r0) = self.walls.cell_of((at.0 - r, at.1 + r));
        let (c1, r1) = self.walls.cell_of((at.0 + r, at.1 - r));
        let mut cells = Vec::new();
        for row in r0..=r1 {
            for col in c0..=c1 {
                if self.walls.is_solid(col, row) {
                    cells.push((col, row));
                }
            }
        }
        cells
    }

    /// Where a move from `from` to `to` ends against the walls: along x
    /// first, then y, each backed out to the face of what it hit. What
    /// [`Motion::step`] does after working out `to` - and what a server
    /// moving something itself (a monster, a click-to-walk) uses so that
    /// nothing it moves goes through a wall either.
    pub fn resolve(&self, from: Point, to: Point) -> Point {
        let moved_x = self.slide(from, (to.0, from.1), true);
        self.slide(moved_x, (moved_x.0, to.1), false)
    }

    /// Move one axis, and if that overlaps a wall, back out to the nearest
    /// face of the walls hit, in the direction the move came from.
    fn slide(&self, from: Point, to: Point, along_x: bool) -> Point {
        let hit = self.overlapped(to);
        if hit.is_empty() {
            return to;
        }
        let (cell, origin) = (self.walls.cell, self.walls.origin);
        if along_x {
            let x = if to.0 > from.0 {
                let face = hit.iter().map(|(col, _)| origin.0 + *col as f64 * cell).fold(f64::INFINITY, f64::min);
                (face - self.radius).max(from.0)
            } else {
                let face =
                    hit.iter().map(|(col, _)| origin.0 + (*col + 1) as f64 * cell).fold(f64::NEG_INFINITY, f64::max);
                (face + self.radius).min(from.0)
            };
            (x, to.1)
        } else {
            let y = if to.1 > from.1 {
                let face =
                    hit.iter().map(|(_, row)| origin.1 - (*row + 1) as f64 * cell).fold(f64::INFINITY, f64::min);
                (face - self.radius).max(from.1)
            } else {
                let face = hit.iter().map(|(_, row)| origin.1 - *row as f64 * cell).fold(f64::NEG_INFINITY, f64::max);
                (face + self.radius).min(from.1)
            };
            (to.0, y)
        }
    }
}

impl Motion for TileMotion {
    fn step(&self, at: Point, input: &Input, dt: f64) -> Point {
        let (mut x, mut y) = (input.x, input.y);
        if !x.is_finite() || !y.is_finite() {
            return at;
        }
        let length = x.hypot(y);
        if length > 1.0 {
            (x, y) = (x / length, y / length);
        }
        let wanted = (
            (at.0 + x * self.speed * dt).clamp(self.min.0, self.max.0),
            (at.1 + y * self.speed * dt).clamp(self.min.1, self.max.1),
        );
        self.resolve(at, wanted)
    }
}

/// The models a room can name on its `joined` frame, so a client that links
/// nothing but this crate can predict against any of them.
#[derive(Debug, Clone, PartialEq)]
pub enum BuiltIn {
    TopDown(TopDown),
    Tiles(TileMotion),
}

impl Motion for BuiltIn {
    fn step(&self, at: Point, input: &Input, dt: f64) -> Point {
        match self {
            BuiltIn::TopDown(m) => m.step(at, input, dt),
            BuiltIn::Tiles(m) => m.step(at, input, dt),
        }
    }
}

impl<M: Motion + ?Sized> Motion for Box<M> {
    fn step(&self, at: Point, input: &Input, dt: f64) -> Point {
        (**self).step(at, input, dt)
    }
}

// ---- the server's side ------------------------------------------------------------

/// A player's inputs, waiting their turn: **one is applied per tick.**
///
/// Inputs arrive in clumps and gaps - that is what a network does to a
/// steady stream. Applying each as it lands would move the avatar by fits;
/// queueing them and taking one a tick gives back the steady stream, a
/// little late. The queue is short on purpose: a client that sends faster
/// than the server ticks is not allowed to bank movement for later.
#[derive(Debug, Clone, Default)]
pub struct InputQueue {
    waiting: VecDeque<Input>,
    /// The number of the last input applied.
    acked: u32,
}

/// Inputs that may wait. At ten ticks a second this is most of a second of
/// lag absorbed; past it the oldest are dropped, which the player sees as
/// the server catching up rather than as a replay of stale intentions.
pub const MAX_WAITING: usize = 8;

impl InputQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take an input from the client. One that is old, or out of order, is
    /// ignored: numbers only go up.
    pub fn push(&mut self, input: Input) {
        let newest = self.waiting.back().map(|i| i.seq).unwrap_or(self.acked);
        if input.seq <= newest {
            return;
        }
        if self.waiting.len() >= MAX_WAITING {
            self.waiting.pop_front();
        }
        self.waiting.push_back(input);
    }

    /// This tick's input, if one is waiting.
    pub fn take(&mut self) -> Option<Input> {
        let input = self.waiting.pop_front()?;
        self.acked = input.seq;
        Some(input)
    }

    /// The last input applied - what the client is told, with the position.
    pub fn acked(&self) -> u32 {
        self.acked
    }

    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }
}

/// Where things were, for the last so many ticks.
///
/// A client draws everyone else a little in the past. When it says "I fired
/// at tick 412", the fair question is where the target was *at 412*, not
/// where the server has moved it to since.
///
/// Only what **moved** is kept for each tick: a crate that never stirs costs
/// one entry however long it is remembered, not one a tick.
#[derive(Debug, Clone)]
pub struct History {
    keep: usize,
    /// (tick, what changed then: an id's new position, or `None` for gone)
    /// - oldest first.
    ticks: VecDeque<(u64, Vec<(u64, Option<Point>)>)>,
    /// Where everything was just before the oldest tick kept.
    base: std::collections::HashMap<u64, Point>,
    /// Where everything is as of the newest tick, to tell what moved.
    last: std::collections::HashMap<u64, Point>,
}

impl History {
    /// Remember this many ticks. A second's worth is plenty: a player
    /// further behind than that has other problems.
    pub fn new(keep: usize) -> Self {
        History { keep: keep.max(1), ticks: VecDeque::new(), base: Default::default(), last: Default::default() }
    }

    pub fn record(&mut self, tick: u64, positions: impl IntoIterator<Item = (u64, Point)>) {
        if self.ticks.len() >= self.keep
            && let Some((_, changes)) = self.ticks.pop_front()
        {
            for (id, at) in changes {
                match at {
                    Some(p) => self.base.insert(id, p),
                    None => self.base.remove(&id),
                };
            }
        }
        let now: std::collections::HashMap<u64, Point> = positions.into_iter().collect();
        let mut changes: Vec<(u64, Option<Point>)> =
            now.iter().filter(|(id, p)| self.last.get(id) != Some(p)).map(|(id, p)| (*id, Some(*p))).collect();
        changes.extend(self.last.keys().filter(|id| !now.contains_key(id)).map(|id| (*id, None)));
        changes.sort_unstable_by_key(|(id, _)| *id);
        self.last = now;
        self.ticks.push_back((tick, changes));
    }

    /// Where `id` was at `tick`. `None` for a tick older than is
    /// remembered - a claim to have seen further back than that is not
    /// believed, and the room decides what to do instead (judge it now, or
    /// not at all) - and for an entity that was not there. A tick from the
    /// future answers with the newest.
    pub fn at(&self, id: u64, tick: u64) -> Option<Point> {
        if tick < self.oldest()? {
            return None;
        }
        for (_, changes) in self.ticks.iter().rev().filter(|(t, _)| *t <= tick) {
            if let Ok(i) = changes.binary_search_by_key(&id, |(i, _)| *i) {
                return changes[i].1;
            }
        }
        self.base.get(&id).copied()
    }

    pub fn oldest(&self) -> Option<u64> {
        self.ticks.front().map(|(t, _)| *t)
    }

    /// How many positions are held, all ticks together - what remembering
    /// costs.
    pub fn held(&self) -> usize {
        self.base.len() + self.last.len() + self.ticks.iter().map(|(_, c)| c.len()).sum::<usize>()
    }
}

// ---- the client's side ------------------------------------------------------------

/// How fast a correction is eased out, per second. At 12 a wrong guess is
/// nine-tenths gone in a fifth of a second.
const EASE: f64 = 12.0;
/// A correction longer than this is not eased: the avatar is somewhere else
/// (a teleport, a door), and gliding there would be a lie.
const SNAP: f64 = 200.0;

/// The client's own avatar: predicted now, corrected when the server speaks.
#[derive(Debug, Clone)]
pub struct Reconciler<M: Motion> {
    motion: M,
    /// The length of a step: the same number the server moves by.
    dt: f64,
    /// Inputs sent and not yet acknowledged.
    pending: VecDeque<Input>,
    /// Where the avatar is, by our reckoning, after every input so far.
    predicted: Point,
    /// Where it was one step ago, to draw between steps.
    before: Point,
    /// What is still being eased out after a correction.
    error: Point,
    /// How far the server's word may differ from ours and still be taken as
    /// agreement. Positions cross the wire rounded; a difference that small
    /// is the rounding, and correcting for it would be a twitch every tick.
    slack: f64,
    next_seq: u32,
}

impl<M: Motion> Reconciler<M> {
    pub fn new(motion: M, dt: f64, at: Point) -> Self {
        Reconciler {
            motion,
            dt: dt.max(1e-3),
            pending: VecDeque::new(),
            predicted: at,
            before: at,
            error: (0.0, 0.0),
            slack: 0.0,
            next_seq: 1,
        }
    }

    /// Treat a difference this small between the server's position and ours
    /// as agreement - the size of whatever rounding the wire does.
    pub fn with_slack(mut self, slack: f64) -> Self {
        self.slack = slack.max(0.0);
        self
    }

    /// Number this reconciler's inputs from `seq` on, rather than from 1.
    ///
    /// The server's [`InputQueue`] drops anything not newer than what it has
    /// already applied, so a client that makes a fresh reconciler for each
    /// spell of steering - as `jengine-online` does, letting go of it once
    /// the avatar has been at rest - must carry the count over
    /// ([`Reconciler::next_seq`]), or every walk after the first is dropped
    /// as old: the client walks alone and nobody else sees it.
    pub fn starting_at(mut self, seq: u32) -> Self {
        self.next_seq = seq.max(1);
        self
    }

    /// The number the next input will carry.
    pub fn next_seq(&self) -> u32 {
        self.next_seq
    }

    /// Change how the avatar moves from the next step - a wall built or
    /// broken while the player is in the room. What was predicted under the
    /// old rule stands until the server's next word corrects it.
    pub fn set_motion(&mut self, motion: M) {
        self.motion = motion;
    }

    /// A step with no input: the avatar stands, and is drawn standing.
    pub fn rest(&mut self) {
        self.before = self.predicted;
    }

    pub fn dt(&self) -> f64 {
        self.dt
    }

    /// One fixed step of input: applied here at once, and returned to be
    /// sent. Call it once per step, not once per frame.
    pub fn input(&mut self, x: f64, y: f64) -> Input {
        let input = Input { seq: self.next_seq, x, y };
        self.next_seq += 1;
        self.before = self.predicted;
        self.predicted = self.motion.step(self.predicted, &input, self.dt);
        self.pending.push_back(input);
        input
    }

    /// The server's word: the avatar was at `at` after input `acked`. Start
    /// from there and replay what it has not seen yet.
    pub fn server(&mut self, at: Point, acked: u32) {
        while self.pending.front().is_some_and(|i| i.seq <= acked) {
            self.pending.pop_front();
        }
        let was = self.predicted;
        let mut now = at;
        for input in &self.pending {
            now = self.motion.step(now, input, self.dt);
        }
        if (was.0 - now.0).hypot(was.1 - now.1) <= self.slack {
            // The same answer, give or take the wire's rounding: ours is the
            // unrounded one, so it is kept.
            return;
        }
        self.predicted = now;
        // What is drawn must not jump: whatever the correction moved the
        // avatar by is carried as an offset, and eased away.
        let moved = (was.0 - now.0, was.1 - now.1);
        self.before = (self.before.0 - moved.0, self.before.1 - moved.1);
        self.error = (self.error.0 + moved.0, self.error.1 + moved.1);
        if self.error.0.hypot(self.error.1) > SNAP {
            self.error = (0.0, 0.0);
            self.before = now;
        }
    }

    /// Ease the last correction out. Call once a frame.
    pub fn ease(&mut self, frame_dt: f64) {
        let keep = (-EASE * frame_dt).exp();
        self.error = (self.error.0 * keep, self.error.1 * keep);
        if self.error.0.hypot(self.error.1) < 0.01 {
            self.error = (0.0, 0.0);
        }
    }

    /// Where to draw the avatar: `alpha` is how far the frame is into the
    /// current step, 0 to 1, so it moves every frame and not ten times a
    /// second.
    pub fn position(&self, alpha: f64) -> Point {
        let alpha = alpha.clamp(0.0, 1.0);
        (
            self.before.0 + (self.predicted.0 - self.before.0) * alpha + self.error.0,
            self.before.1 + (self.predicted.1 - self.before.1) * alpha + self.error.1,
        )
    }

    /// Where the avatar is by our reckoning, with nothing eased or drawn between.
    pub fn predicted(&self) -> Point {
        self.predicted
    }

    /// Inputs the server has not acknowledged.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// How far what is drawn still is from what is predicted.
    pub fn error(&self) -> f64 {
        self.error.0.hypot(self.error.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIELD: TopDown = TopDown { speed: 100.0, min: (0.0, 0.0), max: (1000.0, 1000.0) };
    const DT: f64 = 0.1;

    /// A server, in a dozen lines: a queue, a position, one input a tick.
    struct Server {
        queue: InputQueue,
        at: Point,
        /// A wall the client does not know about, if any.
        wall: Option<f64>,
    }

    impl Server {
        fn tick(&mut self) -> (Point, u32) {
            if let Some(input) = self.queue.take() {
                self.at = FIELD.step(self.at, &input, DT);
                if let Some(wall) = self.wall {
                    self.at.0 = self.at.0.min(wall);
                }
            }
            (self.at, self.queue.acked())
        }
    }

    #[test]
    fn a_diagonal_is_no_faster_and_the_field_has_edges() {
        let straight = FIELD.step((500.0, 500.0), &Input { seq: 1, x: 1.0, y: 0.0 }, 1.0);
        let diagonal = FIELD.step((500.0, 500.0), &Input { seq: 1, x: 1.0, y: 1.0 }, 1.0);
        assert_eq!(straight, (600.0, 500.0));
        let moved = (diagonal.0 - 500.0).hypot(diagonal.1 - 500.0);
        assert!((moved - 100.0).abs() < 1e-9, "{moved}");
        // Pushing "harder" than 1 is pushing at 1.
        assert_eq!(FIELD.step((500.0, 500.0), &Input { seq: 1, x: 50.0, y: 0.0 }, 1.0), straight);
        assert_eq!(FIELD.step((990.0, 0.0), &Input { seq: 1, x: 1.0, y: -1.0 }, 1.0), (1000.0, 0.0));
        assert_eq!(FIELD.step((5.0, 5.0), &Input { seq: 1, x: f64::NAN, y: 0.0 }, 1.0), (5.0, 5.0));
    }

    /// The whole point: with a shared step function, the server's answer -
    /// arriving late - changes nothing the client had already drawn.
    #[test]
    fn a_right_prediction_is_never_corrected() {
        let mut server = Server { queue: InputQueue::new(), at: (500.0, 500.0), wall: None };
        let mut client = Reconciler::new(FIELD, DT, (500.0, 500.0));
        let mut in_flight: VecDeque<Input> = VecDeque::new();
        for step in 0..20 {
            let input = client.input(1.0, 0.0);
            assert_eq!(client.predicted().0, 500.0 + 10.0 * f64::from(step + 1), "moved at once, not a round trip later");
            in_flight.push_back(input);
            // Three steps of lag: the server sees each input late.
            if in_flight.len() > 3 {
                server.queue.push(in_flight.pop_front().unwrap());
            }
            let (at, acked) = server.tick();
            let before = client.predicted();
            client.server(at, acked);
            assert_eq!(client.predicted(), before, "the server agreed, so nothing moved");
            assert_eq!(client.error(), 0.0);
        }
        assert_eq!(client.pending(), 3, "the three still on their way");
    }

    #[test]
    fn a_wrong_prediction_ends_where_the_server_says_and_is_eased_not_snapped() {
        let mut server = Server { queue: InputQueue::new(), at: (500.0, 500.0), wall: Some(520.0) };
        let mut client = Reconciler::new(FIELD, DT, (500.0, 500.0));
        // Five steps right, all sent before any answer: the client thinks 550.
        let inputs: Vec<Input> = (0..5).map(|_| client.input(1.0, 0.0)).collect();
        assert_eq!(client.predicted().0, 550.0);
        for input in inputs {
            server.queue.push(input);
        }
        let mut answer = server.tick();
        for _ in 0..4 {
            answer = server.tick();
        }
        assert_eq!(answer, ((520.0, 500.0), 5), "the wall stopped it");

        let drawn_before = client.position(1.0);
        client.server(answer.0, answer.1);
        assert_eq!(client.predicted(), (520.0, 500.0), "the server's word is where it is");
        assert_eq!(client.position(1.0), drawn_before, "but what is drawn has not jumped");
        for _ in 0..60 {
            client.ease(1.0 / 60.0);
        }
        assert_eq!(client.position(1.0), (520.0, 500.0), "and a second later it is there");
    }

    #[test]
    fn inputs_are_applied_one_a_tick_in_order_and_old_ones_are_ignored() {
        let mut queue = InputQueue::new();
        for seq in [1, 2, 2, 1, 3] {
            queue.push(Input { seq, x: 1.0, y: 0.0 });
        }
        assert_eq!(queue.waiting(), 3, "the repeats were dropped");
        assert_eq!([queue.take().unwrap().seq, queue.take().unwrap().seq, queue.take().unwrap().seq], [1, 2, 3]);
        assert!(queue.take().is_none(), "no input, no movement");
        assert_eq!(queue.acked(), 3);
        queue.push(Input { seq: 2, x: 1.0, y: 0.0 });
        assert_eq!(queue.waiting(), 0, "an input from the past is not replayed");

        // A client that sends faster than the server ticks banks nothing.
        for seq in 4..40 {
            queue.push(Input { seq, x: 1.0, y: 0.0 });
        }
        assert_eq!(queue.waiting(), MAX_WAITING);
        assert_eq!(queue.take().unwrap().seq, 40 - MAX_WAITING as u32, "the oldest were let go");
    }

    #[test]
    fn drawing_between_steps_moves_every_frame() {
        let mut client = Reconciler::new(FIELD, DT, (0.0, 0.0));
        client.input(1.0, 0.0);
        assert_eq!(client.position(0.0), (0.0, 0.0));
        assert_eq!(client.position(0.5), (5.0, 0.0));
        assert_eq!(client.position(1.0), (10.0, 0.0));
    }

    #[test]
    fn rounding_on_the_wire_is_not_a_correction() {
        let mut client = Reconciler::new(FIELD, DT, (0.0, 0.0)).with_slack(0.02);
        client.input(0.6, 0.8);
        let ours = client.predicted();
        // The server computed the same and sent it to two decimals.
        client.server(((ours.0 * 100.0).round() / 100.0 + 0.004, (ours.1 * 100.0).round() / 100.0), 1);
        assert_eq!(client.predicted(), ours);
        assert_eq!(client.error(), 0.0);
        // A real difference is still a difference.
        client.server((ours.0 + 5.0, ours.1), 1);
        assert_eq!(client.predicted().0, ours.0 + 5.0);
    }

    #[test]
    fn a_teleport_is_not_glided_to() {
        let mut client = Reconciler::new(FIELD, DT, (0.0, 0.0));
        client.server((900.0, 900.0), 0);
        assert_eq!(client.position(1.0), (900.0, 900.0));
    }

    #[test]
    fn history_answers_for_the_tick_that_was_seen() {
        let mut history = History::new(3);
        for tick in 10..=14 {
            history.record(tick, [(7, (tick as f64, 0.0)), (3, (0.0, tick as f64))]);
        }
        assert_eq!(history.oldest(), Some(12), "only so much is remembered");
        assert_eq!(history.at(7, 13), Some((13.0, 0.0)));
        assert_eq!(history.at(3, 13), Some((0.0, 13.0)));
        assert_eq!(history.at(7, 5), None, "further back than that is not believed, and says so");
        assert_eq!(history.at(7, 99), Some((14.0, 0.0)));
        assert_eq!(history.at(42, 13), None);
    }

    /// A thing that never moves is one entry, not one a tick; one that
    /// leaves is gone from the tick it left, and was there before it.
    #[test]
    fn history_keeps_only_what_moved() {
        let mut history = History::new(60);
        for tick in 0..60 {
            let crates = (100..200).map(|id| (id, (id as f64, 0.0)));
            let wolf = (tick < 50).then_some((7, (tick as f64, 0.0)));
            history.record(tick, crates.chain(wolf));
        }
        assert!(history.held() < 400, "a hundred still crates for sixty ticks: {}", history.held());
        assert_eq!(history.at(150, 3), Some((150.0, 0.0)));
        assert_eq!(history.at(7, 30), Some((30.0, 0.0)));
        assert_eq!(history.at(7, 49), Some((49.0, 0.0)));
        assert_eq!(history.at(7, 50), None, "gone");
        // Past the window, the still crate is still answered from the base.
        for tick in 60..200 {
            history.record(tick, (100..200).map(|id| (id, (id as f64, 0.0))));
        }
        assert_eq!(history.at(150, 150), Some((150.0, 0.0)));
        assert_eq!(history.at(7, 150), None);
    }

    #[test]
    fn walls_stop_and_slide_a_steered_avatar() {
        let walls = Walls::from_rows(&["....", "..#.", "...."], 10.0, (0.0, 30.0));
        assert!(walls.is_solid(2, 1) && !walls.is_solid(1, 1) && !walls.is_solid(9, 9));
        assert_eq!(walls.cell_of((25.0, 15.0)), (2, 1));
        assert_eq!(walls.centre(2, 1), (25.0, 15.0));
        let motion = TileMotion { speed: 100.0, radius: 2.0, min: (0.0, 0.0), max: (40.0, 30.0), walls: walls.clone() };
        // Walking east into the block from its left: stopped at its face, less the radius.
        let east = Input { seq: 1, x: 1.0, y: 0.0 };
        let mut at = (10.0, 15.0);
        for _ in 0..5 {
            at = motion.step(at, &east, 0.1);
        }
        assert_eq!(at, (18.0, 15.0), "the block starts at x = 20");
        // Diagonally into it: the x is stopped, the y still moves - a slide.
        let slid = motion.step(at, &Input { seq: 2, x: 1.0, y: 1.0 }, 0.1);
        assert_eq!(slid.0, 18.0);
        assert!(slid.1 > 15.0);
        // Coming down onto it from above stops at its top face.
        let mut at = (25.0, 28.0);
        for _ in 0..5 {
            at = motion.step(at, &Input { seq: 3, x: 0.0, y: -1.0 }, 0.1);
        }
        assert_eq!(at, (25.0, 22.0), "the block's top is y = 20");
        // The rectangle still holds.
        let out = motion.step((39.0, 5.0), &east, 1.0);
        assert_eq!(out.0, 40.0);
        // The wire form round-trips.
        let again = Walls::unpack(walls.cols, walls.rows, walls.cell, walls.origin, &walls.packed()).unwrap();
        assert_eq!(again, walls);
        assert_eq!(walls.solid_cells().collect::<Vec<_>>(), [(2, 1)]);
        assert!(Walls::unpack(4, 3, 10.0, (0.0, 0.0), "01").is_none());
    }

    #[test]
    fn a_reconciler_replays_over_tile_motion_too() {
        let walls = Walls::from_rows(&["..#"], 10.0, (0.0, 10.0));
        let motion = BuiltIn::Tiles(TileMotion { speed: 100.0, radius: 2.0, min: (0.0, 0.0), max: (30.0, 10.0), walls });
        let mut client = Reconciler::new(motion.clone(), 0.1, (5.0, 5.0));
        for _ in 0..4 {
            client.input(1.0, 0.0);
        }
        // The server's answer for input 2 is what the same motion says.
        let mut server_at = (5.0, 5.0);
        for seq in 1..=2 {
            server_at = motion.step(server_at, &Input { seq, x: 1.0, y: 0.0 }, 0.1);
        }
        client.server(server_at, 2);
        assert_eq!(client.error(), 0.0, "the replay agreed");
        assert_eq!(client.predicted(), (18.0, 5.0), "and the wall holds it at the same place both ends");
    }
}
