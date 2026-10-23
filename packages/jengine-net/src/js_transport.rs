//! A [`Transport`] backed by a JavaScript library the page loaded.
//!
//! Some networking is only practical as JS. Colyseus ships a browser SDK with
//! its own framing and schema decoding; reimplementing that protocol in Rust
//! would be a large, permanently-chasing-upstream job, and the SDK is right
//! there. Same for anything else a game wants — Firebase, PlayFab, a studio's
//! in-house client.
//!
//! So the engine does not bind any of them. It defines a **contract**, and a
//! few lines of JS in the page adapt whichever library to it. The engine stays
//! ignorant of Colyseus; Colyseus stays ignorant of the engine.
//!
//! # The contract
//!
//! The page installs a global object — by default `globalThis.jengineNet` —
//! with four methods:
//!
//! ```js
//! globalThis.jengineNet = {
//!   // Begin connecting. Returns a handle (any JS value) identifying it.
//!   connect(url) { return handle; },
//!   // Queue a message. Called only between `open` and `close`.
//!   send(handle, text) {},
//!   // Return everything since the last call and forget it. Each entry is
//!   // { type: "open" } | { type: "message", data } |
//!   // { type: "close", code, reason } | { type: "error", message }
//!   poll(handle) { return events; },
//!   close(handle) {},
//! };
//! ```
//!
//! `poll` drains rather than peeks, matching [`Transport::poll`]: an adapter
//! that returns the same events twice delivers every message twice.
//!
//! Load the library and the adapter through `settings.web_scripts` in
//! `project.jengine`; the builder emits them as classic scripts ahead of the
//! module that boots the wasm, so the global exists before the first frame.
//! See this package's README for a worked Colyseus adapter.

use crate::event::{NetError, NetEvent};
use crate::transport::Transport;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Default global the adapter installs.
pub const DEFAULT_GLOBAL: &str = "jengineNet";

/// Drives a JS adapter object through the contract above.
pub struct JsTransport {
    adapter: js_sys::Object,
    handle: JsValue,
    /// Set once a terminal event has been seen, so a dead connection stops
    /// being polled and stops producing fresh errors every frame.
    finished: bool,
}

impl JsTransport {
    /// Connect using the adapter at `globalThis.<global>`.
    ///
    /// Fails when the global is missing or is not shaped like the contract —
    /// which is almost always a script that did not load, so the error says
    /// that rather than leaving a silent no-op connection.
    pub fn connect(global: &str, url: &str) -> Result<Self, NetError> {
        let adapter = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str(global))
            .ok()
            .filter(|v| !v.is_undefined() && !v.is_null())
            .and_then(|v| v.dyn_into::<js_sys::Object>().ok())
            .ok_or_else(|| {
                NetError::Transport(format!(
                    "globalThis.{global} is missing - is the adapter script listed in \
                     settings.web_scripts, and did it load?"
                ))
            })?;

        for name in ["connect", "send", "poll", "close"] {
            if !is_callable(&adapter, name) {
                return Err(NetError::Transport(format!(
                    "globalThis.{global}.{name} is not a function"
                )));
            }
        }

        let handle = call1(&adapter, "connect", &JsValue::from_str(url))
            .map_err(|e| NetError::Transport(format!("connect: {}", describe(&e))))?;

        Ok(Self {
            adapter,
            handle,
            finished: false,
        })
    }
}

fn is_callable(object: &js_sys::Object, name: &str) -> bool {
    js_sys::Reflect::get(object, &JsValue::from_str(name))
        .map(|v| v.is_function())
        .unwrap_or(false)
}

fn call1(object: &js_sys::Object, name: &str, arg: &JsValue) -> Result<JsValue, JsValue> {
    let f: js_sys::Function = js_sys::Reflect::get(object, &JsValue::from_str(name))?.dyn_into()?;
    f.call1(object, arg)
}

