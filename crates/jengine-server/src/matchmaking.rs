//! **A queue that matches players, then opens their room** - for a game
//! where any opponent will not do.
//!
//! A [`Matchmaker`] is an ordinary kind of room. Players join it saying
//! what they bring (`{"rating":1520,"region":"eu"}`); every tick it hands
//! everyone waiting to a rule that groups them; each group is sent, on the
//! connection they are on, into a fresh room of the match kind - the same
//! `transfer` a door between zones is - with what the queue knew of them in
//! its options, so the match's own `on_join` can seat them.
//!
//! ```no_run
//! # use jengine_server::{KindOptions, Server, matchmaking::Matchmaker};
//! # struct Duel;
//! # impl jengine_server::Room for Duel {
//! #     fn on_join(&mut self, _: &mut jengine_server::RoomCtx, _: &mut jengine_server::Player, _: &serde_json::Value)
//! #         -> Result<serde_json::Value, String> { Ok(serde_json::Value::Null) }
//! # }
//! Server::new()
//!     .room("duel", KindOptions::default().max_players(2), |_| Duel)
//!     // Two at a time, by rating: within 100 to start with, widening by 25
//!     // a second for whoever has waited longest, one region unless a wait
//!     // passes 20 seconds.
//!     .room("queue", KindOptions::ticking(4.0).max_players(10_000).persistent(), |_| {
//!         Matchmaker::by_rating("duel", 2).window(100.0, 25.0).region_patience(20.0)
//!     });
//! ```
//!
//! A client joins `{"t":"join","kind":"queue","room":"ranked","options":{"rating":1520}}`,
//! hears `{"t":"queued","waiting":n}` as the queue changes, and then the
//! match's own `joined`. Leaving the queue is leaving the room. The rule is
//! the game's: [`Matchmaker::new`] takes any function from who is waiting
//! to who meets whom.

use serde_json::{Value, json};

use crate::room::{JoinRequest, Player, PlayerId, Players, Room, RoomCtx};

/// Somebody in the queue, as the rule sees them.
#[derive(Debug, Clone, PartialEq)]
pub struct Waiting {
    pub player: PlayerId,
    /// What they said they bring: `rating` and `region` read here, the
    /// rest handed on to the match.
    pub options: Value,
    pub rating: f64,
    pub region: String,
    /// When they joined the queue, on its clock.
    pub since: f64,
}

/// Who meets whom: given everyone waiting and the time, the groups to send
/// into a match together, as indices into the list. Anyone in no group
/// waits on.
pub type Rule = Box<dyn FnMut(&[Waiting], f64) -> Vec<Vec<usize>> + Send>;

/// A kind of room that is a queue in front of another kind.
pub struct Matchmaker {
    /// The kind of room a group is sent into.
    kind: String,
    rule: Grouping,
    waiting: Vec<Waiting>,
    /// How many matches have been made, for room names.
    made: u64,
    /// Whether the queue's size changed since everyone was last told it.
    changed: bool,
}

/// How a queue groups its players.
enum Grouping {
    ByRating { size: usize, start: f64, widen: f64, region_patience: f64 },
    Custom(Rule),
}

impl Matchmaker {
    /// A queue sending groups into rooms of `kind` by `rule`.
    pub fn new(kind: &str, rule: impl FnMut(&[Waiting], f64) -> Vec<Vec<usize>> + Send + 'static) -> Self {
        Self::with(kind, Grouping::Custom(Box::new(rule)))
    }

    /// Groups of `size` whose ratings are close, from one region - the
    /// window opening the longer they wait ([`Matchmaker::window`]): 100 to
    /// start with, 25 wider each second, any region never.
    pub fn by_rating(kind: &str, size: usize) -> Self {
        Self::with(
            kind,
            Grouping::ByRating { size: size.max(1), start: 100.0, widen: 25.0, region_patience: f64::INFINITY },
        )
    }

    fn with(kind: &str, rule: Grouping) -> Self {
        Matchmaker { kind: kind.to_string(), rule, waiting: Vec::new(), made: 0, changed: false }
    }

    /// With [`Matchmaker::by_rating`]: ratings within `start` of each other
    /// meet at once, and the window grows by `widen` a second for as long as
    /// the longest-waiting of them has waited, so nobody waits for ever for
    /// a perfect match.
    pub fn window(mut self, start: f64, widen: f64) -> Self {
        if let Grouping::ByRating { start: s, widen: w, .. } = &mut self.rule {
            (*s, *w) = (start.max(0.0), widen.max(0.0));
        }
        self
    }

