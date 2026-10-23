//! Players who are not there: connect a crowd to the `zones` example and
//! walk it about, to watch a busy server on its dashboard or to see how one
//! holds up over real sockets.
//!
//! ```text
//! JENGINE_ADMIN_TOKEN=secret cargo run -p jengine-server --release --example zones
//! cargo run -p jengine-server --release --example bots -- ws://127.0.0.1:9001 200
//! ```
//!
//! Each bot is the engine's own client (`jengine-net`), so this is also two
//! hundred copies of what a game ships with. They wander, and every so often
//! one heads for the door to the next zone.

use std::time::{Duration, Instant};

use jengine_net::{NetConnection, connect};
use serde_json::json;

struct Bot {
    conn: NetConnection,
    said_hello: bool,
    next_move: Instant,
    seed: u64,
}

impl Bot {
    fn random(&mut self) -> f64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let url = args.next().unwrap_or_else(|| "ws://127.0.0.1:9001".to_string());
    let count: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(50);
    let seconds: u64 = args.next().and_then(|n| n.parse().ok()).unwrap_or(u64::MAX / 4);

    let mut bots: Vec<Bot> = (0..count)
        .map(|i| {
            let mut conn = NetConnection::new();
            connect(&mut conn, &url);
            Bot { conn, said_hello: false, next_move: Instant::now(), seed: 0x9e3779b97f4a7c15 ^ (i as u64 + 1) }
        })
        .collect();
    println!("{count} bots connecting to {url}");

    let (started, mut frames, mut last_report) = (Instant::now(), 0usize, Instant::now());
    while started.elapsed() < Duration::from_secs(seconds) {
        for (i, bot) in bots.iter_mut().enumerate() {
            bot.conn.pump();
            frames += bot.conn.drain().len();
            if !bot.conn.is_connected() {
                continue;
            }
            if !bot.said_hello {
                bot.said_hello = true;
                let zone = if i % 3 == 0 { "cave" } else { "meadow" };
                let _ = bot.conn.send_text(&json!({ "t": "hello", "name": format!("Bot {i}") }).to_string());
                let _ = bot.conn.send_text(&json!({ "t": "join", "kind": "zone", "room": zone }).to_string());
            }
            if Instant::now() >= bot.next_move {
                // Mostly somewhere in the zone; now and then, out of a door.
                let x = if bot.random() < 0.1 { if bot.random() < 0.5 { -10.0 } else { 1010.0 } } else { bot.random() * 1000.0 };
                let y = bot.random() * 600.0;
                let _ = bot.conn.send_text(&json!({ "t": "send", "body": { "go": [x, y] } }).to_string());
                bot.next_move = Instant::now() + Duration::from_secs_f64(1.0 + bot.random() * 4.0);
            }
        }
        if last_report.elapsed() >= Duration::from_secs(5) {
            let up = bots.iter().filter(|b| b.conn.is_connected()).count();
            println!("{up}/{count} connected, {:.0} frames/s received", frames as f64 / last_report.elapsed().as_secs_f64());
            (frames, last_report) = (0, Instant::now());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
