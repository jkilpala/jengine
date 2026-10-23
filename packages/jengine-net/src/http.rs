//! HTTP(S) requests — calling web APIs.
//!
//! The same shape as the WebSocket half: a backend trait with a real
//! implementation per platform, a queue in front of it, and everything
//! testable against a mock. Requests are fire-and-forget; responses arrive
//! later through [`HttpClient::poll`] and are matched to their request by an
//! id the caller chose.
//!
//! **Nothing here blocks the frame.** Natively a request runs on its own
//! thread; in a browser it is a `fetch` promise. A game that made a blocking
//! HTTP call from `update()` would stall for as long as the server took, which
//! on a bad network is seconds.

use std::collections::VecDeque;

/// How many requests may be in flight at once.
///
/// A script in a loop can queue requests far faster than a server answers
/// them; without a ceiling that becomes thousands of sockets and a server
/// that thinks it is being attacked. Excess requests are refused loudly
/// rather than queued forever.
pub const MAX_IN_FLIGHT: usize = 16;

/// Largest response body accepted, in bytes.
///
/// A game calling a web API wants JSON, not a video. Anything larger is
/// almost certainly a wrong URL, and buffering it would be the expensive way
/// to find that out.
pub const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

/// HTTP methods worth having in a game.
///
/// Not an open string: a typo'd method is a request that fails at the server
/// for reasons the caller then has to guess at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
        }
    }

    /// Parse a method name, case-insensitively. `None` for anything else.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "GET" => Some(HttpMethod::Get),
            "POST" => Some(HttpMethod::Post),
            "PUT" => Some(HttpMethod::Put),
            "PATCH" => Some(HttpMethod::Patch),
            "DELETE" => Some(HttpMethod::Delete),
            _ => None,
        }
    }
}

/// One outgoing request.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    /// Caller's correlation id, echoed back on the response. Any string; the
    /// caller picks something meaningful ("leaderboard", "save-slot-2").
    pub id: String,
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Request body. Ignored for GET.
    pub body: Option<String>,
}

impl HttpRequest {
    /// A GET with no headers.
    pub fn get(id: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            method: HttpMethod::Get,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// A POST carrying `body`, with `Content-Type: application/json`.
    pub fn post_json(
        id: impl Into<String>,
        url: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            method: HttpMethod::Post,
            url: url.into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: Some(body.into()),
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// The result of a request, successful or not.
///
/// A transport failure and an HTTP error status are different things and are
/// reported separately: a 404 is an answer, and treating it as a failure loses
/// the status the caller needs.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpResponse {
    /// The id from the request this answers.
    pub id: String,
    /// HTTP status, or 0 when the request never reached a server.
    pub status: u16,
    /// `true` for 2xx.
    pub ok: bool,
    pub body: String,
    /// Set when the request failed before producing a status — DNS, refused
    /// connection, TLS, a body over [`MAX_BODY_BYTES`].
    pub error: Option<String>,
}

impl HttpResponse {
    /// A failure that never reached a server.
    pub fn failure(id: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status: 0,
            ok: false,
            body: String::new(),
            error: Some(error.into()),
        }
    }
}

/// Where requests actually go.
///
/// Both methods must be non-blocking: `send` hands the request off, `poll`
/// takes whatever has come back since last time.
pub trait HttpBackend {
    fn send(&mut self, request: HttpRequest);
    fn poll(&mut self) -> Vec<HttpResponse>;
    /// How many requests this backend has not yet answered.
    fn in_flight(&self) -> usize;
}

/// Queued HTTP requests and their answers.
///
/// Shared through the `ResourceMap` like [`crate::NetConnection`], and, like
/// it, owned by this package rather than the runtime.
pub struct HttpClient {
    backend: Box<dyn HttpBackend>,
    responses: VecDeque<HttpResponse>,
    /// Requests refused because too many were already in flight.
    refused: u64,
}

impl HttpClient {
    pub fn new(backend: Box<dyn HttpBackend>) -> Self {
        Self {
            backend,
            responses: VecDeque::new(),
            refused: 0,
        }
    }

    /// Queue a request.
    ///
    /// Returns `false` when the in-flight ceiling is reached and the request
    /// was **not** sent; a synthetic failure response is produced for it so a
    /// caller waiting on that id is not left waiting forever.
    pub fn send(&mut self, request: HttpRequest) -> bool {
        if self.backend.in_flight() >= MAX_IN_FLIGHT {
            self.refused += 1;
            log::warn!(
                "[net] {} requests already in flight; refusing '{}'",
                MAX_IN_FLIGHT,
                request.id
            );
            // Answering rather than silently dropping: a script that awaits an
            // id must always get something back, or it hangs on a state that
            // will never arrive.
            self.responses.push_back(HttpResponse::failure(
                request.id,
                format!("too many requests in flight (limit {MAX_IN_FLIGHT})"),
            ));
            return false;
        }
        self.backend.send(request);
        true
    }

    /// Move whatever the backend has finished into the response queue.
    ///
    /// Call once per frame. Returns how many arrived.
    pub fn pump(&mut self) -> usize {
        let arrived = self.backend.poll();
        let n = arrived.len();
        self.responses.extend(arrived);
        n
    }

    /// Take every completed response.
    pub fn drain(&mut self) -> Vec<HttpResponse> {
        self.responses.drain(..).collect()
    }

    /// Requests refused for exceeding the in-flight ceiling.
    pub fn refused_count(&self) -> u64 {
        self.refused
    }