fn call2(
    object: &js_sys::Object,
    name: &str,
    a: &JsValue,
    b: &JsValue,
) -> Result<JsValue, JsValue> {
    let f: js_sys::Function = js_sys::Reflect::get(object, &JsValue::from_str(name))?.dyn_into()?;
    f.call2(object, a, b)
}

fn describe(value: &JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| format!("{value:?}"))
}

fn field(object: &JsValue, name: &str) -> Option<JsValue> {
    js_sys::Reflect::get(object, &JsValue::from_str(name)).ok()
}

fn string_field(object: &JsValue, name: &str) -> Option<String> {
    field(object, name)?.as_string()
}

/// Turn one adapter event object into a [`NetEvent`].
///
/// An unrecognised `type` is reported rather than ignored: a typo in an
/// adapter would otherwise mean messages vanishing with nothing to show why.
fn parse_event(value: &JsValue) -> Option<NetEvent> {
    let kind = string_field(value, "type")?;
    match kind.as_str() {
        "open" => Some(NetEvent::Opened),
        "message" => {
            let data = field(value, "data")?;
            if let Some(text) = data.as_string() {
                return Some(NetEvent::Text(text));
            }
            // An adapter may hand back bytes for a binary protocol.
            if let Ok(buf) = data.clone().dyn_into::<js_sys::ArrayBuffer>() {
                return Some(NetEvent::Binary(js_sys::Uint8Array::new(&buf).to_vec()));
            }
            if let Ok(arr) = data.clone().dyn_into::<js_sys::Uint8Array>() {
                return Some(NetEvent::Binary(arr.to_vec()));
            }
            // Anything else - an object, say - is stringified as JSON, which
            // is what a Colyseus-style adapter naturally produces.
            js_sys::JSON::stringify(&data)
                .ok()
                .and_then(|s| s.as_string())
                .map(NetEvent::Text)
        }
        "close" => Some(NetEvent::Closed {
            code: field(value, "code")
                .and_then(|v| v.as_f64())
                .unwrap_or(1000.0) as u16,
            reason: string_field(value, "reason").unwrap_or_default(),
        }),
        "error" => Some(NetEvent::Failed(
            string_field(value, "message").unwrap_or_else(|| "adapter error".into()),
        )),
        other => {
            log::warn!("[net] adapter produced an unknown event type '{other}'; ignoring");
            None
        }
    }
}

impl Transport for JsTransport {
    fn poll(&mut self) -> Vec<NetEvent> {
        if self.finished {
            return Vec::new();
        }
        let raw = match call1(&self.adapter, "poll", &self.handle) {
            Ok(v) => v,
            Err(e) => {
                self.finished = true;
                return vec![NetEvent::Failed(format!("poll: {}", describe(&e)))];
            }
        };
        let Ok(array) = raw.dyn_into::<js_sys::Array>() else {
            self.finished = true;
            return vec![NetEvent::Failed("adapter poll did not return an array".into())];
        };

        let mut out = Vec::with_capacity(array.length() as usize);
        for value in array.iter() {
            if let Some(event) = parse_event(&value) {
                if matches!(event, NetEvent::Closed { .. } | NetEvent::Failed(_)) {
                    self.finished = true;
                }
                out.push(event);
            }
        }
        out
    }

    fn send_text(&mut self, text: &str) -> Result<(), NetError> {
        call2(
            &self.adapter,
            "send",
            &self.handle,
            &JsValue::from_str(text),
        )
        .map(|_| ())
        .map_err(|e| NetError::Transport(format!("send: {}", describe(&e))))
    }

    fn send_binary(&mut self, data: &[u8]) -> Result<(), NetError> {
        let array = js_sys::Uint8Array::from(data);
        call2(&self.adapter, "send", &self.handle, &array)
            .map(|_| ())
            .map_err(|e| NetError::Transport(format!("send: {}", describe(&e))))
    }

    fn close(&mut self) {
        let _ = call1(&self.adapter, "close", &self.handle);
    }
}

impl Drop for JsTransport {
    fn drop(&mut self) {
        self.close();
    }
}
