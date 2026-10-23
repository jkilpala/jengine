//! Display and cursor settings (Oct 2026; Godot's `DisplayServer` window
//! mode, vsync and mouse mode, Unity's `Screen.fullScreenMode` /
//! `QualitySettings.vSyncCount` / `Cursor`).
//!
//! What an options menu changes: windowed, borderless or exclusive
//! fullscreen, the window's size (or the fullscreen resolution), vsync, a
//! frame cap, and the cursor - shown or hidden, kept inside the window, a
//! system shape or a picture. Like haptics and the language, these are the
//! **process's**, not the world's: a script writes what it wants here
//! ([`Settings`], under a generation counter), and each host, once a frame,
//! applies whatever changed since it last looked ([`Applied::changes`]) and
//! says what it got ([`publish`], read back by `Display.get_*`). So the
//! settings survive a scene change - the options scene sets fullscreen and
//! the game scene is fullscreen too.
//!
//! The hosts: the native runner through winit and the swapchain's present
//! mode (`jengine-runtime/src/display_native.rs`); both browser hosts
//! through [`web::sync`] - the Fullscreen API and the canvas's cursor style.
//! A browser cannot size its window, switch vsync, take exclusive
//! fullscreen or keep the cursor in (pointer lock is relative motion, not
//! confinement), so it says so through [`Info::supports`] rather than
//! pretending; a menu hides those rows.
//!
//! **A locked pointer** (`Cursor.set_locked`, Oct 2026) is hidden and held,
//! and the game reads its motion with `Input.get_mouse_delta` - what a
//! spinner, a trackball or a mouse-look wants (a tube shooter's spinner used
//! to stop at the window edge). Natively that is winit's
//! `Locked` grab where the platform has it and `Confined` where it does not
//! (Windows) - the motion is the device's raw motion either way, so it keeps
//! counting at the edge; in a browser the Pointer Lock API, which, like
//! fullscreen, needs a recent click and is let go on Esc.

use std::sync::Mutex;

/// How the window sits on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    #[default]
    Windowed,
    /// The whole screen, at the desktop's resolution, without changing it.
    Borderless,
    /// The whole screen at a resolution of its own (a video mode).
    Fullscreen,
}

impl WindowMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "windowed" | "window" => Some(Self::Windowed),
            "borderless" | "borderless_fullscreen" | "fullscreen_window" => Some(Self::Borderless),
            "fullscreen" | "exclusive" | "exclusive_fullscreen" => Some(Self::Fullscreen),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Windowed => "windowed",
            Self::Borderless => "borderless",
            Self::Fullscreen => "fullscreen",
        }
    }
}

/// The system cursor shapes a script can ask for by name - the subset every
/// platform has (winit's `CursorIcon`, CSS's `cursor`).
pub const CURSOR_ICONS: &[&str] = &[
    "default", "pointer", "crosshair", "text", "move", "wait", "progress", "help", "not_allowed", "grab",
    "grabbing",
];

/// The cursor's picture.
#[derive(Debug, Clone, PartialEq)]
pub enum CursorShape {
    /// One of [`CURSOR_ICONS`].
    Icon(&'static str),
    /// A texture, its hot spot in its pixels from the top left.
    Image { texture_id: String, hot_x: u16, hot_y: u16 },
}

impl Default for CursorShape {
    fn default() -> Self {
        Self::Icon("default")
    }
}

/// What the scripts asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub mode: WindowMode,
    /// The window's size in pixels, or the fullscreen resolution; `None`
    /// keeps whatever it is.
    pub size: Option<(u32, u32)>,
    pub vsync: bool,
    /// Frames a second at most; `None` is the project's
    /// `settings.frame_rate_cap`, `Some(0)` none at all.
    pub max_fps: Option<u32>,
    pub cursor_visible: bool,
    /// Kept inside the window.
    pub cursor_confined: bool,
    /// Hidden and held, read as motion (`Input.get_mouse_delta`).
    pub cursor_locked: bool,
    pub cursor: CursorShape,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: WindowMode::Windowed,
            size: None,
            vsync: true,
            max_fps: None,
            cursor_visible: true,
            cursor_confined: false,
            cursor_locked: false,
            cursor: CursorShape::default(),
        }
    }
}

