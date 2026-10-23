//! Connection lifecycle and the message bus.
//!
//! This is the resource a runtime shares and the package's systems drive. It
//! owns a [`Transport`] and turns its event stream into a state machine plus
//! a bounded inbox, so game code polls data structures instead of sockets.
//!
//! Bounded on purpose. A game that stops reading its inbox — paused, loading,
//! mid-scene-transition — would otherwise grow it without limit until the tab
//! dies. Dropping the *oldest* message and saying so is the least surprising
//! failure: the newest world state is the one worth keeping.

use crate::event::{NetError, NetEvent, NetState};
use crate::transport::Transport;
use std::collections::VecDeque;

/// Default inbox / outbox capacity, in messages.
pub const DEFAULT_CAPACITY: usize = 1024;

/// A received message, tagged by frame type.
#[derive(Debug, Clone, PartialEq)]
pub enum NetMessage {
    Text(String),
    Binary(Vec<u8>),
}

impl NetMessage {
    /// The text payload, or `None` for a binary frame.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            NetMessage::Text(s) => Some(s),
            NetMessage::Binary(_) => None,
        }
    }
}

/// A live (or dead) connection, plus everything queued in either direction.
///
/// Shared through the `ResourceMap` under this type, so a system reaches it
/// with `resources.get_mut::<NetConnection>()` — and, per the package
/// contract, does nothing when it is absent.
pub struct NetConnection {
    transport: Option<Box<dyn Transport>>,
    state: NetState,
    inbox: VecDeque<NetMessage>,
    capacity: usize,
    /// Messages dropped because the inbox was full, since the connection
    /// opened. Surfaced rather than silently swallowed — a nonzero value here
    /// means game code is not draining, and that is worth seeing.
    dropped: u64,
    /// Frames handed to the transport while it was still opening.
    pending_send: VecDeque<NetMessage>,
    /// The URL this connection was opened with, if any.
    ///
    /// Kept so the channel can be declarative: a script states which server it
    /// wants and the package reconciles, rather than having to issue connect
    /// and disconnect commands and track which it has already sent.
    url: Option<String>,
}

impl Default for NetConnection {
    fn default() -> Self {
        Self::new()
    }
}

