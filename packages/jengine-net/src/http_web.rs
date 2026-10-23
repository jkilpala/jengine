//! Browser HTTP backend, over `fetch`.
//!
//! No threads: `fetch` is already asynchronous, so each request is a promise
//! driven by `spawn_local` that parks its result where `poll` can find it.
//!
//! **CORS applies.** A browser will refuse a cross-origin request unless the
//! server opts in, and it deliberately does not say why. That is a property of
//! the platform, not of this code — an API that works natively can fail in a
//! web build for this reason alone, which is worth knowing before debugging
//! the wrong layer.

use crate::http::{HttpBackend, HttpRequest, HttpResponse, MAX_BODY_BYTES};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};

#[derive(Default)]
pub struct WebHttpBackend {
    done: Rc<RefCell<Vec<HttpResponse>>>,
    in_flight: Rc<Cell<usize>>,
}

impl WebHttpBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl HttpBackend for WebHttpBackend {
    fn send(&mut self, request: HttpRequest) {
        let done = Rc::clone(&self.done);
        let in_flight = Rc::clone(&self.in_flight);
        in_flight.set(in_flight.get() + 1);

        spawn_local(async move {
            let response = perform(&request).await;
            in_flight.set(in_flight.get().saturating_sub(1));
            done.borrow_mut().push(response);
        });
    }

    fn poll(&mut self) -> Vec<HttpResponse> {
        std::mem::take(&mut *self.done.borrow_mut())
    }

    fn in_flight(&self) -> usize {
        self.in_flight.get()
    }
}

async fn perform(request: &HttpRequest) -> HttpResponse {
    match try_perform(request).await {
        Ok(r) => r,
        Err(e) => HttpResponse::failure(&request.id, describe(&e)),
    }
}

async fn try_perform(request: &HttpRequest) -> Result<HttpResponse, JsValue> {
    let opts = web_sys::RequestInit::new();
    opts.set_method(request.method.as_str());
    if let Some(body) = &request.body {
        opts.set_body(&JsValue::from_str(body));
    }

    let req = web_sys::Request::new_with_str_and_init(&request.url, &opts)?;
    let headers = req.headers();
    for (name, value) in &request.headers {
        headers.set(name, value)?;
    }

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_request(&req))
        .await?
        .dyn_into()?;

    let status = response.status();
    let body = JsFuture::from(response.text()?)
        .await?
        .as_string()
        .unwrap_or_default();

    if body.len() > MAX_BODY_BYTES {
        return Ok(HttpResponse::failure(
            &request.id,
            format!("response body exceeds {MAX_BODY_BYTES} bytes"),
        ));
    }

    Ok(HttpResponse {
        id: request.id.clone(),
        status,
        // `response.ok()` is the browser's own 2xx test; using it keeps this
        // agreeing with the platform rather than with our own arithmetic.
        ok: response.ok(),
        body,
        error: None,
    })
}

fn describe(value: &JsValue) -> String {
    if let Some(s) = value.as_string() {
        return s;
    }
    // A failed `fetch` rejects with a TypeError whose message is
    // deliberately vague for cross-origin reasons. Say so, rather than
    // printing `JsValue(TypeError)` and leaving the reader to guess.
    format!("{value:?} (a CORS refusal looks like this and gives no detail)")
}