    /// With [`Matchmaker::by_rating`]: players meet only within their
    /// region until one has waited this many seconds - then anyone will do.
    pub fn region_patience(mut self, secs: f64) -> Self {
        if let Grouping::ByRating { region_patience, .. } = &mut self.rule {
            *region_patience = secs.max(0.0);
        }
        self
    }

    /// Everyone waiting, longest first - for a room that shows the queue.
    pub fn waiting(&self) -> &[Waiting] {
        &self.waiting
    }

    fn groups(&mut self, now: f64) -> Vec<Vec<usize>> {
        match &mut self.rule {
            Grouping::ByRating { size, start, widen, region_patience } => {
                group_by_rating(&self.waiting, now, *size, *start, *widen, *region_patience)
            }
            Grouping::Custom(rule) => rule(&self.waiting, now),
        }
    }
}

/// The rule behind [`Matchmaker::by_rating`]: the longest-waiting first,
/// each with the `size - 1` closest ratings that fit its window.
pub fn group_by_rating(waiting: &[Waiting], now: f64, size: usize, start: f64, widen: f64, region_patience: f64) -> Vec<Vec<usize>> {
    let mut taken = vec![false; waiting.len()];
    let mut groups = Vec::new();
    let mut order: Vec<usize> = (0..waiting.len()).collect();
    order.sort_by(|a, b| waiting[*a].since.total_cmp(&waiting[*b].since));
    for &first in &order {
        if taken[first] {
            continue;
        }
        let me = &waiting[first];
        let waited = (now - me.since).max(0.0);
        let window = start + widen * waited;
        let any_region = waited >= region_patience;
        let mut near: Vec<usize> = (0..waiting.len())
            .filter(|i| *i != first && !taken[*i])
            .filter(|i| (waiting[*i].rating - me.rating).abs() <= window)
            .filter(|i| any_region || waiting[*i].region == me.region)
            .collect();
        near.sort_by(|a, b| (waiting[*a].rating - me.rating).abs().total_cmp(&(waiting[*b].rating - me.rating).abs()));
        if near.len() + 1 < size {
            continue;
        }
        let mut group = vec![first];
        group.extend(near.into_iter().take(size - 1));
        for i in &group {
            taken[*i] = true;
        }
        groups.push(group);
    }
    groups
}

impl Room for Matchmaker {
    fn on_join(&mut self, ctx: &mut RoomCtx, player: &mut Player, options: &Value) -> Result<Value, String> {
        let options = if options.is_object() { options.clone() } else { json!({}) };
        let rating = options["rating"].as_f64().unwrap_or(0.0);
        let region = options["region"].as_str().unwrap_or("").to_string();
        self.waiting.push(Waiting { player: player.id.clone(), options, rating, region, since: ctx.time() });
        self.changed = true;
        Ok(json!({ "queued": true, "waiting": self.waiting.len() }))
    }

    fn on_leave(&mut self, _: &mut RoomCtx, player: &mut Player) {
        self.waiting.retain(|w| w.player != player.id);
        self.changed = true;
    }

    fn on_tick(&mut self, ctx: &mut RoomCtx, _: &mut Players, _dt: f64) {
        let groups = self.groups(ctx.time());
        let mut gone = vec![false; self.waiting.len()];
        for group in groups {
            let group: Vec<usize> = group.into_iter().filter(|i| *i < gone.len() && !gone[*i]).collect();
            if group.is_empty() {
                continue;
            }
            self.made += 1;
            // A room of its own, named so two queues cannot collide.
            let room = format!("{}-{}-{}", ctx.id(), self.made, crate::server::random_hex(3));
            let players: Vec<&str> = group.iter().map(|i| self.waiting[*i].player.as_str()).collect();
            for i in &group {
                let w = &self.waiting[*i];
                let mut options = w.options.clone();
                options["match"] = json!(room);
                options["players"] = json!(players);
                ctx.transfer(&w.player, JoinRequest::named(&self.kind, &room).with(options));
                gone[*i] = true;
            }
        }
        if gone.iter().any(|g| *g) {
            let mut i = 0;
            self.waiting.retain(|_| {
                let keep = !gone[i];
                i += 1;
                keep
            });
            self.changed = true;
        }
        if std::mem::take(&mut self.changed) {
            ctx.broadcast(json!({ "t": "queued", "waiting": self.waiting.len() }));
        }
    }

