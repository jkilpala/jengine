//! The clipboard, as far as the engine goes (Oct 2026): a text field's copy
//! and cut put the selection here ([`copy`]), and the host moves it to the
//! system clipboard once a frame ([`take_copy`]) - `arboard` natively,
//! `navigator.clipboard.writeText` in a browser. A paste goes the other way:
//! the host reads the system clipboard when ctrl+V is pressed over a field
//! (or the page's own paste event fires) and hands the text to
//! [`crate::input::InputState::paste`]. The process's, like haptics.

use std::sync::Mutex;

static COPY: Mutex<Option<String>> = Mutex::new(None);

/// A field copied `text`.
pub fn copy(text: &str) {
    if text.is_empty() {
        return;
    }
    if let Ok(mut c) = COPY.lock() {
        *c = Some(text.to_string());
    }
}

/// What to put on the system clipboard, if anything was copied since the
/// last call.
pub fn take_copy() -> Option<String> {
    COPY.lock().ok().and_then(|mut c| c.take())
}
