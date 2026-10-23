//! Where a frame's time goes (Oct 2026): named **spans** around the engine's
//! systems and passes, each script's cost, and the draws made inside each
//! span - recorded per frame while something is capturing, and a
//! [`Capture`] of many frames that the editor's Profiler panel reads and
//! the native runtime writes (`JENGINE_PROFILE_CAPTURE`).
//!
//! The spans sit in the functions every host already shares
//! (`behaviors::tick_motion`, `PhysicsWorld::step_once`,
//! `ecs::frame_draw::draw_frame`, ...), so every runtime reports the same
//! breakdown with no host wiring. A span nests in whatever span is open,
//! and a parent's time minus its children's is its own work - for
//! `render.world`, the collecting and sorting of the world's drawables.
//!
//! **Off, it costs one thread-local read a span**: nothing is recorded
//! until [`set_enabled`] (the editor turns it on while its panel records;
//! natively the capture variable does), so a shipped game pays nothing.
//! Draws are counted always ([`draw`], a thread-local increment), since a
//! span needs the count from before it opened.
//!
//! Times are **CPU** milliseconds - for the render spans, the time spent
//! building and encoding the pass, not the GPU's time drawing it.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// One span of one frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpanRecord {
    /// Dotted by convention (`motion.navigation`, `render.lights`).
    pub name: Cow<'static, str>,
    /// How many spans were open around it: 0 for a top-level one.
    pub depth: u8,
    /// When it opened, in milliseconds after the frame began.
    pub start_ms: f32,
    pub ms: f32,
    /// Draw calls made while it was open (children's included).
    #[serde(default)]
    pub draws: u32,
}

/// What one script cost in one frame: every call into it, summed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptCost {
    /// The script's asset path (`scripts/hero.lua`).
    pub path: String,
    pub ms: f32,
    pub calls: u32,
}

/// One recorded frame.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FrameRecord {
    /// Counts frames since recording began.
    pub index: u64,
    /// Wall time from the previous frame's end to this one's.
    pub ms: f32,
    /// In the order they opened; a parent before its children.
    pub spans: Vec<SpanRecord>,
    #[serde(default)]
    pub scripts: Vec<ScriptCost>,
    /// Every draw call this frame.
    #[serde(default)]
    pub draws: u32,
}

impl FrameRecord {
    /// The time the top-level spans account for.
    pub fn traced_ms(&self) -> f32 {
        self.spans.iter().filter(|s| s.depth == 0).map(|s| s.ms).sum()
    }
}

/// Many frames, as saved and loaded (`.jprof`, JSON).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Capture {
    /// Who recorded it: `native`, `editor`, `web`.
    pub runtime: String,
    pub frames: Vec<FrameRecord>,
}

struct Recorder {
    enabled: bool,
    depth: u8,
    frame_start: f64,
    index: u64,
    draws_at_start: u64,
    spans: Vec<SpanRecord>,
    scripts: HashMap<String, (f64, u32)>,
}

thread_local! {
    static RECORDER: RefCell<Recorder> = RefCell::new(Recorder {
        enabled: false,
        depth: 0,
        frame_start: 0.0,
        index: 0,
        draws_at_start: 0,
        spans: Vec::new(),
        scripts: HashMap::new(),
    });
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static DRAWS: Cell<u64> = const { Cell::new(0) };
}

/// Spans this long a frame are kept at most; past it a frame's spans are
/// dropped rather than grown without end (a span in a loop that runs away).
pub const MAX_SPANS_PER_FRAME: usize = 4096;

/// Start or stop recording on this thread. Starting begins a fresh frame.
pub fn set_enabled(on: bool) {
    ENABLED.with(|e| e.set(on));
    RECORDER.with(|r| {
        let mut r = r.borrow_mut();
        if on && !r.enabled {
            r.frame_start = super::now_ms();
            r.draws_at_start = DRAWS.with(Cell::get);
            r.index = 0;
            r.depth = 0;
            r.spans.clear();
            r.scripts.clear();
        }
        r.enabled = on;
    });
}

/// Whether this thread is recording.
pub fn enabled() -> bool {
    ENABLED.with(Cell::get)
}

/// Count one draw call (the sprite batches call this at every flush).
#[inline]
pub fn draw() {
    DRAWS.with(|d| d.set(d.get() + 1));
}

/// Draw calls counted on this thread so far.
pub fn draws() -> u64 {
    DRAWS.with(Cell::get)
}

