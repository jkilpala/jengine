//! Browser WebSocket transport.
//!
//! The browser delivers frames through JS callbacks on the main thread, so
//! there is no socket thread here and no blocking to avoid. The work is
//! plumbing: keep the closures alive for as long as the socket, and park what
//! they receive somewhere `poll` can drain.
//!
//! `Rc<RefCell<…>>` rather than `Arc<Mutex<…>>` on purpose — wasm is
//! single-threaded and a `web_sys::WebSocket` is not `Send`, so the atomic
//! machinery would cost something and buy nothing.

use crate::event::{NetError, NetEvent};
use crate::transport::Transport;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// A WebSocket connection in the browser.
pub struct WebSocketTransport {
    socket: web_sys::WebSocket,
    /// Filled by the JS callbacks, drained by `poll`.
    events: Rc<RefCell<Vec<NetEvent>>>,
    /// The closures must outlive the calls that registered them; dropping
    /// them would unregister the handlers and the socket would go silent.
    _on_open: Closure<dyn FnMut()>,
    _on_message: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _on_close: Closure<dyn FnMut(web_sys::CloseEvent)>,
    _on_error: Closure<dyn FnMut(web_sys::Event)>,
}

impl WebSocketTransport {
    /// Open a connection to `url` (`ws://` or `wss://`).
    ///
    /// Fails only if the browser rejects the URL outright; everything else
    /// arrives as an event, matching the native backend.
    pub fn connect(url: &str) -> Result<Self, NetError> {
        let socket = web_sys::WebSocket::new(url)
            .map_err(|e| NetError::Transport(format!("open {url}: {}", describe(&e))))?;
        // Ask for ArrayBuffer rather than the default Blob: a Blob is only
        // readable asynchronously, which would turn every binary frame into a
        // second trip through the event loop.
        socket.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let events: Rc<RefCell<Vec<NetEvent>>> = Rc::new(RefCell::new(Vec::new()));

        let on_open = {
            let events = Rc::clone(&events);
            Closure::<dyn FnMut()>::new(move || {
                events.borrow_mut().push(NetEvent::Opened);
            })
        };
        socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));

        let on_message = {
            let events = Rc::clone(&events);
            Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |e: web_sys::MessageEvent| {
                let data = e.data();
                if let Some(text) = data.as_string() {
                    events.borrow_mut().push(NetEvent::Text(text));
                } else if let Ok(buf) = data.clone().dyn_into::<js_sys::ArrayBuffer>() {
                    let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                    events.borrow_mut().push(NetEvent::Binary(bytes));
                } else {
                    // A Blob, despite the binary_type above, or something
                    // exotic. Say so rather than dropping it silently.
                    log::warn!("[net] ignoring a frame that is neither text nor ArrayBuffer");
                }
            })
        };
        socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        let on_close = {
            let events = Rc::clone(&events);
            Closure::<dyn FnMut(web_sys::CloseEvent)>::new(move |e: web_sys::CloseEvent| {
                events.borrow_mut().push(NetEvent::Closed {
                    code: e.code(),
                    reason: e.reason(),
                });
            })
        };
        socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));

        let on_error = {
            let events = Rc::clone(&events);
            Closure::<dyn FnMut(web_sys::Event)>::new(move |_e: web_sys::Event| {
                // The browser deliberately withholds the reason for a
                // WebSocket error, to avoid leaking cross-origin information.
                // There is no detail to report and inventing one would be
                // worse than admitting that.
                events.borrow_mut().push(NetEvent::Failed(
                    "websocket error (the browser does not say why)".into(),
                ));
            })
        };
        socket.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        Ok(Self {
            socket,
            events,
            _on_open: on_open,
            _on_message: on_message,
            _on_close: on_close,
            _on_error: on_error,
        })
    }
}

fn describe(value: &JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| format!("{value:?}"))
}

impl Transport for WebSocketTransport {
    fn poll(&mut self) -> Vec<NetEvent> {
        std::mem::take(&mut *self.events.borrow_mut())
    }

    fn send_text(&mut self, text: &str) -> Result<(), NetError> {
        self.socket
            .send_with_str(text)
            .map_err(|e| NetError::Transport(describe(&e)))
    }

    fn send_binary(&mut self, data: &[u8]) -> Result<(), NetError> {
        self.socket
            .send_with_u8_array(data)
            .map_err(|e| NetError::Transport(describe(&e)))
    }

    fn close(&mut self) {
        let _ = self.socket.close();
    }
}

impl Drop for WebSocketTransport {
    fn drop(&mut self) {
        // Clear the handlers before closing: a callback firing after the
        // closures are dropped would reach freed memory.
        self.socket.set_onopen(None);
        self.socket.set_onmessage(None);
        self.socket.set_onclose(None);
        self.socket.set_onerror(None);
        let _ = self.socket.close();
    }
}
