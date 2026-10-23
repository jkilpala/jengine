//! Native HTTP backend.
//!
//! `ureq` is a blocking client, so each request runs on its own thread and
//! reports back through a channel — the same arrangement as the WebSocket
//! backend, and for the same reason: nothing in the frame loop may block.
//!
//! A thread per request rather than a pool. Requests are capped at
//! [`MAX_IN_FLIGHT`](crate::http::MAX_IN_FLIGHT), so the worst case is a
//! bounded handful of short-lived threads, and a pool would add a scheduler
//! to manage for no benefit at that scale.

use crate::http::{HttpBackend, HttpRequest, HttpResponse, MAX_BODY_BYTES};
use crossbeam_channel::{Receiver, Sender};
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub struct NativeHttpBackend {
    to_game: Sender<HttpResponse>,
    from_workers: Receiver<HttpResponse>,
    /// Requests dispatched but not yet answered. Shared with the worker
    /// threads so the count is right even while they are in flight.
    in_flight: Arc<AtomicUsize>,
}

impl Default for NativeHttpBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeHttpBackend {
    pub fn new() -> Self {
        let (to_game, from_workers) = crossbeam_channel::unbounded();
        Self {
            to_game,
            from_workers,
            in_flight: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl HttpBackend for NativeHttpBackend {
    fn send(&mut self, request: HttpRequest) {
        let tx = self.to_game.clone();
        let in_flight = Arc::clone(&self.in_flight);
        in_flight.fetch_add(1, Ordering::SeqCst);
        // Kept out of the closure so the failure path below can still name
        // the request that could not be dispatched.
        let id = request.id.clone();

        let spawned = std::thread::Builder::new()
            .name("jengine-net-http".into())
            .spawn(move || {
                let response = perform(&request);
                // Decrement before sending, so a caller that polls the moment
                // it reads the response sees an accurate count.
                in_flight.fetch_sub(1, Ordering::SeqCst);
                let _ = tx.send(response);
            });

        if spawned.is_err() {
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            let _ = self.to_game.send(HttpResponse::failure(
                id,
                "could not spawn a request thread",
            ));
        }
    }

    fn poll(&mut self) -> Vec<HttpResponse> {
        // `try_recv` ends on both Empty and Disconnected; the latter cannot
        // happen while `self` still holds a sender, and "nothing more" is the
        // safe reading of it either way.
        let mut out = Vec::new();
        while let Ok(r) = self.from_workers.try_recv() {
            out.push(r);
        }
        out
    }

    fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }
}

/// Run one request to completion. Called on a worker thread.
fn perform(request: &HttpRequest) -> HttpResponse {
    let mut req = ureq::request(request.method.as_str(), &request.url);
    for (name, value) in &request.headers {
        req = req.set(name, value);
    }

    let result = match &request.body {
        Some(body) => req.send_string(body),
        None => req.call(),
    };

    match result {
        Ok(response) => read_response(&request.id, response),
        // A non-2xx status is an answer, not a failure: `ureq` reports it as
        // an error, and folding it into `error` would lose the status and the
        // body, which is usually where the API explains what went wrong.
        Err(ureq::Error::Status(status, response)) => {
            let mut out = read_response(&request.id, response);
            out.status = status;
            out.ok = false;
            out
        }
        Err(e) => HttpResponse::failure(&request.id, e.to_string()),
    }
}

fn read_response(id: &str, response: ureq::Response) -> HttpResponse {
    let status = response.status();
    let mut body = String::new();
    // Bounded read: a wrong URL pointing at something enormous should fail
    // fast rather than buffer until memory runs out.
    let read = response
        .into_reader()
        .take(MAX_BODY_BYTES as u64 + 1)
        .read_to_string(&mut body);

    match read {
        Ok(_) if body.len() > MAX_BODY_BYTES => HttpResponse::failure(
            id,
            format!("response body exceeds {MAX_BODY_BYTES} bytes"),
        ),
        Ok(_) => HttpResponse {
            id: id.to_string(),
            status,
            ok: (200..300).contains(&status),
            body,
            error: None,
        },
        // Non-UTF-8 lands here. The script channel is text, and inventing an
        // encoding for a game's API call is not this layer's job.
        Err(e) => HttpResponse::failure(id, format!("reading body: {e}")),
    }
}