/// Which parts of [`Settings`] differ - what a host has to redo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Changes {
    /// The mode or the size (one decides the other in fullscreen).
    pub window: bool,
    pub vsync: bool,
    pub max_fps: bool,
    /// Visibility, confinement, the lock or the shape.
    pub cursor: bool,
}

impl Changes {
    pub fn between(a: &Settings, b: &Settings) -> Self {
        Self {
            window: a.mode != b.mode || a.size != b.size,
            vsync: a.vsync != b.vsync,
            max_fps: a.max_fps != b.max_fps,
            cursor: a.cursor_visible != b.cursor_visible
                || a.cursor_confined != b.cursor_confined
                || a.cursor_locked != b.cursor_locked
                || a.cursor != b.cursor,
        }
    }

    pub fn any(&self) -> bool {
        self.window || self.vsync || self.max_fps || self.cursor
    }
}

/// What a host can do, for a menu to show only the rows that work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Supports {
    pub fullscreen: bool,
    /// A fullscreen resolution of its own, not just the desktop's.
    pub exclusive: bool,
    pub window_size: bool,
    pub vsync: bool,
    pub max_fps: bool,
    pub cursor_image: bool,
    pub cursor_confine: bool,
    pub cursor_lock: bool,
}

impl Supports {
    /// `Display.supports(name)`.
    pub fn get(&self, name: &str) -> bool {
        match name.trim().to_ascii_lowercase().as_str() {
            "fullscreen" | "borderless" => self.fullscreen,
            "exclusive" | "exclusive_fullscreen" | "resolution" => self.exclusive,
            "window_size" | "size" => self.window_size,
            "vsync" => self.vsync,
            "max_fps" | "frame_cap" => self.max_fps,
            "cursor_image" => self.cursor_image,
            "cursor_confine" | "confine" => self.cursor_confine,
            "cursor_lock" | "lock" | "pointer_lock" => self.cursor_lock,
            "cursor_icon" | "cursor_visible" | "cursor" => true,
            _ => false,
        }
    }
}

/// What the host has, as `Display.get_*` reads it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Info {
    pub mode: WindowMode,
    /// The drawable size in pixels.
    pub width: u32,
    pub height: u32,
    pub vsync: bool,
    /// The frame cap in force; 0 is none.
    pub max_fps: u32,
    /// The resolutions the screen offers, largest first: `(w, h, hz)`, one
    /// per size at its highest rate.
    pub modes: Vec<(u32, u32, u32)>,
    pub supports: Supports,
}

static DESIRED: Mutex<(u64, Option<Settings>)> = Mutex::new((0, None));
static INFO: Mutex<Option<Info>> = Mutex::new(None);

fn change(f: impl FnOnce(&mut Settings)) {
    if let Ok(mut d) = DESIRED.lock() {
        let mut s = d.1.clone().unwrap_or_default();
        let before = s.clone();
        f(&mut s);
        if s != before {
            d.0 += 1;
            d.1 = Some(s);
        }
    }
}

/// `Display.set_mode(name)`; an unknown name is ignored and logged.
pub fn set_mode(name: &str) {
    match WindowMode::parse(name) {
        Some(m) => change(|s| s.mode = m),
        None => log::warn!("[display] unknown window mode '{name}' (windowed, borderless, fullscreen)"),
    }
}

/// `Display.set_size(w, h)`: the windowed size, or the fullscreen resolution.
pub fn set_size(w: u32, h: u32) {
    if w == 0 || h == 0 {
        change(|s| s.size = None);
    } else {
        change(|s| s.size = Some((w.min(16384), h.min(16384))));
    }
}

/// `Display.set_vsync(on)`.
pub fn set_vsync(on: bool) {
    change(|s| s.vsync = on);
}

/// `Display.set_max_fps(n)`: 0 is no cap; a negative number is the
/// project's own cap again.
pub fn set_max_fps(n: i64) {
    change(|s| s.max_fps = (n >= 0).then(|| n.min(1000) as u32));
}

/// `Cursor.set_visible(on)`.
pub fn set_cursor_visible(on: bool) {
    change(|s| s.cursor_visible = on);
}

/// `Cursor.set_confined(on)`.
pub fn set_cursor_confined(on: bool) {
    change(|s| s.cursor_confined = on);
}