/// An open span; it closes when dropped.
#[must_use = "a span closes when dropped - bind it: `let _span = span(..)`"]
pub struct Span {
    index: usize,
    start: f64,
    draws: u64,
}

/// Open a span named `name` until the returned guard drops.
#[inline]
pub fn span(name: &'static str) -> Span {
    if !enabled() {
        return Span { index: usize::MAX, start: 0.0, draws: 0 };
    }
    open(Cow::Borrowed(name))
}

/// [`span`] with a name made at run time.
pub fn span_owned(name: String) -> Span {
    if !enabled() {
        return Span { index: usize::MAX, start: 0.0, draws: 0 };
    }
    open(Cow::Owned(name))
}

fn open(name: Cow<'static, str>) -> Span {
    let now = super::now_ms();
    let draws = draws();
    RECORDER.with(|r| {
        let mut r = r.borrow_mut();
        if r.spans.len() >= MAX_SPANS_PER_FRAME {
            return Span { index: usize::MAX, start: 0.0, draws: 0 };
        }
        let index = r.spans.len();
        let depth = r.depth;
        let start_ms = (now - r.frame_start) as f32;
        r.spans.push(SpanRecord { name, depth, start_ms, ms: 0.0, draws: 0 });
        r.depth = r.depth.saturating_add(1);
        Span { index, start: now, draws }
    })
}

impl Drop for Span {
    fn drop(&mut self) {
        if self.index == usize::MAX {
            return;
        }
        let now = super::now_ms();
        let draws = draws();
        RECORDER.with(|r| {
            let mut r = r.borrow_mut();
            r.depth = r.depth.saturating_sub(1);
            if let Some(s) = r.spans.get_mut(self.index) {
                s.ms = (now - self.start) as f32;
                s.draws = (draws - self.draws) as u32;
            }
        });
    }
}

/// Run `f` inside a span named `name`.
#[inline]
pub fn time<R>(name: &'static str, f: impl FnOnce() -> R) -> R {
    let _span = span(name);
    f()
}

/// Add `ms` to the script at `path`'s cost this frame (one call).
pub fn script(path: &str, ms: f64) {
    if !enabled() {
        return;
    }
    RECORDER.with(|r| {
        let mut r = r.borrow_mut();
        let entry = r.scripts.entry(path.to_string()).or_insert((0.0, 0));
        entry.0 += ms;
        entry.1 += 1;
    });
}

/// Close this thread's frame: everything recorded since the last call, or
/// `None` when not recording. Hosts call it once a frame, after drawing.
pub fn take_frame() -> Option<FrameRecord> {
    if !enabled() {
        return None;
    }
    let now = super::now_ms();
    let draws = draws();
    RECORDER.with(|r| {
        let mut r = r.borrow_mut();
        let mut scripts: Vec<ScriptCost> = r
            .scripts
            .drain()
            .map(|(path, (ms, calls))| ScriptCost { path, ms: ms as f32, calls })
            .collect();
        scripts.sort_by(|a, b| b.ms.total_cmp(&a.ms).then_with(|| a.path.cmp(&b.path)));
        let record = FrameRecord {
            index: r.index,
            ms: (now - r.frame_start) as f32,
            spans: std::mem::take(&mut r.spans),
            scripts,
            draws: (draws - r.draws_at_start) as u32,
        };
        r.index += 1;
        r.frame_start = now;
        r.draws_at_start = draws;
        r.depth = 0;
        Some(record)
    })
}

/// A capture written to a file as frames arrive - the native runtime's,
/// under `JENGINE_PROFILE_CAPTURE=<path>`. It keeps the last `keep` frames
/// (`JENGINE_PROFILE_FRAMES`, 600 by default) and rewrites the file each
/// time that many more have come, and when dropped - so a run that ends
/// without a clean exit still leaves a capture behind.
#[cfg(not(target_arch = "wasm32"))]
pub struct CaptureWriter {
    path: std::path::PathBuf,
    keep: usize,
    since_write: usize,
    capture: Capture,
}

#[cfg(not(target_arch = "wasm32"))]
impl CaptureWriter {
    pub fn new(path: impl Into<std::path::PathBuf>, runtime: &str, keep: usize) -> Self {
        set_enabled(true);
        Self {
            path: path.into(),
            keep: keep.max(1),
            since_write: 0,
            capture: Capture { runtime: runtime.into(), frames: Vec::new() },
        }
    }

