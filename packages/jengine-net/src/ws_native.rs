//! Native WebSocket transport.
//!
//! `tungstenite` is a blocking client, and the frame loop cannot block, so the
//! socket lives on its own thread and talks to the game through two channels.
//! That is the whole design: the thread owns the socket, the handle owns the
//! channel ends, and [`Transport::poll`] is a non-blocking drain.
//!
//! Deliberately no async runtime. Pulling tokio into the engine to service one
//! socket would be a large dependency for a small job, and the thread here has
//! no scheduling subtleties — it reads until the socket closes.

use crate::event::{NetError, NetEvent};
use crate::transport::Transport;
use crossbeam_channel::{Receiver, Sender, TryRecvError};

/// What the game thread asks the socket thread to do.
enum Command {
    Text(String),
    Binary(Vec<u8>),
    Close,
}

/// A WebSocket connection driven by a background thread.
pub struct WebSocketTransport {
    to_socket: Sender<Command>,
    from_socket: Receiver<NetEvent>,
    /// Kept so the thread can be joined on drop rather than detached.
    thread: Option<std::thread::JoinHandle<()>>,
    /// Set once a terminal event has been observed, so `poll` stops reporting
    /// a disconnected channel as a fresh failure every frame.
    finished: bool,
}

impl WebSocketTransport {
    /// Start connecting to `url` (`ws://` or `wss://`).
    ///
    /// Returns immediately — the connect happens on the socket thread and its
    /// success or failure arrives as [`NetEvent::Opened`] or
    /// [`NetEvent::Failed`] through `poll`. A connect that blocked here would
    /// stall the frame that called it, and DNS alone can take seconds.
    pub fn connect(url: &str) -> Self {
        let (to_socket, cmd_rx) = crossbeam_channel::unbounded::<Command>();
        let (event_tx, from_socket) = crossbeam_channel::unbounded::<NetEvent>();
        let url = url.to_string();

        let thread = std::thread::Builder::new()
            .name("jengine-net-ws".into())
            .spawn(move || socket_thread(url, cmd_rx, event_tx))
            .ok();

        Self {
            to_socket,
            from_socket,
            thread,
            finished: false,
        }
    }
}

