//! A player at the other end of a real socket - the engine's own client
//! (`jengine_net::connect`, `NetConnection::pump`), because the point is that
//! what a game ships with is what talks to the server.
#![allow(dead_code)]

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use jengine_net::{HttpRequest, NetConnection, connect, http_client};
use serde_json::{Value, json};

pub struct Client {
    pub conn: NetConnection,
    /// Every frame received, in order.
    pub frames: Vec<Value>,
}

impl Client {
    pub fn connect(url: &str) -> Self {
        let mut conn = NetConnection::new();
        connect(&mut conn, url);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !conn.is_connected() {
            conn.pump();
            assert!(Instant::now() < deadline, "never connected: {:?}", conn.state());
            std::thread::sleep(Duration::from_millis(5));
        }
        Client { conn, frames: Vec::new() }
    }

    pub fn send(&mut self, frame: Value) {
        self.conn.send_text(&frame.to_string()).expect("a connected socket");
    }

    pub fn pump(&mut self) {
        self.conn.pump();
        for m in self.conn.drain() {
            if let Some(text) = m.as_text() {
                self.frames.push(serde_json::from_str(text).expect("the server sends JSON"));
            }
        }
    }

    /// Pump until `pred` says the frames so far are enough, or give up.
    pub fn until(&mut self, what: &str, pred: impl Fn(&[Value]) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            self.pump();
            if pred(&self.frames) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for {what}; state {:?}, frames {:#?}", self.conn.state(), self.frames);
    }

    /// Wait for a frame of this type and return the latest one.
    pub fn wait(&mut self, t: &str) -> Value {
        self.until(t, |f| f.iter().any(|x| x["t"] == t));
        self.last(t).unwrap()
    }

    pub fn last(&self, t: &str) -> Option<Value> {
        self.frames.iter().rev().find(|f| f["t"] == t).cloned()
    }

    pub fn count(&self, t: &str) -> usize {
        self.frames.iter().filter(|f| f["t"] == t).count()
    }

    /// Sign in. An empty `token` makes a new player. Returns the `welcome`.
    pub fn hello(&mut self, name: &str, token: &str) -> Value {
        self.send(json!({ "t": "hello", "name": name, "token": token }));
        self.wait("welcome")
    }
}

/// Sign in on fresh connections until the player's saved state satisfies
/// `ready` - the connection that last had them may still be saying goodbye.
pub fn hello_when(url: &str, token: &str, ready: impl Fn(&Value) -> bool) -> (Client, Value) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut client = Client::connect(url);
        let welcome = client.hello("", token);
        if ready(&welcome) || Instant::now() > deadline {
            return (client, welcome);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `GET` a path beside the socket and return the body.
pub fn http_get(addr: SocketAddr, path: &str) -> String {
    let mut http = http_client();
    http.send(HttpRequest::get("get", format!("http://{addr}{path}")));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        http.pump();
        if let Some(r) = http.drain().pop() {
            return r.body;
        }
        assert!(Instant::now() < deadline, "no answer from {path}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub fn stats(addr: SocketAddr) -> Value {
    serde_json::from_str(&http_get(addr, "/stats")).expect("/stats is JSON")
}

/// A WebSocket written by hand, for what the engine's client does not do:
/// send headers of a test's choosing, or a frame and its close in one write.
pub struct RawWs(pub std::net::TcpStream);

impl RawWs {
    pub fn open(addr: SocketAddr, headers: &[(&str, &str)]) -> RawWs {
        use std::io::{Read, Write};
        let mut socket = std::net::TcpStream::connect(addr).unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut head = format!(
            "GET /ws HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n"
        );
        for (k, v) in headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("\r\n");
        socket.write_all(head.as_bytes()).unwrap();
        let mut answer = Vec::new();
        let mut byte = [0u8; 1];
        while !answer.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).unwrap();
            answer.push(byte[0]);
        }
        assert!(answer.starts_with(b"HTTP/1.1 101"), "not upgraded: {}", String::from_utf8_lossy(&answer));
        RawWs(socket)
    }

    /// A masked frame of `opcode` carrying `payload`, as a client sends it.
    pub fn frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
        let mask = [0x11u8, 0x22, 0x33, 0x44];
        let mut out = vec![0x80 | opcode];
        match payload.len() {
            n if n < 126 => out.push(0x80 | n as u8),
            n => {
                out.push(0x80 | 126);
                out.extend_from_slice(&(n as u16).to_be_bytes());
            }
        }
        out.extend_from_slice(&mask);
        out.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        out
    }

    pub fn send(&mut self, frame: Value) {
        use std::io::Write;
        self.0.write_all(&Self::frame(1, frame.to_string().as_bytes())).unwrap();
    }

    /// Send `frame` and close the socket in the same write - a message and
    /// the line going, as close together as a network can put them.
    pub fn send_and_vanish(mut self, frame: Value) {
        use std::io::Write;
        let mut bytes = Self::frame(1, frame.to_string().as_bytes());
        bytes.extend(Self::frame(8, &[]));
        self.0.write_all(&bytes).unwrap();
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }

    /// The next text frame from the server, as JSON.
    pub fn recv(&mut self) -> Value {
        use std::io::Read;
        loop {
            let mut head = [0u8; 2];
            self.0.read_exact(&mut head).unwrap();
            let mut len = u64::from(head[1] & 0x7f);
            if len == 126 {
                let mut ext = [0u8; 2];
                self.0.read_exact(&mut ext).unwrap();
                len = u64::from(u16::from_be_bytes(ext));
            } else if len == 127 {
                let mut ext = [0u8; 8];
                self.0.read_exact(&mut ext).unwrap();
                len = u64::from_be_bytes(ext);
            }
            let mut payload = vec![0u8; len as usize];
            self.0.read_exact(&mut payload).unwrap();
            if head[0] & 0x0f == 1 {
                return serde_json::from_slice(&payload).unwrap();
            }
        }
    }

    /// Frames until one of type `t`, which is returned.
    pub fn wait(&mut self, t: &str) -> Value {
        loop {
            let frame = self.recv();
            if frame["t"] == t {
                return frame;
            }
        }
    }
}
