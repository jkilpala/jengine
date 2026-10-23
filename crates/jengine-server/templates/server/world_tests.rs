
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use jengine_server::{MemoryStore, Release, RoomRunner};

    use super::*;

    fn meadow() -> RoomRunner {
        RoomRunner::new("zone", "meadow", Box::new(Zone::new("meadow")), Arc::new(MemoryStore::new()), 100).0
    }

    /// Where a `sync` frame says `entity` moved to, if it did.
    fn moved(frame: &Value, entity: u64) -> Option<(f64, f64)> {
        let m = frame["move"].as_array()?.iter().find(|m| m[0] == entity)?;
        Some((m[1].as_f64()?, m[2].as_f64()?))
    }

    #[test]
    fn a_player_walks_where_the_server_takes_them_and_picks_up_a_coin() {
        let mut zone = meadow();
        zone.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        zone.message("a", json!({ "go": [560, 300] }));
        let mut coins = 0.0;
        for _ in 0..40 {
            for (_, frame) in &zone.tick(0.1).frames {
                let frame = frame.to_json();
                if frame["t"] == "coins" {
                    coins = frame["coins"].as_f64().unwrap();
                }
            }
        }
        assert_eq!(coins, 1.0);
        let left = zone.leave("a");
        let (player, _) = &left.released[0];
        assert_eq!((player.num("x", 0.0), player.num("coins", 0.0)), (560.0, 1.0), "where she stood, and what she had");
    }

    #[test]
    fn the_east_edge_is_a_door_to_the_next_zone() {
        let mut zone = meadow();
        zone.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        zone.message("a", json!({ "go": [1010, 300] }));
        let moved = (0..60).find_map(|_| zone.tick(0.1).released.into_iter().next());
        let (player, why) = moved.expect("she reached the door");
        assert_eq!(why, Release::Transfer(JoinRequest::named("zone", "cave")));
        assert_eq!(player.text("zone", ""), "cave", "saved as already being there");
    }

    /// Steering is one fixed step per input, acknowledged - which is what
    /// lets a client have made the same move before being told.
    #[test]
    fn an_input_moves_one_step_and_is_acknowledged() {
        let mut zone = meadow();
        let joined = zone.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        assert_eq!(joined.frames[0].1["netcode"]["step"], STEP, "the client is told the numbers to predict with");
        let me = joined.frames[0].1["you"].as_u64().unwrap();
        zone.tick(0.033); // her own arrival in view, out of the way
        for seq in 1..=3 {
            zone.message("a", json!({ "input": { "seq": seq, "x": 1, "y": 0 } }));
        }
        // Three inputs at once still take three ticks.
        let mut acks = Vec::new();
        for _ in 0..4 {
            for (_, frame) in &zone.tick(0.033).frames {
                let frame = frame.to_json();
                if let (Some(ack), Some((x, _))) = (frame["ack"].as_u64(), moved(&frame, me)) {
                    acks.push((ack, x));
                }
            }
        }
        assert_eq!(acks, [(1, 530.0), (2, 560.0), (3, 590.0)], "thirty units a step, whatever the tick's own length");
    }

    /// A tag is judged against where the target was when the tagger saw
    /// them: the same claim made about "now" misses, because by now the
    /// target has walked on.
    #[test]
    fn a_tag_is_judged_where_the_target_was_seen() {
        let mut zone = meadow();
        zone.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        let bo = zone.join(Player::new("b", "Bo"), &Value::Null).unwrap();
        let bo_entity = bo.frames[0].1["you"].as_u64().unwrap();
        zone.tick(0.033); // arrivals in view, out of the way
        // Both start in the middle. Bo runs east; Ada stays.
        let mut seen_at = 0;
        for seq in 1..=8 {
            zone.message("b", json!({ "input": { "seq": seq, "x": 1, "y": 0 } }));
            for (to, frame) in &zone.tick(0.1).frames {
                let frame = frame.to_json();
                // The tick of the first frame in which Ada sees Bo move.
                if to == "a" && seen_at == 0 && moved(&frame, bo_entity).is_some() {
                    seen_at = frame["tick"].as_u64().unwrap();
                }
            }
        }
        // Ada's measured lag: a 1.2 s round trip, ample for the ticks she
        // claims to have seen Bo at.
        let answer = |zone: &mut RoomRunner, tick: u64| {
            let said = zone.message_after("a", json!({ "tag": { "target": bo_entity, "tick": tick } }), 0.6);
            let to_ada = said.frames.iter().find(|(to, f)| to == "a" && f.to_json()["t"] == "tag").unwrap();
            let told_bo = said.frames.iter().any(|(to, f)| to == "b" && f.to_json()["t"] == "tagged");
            (to_ada.1.to_json()["hit"] == true, told_bo)
        };
        assert_eq!(answer(&mut zone, seen_at), (true, true), "he was one step away when she saw him");
        assert_eq!(answer(&mut zone, u64::MAX), (false, false), "and is eight steps away now");
        // The same claim from a line with no lag to speak of is judged at
        // the oldest tick that lag allows - where Bo was already far off.
        let no_lag = zone.message("a", json!({ "tag": { "target": bo_entity, "tick": seen_at } }));
        let judged = no_lag.frames.iter().find(|(to, f)| to == "a" && f.to_json()["t"] == "tag").unwrap();
        assert_eq!(judged.1.to_json()["hit"], false, "a claim further back than the lag is not believed");
        let own = zone.message("a", json!({ "tag": { "target": bo_entity + 999, "tick": seen_at } }));
        assert_eq!(own.frames[0].1.to_json()["hit"], false, "nobody there");
    }

    /// The walls are the engine's tilemap, handed to the netcode crate: a
    /// steered avatar is stopped by them, and a client is told them.
    #[test]
    fn walls_stop_a_steered_avatar_and_are_told_to_the_client() {
        let mut zone = meadow();
        let joined = zone.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        let me = joined.frames[0].1["you"].as_u64().unwrap();
        let walls = &joined.frames[0].1["netcode"]["walls"];
        assert_eq!((walls["cols"].clone(), walls["rows"].clone(), walls["cell"].clone()), (json!(20), json!(12), json!(CELL)));
        let solid = walls["solid"].as_str().unwrap();
        assert_eq!(solid.len(), 240);
        assert_eq!(&solid[2 * 20 + 10..2 * 20 + 11], "1", "the block above the start, at column 10 rows 2 and 3");
        zone.tick(0.033);
        // North from (500, 300): the block at column 10 spans y 400..500, so she stops at 390.
        let mut y = 0.0;
        for seq in 1..=12 {
            zone.message("a", json!({ "input": { "seq": seq, "x": 0, "y": 1 } }));
            for (_, frame) in &zone.tick(0.033).frames {
                let frame = frame.to_json();
                if let Some((_, moved_y)) = moved(&frame, me) {
                    y = moved_y;
                }
            }
        }
        assert_eq!(y, 390.0, "held at the wall's face less her radius");
    }

    /// A monster finds its way round a wall to a player, with the engine's
    /// A* over the zone's own tilemap, and bites when it arrives.
    #[test]
    fn a_monster_hunts_round_the_walls_and_bites() {
        let mut zone = meadow();
        zone.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        // Stand her within sight of the monster at (125, 525), behind the
        // column of wall at x 200..250.
        zone.message("a", json!({ "go": [330, 450] }));
        for _ in 0..30 {
            zone.tick(0.1);
        }
        // The straight line from the monster to her crosses a wall - so the
        // only way to her is round it.
        let drawing = walls_of("meadow");
        let solid = |x: f64, y: f64| {
            let (col, row) = ((x / CELL).floor() as i64, ((HEIGHT - y) / CELL).floor() as i64);
            (0..12).contains(&row) && (0..20).contains(&col) && drawing[row as usize].as_bytes()[col as usize] == b'#'
        };
        let (from, to) = ((125.0, 525.0), (330.0, 450.0));
        assert!((0..=100).any(|i| {
            let t = i as f64 / 100.0;
            solid(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
        }));
        let mut bitten = None;
        for _ in 0..120 {
            for (to, frame) in &zone.tick(0.1).frames {
                let frame = frame.to_json();
                if to == "a" && frame["t"] == "bitten" {
                    bitten = Some(frame["coins"].as_f64().unwrap());
                }
                // Nothing that moves is ever inside a wall cell.
                for m in frame["move"].as_array().into_iter().flatten() {
                    let (x, y) = (m[1].as_f64().unwrap(), m[2].as_f64().unwrap());
                    assert!(!solid(x, y), "something walked into a wall at ({x}, {y})");
                }
            }
            if bitten.is_some() {
                break;
            }
        }
        assert_eq!(bitten, Some(0.0), "bitten, with no coins to lose: it came round the wall");
    }

    #[test]
    fn a_zone_that_is_not_on_the_map_lets_nobody_in() {
        let mut nowhere =
            RoomRunner::new("zone", "atlantis", Box::new(Zone::new("atlantis")), Arc::new(MemoryStore::new()), 100).0;
        assert!(nowhere.join(Player::new("a", "Ada"), &Value::Null).is_err());
    }
}