    fn describe(&self) -> Value {
        json!({ "waiting": self.waiting.len(), "matches_made": self.made, "into": self.kind })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::room::{Release, RoomRunner};
    use crate::store::MemoryStore;

    fn queue(m: Matchmaker) -> RoomRunner {
        RoomRunner::new("queue", "ranked", Box::new(m), Arc::new(MemoryStore::new()), 1000).0
    }

    /// Where each player was sent, by player.
    fn sent(effects: &crate::room::Effects) -> Vec<(String, String, Value)> {
        let mut out: Vec<(String, String, Value)> = effects
            .released
            .iter()
            .filter_map(|(p, why)| match why {
                Release::Transfer(to) => Some((p.id.clone(), to.room.clone().unwrap_or_default(), to.options.clone())),
                _ => None,
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// S14: close ratings meet at once, in a room of their own, told who
    /// they meet; a far one waits until the window has opened to them.
    #[test]
    fn close_ratings_meet_and_a_far_one_waits_for_the_window() {
        let mut q = queue(Matchmaker::by_rating("duel", 2).window(100.0, 50.0));
        for (id, rating) in [("ada", 1500), ("bo", 1540), ("cy", 1900)] {
            q.join(Player::new(id, id), &json!({ "rating": rating })).unwrap();
        }
        let first = sent(&q.tick(0.25));
        assert_eq!(first.len(), 2, "{first:?}");
        assert_eq!((first[0].0.as_str(), first[1].0.as_str()), ("ada", "bo"));
        assert_eq!(first[0].1, first[1].1, "one room for both");
        assert_eq!(first[0].2["players"], json!(["ada", "bo"]));
        assert_eq!(first[0].2["rating"], 1500, "what they brought goes with them");
        assert_eq!(q.room::<Matchmaker>().unwrap().waiting().len(), 1);

        // Cy waits; Di arrives 360 away. The window opens by 50 a second for
        // whoever waited longest: Cy, after about six seconds.
        q.join(Player::new("di", "di"), &json!({ "rating": 1540 })).unwrap();
        let mut met = Vec::new();
        for _ in 0..40 {
            met = sent(&q.tick(0.25));
            if !met.is_empty() {
                break;
            }
        }
        assert_eq!(met.iter().map(|m| m.0.as_str()).collect::<Vec<_>>(), ["cy", "di"]);
        assert!(q.room::<Matchmaker>().unwrap().waiting().is_empty());
    }

    #[test]
    fn regions_meet_their_own_until_patience_runs_out_and_leaving_leaves_the_queue() {
        let mut q = queue(Matchmaker::by_rating("duel", 2).window(1000.0, 0.0).region_patience(3.0));
        q.join(Player::new("ada", "ada"), &json!({ "rating": 1500, "region": "eu" })).unwrap();
        q.join(Player::new("bo", "bo"), &json!({ "rating": 1500, "region": "us" })).unwrap();
        assert!(sent(&q.tick(1.0)).is_empty(), "not across regions yet");
        q.join(Player::new("cy", "cy"), &json!({ "rating": 1500, "region": "us" })).unwrap();
        assert_eq!(sent(&q.tick(0.1)).iter().map(|m| m.0.as_str()).collect::<Vec<_>>(), ["bo", "cy"]);
        q.join(Player::new("di", "di"), &json!({ "rating": 1500, "region": "asia" })).unwrap();
        q.leave("di");
        assert!(sent(&q.tick(5.0)).is_empty(), "nobody left for ada");
        q.join(Player::new("ed", "ed"), &json!({ "rating": 1500, "region": "asia" })).unwrap();
        assert_eq!(sent(&q.tick(0.1)).len(), 2, "ada has waited past her patience: any region");
    }

    #[test]
    fn a_game_can_bring_its_own_rule() {
        // Four at a time, whoever they are.
        let mut q = queue(Matchmaker::new("table", |w: &[Waiting], _| if w.len() >= 4 { vec![(0..4).collect()] } else { vec![] }));
        for id in ["a", "b", "c"] {
            q.join(Player::new(id, id), &Value::Null).unwrap();
        }
        assert!(sent(&q.tick(0.25)).is_empty());
        q.join(Player::new("d", "d"), &Value::Null).unwrap();
        let table = sent(&q.tick(0.25));
        assert_eq!(table.len(), 4);
        assert!(table.iter().all(|t| t.1 == table[0].1));
    }
}
