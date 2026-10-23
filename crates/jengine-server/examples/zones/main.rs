//! A small persistent world on `jengine-server`: two zones with a door
//! between them, and players who are remembered.
//!
//! ```text
//! cargo run -p jengine-server --example zones -- 0.0.0.0:9001 zones.db
//! ```
//!
//! Try it with any WebSocket client:
//!
//! ```text
//! > {"t":"hello","name":"Ada"}
//! < {"t":"welcome","player":"p_..","token":"..","state":{},"room":null}
//! > {"t":"join","kind":"zone","room":"meadow"}
//! < {"t":"joined","kind":"zone","room":"meadow","you":4,"coins":0,..}
//! < {"t":"sync","enter":[{"id":4,"kind":"player",..},{"id":1,"kind":"coin",..}]}
//! > {"t":"send","body":{"go":[1010,300]}}     walk east, through the door
//! < {"t":"sync","update":[{"id":4,"x":530,"y":300}]}  ...
//! < {"t":"joined","kind":"zone","room":"cave",..}     the same player, next zone
//! ```
//!
//! Keep the token: `{"t":"hello","token":".."}` on a later connection - or
//! after the server has been restarted - is the same player, where they were.

mod world;

use std::sync::Arc;

use jengine_server::{KindOptions, Server, SqliteStore};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut args = std::env::args().skip(1);
    let addr = args.next().unwrap_or_else(|| "127.0.0.1:9001".to_string());
    let db = args.next().unwrap_or_else(|| "zones.db".to_string());
    let store = SqliteStore::open(&db).unwrap_or_else(|e| {
        eprintln!("zones: could not open {db}: {e}");
        std::process::exit(1);
    });
    let mut server = Server::new()
        .store(Arc::new(store))
        // JENGINE_ADMIN_TOKEN=secret turns on http://<addr>/admin
        .admin_token(&std::env::var("JENGINE_ADMIN_TOKEN").unwrap_or_default())
        // Turn-based games on the same port, as a bare `join`.
        .relay()
        // Chat beside the zones, open to anyone signed in.
        .open_channels()
        // A zone stays open with nobody in it: it is a place, not a match.
        // And a connection that blinks is not a player who left: the seat is
        // held for fifteen seconds.
        .room("zone", KindOptions::ticking(10.0).max_players(500).persistent().grace(15.0), world::Zone::new);
    // With JENGINE_CLUSTER set this is one node of several: the zones are
    // spread over them, and a door may lead to another machine.
    if let Some(cluster) = jengine_server::ClusterConfig::from_env() {
        eprintln!("zones: node '{}' of the cluster at {}", cluster.node, cluster.coordinator);
        server = server.cluster(cluster);
    }
    if let Err(e) = server.run(&addr) {
        eprintln!("zones: could not listen on {addr}: {e}");
        std::process::exit(1);
    }
}