/// Own the socket, translate commands into frames and frames into events.
fn socket_thread(url: String, commands: Receiver<Command>, events: Sender<NetEvent>) {
    let mut socket = match tungstenite::connect(&url) {
        Ok((socket, _response)) => socket,
        Err(e) => {
            let _ = events.send(NetEvent::Failed(format!("connect {url}: {e}")));
            return;
        }
    };

    // Non-blocking reads, so the loop can alternate between "is there a frame
    // to send" and "is there a frame to read" without either starving the
    // other. A blocking read would hold outgoing traffic until the peer spoke.
    if let Err(e) = set_nonblocking(&socket) {
        let _ = events.send(NetEvent::Failed(format!("set nonblocking: {e}")));
        return;
    }
    if events.send(NetEvent::Opened).is_err() {
        return; // The game dropped the transport before we finished connecting.
    }

    // Frames the socket's write buffer would not take yet, oldest first.
    let mut waiting: std::collections::VecDeque<tungstenite::Message> = std::collections::VecDeque::new();
    loop {
        // 1. Anything to send. The socket is non-blocking, so a burst bigger
        //    than the OS will take at once (a few hundred KB) makes a flush
        //    answer WouldBlock: the frame is kept in tungstenite's write
        //    buffer and goes out on a later flush. That is not a failure -
        //    taking it for one dropped the connection under every large
        //    send (a 300 KB upload in `part` frames, Oct 2026).
        loop {
            match commands.try_recv() {
                Ok(Command::Text(s)) => waiting.push_back(tungstenite::Message::Text(s)),
                Ok(Command::Binary(b)) => waiting.push_back(tungstenite::Message::Binary(b)),
                Ok(Command::Close) => {
                    let _ = socket.close(None);
                    let _ = socket.flush();
                    let _ = events.send(NetEvent::Closed {
                        code: 1000,
                        reason: "client closed".into(),
                    });
                    return;
                }
                // The game side is gone; nothing left to serve.
                Err(TryRecvError::Disconnected) => {
                    let _ = socket.close(None);
                    return;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        while let Some(message) = waiting.pop_front() {
            match socket.write(message) {
                Ok(()) => {}
                // Queued in the write buffer; flushed below or next time round.
                Err(tungstenite::Error::Io(e)) if would_block(&e) => {}
                // The buffer is at its limit: this frame waits its turn.
                Err(tungstenite::Error::WriteBufferFull(message)) => {
                    waiting.push_front(message);
                    break;
                }
                Err(e) => {
                    let _ = events.send(NetEvent::Failed(format!("send: {e}")));
                    return;
                }
            }
        }
        match socket.flush() {
            Ok(()) => {}
            Err(tungstenite::Error::Io(e)) if would_block(&e) => {}
            Err(e) => {
                let _ = events.send(NetEvent::Failed(format!("send: {e}")));
                return;
            }
        }

        // 2. Anything to read.
        match socket.read() {
            Ok(tungstenite::Message::Text(s)) => {
                if events.send(NetEvent::Text(s.to_string())).is_err() {
                    return;
                }
            }
            Ok(tungstenite::Message::Binary(b)) => {
                if events.send(NetEvent::Binary(b.to_vec())).is_err() {
                    return;
                }
            }
            Ok(tungstenite::Message::Close(frame)) => {
                let (code, reason) = frame
                    .map(|f| (u16::from(f.code), f.reason.to_string()))
                    .unwrap_or((1000, String::new()));
                let _ = events.send(NetEvent::Closed { code, reason });
                return;
            }
            // Ping/pong are answered by tungstenite itself.
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if would_block(&e) => {
                // Nothing to read. Yield rather than spin: this thread has
                // nothing useful to do and a busy loop would burn a core.
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                let _ = events.send(NetEvent::Closed {
                    code: 1000,
                    reason: "connection closed".into(),
                });
                return;
            }
            Err(e) => {
                let _ = events.send(NetEvent::Failed(format!("read: {e}")));
                return;
            }
        }
    }
}

fn would_block(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

/// Make the underlying TCP socket non-blocking, with no Nagle delay.
///
/// `tungstenite`'s stream is either plain TCP or a rustls session wrapped
/// around one; only the TCP socket has `set_nonblocking`, so this is the one
/// place that distinction is unwrapped. A variant this build does not know
/// about is left blocking, which still works - reads just wait for the peer.
fn set_nonblocking(
    socket: &tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
) -> std::io::Result<()> {
    use tungstenite::stream::MaybeTlsStream;
    let tcp = match socket.get_ref() {
        MaybeTlsStream::Plain(s) => s,
        MaybeTlsStream::Rustls(s) => &s.sock,
        _ => return Ok(()),
    };
    // And no Nagle: a game sends small frames it wants gone now, not held
    // back for the server's ACK of the last one.
    let _ = tcp.set_nodelay(true);
    tcp.set_nonblocking(true)
}

impl Transport for WebSocketTransport {
    fn poll(&mut self) -> Vec<NetEvent> {
        let mut out = Vec::new();
        loop {
            match self.from_socket.try_recv() {
                Ok(event) => {
                    if matches!(event, NetEvent::Closed { .. } | NetEvent::Failed(_)) {
                        self.finished = true;
                    }
                    out.push(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // The thread ended. Only report that as a failure if it
                    // never told us why - otherwise the caller would see a
                    // spurious `Failed` after every clean close.
                    if !self.finished {
                        self.finished = true;
                        out.push(NetEvent::Failed("socket thread stopped".into()));
                    }
                    break;
                }
            }
        }
        out
    }

    fn send_text(&mut self, text: &str) -> Result<(), NetError> {
        self.to_socket
            .send(Command::Text(text.to_string()))
            .map_err(|_| NetError::Transport("socket thread is gone".into()))
    }

    fn send_binary(&mut self, data: &[u8]) -> Result<(), NetError> {
        self.to_socket
            .send(Command::Binary(data.to_vec()))
            .map_err(|_| NetError::Transport("socket thread is gone".into()))
    }

    fn close(&mut self) {
        let _ = self.to_socket.send(Command::Close);
    }
}

impl Drop for WebSocketTransport {
    fn drop(&mut self) {
        // Ask politely, then let the channel disconnect do the rest: the
        // thread's `try_recv` sees `Disconnected` and returns. Joining is
        // bounded because every path out of the loop is non-blocking.
        let _ = self.to_socket.send(Command::Close);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}
