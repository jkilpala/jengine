//! The seam between "a connection" and "how bytes actually move".
//!
//! Everything above this trait — lifecycle, queues, the script channel — is
//! plain data and is tested against [`MockTransport`] without opening a
//! socket. That is deliberate: the parts most likely to be wrong are the
//! state transitions, and they should not need a server to exercise.
//!
//! A Colyseus client, or a custom protocol, is a *user* of this trait rather
//! than a replacement for it: both speak WebSocket underneath.

use crate::event::{NetError, NetEvent};

/// A duplex message connection.
///
/// **Every method must be non-blocking.** These are called from inside the
/// frame loop; a transport that blocks on I/O stalls the game. The native
/// backend keeps its blocking socket on its own thread and talks to it
/// through a channel for exactly this reason.
///
/// Not `Send`: the browser backend owns a `web_sys::WebSocket`, which is
/// bound to the JS main thread and can never be. Requiring `Send` here would
/// make the only transport that works in a browser impossible to express.
pub trait Transport {
    /// Take every event that has arrived since the last call, oldest first.
    ///
    /// Draining rather than peeking keeps ownership simple and means a caller
    /// that forgets to poll grows a queue instead of losing messages.
    fn poll(&mut self) -> Vec<NetEvent>;

    /// Queue a text frame.
    ///
    /// May be called before `Opened` arrives; a transport buffers rather than
    /// rejecting, since "connect then immediately send" is the normal shape of
    /// a join handshake.
    fn send_text(&mut self, text: &str) -> Result<(), NetError>;

    /// Queue a binary frame.
    fn send_binary(&mut self, data: &[u8]) -> Result<(), NetError>;

    /// Begin a clean close. The `Closed` event still arrives through `poll`.
    fn close(&mut self);
}

/// An in-memory transport for tests and for offline development.
///
/// Records what was sent and replays whatever the test queues, so the entire
/// connection layer can be driven deterministically — no ports, no timing, no
/// server. Also genuinely useful in the editor: a scene can be built against
/// scripted traffic before a server exists.
#[derive(Default)]
pub struct MockTransport {
    /// Events the next `poll` will hand back.
    pending: Vec<NetEvent>,
    /// Text frames passed to `send_text`, in order.
    pub sent_text: Vec<String>,
    /// Binary frames passed to `send_binary`, in order.
    pub sent_binary: Vec<Vec<u8>>,
    /// Set by `close`.
    pub closed: bool,
    /// When set, every send fails with this error.
    pub fail_sends: Option<NetError>,
}

impl MockTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue an event for the next `poll`.
    pub fn push_event(&mut self, event: NetEvent) {
        self.pending.push(event);
    }

    /// Convenience: queue an inbound text frame.
    pub fn push_text(&mut self, text: &str) {
        self.pending.push(NetEvent::Text(text.to_string()));
    }
}

impl Transport for MockTransport {
    fn poll(&mut self) -> Vec<NetEvent> {
        std::mem::take(&mut self.pending)
    }

    fn send_text(&mut self, text: &str) -> Result<(), NetError> {
        if let Some(e) = &self.fail_sends {
            return Err(e.clone());
        }
        self.sent_text.push(text.to_string());
        Ok(())
    }

    fn send_binary(&mut self, data: &[u8]) -> Result<(), NetError> {
        if let Some(e) = &self.fail_sends {
            return Err(e.clone());
        }
        self.sent_binary.push(data.to_vec());
        Ok(())
    }

    fn close(&mut self) {
        self.closed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_drains_rather_than_peeks() {
        let mut t = MockTransport::new();
        t.push_text("a");
        t.push_text("b");
        assert_eq!(t.poll().len(), 2);
        assert!(t.poll().is_empty(), "a second poll must not repeat events");
    }

    #[test]
    fn events_come_back_oldest_first() {
        let mut t = MockTransport::new();
        t.push_event(NetEvent::Opened);
        t.push_text("first");
        t.push_text("second");
        assert_eq!(
            t.poll(),
            vec![
                NetEvent::Opened,
                NetEvent::Text("first".into()),
                NetEvent::Text("second".into()),
            ]
        );
    }

    #[test]
    fn a_failing_transport_does_not_record_the_send() {
        let mut t = MockTransport::new();
        t.fail_sends = Some(NetError::Transport("boom".into()));
        assert!(t.send_text("x").is_err());
        assert!(t.sent_text.is_empty(), "a rejected send must not be recorded as sent");
    }
}
