//! **A game server framework for jEngine** - from a relay to a small MMO.
//!
//! `jengine-relay` lets two copies of a turn-based game agree on an order of
//! moves, and knows nothing else. This is the general thing: a server you
//! describe by registering **kinds of room**, where a room is whatever a game
//! needs it to be - a relay, a lobby, a match, one zone of a world.
//!
//! ```no_run
//! use jengine_server::{KindOptions, Server};
//! # struct Zone; impl Zone { fn new(_: &str) -> Self { Zone } }
//! # impl jengine_server::Room for Zone {
//! #     fn on_join(&mut self, _: &mut jengine_server::RoomCtx, _: &mut jengine_server::Player,
//! #         _: &serde_json::Value) -> Result<serde_json::Value, String> { Ok(serde_json::Value::Null) }
//! # }
//! Server::new()
//!     .relay()                                              // turn-based games, unchanged
//!     .room("zone", KindOptions::ticking(10.0).persistent(), Zone::new)
//!     .run("0.0.0.0:9001")
//!     .unwrap();
//! ```
//!
//! # The three things it is built around
//!
//! **A player's state outlives a room.** A [`Player`] carries a state
//! document that the server loads at `hello`, hands to whichever room the
//! player is in, and saves when they leave it - so a [`RoomCtx::transfer`]
//! from one zone to the next, or from a lobby into a match and back, moves
//! the same player along on the same connection. Frameworks built around a
//! room per match leave this to the game; here it is the centre. See [`room`].
//!
//! **A room is plain synchronous code.** The [`Room`] trait is five
//! callbacks that record what they want done; no sockets, no `async`. The
//! same room is driven by a [`RoomRunner`] in a unit test and by the server
//! in production, so a game's server rules are tested like any other Rust.
//!
//! **A big room tells each player only what is near them.** [`sync::Space`]
//! holds a room's entities and sends each watcher what entered their view,
//! what moved, what changed and what left - the difference between a room of
//! sixteen and a zone of hundreds.
//!
//! # Beside the socket
//!
//! [`Server::get`] / [`Server::post`] add the game's own HTTP routes under
//! `/api/` - a player's stats, who is online, a leaderboard - see
//! [`server::api`]. [`netcode`] is what a room and a client share to predict
//! steering: one numbered input per fixed step, acknowledged in `sync`, with
//! the walls the server moves things through. A server links the engine's
//! own crates for the rest - `examples/zones` paths its monsters with
//! `jengine_core`'s A* over a `Tilemap` it owns.
//!
//! # Protocol
//!
//! JSON text frames over one WebSocket - the only transport a browser has,
//! and the one `jengine-net` speaks on every platform. Client to server:
//!
//! | Frame | Does |
//! |---|---|
//! | `{"t":"hello","token":"..","name":"Ada"}` | sign in; no token makes a new player. With [`PasswordAuth`]: `"account"`, `"password"`, and `"register":true` the first time; `"recover":true` asks for a recovery code and `"code"` with a new `"password"` uses it. **A password is refused on a socket that crossed the network unencrypted** ([`PASSWORD_NEEDS_TLS`]) |
//! | `{"t":"join","kind":"zone","room":"meadow","options":{..}}` | join that room, opening it; no `room` asks for any with space |
//! | `{"t":"send","body":<any JSON>}` | to the room's `on_message` |
//! | `{"t":"leave"}` | come out of the room |
//! | `{"t":"account","change_password":{"old","new"}}` / `{"t":"account","sign_out_everywhere":true}` | with [`PasswordAuth`]: answered by `{"t":"account","ok":true,"token"}` |
//! | `{"t":"sub","channel":"guild:7"}` / `{"t":"unsub",..}` | join or leave a channel, if the server has them |
//! | `{"t":"say","channel":"guild:7","body":<any JSON>}` | speak in one |
//!
//! Server to client:
//!
//! | Frame | When |
//! |---|---|
//! | `{"t":"welcome","player","token","name","state","room"}` | after `hello`; `room` is where they last were |
//! | `{"t":"joined","kind","room", ..}` | in a room - also after a transfer; the rest is the room's. `"resumed":true` when it is a held seat being taken up again |
//! | `{"t":"subbed","channel","members","recent"}` / `{"t":"said","channel","from","name","body"}` / `{"t":"presence","channel","joined" or "left"}` | channels |
//! | `{"t":"left"}` / `{"t":"kicked","why"}` / `{"t":"closed"}` | out of a room |
//! | `{"t":"state","sys":true,"state"}` | an operator changed the player's saved state (or a reconnect: what the room has). `sys` marks it the server's, so a room may name a frame of its own `state` |
//! | `{"t":"redirect","url","kind","room"}` | in a cluster: that room is on another node - connect there, `hello` with the token, and join |
//! | `{"t":"error","why"}` | to the one who asked |
//! | anything else | whatever the room sends - `msg` from a relay, `sync` from a [`sync::Space`] |
//!
//! A client that skips `hello` and goes straight to `join` is an unsaved
//! guest, and a `join` with no `kind` means the server's default one. With
//! [`Server::relay`] that is exactly `jengine-relay`'s protocol.
//!
//! Beside the socket: `GET /healthz` for a load balancer, `GET /stats` for a
//! dashboard or an autoscaler (behind the admin token unless
//! [`Server::public_stats`]).

pub mod auth;
pub mod cluster;
#[cfg(feature = "fleet")]
pub mod fleet;
pub mod listen;
pub mod liveops;
pub mod matchmaking;
pub mod lockstep;
pub mod relay;
pub mod room;
pub mod scaffold;
pub mod server;
pub mod store;
pub mod sync;

/// Prediction-ready netcode, shared with the client package: a fixed-step
/// motion model, input queues, reconciliation, lag compensation.
pub use jengine_netcode as netcode;

pub use auth::{PasswordAuth, RECOVERY_SENT, Recovery, SignedAuth};
pub use cluster::{ClusterConfig, RemoteStore};
pub use listen::Tls;
pub use liveops::LiveOps;
pub use room::{
    Effects, Frame, JoinRequest, Player, PlayerId, Players, Release, Room, RoomCtx, RoomRunner,
};
pub use server::{
    Api, ApiError, ApiHandler, ApiRequest, Authenticator, ChannelAsk, Config, GuestAuth, Identity, KindOptions,
    Operator, PASSWORD_NEEDS_TLS, ROOM_FULL, Role, Running, Server,
};
#[cfg(feature = "sqlite")]
pub use store::SqliteStore;
pub use store::{MemoryStore, Order, Scored, Store, WriteBehind};

/// Log to standard error at `info` (or whatever `RUST_LOG` says) - what a
/// server's `main` wants, without a logging crate of its own to depend on.
pub fn init_logging() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).try_init();
}
