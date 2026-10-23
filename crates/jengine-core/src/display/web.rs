//! Display and cursor settings in a browser, for the editor viewport and
//! web builds alike (`super` has the model). Each host calls
//! [`WebDisplay::sync`] once a frame with its canvas.
//!
//! What a page can do: put the canvas in fullscreen (the Fullscreen API -
//! "borderless"; "fullscreen" falls back to it, a page has no video modes)
//! and change the cursor over the canvas (its CSS `cursor`: a system shape,
//! `none`, or a picture as a data URL). What it cannot: size its window,
//! switch vsync, or keep the pointer in - pointer lock is relative motion,
//! not confinement. [`super::Supports`] says so.
//!
//! A page enters fullscreen only on a recent click or key press, and leaves
//! it when the player presses Esc, so the mode a script asked for is not
//! always the mode it has. [`super::Info::mode`] is read from the document
//! every frame; a request that came to nothing, and a fullscreen the player
//! left, set the wanted mode back to windowed, so the next request is a
//! change again and an options menu shows what is true.
//!
//! The pointer lock (`Cursor.set_locked`) is the same shape: the Pointer
//! Lock API on the canvas, granted only after a recent click and let go on
//! Esc, so a refused request and a lock the player let go set the wanted
//! lock back off. While locked, `mousemove`'s `movementX` / `movementY` keep
//! coming, which is what the hosts feed `InputState::mouse_motion` with.

use std::collections::HashSet;

use wasm_bindgen::JsCast;

use super::{Applied, CursorShape, Info, Supports, WindowMode};
use crate::asset::AssetManager;

/// How long a fullscreen request may take before it is taken as refused.
const REQUEST_GRACE_MS: f64 = 1500.0;

#[derive(Default)]
pub struct WebDisplay {
    applied: Applied,
    /// When fullscreen was last asked for, until the page enters it.
    requested_at: Option<f64>,
    /// Whether the canvas was in fullscreen last frame.
    was_fullscreen: bool,
    /// When the pointer lock was last asked for, until the page grants it.
    lock_requested_at: Option<f64>,
    /// Whether the canvas held the pointer lock last frame.
    was_locked: bool,
    /// The cursor picture's CSS, by texture and hot spot.
    cursor_css: Option<(String, u16, u16, String)>,
}

fn now_ms() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map(|p| p.now()).unwrap_or(0.0)
}

fn is_fullscreen() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.fullscreen_element())
        .is_some()
}

fn is_locked(canvas: &web_sys::HtmlCanvasElement) -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.pointer_lock_element())
        .is_some_and(|e| wasm_bindgen::JsValue::from(e) == wasm_bindgen::JsValue::from(canvas.clone()))
}

fn css_icon(name: &str) -> &'static str {
    match name {
        "pointer" => "pointer",
        "crosshair" => "crosshair",
        "text" => "text",
        "move" => "move",
        "wait" => "wait",
        "progress" => "progress",
        "help" => "help",
        "not_allowed" => "not-allowed",
        "grab" => "grab",
        "grabbing" => "grabbing",
        _ => "default",
    }
}

/// A texture as a `url(data:...)` cursor, through a scratch canvas.
fn cursor_url(assets: &AssetManager, id: &str, hot_x: u16, hot_y: u16) -> Option<String> {
    let raw = assets.get_texture_bytes(id)?;
    let document = web_sys::window()?.document()?;
    let canvas: web_sys::HtmlCanvasElement = document.create_element("canvas").ok()?.dyn_into().ok()?;
    canvas.set_width(raw.width);
    canvas.set_height(raw.height);
    let ctx: web_sys::CanvasRenderingContext2d = canvas.get_context("2d").ok()??.dyn_into().ok()?;
    let data = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
        wasm_bindgen::Clamped(&raw.rgba),
        raw.width,
        raw.height,
    )
    .ok()?;
    ctx.put_image_data(&data, 0.0, 0.0).ok()?;
    let url = canvas.to_data_url().ok()?;
    Some(format!("url({url}) {hot_x} {hot_y}, auto"))
}