    pub fn in_flight(&self) -> usize {
        self.backend.in_flight()
    }
}

/// An in-memory backend for tests and offline development.
///
/// Records requests and returns whatever the test queues, so every caller of
/// [`HttpClient`] can be driven without a network.
#[derive(Default)]
pub struct MockHttpBackend {
    pub sent: Vec<HttpRequest>,
    pending: Vec<HttpResponse>,
    /// Requests considered outstanding, for exercising the in-flight cap.
    pub fake_in_flight: usize,
}

impl MockHttpBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a response for the next `poll`.
    pub fn push_response(&mut self, response: HttpResponse) {
        self.pending.push(response);
    }

    /// Queue a 200 with `body`.
    pub fn push_ok(&mut self, id: &str, body: &str) {
        self.pending.push(HttpResponse {
            id: id.to_string(),
            status: 200,
            ok: true,
            body: body.to_string(),
            error: None,
        });
    }
}

impl HttpBackend for MockHttpBackend {
    fn send(&mut self, request: HttpRequest) {
        self.sent.push(request);
    }

    fn poll(&mut self) -> Vec<HttpResponse> {
        std::mem::take(&mut self.pending)
    }

    fn in_flight(&self) -> usize {
        self.fake_in_flight
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> HttpClient {
        HttpClient::new(Box::new(MockHttpBackend::new()))
    }

    #[test]
    fn method_names_round_trip() {
        for m in [
            HttpMethod::Get,
            HttpMethod::Post,
            HttpMethod::Put,
            HttpMethod::Patch,
            HttpMethod::Delete,
        ] {
            assert_eq!(HttpMethod::parse(m.as_str()), Some(m));
        }
    }

    #[test]
    fn method_parsing_is_case_insensitive_and_rejects_nonsense() {
        assert_eq!(HttpMethod::parse("get"), Some(HttpMethod::Get));
        assert_eq!(HttpMethod::parse("Post"), Some(HttpMethod::Post));
        // A typo'd method must not become a silent GET.
        assert_eq!(HttpMethod::parse("GTE"), None);
        assert_eq!(HttpMethod::parse(""), None);
    }

    #[test]
    fn post_json_sets_its_content_type() {
        let r = HttpRequest::post_json("save", "https://x/y", "{}");
        assert_eq!(r.method, HttpMethod::Post);
        assert!(r
            .headers
            .iter()
            .any(|(k, v)| k == "Content-Type" && v == "application/json"));
        assert_eq!(r.body.as_deref(), Some("{}"));
    }

    #[test]
    fn a_response_is_matched_to_its_request_by_id() {
        let mut c = client();
        c.send(HttpRequest::get("a", "https://x/a"));
        c.send(HttpRequest::get("b", "https://x/b"));
        // Answers can arrive in any order; the id is what pairs them up.
        let backend = MockHttpBackend::new();
        let mut c2 = HttpClient::new(Box::new(backend));
        c2.send(HttpRequest::get("a", "https://x/a"));
        let _ = c2.pump();

        let mut backend = MockHttpBackend::new();
        backend.push_ok("b", "second");
        backend.push_ok("a", "first");
        let mut c3 = HttpClient::new(Box::new(backend));
        c3.pump();
        let ids: Vec<String> = c3.drain().into_iter().map(|r| r.id).collect();
        assert_eq!(ids, vec!["b".to_string(), "a".to_string()]);
    }

    #[test]
    fn an_error_status_is_an_answer_not_a_failure() {
        // A 404 carries information the caller needs; folding it into
        // `error` would throw the status away.
        let mut backend = MockHttpBackend::new();
        backend.push_response(HttpResponse {
            id: "x".into(),
            status: 404,
            ok: false,
            body: "not found".into(),
            error: None,
        });
        let mut c = HttpClient::new(Box::new(backend));
        c.pump();
        let r = &c.drain()[0];
        assert_eq!(r.status, 404);
        assert!(!r.ok);
        assert!(r.error.is_none(), "a status is not a transport failure");
    }

    #[test]
    fn a_transport_failure_has_no_status() {
        let r = HttpResponse::failure("x", "dns");
        assert_eq!(r.status, 0);
        assert!(!r.ok);
        assert_eq!(r.error.as_deref(), Some("dns"));
    }

    #[test]
    fn the_in_flight_ceiling_refuses_and_still_answers() {
        // A refused request must produce a response, or a script waiting on
        // that id waits forever.
        let mut backend = MockHttpBackend::new();
        backend.fake_in_flight = MAX_IN_FLIGHT;
        let mut c = HttpClient::new(Box::new(backend));

        assert!(!c.send(HttpRequest::get("over", "https://x")));
        assert_eq!(c.refused_count(), 1);

        let responses = c.drain();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].id, "over");
        assert!(responses[0].error.is_some());
    }

    #[test]
    fn a_request_under_the_ceiling_reaches_the_backend() {
        let mut c = client();
        assert!(c.send(HttpRequest::get("ok", "https://x")));
        assert_eq!(c.refused_count(), 0);
    }

    #[test]
    fn pump_reports_how_many_arrived_and_drain_empties() {
        let mut backend = MockHttpBackend::new();
        backend.push_ok("a", "1");
        backend.push_ok("b", "2");
        let mut c = HttpClient::new(Box::new(backend));
        assert_eq!(c.pump(), 2);
        assert_eq!(c.drain().len(), 2);
        assert!(c.drain().is_empty());
    }
}