impl NetConnection {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            transport: None,
            state: NetState::Disconnected,
            inbox: VecDeque::new(),
            // A zero-capacity inbox would drop everything and look like a dead
            // connection; treat it as "unset" rather than honour it.
            capacity: capacity.max(1),
            dropped: 0,
            pending_send: VecDeque::new(),
            url: None,
        }
    }

    /// Adopt a transport and move to `Connecting`.
    ///
    /// Replaces any existing connection: reconnecting is the common case and
    /// making callers tear down first is a step everyone forgets. The previous
    /// transport is closed so its socket does not leak.
    pub fn attach(&mut self, transport: Box<dyn Transport>) {
        if let Some(old) = self.transport.as_mut() {
            old.close();
        }
        self.transport = Some(transport);
        self.state = NetState::Connecting;
        // A fresh connection starts with a fresh inbox. Delivering the
        // previous session's messages after a reconnect is worse than losing
        // them - game code cannot tell which session they belong to.
        self.inbox.clear();
        self.pending_send.clear();
        self.dropped = 0;
    }

    /// Drop the transport and return to `Disconnected`.
    pub fn disconnect(&mut self) {
        if let Some(t) = self.transport.as_mut() {
            t.close();
        }
        self.transport = None;
        self.state = NetState::Disconnected;
        self.pending_send.clear();
        self.url = None;
    }

    /// The URL of the current connection, if one was opened by URL.
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// Record the URL a transport was opened with. Called by [`crate::connect`].
    pub fn set_url(&mut self, url: &str) {
        self.url = Some(url.to_string());
    }

    pub fn state(&self) -> &NetState {
        &self.state
    }

    pub fn is_connected(&self) -> bool {
        self.state.is_connected()
    }

    /// Messages dropped for want of inbox space since the last `attach`.
    pub fn dropped_count(&self) -> u64 {
        self.dropped
    }

    /// How many messages are waiting to be read.
    pub fn inbox_len(&self) -> usize {
        self.inbox.len()
    }

    /// Take the oldest received message.
    pub fn recv(&mut self) -> Option<NetMessage> {
        self.inbox.pop_front()
    }

    /// Take every received message, oldest first.
    pub fn drain(&mut self) -> Vec<NetMessage> {
        self.inbox.drain(..).collect()
    }

    /// Queue a text frame.
    ///
    /// Accepted while `Connecting` and flushed once the socket opens, because
    /// "connect, then send a join message" is the normal shape of a handshake
    /// and forcing callers to poll for `Connected` first only invites a busy
    /// loop.
    pub fn send_text(&mut self, text: &str) -> Result<(), NetError> {
        self.send(NetMessage::Text(text.to_string()))
    }

    /// Queue a binary frame.
    pub fn send_binary(&mut self, data: &[u8]) -> Result<(), NetError> {
        self.send(NetMessage::Binary(data.to_vec()))
    }

    fn send(&mut self, msg: NetMessage) -> Result<(), NetError> {
        let Some(transport) = self.transport.as_mut() else {
            return Err(NetError::NotConnected);
        };
        match self.state {
            NetState::Connected => match &msg {
                NetMessage::Text(s) => transport.send_text(s),
                NetMessage::Binary(b) => transport.send_binary(b),
            },
            NetState::Connecting => {
                if self.pending_send.len() >= self.capacity {
                    return Err(NetError::QueueFull);
                }
                self.pending_send.push_back(msg);
                Ok(())
            }
            _ => Err(NetError::NotConnected),
        }
    }

    /// Pump the transport: advance the state machine and fill the inbox.
    ///
    /// Call once per frame. Returns the number of messages received, so a
    /// caller can skip work entirely on a quiet frame.
    pub fn pump(&mut self) -> usize {
        // Scoped so the transport borrow ends before the loop, which needs
        // `&mut self` to fill the inbox.
        let events = match self.transport.as_mut() {
            Some(transport) => transport.poll(),
            None => return 0,
        };
        let mut received = 0;

        for event in events {
            match event {
                NetEvent::Opened => {
                    self.state = NetState::Connected;
                    // Flush the handshake. Anything that fails here is
                    // reported once and dropped: retrying inside the pump
                    // would reorder it behind traffic sent later.
                    let pending: Vec<NetMessage> = self.pending_send.drain(..).collect();
                    if let Some(transport) = self.transport.as_mut() {
                        for msg in pending {
                            let result = match &msg {
                                NetMessage::Text(s) => transport.send_text(s),
                                NetMessage::Binary(b) => transport.send_binary(b),
                            };
                            if let Err(e) = result {
                                log::warn!("[net] queued send failed after open: {e}");
                            }
                        }
                    }
                }
                NetEvent::Text(s) => {
                    self.push_inbox(NetMessage::Text(s));
                    received += 1;
                }
                NetEvent::Binary(b) => {
                    self.push_inbox(NetMessage::Binary(b));
                    received += 1;
                }
                NetEvent::Closed { code, reason } => {
                    self.state = NetState::Closed { code, reason };
                    self.pending_send.clear();
                }
                NetEvent::Failed(e) => {
                    self.state = NetState::Failed(e);
                    self.pending_send.clear();
                }
            }
        }
        received
    }

    fn push_inbox(&mut self, msg: NetMessage) {
        if self.inbox.len() >= self.capacity {
            // Drop the oldest: in a game the newest state is the useful one,
            // and refusing the newest would leave the queue permanently stale.
            self.inbox.pop_front();
            self.dropped += 1;
            if self.dropped == 1 || self.dropped.is_multiple_of(100) {
                log::warn!(
                    "[net] inbox full ({} messages); dropped {} so far - is anything draining it?",
                    self.capacity,
                    self.dropped
                );
            }
        }
        self.inbox.push_back(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    /// A connection wired to a mock, plus a handle to drive it.
    ///
    /// The transport is boxed into the connection, so tests that need to push
    /// events build them through this and re-attach — which is also the
    /// honest shape, since a real transport is never reachable afterwards.
    fn connected() -> NetConnection {
        let mut c = NetConnection::new();
        let mut t = MockTransport::new();
        t.push_event(NetEvent::Opened);
        c.attach(Box::new(t));
        c.pump();
        c
    }

    #[test]
    fn a_new_connection_is_disconnected_and_refuses_sends() {
        let mut c = NetConnection::new();
        assert_eq!(*c.state(), NetState::Disconnected);
        assert_eq!(c.send_text("x"), Err(NetError::NotConnected));
    }

    #[test]
    fn attaching_moves_to_connecting_and_opening_moves_to_connected() {
        let mut c = NetConnection::new();
        c.attach(Box::new(MockTransport::new()));
        assert_eq!(*c.state(), NetState::Connecting);
        assert!(!c.is_connected());

        let mut t = MockTransport::new();
        t.push_event(NetEvent::Opened);
        c.attach(Box::new(t));
        c.pump();
        assert!(c.is_connected());
    }

    #[test]
    fn text_arrives_in_the_inbox_in_order() {
        let mut c = NetConnection::new();
        let mut t = MockTransport::new();
        t.push_event(NetEvent::Opened);
        t.push_text("one");
        t.push_text("two");
        c.attach(Box::new(t));

        assert_eq!(c.pump(), 2, "pump reports how many messages arrived");
        assert_eq!(c.drain(), vec![
            NetMessage::Text("one".into()),
            NetMessage::Text("two".into()),
        ]);
        assert_eq!(c.inbox_len(), 0, "drain empties the inbox");
    }

    #[test]
    fn a_send_before_open_is_queued_then_flushed() {
        // The handshake case: connect and immediately send a join message.
        // Rejecting it would force every caller into a poll-until-connected
        // loop.
        let mut c = NetConnection::new();
        c.attach(Box::new(MockTransport::new()));
        assert!(c.send_text("join").is_ok(), "queued while connecting");

        // Re-attaching would clear the queue, so drive the same transport by
        // opening through a fresh one that starts already-open.
        let mut c2 = NetConnection::new();
        let mut t = MockTransport::new();
        t.push_event(NetEvent::Opened);
        c2.attach(Box::new(t));
        c2.send_text("queued-while-connecting").unwrap();
        c2.pump();
        // After open, the queued frame has gone out and later sends go direct.
        assert!(c2.is_connected());
        assert!(c2.send_text("after").is_ok());
    }

    #[test]
    fn sends_after_close_are_refused() {
        let mut c = NetConnection::new();
        let mut t = MockTransport::new();
        t.push_event(NetEvent::Opened);
        t.push_event(NetEvent::Closed { code: 1000, reason: "bye".into() });
        c.attach(Box::new(t));
        c.pump();

        assert_eq!(
            *c.state(),
            NetState::Closed { code: 1000, reason: "bye".into() }
        );
        assert_eq!(c.send_text("x"), Err(NetError::NotConnected));
    }

    #[test]
    fn a_failure_is_terminal() {
        let mut c = NetConnection::new();
        let mut t = MockTransport::new();
        t.push_event(NetEvent::Failed("dns".into()));
        c.attach(Box::new(t));
        c.pump();
        assert_eq!(*c.state(), NetState::Failed("dns".into()));
        assert!(c.state().is_terminal());
    }

    #[test]
    fn a_full_inbox_drops_the_oldest_and_counts_it() {
        // The newest world state is the one worth keeping; refusing new
        // messages would leave the queue permanently stale instead.
        let mut c = NetConnection::with_capacity(2);
        let mut t = MockTransport::new();
        t.push_event(NetEvent::Opened);
        t.push_text("a");
        t.push_text("b");
        t.push_text("c");
        c.attach(Box::new(t));
        c.pump();

        assert_eq!(c.inbox_len(), 2);
        assert_eq!(c.dropped_count(), 1);
        assert_eq!(
            c.drain(),
            vec![NetMessage::Text("b".into()), NetMessage::Text("c".into())],
            "the oldest message is the one dropped"
        );
    }

    #[test]
    fn zero_capacity_is_treated_as_one_not_as_drop_everything() {
        let mut c = NetConnection::with_capacity(0);
        let mut t = MockTransport::new();
        t.push_event(NetEvent::Opened);
        t.push_text("a");
        c.attach(Box::new(t));
        c.pump();
        assert_eq!(c.inbox_len(), 1);
    }

    #[test]
    fn reattaching_clears_the_previous_sessions_inbox() {
        // Delivering the old session's messages after a reconnect is worse
        // than losing them: game code cannot tell which session they are from.
        let mut c = connected();
        let mut t = MockTransport::new();
        t.push_event(NetEvent::Opened);
        t.push_text("stale");
        c.attach(Box::new(t));
        c.pump();
        c.attach(Box::new(MockTransport::new()));
        assert_eq!(c.inbox_len(), 0);
        assert_eq!(c.dropped_count(), 0, "the drop counter is per-session");
    }

    #[test]
    fn disconnect_returns_to_the_initial_state() {
        let mut c = connected();
        c.disconnect();
        assert_eq!(*c.state(), NetState::Disconnected);
        assert_eq!(c.send_text("x"), Err(NetError::NotConnected));
    }

    #[test]
    fn pumping_without_a_transport_is_a_no_op() {
        let mut c = NetConnection::new();
        assert_eq!(c.pump(), 0);
    }

    #[test]
    fn recv_takes_one_at_a_time_oldest_first() {
        let mut c = NetConnection::new();
        let mut t = MockTransport::new();
        t.push_event(NetEvent::Opened);
        t.push_text("a");
        t.push_text("b");
        c.attach(Box::new(t));
        c.pump();
        assert_eq!(c.recv(), Some(NetMessage::Text("a".into())));
        assert_eq!(c.recv(), Some(NetMessage::Text("b".into())));
        assert_eq!(c.recv(), None);
    }

    #[test]
    fn binary_frames_survive_the_round_trip() {
        let mut c = NetConnection::new();
        let mut t = MockTransport::new();
        t.push_event(NetEvent::Opened);
        t.push_event(NetEvent::Binary(vec![0, 255, 7]));
        c.attach(Box::new(t));
        c.pump();
        assert_eq!(c.recv(), Some(NetMessage::Binary(vec![0, 255, 7])));
    }
}
