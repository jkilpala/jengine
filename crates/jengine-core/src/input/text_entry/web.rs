//! Typing in a browser through a hidden `<textarea>` (Oct 2026), shared by
//! the editor viewport and web builds: while a field is focused the proxy is
//! focused, placed at the caret, and what reaches it - a key's character, an
//! IME's composition and commit, a paste, a phone's on-screen keyboard - is
//! handed to the frame's `InputState`. A canvas cannot receive any of
//! those: a page has no composition or paste events for it, and a phone
//! raises its keyboard only for an editable element.
//!
//! The hosts' own key listeners still see every key (the events bubble to
//! the document), so Backspace and the arrows arrive as keys as always; they
//! only leave the proxy's characters to it ([`TextProxy::is_proxy_event`]),
//! or each letter would be typed twice, and leave its keys their default
//! action, or the textarea would never receive them.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use super::{preedit, set_preedit, take};
use crate::input::{InputState, clipboard};
use crate::ui::scaling::UiScale;

/// The proxy's element id.
pub const PROXY_ID: &str = "jengine-text-proxy";

#[derive(Default)]
struct Queue {
    typed: String,
    pasted: String,
}

#[derive(Default)]
pub struct TextProxy {
    el: Option<web_sys::HtmlTextAreaElement>,
    focused: bool,
    queue: Rc<RefCell<Queue>>,
}

fn reflect_str(obj: &JsValue, key: &str) -> Option<String> {
    js_sys::Reflect::get(obj, &JsValue::from_str(key)).ok()?.as_string()
}

impl TextProxy {
    /// Whether a key event came from the proxy - which types its own
    /// characters, and must keep its default action.
    pub fn is_proxy_event(e: &web_sys::Event) -> bool {
        e.target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .is_some_and(|el| el.id() == PROXY_ID)
    }

    fn ensure(&mut self) -> Option<web_sys::HtmlTextAreaElement> {
        if let Some(el) = &self.el {
            return Some(el.clone());
        }
        let document = web_sys::window()?.document()?;
        let el: web_sys::HtmlTextAreaElement = document.create_element("textarea").ok()?.dyn_into().ok()?;
        el.set_id(PROXY_ID);
        for (k, v) in [("autocomplete", "off"), ("autocorrect", "off"), ("autocapitalize", "off"), ("spellcheck", "false"), ("aria-hidden", "true")] {
            let _ = el.set_attribute(k, v);
        }
        // Invisible but focusable, and 16px so a phone does not zoom to it.
        let _ = el.style().set_css_text(
            "position:fixed;left:0;top:0;width:2px;height:24px;opacity:0;border:0;padding:0;margin:0;\
             resize:none;overflow:hidden;font-size:16px;pointer-events:none;z-index:2147483647;",
        );
        document.body()?.append_child(&el).ok()?;

        // What was typed: the textarea's value, emptied again, unless an IME
        // is still composing it.
        let q = Rc::clone(&self.queue);
        let area = el.clone();
        let on_input = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let composing = js_sys::Reflect::get(&e, &JsValue::from_str("isComposing"))
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if composing {
                return;
            }
            let v = area.value();
            if !v.is_empty() {
                q.borrow_mut().typed.push_str(&v);
                area.set_value("");
            }
        });
        let _ = el.add_event_listener_with_callback("input", on_input.as_ref().unchecked_ref());
        on_input.forget();
        let on_compose = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            set_preedit(&reflect_str(&e, "data").unwrap_or_default());
        });
        let _ = el.add_event_listener_with_callback("compositionupdate", on_compose.as_ref().unchecked_ref());
        on_compose.forget();
        let q = Rc::clone(&self.queue);
        let area = el.clone();
        let on_end = Closure::<dyn FnMut(web_sys::Event)>::new(move |_e: web_sys::Event| {
            set_preedit("");
            let v = area.value();
            if !v.is_empty() {
                q.borrow_mut().typed.push_str(&v);
                area.set_value("");
            }
        });
        let _ = el.add_event_listener_with_callback("compositionend", on_end.as_ref().unchecked_ref());
        on_end.forget();
        // A paste arrives whole, as a paste.
        let q = Rc::clone(&self.queue);
        let on_paste = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let data = js_sys::Reflect::get(&e, &JsValue::from_str("clipboardData")).unwrap_or(JsValue::NULL);
            let get = js_sys::Reflect::get(&data, &JsValue::from_str("getData"))
                .ok()
                .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
            if let Some(get) = get
                && let Ok(text) = get.call1(&data, &JsValue::from_str("text"))
                && let Some(text) = text.as_string()
            {
                q.borrow_mut().pasted.push_str(&text);
                e.prevent_default();
            }
        });
        let _ = el.add_event_listener_with_callback("paste", on_paste.as_ref().unchecked_ref());
        on_paste.forget();
        self.el = Some(el.clone());
        Some(el)
    }

    /// Once a frame, before the UI updates: hand over what the proxy got,
    /// put a copy on the clipboard, and follow the focused field - focus
    /// and place the proxy while there is one, let it go when there is not.
    pub fn sync(&mut self, canvas: &web_sys::HtmlCanvasElement, ui_scale: &UiScale, input: &mut InputState) {
        {
            let mut q = self.queue.borrow_mut();
            if !q.typed.is_empty() {
                input.type_str(&std::mem::take(&mut q.typed));
            }
            if !q.pasted.is_empty() {
                input.paste(&std::mem::take(&mut q.pasted));
            }
        }
        if let Some(text) = clipboard::take_copy() {
            write_clipboard(&text);
        }
        match take() {
            Some(active) => {
                let Some(el) = self.ensure() else { return };
                // The caret in CSS pixels: UI pixels to the canvas's own,
                // then to the page's.
                let (cw, ch) = (canvas.width() as f32, canvas.height() as f32);
                let (x, y) = ui_scale.to_screen(active.caret[0], active.caret[1], cw, ch);
                let r = canvas.get_bounding_client_rect();
                let k = if cw > 0.0 { r.width() as f32 / cw } else { 1.0 };
                let style = el.style();
                let _ = style.set_property("left", &format!("{}px", r.left() as f32 + x * k));
                let _ = style.set_property("top", &format!("{}px", r.top() as f32 + y * k));
                if !self.focused {
                    let _ = el.focus();
                    self.focused = true;
                }
            }
            None => self.release(),
        }
    }

    /// No field is focused: the proxy lets go, and any composition with it.
    pub fn release(&mut self) {
        if self.focused {
            if let Some(el) = &self.el {
                let _ = el.blur();
                el.set_value("");
            }
            self.focused = false;
        }
        if !preedit().is_empty() {
            set_preedit("");
        }
    }
}

/// `navigator.clipboard.writeText(text)`, looked up rather than bound (the
/// Clipboard API is behind web-sys's unstable flag). It needs the page
/// focused and a recent key press - which ctrl+C is.
fn write_clipboard(text: &str) {
    let Some(window) = web_sys::window() else { return };
    let navigator = js_sys::Reflect::get(&window, &JsValue::from_str("navigator")).unwrap_or(JsValue::NULL);
    let clip = js_sys::Reflect::get(&navigator, &JsValue::from_str("clipboard")).unwrap_or(JsValue::NULL);
    if clip.is_undefined() || clip.is_null() {
        log::warn!("[text] this page has no clipboard (it needs https)");
        return;
    }
    if let Some(write) = js_sys::Reflect::get(&clip, &JsValue::from_str("writeText"))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
    {
        let _ = write.call1(&clip, &JsValue::from_str(text));
    }
}