    /// From `JENGINE_PROFILE_CAPTURE` / `JENGINE_PROFILE_FRAMES`, or `None`
    /// when the first is unset. Turns recording on.
    pub fn from_env(runtime: &str) -> Option<Self> {
        let path = std::env::var_os("JENGINE_PROFILE_CAPTURE")?;
        let keep = std::env::var("JENGINE_PROFILE_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(600);
        log::info!("[profile] recording frames to {}", std::path::Path::new(&path).display());
        Some(Self::new(path, runtime, keep))
    }

    /// Take this frame's record (`take_frame`) and keep it.
    pub fn frame(&mut self) {
        let Some(f) = take_frame() else { return };
        if self.capture.frames.len() >= self.keep {
            self.capture.frames.remove(0);
        }
        self.capture.frames.push(f);
        self.since_write += 1;
        if self.since_write >= self.keep {
            self.write();
        }
    }

    pub fn capture(&self) -> &Capture {
        &self.capture
    }

    pub fn write(&mut self) {
        self.since_write = 0;
        match serde_json::to_string(&self.capture) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&self.path, text) {
                    log::warn!("[profile] could not write {}: {e}", self.path.display());
                }
            }
            Err(e) => log::warn!("[profile] could not encode the capture: {e}"),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for CaptureWriter {
    fn drop(&mut self) {
        if !self.capture.frames.is_empty() {
            self.write();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spin(ms: f64) {
        let t = super::super::now_ms();
        while super::super::now_ms() - t < ms {}
    }

    #[test]
    fn records_nothing_until_enabled() {
        set_enabled(false);
        let _s = span("off");
        script("scripts/a.lua", 1.0);
        assert!(take_frame().is_none());
    }

    #[test]
    fn spans_nest_time_and_count_draws() {
        set_enabled(true);
        {
            let _outer = span("render");
            draw();
            {
                let _inner = span("render.sprites");
                spin(2.0);
                draw();
                draw();
            }
            time("render.ui", || draw());
        }
        script("scripts/hero.lua", 0.5);
        script("scripts/hero.lua", 0.25);
        script("scripts/cam.lua", 1.0);
        let f = take_frame().unwrap();
        set_enabled(false);
        let names: Vec<(&str, u8)> = f.spans.iter().map(|s| (s.name.as_ref(), s.depth)).collect();
        assert_eq!(names, [("render", 0), ("render.sprites", 1), ("render.ui", 1)]);
        assert_eq!(f.spans.iter().map(|s| s.draws).collect::<Vec<_>>(), [4, 2, 1]);
        assert!(f.spans[1].ms >= 2.0 && f.spans[0].ms >= f.spans[1].ms);
        assert!(f.spans[1].start_ms >= f.spans[0].start_ms);
        assert_eq!(f.draws, 4);
        assert_eq!(f.scripts[0], ScriptCost { path: "scripts/cam.lua".into(), ms: 1.0, calls: 1 });
        assert_eq!(f.scripts[1].calls, 2);
        assert!((f.traced_ms() - f.spans[0].ms).abs() < 1e-6);
    }

    #[test]
    fn frames_follow_one_another() {
        set_enabled(true);
        let _ = take_frame();
        let a = take_frame().unwrap();
        let _s = span("x");
        drop(_s);
        let b = take_frame().unwrap();
        set_enabled(false);
        assert_eq!(b.index, a.index + 1);
        assert!(a.spans.is_empty() && b.spans.len() == 1);
    }

    #[test]
    fn a_capture_file_keeps_the_last_frames() {
        let path = std::env::temp_dir().join(format!("jengine-trace-{}.jprof", std::process::id()));
        {
            let mut w = CaptureWriter::new(&path, "native", 3);
            for _ in 0..5 {
                let _s = span("tick");
                drop(_s);
                w.frame();
            }
            assert_eq!(w.capture().frames.iter().map(|f| f.index).collect::<Vec<_>>(), [2, 3, 4]);
        }
        set_enabled(false);
        let back: Capture = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.runtime, "native");
        assert_eq!(back.frames.len(), 3);
        assert_eq!(back.frames[0].spans[0].name, "tick");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_capture_round_trips() {
        let c = Capture {
            runtime: "native".into(),
            frames: vec![FrameRecord {
                index: 3,
                ms: 6.1,
                spans: vec![SpanRecord { name: "motion".into(), depth: 0, start_ms: 0.1, ms: 0.4, draws: 0 }],
                scripts: vec![ScriptCost { path: "scripts/a.lua".into(), ms: 0.2, calls: 1 }],
                draws: 12,
            }],
        };
        let back: Capture = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }
}
