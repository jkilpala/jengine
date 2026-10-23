//! The vocabulary every transport speaks.
//!
//! Deliberately small and owned: a transport hands back plain `String` /
//! `Vec<u8>` rather than borrowing from its own buffers, because the native
//! backend produces these on a socket thread and the browser backend produces
//! them inside a JS callback. Neither can lend the frame loop a reference.

use std::fmt;

/// Something that happened on a connection, in the order it happened.
#[derive(Debug, Clone, PartialEq)]
pub enum NetEvent {
    /// The socket finished opening. Sends before this are queued, not dropped.
    Opened,
    /// A text frame.
    Text(String),
    /// A binary frame.
    Binary(Vec<u8>),
    /// The peer (or we) closed cleanly.
    Closed { code: u16, reason: String },
    /// The connection failed. Terminal — a transport never recovers on its
    /// own, because a silent reconnect would replay game state the caller
    /// never asked to resend.
    Failed(String),
}

/// Where a connection is in its life.
///
/// A plain enum rather than a bag of booleans: `connecting && connected` is
/// not a state anything should have to reason about.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum NetState {
    /// Never connected, or fully torn down.
    #[default]
    Disconnected,
    /// Socket opening. Sends are queued.
    Connecting,
    /// Open for traffic.
    Connected,
    /// Closed cleanly, with the peer's code and reason.
    Closed { code: u16, reason: String },
    /// Closed by error.
    Failed(String),
}

impl NetState {
    /// Whether traffic can flow right now.
    pub fn is_connected(&self) -> bool {
        matches!(self, NetState::Connected)
    }

    /// Whether this is a resting state — nothing is in flight and nothing
    /// will change without someone calling `connect` again.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            NetState::Disconnected | NetState::Closed { .. } | NetState::Failed(_)
        )
    }

    /// The lowercase name scripts see in the `Net.state` field.
    ///
    /// Stable strings, because game scripts will compare against them; adding
    /// a state is fine, renaming one breaks every script that checked it.
    pub fn as_str(&self) -> &'static str {
        match self {
            NetState::Disconnected => "disconnected",
            NetState::Connecting => "connecting",
            NetState::Connected => "connected",
            NetState::Closed { .. } => "closed",
            NetState::Failed(_) => "failed",
        }
    }
}

/// Why a send could not be accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// No connection has been opened.
    NotConnected,
    /// The outgoing queue is full. The message was **not** sent.
    QueueFull,
    /// The transport itself refused.
    Transport(String),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::NotConnected => write!(f, "not connected"),
            NetError::QueueFull => write!(f, "outgoing queue is full"),
            NetError::Transport(e) => write!(f, "transport error: {e}"),
        }
    }
}

impl std::error::Error for NetError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_connected_carries_traffic() {
        assert!(NetState::Connected.is_connected());
        for s in [
            NetState::Disconnected,
            NetState::Connecting,
            NetState::Closed { code: 1000, reason: String::new() },
            NetState::Failed("x".into()),
        ] {
            assert!(!s.is_connected(), "{s:?}");
        }
    }

    #[test]
    fn connecting_is_not_terminal_but_the_rest_are() {
        // `Connecting` is the one state a caller must keep polling through.
        assert!(!NetState::Connecting.is_terminal());
        assert!(!NetState::Connected.is_terminal());
        assert!(NetState::Disconnected.is_terminal());
        assert!(NetState::Failed("x".into()).is_terminal());
    }

    #[test]
    fn state_names_are_the_ones_scripts_compare_against() {
        // Renaming any of these silently breaks every script that checked it.
        assert_eq!(NetState::Disconnected.as_str(), "disconnected");
        assert_eq!(NetState::Connecting.as_str(), "connecting");
        assert_eq!(NetState::Connected.as_str(), "connected");
        assert_eq!(NetState::Closed { code: 1000, reason: String::new() }.as_str(), "closed");
        assert_eq!(NetState::Failed(String::new()).as_str(), "failed");
    }
}