/// `Cursor.set_locked(on)`: the pointer hidden and held, its motion read
/// with `Input.get_mouse_delta`. A browser grants it only after a recent
/// click and lets it go on Esc; either way the wanted lock is set back off,
/// so the next ask is a change again.
pub fn set_cursor_locked(on: bool) {
    change(|s| s.cursor_locked = on);
}

/// `Cursor.set_icon(name)`: one of [`CURSOR_ICONS`] (`-` or a space for
/// `_` too: `not-allowed`).
pub fn set_cursor_icon(name: &str) {
    let want = name.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    match CURSOR_ICONS.iter().find(|i| **i == want) {
        Some(icon) => change(|s| s.cursor = CursorShape::Icon(icon)),
        None => log::warn!("[display] unknown cursor '{name}' (one of {CURSOR_ICONS:?})"),
    }
}

/// `Cursor.set_image(texture_id, hot_x, hot_y)`. An empty id is the
/// default system cursor again.
pub fn set_cursor_image(texture_id: &str, hot_x: f32, hot_y: f32) {
    if texture_id.is_empty() {
        change(|s| s.cursor = CursorShape::default());
        return;
    }
    let px = |v: f32| if v.is_finite() { v.clamp(0.0, 1024.0) as u16 } else { 0 };
    change(|s| s.cursor = CursorShape::Image { texture_id: texture_id.to_string(), hot_x: px(hot_x), hot_y: px(hot_y) });
}

/// Everything back as a game starts: windowed, vsynced, the system cursor.
/// The editor calls it when Play stops, so a game that went fullscreen or
/// hid the cursor gives the editor back.
pub fn reset() {
    change(|s| *s = Settings::default());
}

/// What the scripts want, and its generation (it moves on every change).
pub fn desired() -> (u64, Settings) {
    DESIRED.lock().map(|d| (d.0, d.1.clone().unwrap_or_default())).unwrap_or_default()
}

/// The host says what it has. Called once a frame.
pub fn publish(info: Info) {
    if let Ok(mut i) = INFO.lock() {
        *i = Some(info);
    }
}

/// What the host last said it has - or, before any host has spoken (a test,
/// the first frame), what was asked for.
pub fn info() -> Info {
    if let Some(i) = INFO.lock().ok().and_then(|i| i.clone()) {
        return i;
    }
    let (_, s) = desired();
    Info { mode: s.mode, vsync: s.vsync, max_fps: s.max_fps.unwrap_or(0), ..Default::default() }
}

/// A host's record of what it has applied.
#[derive(Debug, Clone, Default)]
pub struct Applied {
    generation: u64,
    settings: Settings,
}

impl Applied {
    /// The settings to apply and what changed in them since the last
    /// [`Self::done`], or `None` when nothing did.
    pub fn changes(&self) -> Option<(Settings, Changes)> {
        let (generation, want) = desired();
        if generation == self.generation {
            return None;
        }
        let changes = Changes::between(&self.settings, &want);
        Some((want, changes))
    }

    /// The host applied `settings`. With `cursor_pending` (a cursor picture
    /// whose texture has not loaded yet) the cursor is asked for again next
    /// frame and everything else is kept.
    pub fn done(&mut self, settings: Settings, cursor_pending: bool) {
        if cursor_pending {
            let cursor = (
                self.settings.cursor_visible,
                self.settings.cursor_confined,
                self.settings.cursor_locked,
                self.settings.cursor.clone(),
            );
            self.settings = settings;
            (
                self.settings.cursor_visible,
                self.settings.cursor_confined,
                self.settings.cursor_locked,
                self.settings.cursor,
            ) = cursor;
        } else {
            self.generation = desired().0;
            self.settings = settings;
        }
    }
}

/// The video mode nearest `want` among `modes` (`(w, h, hz)`): the exact
/// size at its highest rate, else the closest size by area and shape.
pub fn closest_mode(modes: &[(u32, u32, u32)], want: (u32, u32)) -> Option<usize> {
    let score = |&(w, h, hz): &(u32, u32, u32)| {
        let dw = (w as f64 - want.0 as f64).abs();
        let dh = (h as f64 - want.1 as f64).abs();
        (((dw + dh) * 1000.0) as u64, u64::MAX - hz as u64)
    };
    (0..modes.len()).min_by_key(|&i| score(&modes[i]))
}

