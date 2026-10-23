//! How much one tick of a crowded zone costs - the number that decides how
//! many players a zone can hold. Run it in release:
//!
//! ```text
//! cargo run -p jengine-server --release --example zone_cost
//! ```
//!
//! Every player is an entity and a watcher; each tick every one of them
//! moves, and the space works out a frame for each and serialises it. That
//! is the whole per-tick cost of sync - sockets are not involved.

use std::time::Instant;

use jengine_server::sync::Space;
use serde_json::json;

fn main() {
    println!("players  area      in view   ms/tick (1 core)  ms/tick (cores)  KB/s per player at 10 Hz");
    for (players, side) in [(100, 2000.0), (500, 4000.0), (500, 1500.0), (365, 1000.0), (2000, 8000.0), (2000, 3000.0)] {
        // The same zone on one core, then on as many as a flush will use.
        let one = measure(players, side, Some(1));
        let many = measure(players, side, None);
        println!(
            "{players:>7}  {side:>5.0}^2  {:>8.0}  {:>16.2}  {:>15.2}  {:>8.1}",
            one.0, one.1, many.1, one.2
        );
    }
}

/// (others in view, ms a tick, KB/s per player at 10 Hz) for one zone.
fn measure(players: usize, side: f64, threads: Option<usize>) -> (f64, f64, f64) {
    {
        let mut space = Space::new(400.0);
        if let Some(threads) = threads {
            space.set_flush_threads(threads);
        }
        let mut seed = 0x9e3779b97f4a7c15u64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let ids: Vec<u64> = (0..players)
            .map(|i| {
                let id = space.spawn(rnd() * side, rnd() * side, json!({ "kind": "player", "hp": 100 }));
                space.watch_from(&format!("p{i}"), id);
                id
            })
            .collect();
        space.flush();

        let ticks = 50;
        let (mut bytes, mut in_view) = (0usize, 0usize);
        let started = Instant::now();
        for _ in 0..ticks {
            for id in &ids {
                let (x, y) = space.position(*id).unwrap();
                space.set_position(*id, (x + rnd() * 6.0 - 3.0).clamp(0.0, side), (y + rnd() * 6.0 - 3.0).clamp(0.0, side));
            }
            for (_, frame) in space.flush() {
                // Every entity told of is one `[` (a move) or one `{` past the frame's own.
                in_view += frame.matches("],[").count() + usize::from(frame.contains("\"move\""));
                bytes += frame.len();
            }
        }
        let ms = started.elapsed().as_secs_f64() * 1000.0 / ticks as f64;
        (
            in_view as f64 / (ticks * players) as f64,
            ms,
            bytes as f64 / (ticks * players) as f64 * 10.0 / 1024.0,
        )
    }
}
