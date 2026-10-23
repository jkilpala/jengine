//! The native runner's half of `jengine_core::display`: once a frame, apply
//! what a script changed - the window's mode and size through winit, vsync
//! through the swapchain's present mode, the frame cap, the cursor - and
//! publish what the window has, for `Display.get_*`.
//!
//! On Android the window is the whole screen and has no cursor, so only
//! vsync and the frame cap apply; `Supports` says so.

use std::collections::HashSet;

use jengine_core::asset::AssetManager;
use jengine_core::display::{self, Applied, Info, Supports, WindowMode};
use jengine_core::renderer::backend::wgpu::WgpuRenderer;
use jengine_core::time::FrameCap;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

pub(crate) struct NativeDisplay {
    applied: Applied,
    /// The project's `settings.frame_rate_cap` for this platform, which
    /// `Display.set_max_fps(-1)` goes back to.
    project_fps: u32,
    /// The cap in force, for `Display.get_max_fps`.
    fps: u32,
    /// The current monitor's resolutions, and which monitor they are of.
    modes: Vec<(u32, u32, u32)>,
    modes_of: Option<String>,
    /// The cursor picture made last, by texture and hot spot.
    #[cfg(not(target_os = "android"))]
    custom: Option<((String, u16, u16), winit::window::CustomCursor)>,
}