impl WebDisplay {
    /// Apply what changed, and publish what the page has. `max_fps` is the
    /// cap the host runs at (`None` for a host without one - the editor's
    /// Play is a development view, deliberately uncapped). Returns a new
    /// cap when a script asked for one: `Some(None)` is the project's.
    /// A cursor picture not loaded yet goes into `missing`, which the host's
    /// texture loader already watches, and is applied once it has arrived.
    pub fn sync(
        &mut self,
        canvas: &web_sys::HtmlCanvasElement,
        assets: &AssetManager,
        missing: &mut HashSet<String>,
        max_fps: Option<u32>,
    ) -> Option<Option<u32>> {
        let full = is_fullscreen();
        let mut new_cap = None;
        // The page left fullscreen by itself (Esc), or never entered it:
        // what is wanted becomes what is.
        if let Some(at) = self.requested_at {
            if full {
                self.requested_at = None;
            } else if now_ms() - at > REQUEST_GRACE_MS {
                log::info!("[display] the page did not go fullscreen (it needs a click or key press first)");
                self.requested_at = None;
                super::set_mode("windowed");
            }
        } else if self.was_fullscreen && !full {
            super::set_mode("windowed");
        }
        self.was_fullscreen = full;
        // The same for the pointer lock: refused, or let go with Esc.
        let locked = is_locked(canvas);
        if let Some(at) = self.lock_requested_at {
            if locked {
                self.lock_requested_at = None;
            } else if now_ms() - at > REQUEST_GRACE_MS {
                log::info!("[display] the page did not lock the pointer (it needs a click first)");
                self.lock_requested_at = None;
                super::set_cursor_locked(false);
            }
        } else if self.was_locked && !locked {
            super::set_cursor_locked(false);
        }
        self.was_locked = locked;

        if let Some((want, changes)) = self.applied.changes() {
            if changes.window {
                let want_full = want.mode != WindowMode::Windowed;
                if want_full && !full {
                    match canvas.request_fullscreen() {
                        Ok(()) => self.requested_at = Some(now_ms()),
                        Err(e) => {
                            log::warn!("[display] fullscreen refused: {e:?}");
                            super::set_mode("windowed");
                        }
                    }
                } else if !want_full
                    && full
                    && let Some(document) = web_sys::window().and_then(|w| w.document())
                {
                    document.exit_fullscreen();
                }
            }
            if changes.max_fps && max_fps.is_some() {
                new_cap = Some(want.max_fps);
            }
            let mut pending = false;
            if changes.cursor {
                if want.cursor_locked && !locked && self.lock_requested_at.is_none() {
                    canvas.request_pointer_lock();
                    self.lock_requested_at = Some(now_ms());
                } else if !want.cursor_locked
                    && locked
                    && let Some(document) = web_sys::window().and_then(|w| w.document())
                {
                    document.exit_pointer_lock();
                }
                let css = if !want.cursor_visible {
                    "none".to_string()
                } else {
                    match &want.cursor {
                        CursorShape::Icon(name) => css_icon(name).to_string(),
                        CursorShape::Image { texture_id, hot_x, hot_y } => {
                            let key = (texture_id.clone(), *hot_x, *hot_y);
                            match &self.cursor_css {
                                Some((id, x, y, css)) if (id, x, y) == (&key.0, &key.1, &key.2) => css.clone(),
                                _ => match cursor_url(assets, texture_id, *hot_x, *hot_y) {
                                    Some(css) => {
                                        self.cursor_css = Some((key.0, key.1, key.2, css.clone()));
                                        css
                                    }
                                    None => {
                                        missing.insert(texture_id.clone());
                                        pending = true;
                                        String::new()
                                    }
                                },
                            }
                        }
                    }
                };
                if !pending {
                    let _ = canvas.style().set_property("cursor", &css);
                }
            }
            self.applied.done(want, pending);
        }

        super::publish(Info {
            mode: if full { WindowMode::Borderless } else { WindowMode::Windowed },
            width: canvas.width(),
            height: canvas.height(),
            vsync: true,
            max_fps: max_fps.unwrap_or(0),
            modes: web_sys::window()
                .and_then(|w| w.screen().ok())
                .map(|s| {
                    let dpr = web_sys::window().map(|w| w.device_pixel_ratio()).unwrap_or(1.0);
                    let w = (s.width().unwrap_or(0) as f64 * dpr).round() as u32;
                    let h = (s.height().unwrap_or(0) as f64 * dpr).round() as u32;
                    vec![(w, h, 60)]
                })
                .unwrap_or_default(),
            supports: Supports {
                fullscreen: true,
                exclusive: false,
                window_size: false,
                vsync: false,
                max_fps: max_fps.is_some(),
                cursor_image: true,
                cursor_confine: false,
                cursor_lock: true,
            },
        });
        new_cap
    }

    /// Give the page back: out of fullscreen, the default cursor. The editor
    /// calls it when Play stops, after [`super::reset`].
    pub fn release(&mut self, canvas: &web_sys::HtmlCanvasElement) {
        super::reset();
        if is_fullscreen()
            && let Some(document) = web_sys::window().and_then(|w| w.document())
        {
            document.exit_fullscreen();
        }
        if is_locked(canvas)
            && let Some(document) = web_sys::window().and_then(|w| w.document())
        {
            document.exit_pointer_lock();
        }
        let _ = canvas.style().set_property("cursor", "");
        self.requested_at = None;
        self.was_fullscreen = false;
        self.lock_requested_at = None;
        self.was_locked = false;
        if let Some((s, _)) = self.applied.changes() {
            self.applied.done(s, false);
        }
    }
}
