//! `jengine-relay [host:port]` - run the turn-based relay.
//!
//! Defaults to `127.0.0.1:9001`, which is what the arcade's online games
//! connect to out of the box: two copies of a game on one machine. Pass
//! `0.0.0.0:9001` to take players from the local network.

fn main() {
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:9001".to_string());
    // No logger crate is pulled in for a binary this small; say the one thing
    // a person starting it needs to see.
    eprintln!("jengine-relay: listening on ws://{addr}  (Ctrl+C to stop)");
    if let Err(e) = jengine_relay::serve(&addr) {
        eprintln!("jengine-relay: could not listen on {addr}: {e}");
        std::process::exit(1);
    }
}