impl NativeDisplay {
    pub(crate) fn new(project_fps: u32) -> Self {
        Self {
            applied: Applied::default(),
            project_fps,
            fps: project_fps,
            modes: Vec::new(),
            modes_of: None,
            #[cfg(not(target_os = "android"))]
            custom: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn sync(
        &mut self,
        window: &Window,
        event_loop: &ActiveEventLoop,
        renderer: &mut WgpuRenderer,
        frame_cap: &mut FrameCap,
        assets: &AssetManager,
        missing: &mut HashSet<String>,
    ) {
        self.refresh_modes(window);
        if let Some((want, changes)) = self.applied.changes() {
            if changes.window {
                self.apply_window(window, &want);
            }
            if changes.vsync {
                renderer.set_vsync(want.vsync);
            }
            if changes.max_fps || changes.vsync {
                self.fps = want.max_fps.unwrap_or(self.project_fps);
                // Without vsync there is no refresh to land on, so the cap
                // waits its whole interval (`FrameCap::exact`).
                *frame_cap = if renderer.vsync() { FrameCap::new(self.fps) } else { FrameCap::exact(self.fps) };
                if !frame_cap.is_capped() {
                    // Uncapped frames ask for their own successors again;
                    // the capped loop may have left the event loop waiting.
                    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
                    window.request_redraw();
                }
                log::info!("[display] frame cap {}", if self.fps == 0 { "off".to_string() } else { format!("{} fps", self.fps) });
            }
            let mut pending = false;
            if changes.cursor {
                pending = self.apply_cursor(window, event_loop, &want, assets, missing);
            }
            self.applied.done(want, pending);
        }
        let (width, height) = renderer.viewport_size();
        display::publish(Info {
            mode: current_mode(window),
            width,
            height,
            vsync: renderer.vsync(),
            max_fps: self.fps,
            modes: self.modes.clone(),
            supports: supports(),
        });
    }

    fn refresh_modes(&mut self, window: &Window) {
        let Some(monitor) = window.current_monitor() else {
            return;
        };
        let name = monitor.name();
        if self.modes_of.is_some() && self.modes_of == name {
            return;
        }
        self.modes = display::distinct_modes(
            monitor.video_modes().map(|m| (m.size().width, m.size().height, m.refresh_rate_millihertz())),
        );
        self.modes_of = name.or(Some(String::new()));
    }

    #[cfg(target_os = "android")]
    fn apply_window(&mut self, _window: &Window, _want: &display::Settings) {}

    #[cfg(not(target_os = "android"))]
    fn apply_window(&mut self, window: &Window, want: &display::Settings) {
        use winit::window::Fullscreen;
        match want.mode {
            WindowMode::Windowed => {
                window.set_fullscreen(None);
                if let Some((w, h)) = want.size {
                    let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(w, h));
                }
            }
            WindowMode::Borderless => window.set_fullscreen(Some(Fullscreen::Borderless(None))),
            WindowMode::Fullscreen => {
                // The video mode nearest the size asked for, or the
                // monitor's own size.
                let Some(monitor) = window.current_monitor() else {
                    window.set_fullscreen(Some(Fullscreen::Borderless(None)));
                    return;
                };
                let modes: Vec<_> = monitor.video_modes().collect();
                let raw: Vec<(u32, u32, u32)> = modes
                    .iter()
                    .map(|m| (m.size().width, m.size().height, m.refresh_rate_millihertz() / 1000))
                    .collect();
                let size = want.size.unwrap_or((monitor.size().width, monitor.size().height));
                match display::closest_mode(&raw, size) {
                    Some(i) => {
                        log::info!("[display] fullscreen at {}x{} {} Hz", raw[i].0, raw[i].1, raw[i].2);
                        window.set_fullscreen(Some(Fullscreen::Exclusive(modes[i].clone())));
                    }
                    None => window.set_fullscreen(Some(Fullscreen::Borderless(None))),
                }
            }
        }
    }

    /// Returns whether the cursor waits for its picture to load.
    #[cfg(target_os = "android")]
    fn apply_cursor(
        &mut self,
        _window: &Window,
        _event_loop: &ActiveEventLoop,
        _want: &display::Settings,
        _assets: &AssetManager,
        _missing: &mut HashSet<String>,
    ) -> bool {
        false
    }

    #[cfg(not(target_os = "android"))]
    fn apply_cursor(
        &mut self,
        window: &Window,
        event_loop: &ActiveEventLoop,
        want: &display::Settings,
        assets: &AssetManager,
        missing: &mut HashSet<String>,
    ) -> bool {
        use jengine_core::display::CursorShape;
        use winit::window::{CursorGrabMode, CursorIcon, CustomCursor};
        // A locked pointer is hidden and held; its motion is the device's raw
        // motion (`DeviceEvent::MouseMotion` in the runner), so it counts at
        // the window edge whichever grab the platform grants - `Locked` where
        // there is one, `Confined` on Windows, which has none.
        window.set_cursor_visible(want.cursor_visible && !want.cursor_locked);
        let grab = if want.cursor_locked {
            window.set_cursor_grab(CursorGrabMode::Locked).or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
        } else if want.cursor_confined {
            window.set_cursor_grab(CursorGrabMode::Confined).or_else(|_| window.set_cursor_grab(CursorGrabMode::Locked))
        } else {
            window.set_cursor_grab(CursorGrabMode::None)
        };
        if let Err(e) = grab {
            log::warn!("[display] cursor grab: {e}");
        }
        match &want.cursor {
            CursorShape::Icon(name) => window.set_cursor(match *name {
                "pointer" => CursorIcon::Pointer,
                "crosshair" => CursorIcon::Crosshair,
                "text" => CursorIcon::Text,
                "move" => CursorIcon::Move,
                "wait" => CursorIcon::Wait,
                "progress" => CursorIcon::Progress,
                "help" => CursorIcon::Help,
                "not_allowed" => CursorIcon::NotAllowed,
                "grab" => CursorIcon::Grab,
                "grabbing" => CursorIcon::Grabbing,
                _ => CursorIcon::Default,
            }),
            CursorShape::Image { texture_id, hot_x, hot_y } => {
                let key = (texture_id.clone(), *hot_x, *hot_y);
                if let Some((k, cursor)) = &self.custom
                    && *k == key
                {
                    window.set_cursor(cursor.clone());
                    return false;
                }
                let Some(raw) = assets.get_texture_bytes(texture_id) else {
                    // The runner loads what is missing; asked again next frame.
                    missing.insert(texture_id.clone());
                    return true;
                };
                let (w, h) = (raw.width.min(u16::MAX as u32) as u16, raw.height.min(u16::MAX as u32) as u16);
                match CustomCursor::from_rgba(
                    raw.rgba.clone(),
                    w,
                    h,
                    (*hot_x).min(w.saturating_sub(1)),
                    (*hot_y).min(h.saturating_sub(1)),
                ) {
                    Ok(source) => {
                        let cursor = event_loop.create_custom_cursor(source);
                        window.set_cursor(cursor.clone());
                        self.custom = Some((key, cursor));
                    }
                    Err(e) => log::warn!("[display] cursor '{texture_id}' is not a cursor: {e}"),
                }
            }
        }
        false
    }
}

fn current_mode(window: &Window) -> WindowMode {
    use winit::window::Fullscreen;
    match window.fullscreen() {
        None => WindowMode::Windowed,
        Some(Fullscreen::Borderless(_)) => WindowMode::Borderless,
        Some(Fullscreen::Exclusive(_)) => WindowMode::Fullscreen,
    }
}

fn supports() -> Supports {
    let desktop = !cfg!(target_os = "android");
    Supports {
        fullscreen: desktop,
        exclusive: desktop,
        window_size: desktop,
        vsync: true,
        max_fps: true,
        cursor_image: desktop,
        cursor_confine: desktop,
        cursor_lock: desktop,
    }
}
