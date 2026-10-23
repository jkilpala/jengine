//! **A burst bigger than the socket takes at once is sent, not dropped.**
//!
//! The native socket is non-blocking; when the OS send buffer is full a flush
//! answers WouldBlock with the frame kept in tungstenite's write buffer. That
//! used to be taken for a failure, so any large burst - a 300 KB upload in
//! `jengine-online`'s `part` frames - dropped the connection (Oct 2026). Over
//! a real socket: a server that does not read for half a second while the
//! client sends 4 MB, then counts what arrived.

#![cfg(not(target_arch = "wasm32"))]

use std::time::{Duration, Instant};

use jengine_net::{NetConnection, connect};

#[test]
fn a_burst_bigger_than_the_socket_takes_is_sent_whole() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    const FRAMES: usize = 40;
    const SIZE: usize = 100 * 1024;
    let server = std::thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        let mut ws = tungstenite::accept(tcp).unwrap();
        // Not reading: the client's sends fill the OS buffers and block.
        std::thread::sleep(Duration::from_millis(500));
        let mut bytes = 0;
        for _ in 0..FRAMES {
            match ws.read().unwrap() {
                tungstenite::Message::Text(t) => bytes += t.len(),
                other => panic!("not a text frame: {other:?}"),
            }
        }
        ws.send(tungstenite::Message::Text(format!("got {bytes}").into())).unwrap();
        // Let the reply go before the socket closes.
        let _ = ws.read();
    });

    let mut conn = NetConnection::new();
    connect(&mut conn, &url);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !conn.is_connected() {
        conn.pump();
        assert!(Instant::now() < deadline, "never connected: {:?}", conn.state());
        std::thread::sleep(Duration::from_millis(5));
    }
    let frame = "x".repeat(SIZE);
    for _ in 0..FRAMES {
        conn.send_text(&frame).expect("a connected socket");
    }
    let mut answer = None;
    while answer.is_none() {
        conn.pump();
        assert!(conn.is_connected(), "the connection was dropped under the burst: {:?}", conn.state());
        answer = conn.recv().and_then(|m| m.as_text().map(str::to_string));
        assert!(Instant::now() < deadline, "no answer");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(answer.unwrap(), format!("got {}", FRAMES * SIZE));
    conn.disconnect();
    let _ = server.join();
}