/// One entry per size, at its highest refresh rate, largest first - what a
/// resolution list shows. `raw` is `(w, h, millihertz)`.
pub fn distinct_modes(raw: impl IntoIterator<Item = (u32, u32, u32)>) -> Vec<(u32, u32, u32)> {
    let mut best: std::collections::BTreeMap<(u32, u32), u32> = Default::default();
    for (w, h, mhz) in raw {
        let hz = (mhz + 500) / 1000;
        let e = best.entry((w, h)).or_insert(0);
        *e = (*e).max(hz);
    }
    let mut out: Vec<(u32, u32, u32)> = best.into_iter().map(|((w, h), hz)| (w, h, hz)).collect();
    out.sort_by(|a, b| (b.0 * b.1, b.0).cmp(&(a.0 * a.1, a.0)));
    out
}

/// The browsers' host (`web::sync`), shared by the editor viewport and web
/// builds.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod web;

#[cfg(test)]
mod tests {
    use super::*;

    /// The settings are process-wide; tests that touch them take turns.
    static TURN: Mutex<()> = Mutex::new(());

    #[test]
    fn a_host_applies_only_what_changed_and_only_once() {
        let _t = TURN.lock().unwrap();
        reset();
        let mut host = Applied::default();
        if let Some((s, _)) = host.changes() {
            host.done(s, false);
        }
        assert!(host.changes().is_none(), "nothing new");
        set_mode("Borderless");
        set_vsync(false);
        let (s, c) = host.changes().expect("a change");
        assert_eq!(s.mode, WindowMode::Borderless);
        assert_eq!(c, Changes { window: true, vsync: true, max_fps: false, cursor: false });
        host.done(s, false);
        assert!(host.changes().is_none());
        // Asking for what is already so changes nothing.
        set_vsync(false);
        assert!(host.changes().is_none());
        reset();
    }

    #[test]
    fn a_cursor_waiting_for_its_picture_is_asked_for_again() {
        let _t = TURN.lock().unwrap();
        reset();
        let mut host = Applied::default();
        if let Some((s, _)) = host.changes() {
            host.done(s, false);
        }
        set_cursor_image("sprites/hand.png", 3.0, 2.0);
        set_size(1280, 720);
        let (s, c) = host.changes().unwrap();
        assert!(c.cursor && c.window);
        host.done(s, true);
        let (s, c) = host.changes().expect("the cursor again");
        assert_eq!(c, Changes { cursor: true, ..Default::default() }, "the size was applied");
        assert_eq!(s.cursor, CursorShape::Image { texture_id: "sprites/hand.png".into(), hot_x: 3, hot_y: 2 });
        host.done(s, false);
        assert!(host.changes().is_none());
        reset();
    }

    #[test]
    fn names_are_forgiving_and_unknown_ones_change_nothing() {
        let _t = TURN.lock().unwrap();
        reset();
        set_mode("exclusive");
        assert_eq!(desired().1.mode, WindowMode::Fullscreen);
        set_mode("sideways");
        assert_eq!(desired().1.mode, WindowMode::Fullscreen);
        set_cursor_icon("Not-Allowed");
        assert_eq!(desired().1.cursor, CursorShape::Icon("not_allowed"));
        set_cursor_icon("banana");
        assert_eq!(desired().1.cursor, CursorShape::Icon("not_allowed"));
        set_max_fps(-1);
        assert_eq!(desired().1.max_fps, None, "the project's cap");
        set_max_fps(0);
        assert_eq!(desired().1.max_fps, Some(0), "none");
        reset();
    }

    #[test]
    fn a_resolution_list_has_each_size_once_at_its_best_rate() {
        let modes = distinct_modes([(1920, 1080, 59_940), (1920, 1080, 144_000), (1280, 720, 60_000), (2560, 1440, 60_000)]);
        assert_eq!(modes, vec![(2560, 1440, 60), (1920, 1080, 144), (1280, 720, 60)]);
        assert_eq!(closest_mode(&modes, (1920, 1080)), Some(1));
        assert_eq!(closest_mode(&modes, (1366, 768)), Some(2), "the nearest");
        assert_eq!(closest_mode(&[], (1, 1)), None);
    }

    #[test]
    fn supports_answers_by_name() {
        let s = Supports { fullscreen: true, vsync: true, ..Default::default() };
        assert!(s.get("fullscreen") && s.get("vsync") && s.get("cursor_icon"));
        assert!(!s.get("exclusive") && !s.get("window_size") && !s.get("nonsense"));
    }
}
