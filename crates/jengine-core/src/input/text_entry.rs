//! Which text field is being written in this frame, for what lives outside
//! the engine (Oct 2026): an IME's candidate window wants to sit by the
//! caret, a phone's keyboard wants to come up, and a browser page wants a
//! focused element to send composition and paste to. The two UI systems
//! [`report`] the focused field's caret each frame they update one; a host
//! reads [`take`] once a frame - `Some` while someone is typing.
//!
//! In a browser that element is a hidden `<textarea>` ([`web`]): focused
//! while a field is, it receives what the page's keyboard, IME and paste
//! produce, and hands it to the frame's `InputState`.

use std::sync::Mutex;

/// The focused field: its caret on screen, in UI pixels from the top left
/// (left, top, width, height).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Active {
    pub caret: [f32; 4],
}

static FRAME: Mutex<(Option<Active>, Option<Active>)> = Mutex::new((None, None));
static PREEDIT: Mutex<String> = Mutex::new(String::new());

/// What an IME is composing and has not committed (empty: nothing). The
/// focused field draws it at its caret, underlined; it is not part of the
/// text until the IME commits it as typed text. Set by the host from its
/// IME's events.
pub fn set_preedit(text: &str) {
    if let Ok(mut p) = PREEDIT.lock() {
        p.clear();
        p.push_str(text);
    }
}

pub fn preedit() -> String {
    PREEDIT.lock().map(|p| p.clone()).unwrap_or_default()
}

/// A field is focused this frame, its caret here.
pub fn report(active: Active) {
    if let Ok(mut f) = FRAME.lock() {
        f.0 = Some(active);
    }
}

/// The field reported since the last call, if any - so a host learns when
/// typing starts and stops. A frame in which no field reported is `None`.
pub fn take() -> Option<Active> {
    FRAME.lock().ok().and_then(|mut f| {
        let now = f.0.take();
        f.1 = now;
        now
    })
}

/// Whether a field was focused when a host last asked.
pub fn is_active() -> bool {
    FRAME.lock().ok().is_some_and(|f| f.1.is_some() || f.0.is_some())
}

/// The hidden `<textarea>` the browser hosts type through.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod web;
