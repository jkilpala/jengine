//! The native runner's half of text entry (`jengine_core::input::text_entry`,
//! `clipboard`): while a field is focused the IME is allowed and its
//! candidate window placed at the caret (on Android, the soft keyboard is
//! up); an IME's composition is shown in the field and its commit typed;
//! the clipboard is the system's (`arboard`, desktop only).

use jengine_core::input::{InputState, KeyCode, clipboard, text_entry};
use jengine_core::ui::scaling::UiScale;
use winit::event::Ime;
use winit::window::Window;

#[derive(Default)]
pub(crate) struct NativeText {
    active: bool,
    area: Option<[i32; 4]>,
}

impl NativeText {
    /// Once a frame: the clipboard copy, and the IME (or the soft keyboard)
    /// following whether a field is focused and where its caret is.
    pub(crate) fn sync(&mut self, window: &Window, ui_scale: &UiScale) {
        if let Some(text) = clipboard::take_copy() {
            set_clipboard(&text);
        }
        let active = text_entry::take();
        if active.is_some() != self.active {
            self.active = active.is_some();
            window.set_ime_allowed(self.active);
            if !self.active {
                text_entry::set_preedit("");
                self.area = None;
            }
            #[cfg(target_os = "android")]
            crate::android::show_keyboard(self.active);
        }
        if let Some(a) = active {
            let size = window.inner_size();
            let (sw, sh) = (size.width as f32, size.height as f32);
            let (x, y) = ui_scale.to_screen(a.caret[0], a.caret[1], sw, sh);
            let f = ui_scale.factor(sw, sh);
            let area = [x as i32, y as i32, (a.caret[2] * f).max(1.0) as i32, (a.caret[3] * f).max(1.0) as i32];
            if self.area != Some(area) {
                self.area = Some(area);
                window.set_ime_cursor_area(
                    winit::dpi::PhysicalPosition::new(area[0], area[1]),
                    winit::dpi::PhysicalSize::new(area[2] as u32, area[3] as u32),
                );
            }
        }
    }

    /// An IME event: a composition shows in the field, a commit is typed.
    pub(crate) fn ime(event: &Ime, input: &mut InputState) {
        match event {
            Ime::Preedit(text, _) => text_entry::set_preedit(text),
            Ime::Commit(text) => {
                text_entry::set_preedit("");
                input.type_str(text);
            }
            Ime::Disabled => text_entry::set_preedit(""),
            Ime::Enabled => {}
        }
    }

    /// ctrl+V over a focused field: the system clipboard's text.
    pub(crate) fn paste_if_asked(code: KeyCode, input: &mut InputState) {
        if code == KeyCode::V
            && input.ctrl_held()
            && text_entry::is_active()
            && let Some(text) = get_clipboard()
        {
            input.paste(&text);
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn set_clipboard(text: &str) {
    if let Err(e) = arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())) {
        log::warn!("[text] could not copy to the clipboard: {e}");
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn get_clipboard() -> Option<String> {
    arboard::Clipboard::new().and_then(|mut c| c.get_text()).ok()
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn set_clipboard(_text: &str) {}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn get_clipboard() -> Option<String> {
    None
}
