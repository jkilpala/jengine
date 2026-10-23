//! Frame pacing — turning a jittery wall-clock delta into one the
//! simulation can integrate without visible judder.
//!
//! A runtime measures `dt` as the wall time between two frame starts. That
//! number is correct on average and wrong every single frame: the gap
//! absorbs swapchain acquire latency, DWM composition scheduling and
//! whatever else the OS did between the two reads, so a display running a
//! rock-steady 165 Hz produces deltas like 5.8 / 6.3 / 5.9 / 6.4 ms rather
//! than a flat 6.06. Integrating that directly moves things by a different
//! amount each frame while the display samples them at a fixed cadence,
//! which reads as stutter even though the frame *rate* is perfect. It is
//! the usual explanation for "the FPS counter looks fine but it doesn't
//! feel smooth".
//!
//! The fix is to snap a delta that is *nearly* a whole number of refresh
//! periods to exactly that many, which is what [`FramePacer`] does. Frames
//! that are nowhere near a multiple — a hitch, a load spike, a throttled
//! background window — are left alone, because inventing a clean delta for
//! those would hide real cost and desync the game clock from the wall.
//!
//! Snapping biases the clock slightly (every frame is rounded the same
//! direction for as long as the error has the same sign), so the rounding
//! residual is accumulated and paid back a whole period at a time. Over any
//! real span the simulated time therefore tracks wall-clock time, which
//! matters because games here compare accumulated `dt` against
//! `get_unix_time()`.
//!
//! **Frames can also arrive in bursts** (Sep 2026). On a vsynced swapchain
//! the CPU may run several frames back to back and then wait out the queue:
//! at 165 Hz a native window on a hybrid-GPU laptop measured 18 / 2 / 2 / 2
//! ms, over and over - the right average, and every frame still shown one
//! refresh after the last. Snapped one at a time, that was three periods and
//! then three deltas too short to snap, which went through raw: the
//! simulation jumped three frames' worth and crawled for three, while the
//! display showed one frame per refresh. A trail recorded its points in
//! bunches, which is how it was found. So the pacer also watches the
//! **average** of the last few deltas: when that sits on a whole number of
//! periods, the display is keeping a cadence whatever the CPU is doing, and
//! every frame gets exactly that many periods, with the difference banked -
//! a burst may run [`BURST_ALLOWANCE`] periods ahead or behind before a
//! period is paid back. A delta past [`HITCH_PERIODS`] is a stall, not a
//! burst, and ends the cadence.

/// How far from an exact multiple of the refresh period a delta may sit and
/// still be snapped to it, as a fraction of one period. Jitter from
/// composition scheduling lives well inside this; a genuine hitch does not.
const SNAP_TOLERANCE: f32 = 0.10;

/// Largest number of refresh periods a delta may be snapped to. Beyond this
/// the frame was a stall rather than a slow frame, and reporting it as a
/// tidy multiple would be a lie.
const MAX_SNAP_PERIODS: f32 = 6.0;

/// How many recent deltas the cadence is judged over: two bursts of the
/// longest seen (four frames), so a burst's average is whole.
const CADENCE_WINDOW: usize = 8;

/// How close the window's average must sit to a whole number of periods to
/// count as a cadence, as a fraction of one. Tighter than a single frame's
/// tolerance, because an average of eight has little jitter left - and a
/// display whose real rate differs from the reported one by more than this
/// is better served by raw deltas than by a clock that drifts.
const CADENCE_TOLERANCE: f32 = 0.04;

/// Periods of banked difference a cadence tolerates before paying one back.
/// A burst of four runs up to three ahead of the display and back.
const BURST_ALLOWANCE: f32 = 3.0;

/// A single delta longer than this many periods is a stall, even inside a
/// cadence: the window forgets it, rather than averaging a hitch into
/// several frames of too-long deltas.
const HITCH_PERIODS: f32 = 4.5;

/// Refresh rates outside this range are rejected as nonsense rather than
/// used to build a snap grid. Some drivers report 0, and a monitor handle
/// can go stale across a hotplug.
const MIN_REFRESH_HZ: f32 = 20.0;
const MAX_REFRESH_HZ: f32 = 1000.0;

/// Smooths per-frame deltas against the refresh rate of the display the
/// window is actually on.
///
/// With no refresh rate set (`None`, the default) every delta passes
/// through untouched, so a runtime that cannot ask its windowing system for
/// a rate behaves exactly as it did before.
#[derive(Debug, Clone, Default)]
pub struct FramePacer {
    /// Seconds per refresh period, or `None` when unknown.
    period: Option<f32>,
    /// Signed rounding debt owed back to the wall clock, in seconds.
    drift: f32,
    /// The last [`CADENCE_WINDOW`] raw deltas, a ring.
    recent: [f32; CADENCE_WINDOW],
    /// How many of `recent` are filled, and where the next goes.
    recent_len: usize,
    recent_next: usize,
    /// Whether the last frame was paced as part of a cadence - its `drift`
    /// is then a burst's, which means nothing to single-frame snapping.
    in_cadence: bool,
}

impl FramePacer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tell the pacer the refresh rate of the display the window is on, in
    /// millihertz — the unit winit's `MonitorHandle::refresh_rate_millihertz`
    /// reports. `None`, zero or an implausible rate disables snapping.
    ///
    /// Call this whenever the window may have changed display: a laptop
    /// panel and an external monitor rarely share a refresh rate, and
    /// snapping to the wrong grid is worse than not snapping at all.
    pub fn set_refresh_millihertz(&mut self, millihertz: Option<u32>) {
        let hz = millihertz.map(|mhz| mhz as f32 / 1000.0);
        let next = match hz {
            Some(hz) if (MIN_REFRESH_HZ..=MAX_REFRESH_HZ).contains(&hz) => Some(1.0 / hz),
            _ => None,
        };
        if next != self.period {
            self.period = next;
            // The old grid's debt means nothing on a new one.
            self.drift = 0.0;
            self.forget();
        }
    }

    /// Seconds per refresh period, if known.
    pub fn period(&self) -> Option<f32> {
        self.period
    }

    /// Forget the recent deltas: a new grid, or a stall.
    fn forget(&mut self) {
        self.recent_len = 0;
        self.recent_next = 0;
        self.in_cadence = false;
    }

    /// Snap one raw wall-clock delta onto the refresh grid.
    ///
    /// Returns `raw` unchanged when the rate is unknown or the delta is not
    /// close to a whole number of periods - unless the recent deltas together
    /// keep a cadence (see the module docs), when every frame gets it.
    pub fn snap(&mut self, raw: f32) -> f32 {
        let Some(period) = self.period else {
            return raw;
        };
        if !raw.is_finite() || raw <= 0.0 {
            return raw;
        }

        if raw > period * HITCH_PERIODS {
            self.forget();
            self.drift = 0.0;
        } else {
            self.recent[self.recent_next] = raw;
            self.recent_next = (self.recent_next + 1) % CADENCE_WINDOW;
            self.recent_len = (self.recent_len + 1).min(CADENCE_WINDOW);
            if let Some(out) = self.cadence(raw, period) {
                return out;
            }
        }
        if self.in_cadence {
            self.in_cadence = false;
            self.drift = 0.0;
        }
        self.snap_one(raw, period)
    }

    /// The delta for a frame of a cadence, or `None` if the recent deltas do
    /// not keep one.
    fn cadence(&mut self, raw: f32, period: f32) -> Option<f32> {
        // Half a window before judging: a burst of four has to be seen whole.
        if self.recent_len < CADENCE_WINDOW / 2 {
            return None;
        }
        let avg = self.recent[..self.recent_len].iter().sum::<f32>() / self.recent_len as f32;
        let periods = (avg / period).round();
        if !(1.0..=MAX_SNAP_PERIODS).contains(&periods) || (avg - periods * period).abs() > period * CADENCE_TOLERANCE {
            return None;
        }
        if !self.in_cadence {
            self.in_cadence = true;
            self.drift = 0.0;
        }
        let allowance = period * BURST_ALLOWANCE;
        let mut out = periods * period;
        self.drift += raw - out;
        if self.drift >= allowance {
            // Fallen behind the wall - a dropped frame, or a reported rate a
            // hair fast: pay a period back.
            self.drift -= period;
            out += period;
        } else if self.drift <= -allowance {
            // Run ahead of the wall - a display a hair faster than it says.
            let back = if out > period { period } else { period * 0.5 };
            self.drift += back;
            out -= back;
        }
        Some(out)
    }

    /// One delta snapped on its own - the pacer before cadences.
    fn snap_one(&mut self, raw: f32, period: f32) -> f32 {
        let periods = (raw / period).round();
        if !(1.0..=MAX_SNAP_PERIODS).contains(&periods) {
            // A stall, or a frame faster than the display can show. Let it
            // through and drop the debt: it is about to be meaningless.
            self.drift = 0.0;
            return raw;
        }

        let snapped = periods * period;
        if (raw - snapped).abs() > period * SNAP_TOLERANCE {
            self.drift = 0.0;
            return raw;
        }

        // Bank what the rounding added or removed, and settle up once it
        // reaches a whole period so the correction is itself on the grid.
        self.drift += raw - snapped;
        if self.drift >= period {
            self.drift -= period;
            return snapped + period;
        }
        if self.drift <= -period {
            // Never hand back a non-positive delta: with only one period to
            // give, give half. (This used to give nothing, so a display a
            // little faster than it reported ran the clock ahead of the wall
            // for as long as it ran.)
            let back = if snapped > period { period } else { period * 0.5 };
            self.drift += back;
            return snapped - back;
        }
        snapped
    }
}

/// `settings.frame_rate_cap` in `project.jengine`: the most frames a second a
/// build may draw, per platform. `0` is "as fast as the display refreshes".
///
/// Per platform because the reasons differ: a phone at 120 Hz spends battery
/// and heat on frames a simple game does not need, so Android defaults to 60;
/// a desktop or a browser has no such budget and defaults to the display.
/// (It replaced `settings.target_fps`, a field Project Settings offered for
/// years and no runtime read - every project had it at 60, so switching that
/// on would have capped every high-refresh monitor without anyone asking.)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FrameRateCaps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub android: Option<u32>,
}

/// Android's cap when a project sets none.
pub const DEFAULT_ANDROID_FPS: u32 = 60;

/// Which of [`FrameRateCaps`] a runtime reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapPlatform {
    Desktop,
    Web,
    Android,
}

impl CapPlatform {
    /// The platform this native build runs on.
    pub fn native() -> Self {
        if cfg!(target_os = "android") { Self::Android } else { Self::Desktop }
    }
}

impl FrameRateCaps {
    /// Read `settings.frame_rate_cap` out of a project document. A missing or
    /// malformed block is the defaults, never an error.
    pub fn from_project(project: &serde_json::Value) -> Self {
        project
            .get("settings")
            .and_then(|s| s.get("frame_rate_cap"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    /// The cap for `platform` in frames a second; `0` is none.
    pub fn for_platform(&self, platform: CapPlatform) -> u32 {
        match platform {
            CapPlatform::Desktop => self.desktop.unwrap_or(0),
            CapPlatform::Web => self.web.unwrap_or(0),
            CapPlatform::Android => self.android.unwrap_or(DEFAULT_ANDROID_FPS),
        }
    }
}

/// Holds a runtime to at most `fps` frames a second.
///
/// A frame may start once most of its interval has passed since the last one
/// started - not all of it: on a vsynced swapchain a frame can only begin on
/// a refresh, so waiting for the exact interval would miss the refresh it
/// meant and land on the next, halving the rate. With a tenth of slack a
/// 120 Hz phone capped at 60 draws every second refresh, and a 165 Hz monitor
/// every third (55 fps): evenly spaced, and never over the cap. Uneven
/// spacing is what a cap must not cause - the frame pacing above exists
/// because it reads as judder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameCap {
    /// Seconds between frames, or `None` for no cap.
    interval: Option<f64>,
    /// The share of the interval a frame waits: 0.9 on a vsynced swapchain
    /// (see above), all of it without one ([`Self::exact`]).
    slack: f64,
}

impl FrameCap {
    /// `fps == 0` is no cap.
    pub fn new(fps: u32) -> Self {
        Self { interval: (fps > 0).then(|| 1.0 / fps as f64), slack: 0.9 }
    }

    /// A cap for a swapchain that does not wait for the refresh (vsync off,
    /// `Display.set_vsync(false)`): there is no refresh to land on, so the
    /// whole interval is waited - with the slack, a cap of 30 drew 33.
    pub fn exact(fps: u32) -> Self {
        Self { slack: 1.0, ..Self::new(fps) }
    }

    pub fn is_capped(&self) -> bool {
        self.interval.is_some()
    }

    /// The earliest a frame may start, in seconds after the last one did.
    pub fn min_gap(&self) -> f64 {
        self.interval.map_or(0.0, |i| i * self.slack)
    }

    /// Whether a frame may start `since_last` seconds after the previous one.
    pub fn ready(&self, since_last: f64) -> bool {
        since_last >= self.min_gap()
    }
}

/// Refresh rates a display plausibly runs at, which [`RefreshEstimate`]
/// rounds a measured rate to.
const COMMON_REFRESH_HZ: [f32; 13] = [48.0, 50.0, 60.0, 72.0, 75.0, 90.0, 100.0, 120.0, 144.0, 165.0, 180.0, 240.0, 360.0];

/// How many recent deltas [`RefreshEstimate`] judges.
const ESTIMATE_WINDOW: usize = 32;

/// How close a measured rate must be to a common one, as a fraction.
const ESTIMATE_TOLERANCE: f32 = 0.04;

/// The display's refresh rate, worked out from the frames themselves.
///
/// A browser does not say how fast its display refreshes, so a browser host
/// cannot give [`FramePacer`] its grid - and its deltas need one more than
/// native's do: `Date.now()` counts whole milliseconds and WebKit rounds
/// `performance.now()` and the `requestAnimationFrame` timestamp to one as
/// well, so 60 Hz arrives as 16 / 17 / 17 / 16 ms and motion steps ±3% a
/// frame. `requestAnimationFrame` runs once a refresh while the page keeps up,
/// so the mean of recent deltas near their median is the period; rounded to
/// a [`COMMON_REFRESH_HZ`] rate it is exact. A page that does not keep up
/// measures no common rate and gets `None`, which leaves deltas raw.
#[derive(Debug, Clone, Default)]
pub struct RefreshEstimate {
    recent: Vec<f32>,
    next: usize,
}

impl RefreshEstimate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take one raw delta in seconds and answer the rate in millihertz, the
    /// unit [`FramePacer::set_refresh_millihertz`] takes - `None` until the
    /// window is full or while the frames keep no common rate.
    pub fn observe(&mut self, raw: f32) -> Option<u32> {
        if raw.is_finite() && raw > 0.0 {
            if self.recent.len() < ESTIMATE_WINDOW {
                self.recent.push(raw);
            } else {
                self.recent[self.next] = raw;
            }
            self.next = (self.next + 1) % ESTIMATE_WINDOW;
        }
        self.rate_millihertz()
    }

    fn rate_millihertz(&self) -> Option<u32> {
        if self.recent.len() < ESTIMATE_WINDOW {
            return None;
        }
        let mut sorted = self.recent.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let median = sorted[sorted.len() / 2];
        // A missed refresh or a hitch is not a period; leave it out.
        let near: Vec<f32> = sorted.into_iter().filter(|d| (d - median).abs() <= median * 0.5).collect();
        let period = near.iter().sum::<f32>() / near.len() as f32;
        let hz = 1.0 / period;
        COMMON_REFRESH_HZ
            .iter()
            .find(|common| ((hz - *common) / *common).abs() <= ESTIMATE_TOLERANCE)
            .map(|common| (common * 1000.0).round() as u32)
    }
}

#[cfg(test)]
mod tests {

    /// Frames drawn out of `refreshes` vblanks at `hz`, capped at `fps`.
    fn drawn(hz: f64, fps: u32, refreshes: usize) -> Vec<usize> {
        let cap = FrameCap::new(fps);
        let period = 1.0 / hz;
        let mut last = f64::NEG_INFINITY;
        let mut out = Vec::new();
        for n in 0..refreshes {
            let t = n as f64 * period;
            if cap.ready(t - last) {
                out.push(n);
                last = t;
            }
        }
        out
    }

    #[test]
    fn a_cap_draws_on_evenly_spaced_refreshes() {
        // 120 Hz phone at 60: every second refresh.
        let f = drawn(120.0, 60, 12);
        assert_eq!(f, vec![0, 2, 4, 6, 8, 10]);
        // 165 Hz monitor at 60: every third - 55 fps, even, never over.
        let f = drawn(165.0, 60, 12);
        assert_eq!(f, vec![0, 3, 6, 9]);
        // 60 Hz at 60: every refresh; the slack keeps timer jitter from
        // skipping one.
        assert_eq!(drawn(60.0, 60, 5), vec![0, 1, 2, 3, 4]);
        // 90 Hz at 30.
        assert_eq!(drawn(90.0, 30, 10), vec![0, 3, 6, 9]);
    }

    #[test]
    fn without_vsync_the_cap_waits_its_whole_interval() {
        // A loop free to start a frame at any time, every millisecond.
        let frames = |cap: FrameCap| {
            let mut last = f64::NEG_INFINITY;
            let mut n = 0;
            for ms in 0..1000 {
                let t = ms as f64 / 1000.0;
                if cap.ready(t - last) {
                    last = t;
                    n += 1;
                }
            }
            n
        };
        assert_eq!(frames(FrameCap::exact(30)), 30);
        assert!(frames(FrameCap::new(30)) > 30, "the vsync slack would overshoot");
    }

    #[test]
    fn no_cap_draws_every_refresh() {
        assert!(!FrameCap::new(0).is_capped());
        assert_eq!(drawn(144.0, 0, 5), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn android_caps_at_sixty_unless_told_otherwise() {
        let none = FrameRateCaps::default();
        assert_eq!(none.for_platform(CapPlatform::Android), 60);
        assert_eq!(none.for_platform(CapPlatform::Desktop), 0);
        assert_eq!(none.for_platform(CapPlatform::Web), 0);
        let doc = serde_json::json!({ "settings": { "frame_rate_cap": { "android": 0, "desktop": 144 } } });
        let caps = FrameRateCaps::from_project(&doc);
        assert_eq!(caps.for_platform(CapPlatform::Android), 0, "0 is uncapped, not the default");
        assert_eq!(caps.for_platform(CapPlatform::Desktop), 144);
        // A malformed block is the defaults.
        let bad = serde_json::json!({ "settings": { "frame_rate_cap": "fast" } });
        assert_eq!(FrameRateCaps::from_project(&bad), FrameRateCaps::default());
    }

    use super::*;

    /// 165 Hz, the refresh of the panel this was written against.
    fn pacer_165() -> FramePacer {
        let mut p = FramePacer::new();
        p.set_refresh_millihertz(Some(165_000));
        p
    }

    #[test]
    fn unknown_refresh_passes_everything_through() {
        let mut p = FramePacer::new();
        for raw in [0.001, 0.0163, 0.25] {
            assert_eq!(p.snap(raw), raw);
        }
    }

    #[test]
    fn implausible_rates_are_rejected() {
        let mut p = FramePacer::new();
        p.set_refresh_millihertz(Some(0));
        assert!(p.period().is_none());
        p.set_refresh_millihertz(Some(5_000)); // 5 Hz
        assert!(p.period().is_none());
        p.set_refresh_millihertz(Some(165_000));
        assert!(p.period().is_some());
    }

    #[test]
    fn jitter_around_one_period_snaps_flat() {
        let mut p = pacer_165();
        let period = 1.0 / 165.0;
        // Deltas scattered within a few percent of a period, as a real
        // vsynced loop produces, must all come back identical.
        for raw in [period * 0.97, period * 1.03, period * 0.99, period * 1.02] {
            let out = p.snap(raw);
            assert!(
                (out - period).abs() < 1e-6,
                "expected {period}, got {out} for raw {raw}"
            );
        }
    }

    #[test]
    fn a_dropped_frame_snaps_to_two_periods() {
        let mut p = pacer_165();
        let period = 1.0 / 165.0;
        let out = p.snap(period * 2.02);
        assert!((out - period * 2.0).abs() < 1e-6, "got {out}");
    }

    #[test]
    fn a_real_hitch_is_left_alone() {
        let mut p = pacer_165();
        // 100 ms is ~16.5 periods — far past the snap ceiling.
        assert_eq!(p.snap(0.1), 0.1);
        // And a delta sitting squarely between two periods is not forced
        // onto either.
        let period = 1.0 / 165.0;
        let between = period * 1.5;
        assert_eq!(p.snap(between), between);
    }

    #[test]
    fn snapped_time_tracks_the_wall_clock() {
        let mut p = pacer_165();
        let period = 1.0 / 165.0;
        // A display whose true period is slightly longer than reported:
        // every frame rounds the same direction, which is exactly the case
        // that would drift without the payback.
        let truth = period * 1.04;
        let mut wall = 0.0f64;
        let mut simulated = 0.0f64;
        for _ in 0..10_000 {
            wall += truth as f64;
            simulated += p.snap(truth) as f64;
        }
        let err = (wall - simulated).abs() / wall;
        assert!(err < 0.01, "drifted {:.3}% ({simulated} vs {wall})", err * 100.0);
    }

    #[test]
    fn changing_display_rebuilds_the_grid() {
        let mut p = pacer_165();
        p.set_refresh_millihertz(Some(59_000));
        let period = 1.0 / 59.0;
        let out = p.snap(period * 1.01);
        assert!((out - period).abs() < 1e-6, "got {out}");
    }

    /// The bursts a hybrid-GPU laptop's vsynced window delivers at 165 Hz:
    /// three frames' worth of wait, then three frames at once. Every frame is
    /// still shown one refresh after the last, so every frame gets one
    /// period - where snapping them one at a time gave 3, raw, raw, raw.
    #[test]
    fn bursts_on_a_steady_display_get_one_period_each() {
        let mut p = pacer_165();
        let period = 1.0 / 165.0;
        let burst = [period * 3.0 - 0.0001, 0.002, 0.002, period * 4.0 - (period * 3.0 - 0.0001) - 0.004];
        let mut wall = 0.0f64;
        let mut simulated = 0.0f64;
        for n in 0..400 {
            let raw = burst[n % 4];
            let out = p.snap(raw);
            wall += raw as f64;
            simulated += out as f64;
            if n >= 8 {
                assert!((out - period).abs() < 1e-6, "frame {n}: {raw} came out {out}, not one period");
            }
        }
        assert!((wall - simulated).abs() < (period * BURST_ALLOWANCE) as f64, "{simulated} vs {wall}");
    }

    #[test]
    fn a_stall_ends_the_cadence_and_is_left_alone() {
        let mut p = pacer_165();
        let period = 1.0 / 165.0;
        for _ in 0..16 {
            p.snap(period);
        }
        assert_eq!(p.snap(0.1), 0.1, "a hitch is not snapped into the cadence");
        // After it, a steady display is snapped again, frame by frame.
        let out = p.snap(period * 1.02);
        assert!((out - period).abs() < 1e-6, "got {out}");
    }

    #[test]
    fn a_dropped_frame_in_a_cadence_is_paid_back() {
        let mut p = pacer_165();
        let period = 1.0 / 165.0;
        let mut wall = 0.0f64;
        let mut simulated = 0.0f64;
        for n in 0..200 {
            let raw = if n % 50 == 25 { period * 2.0 } else { period };
            wall += raw as f64;
            simulated += p.snap(raw) as f64;
        }
        assert!((wall - simulated).abs() <= (period * BURST_ALLOWANCE) as f64 + 1e-6, "{simulated} vs {wall}");
    }

    #[test]
    fn a_display_faster_than_reported_still_tracks_the_wall() {
        // 3% fast: every frame rounds up to a period, and with one period to
        // give there used to be nothing to pay back - the clock ran ahead.
        let mut p = pacer_165();
        let period = 1.0 / 165.0;
        let truth = period * 0.97;
        let mut wall = 0.0f64;
        let mut simulated = 0.0f64;
        for _ in 0..10_000 {
            wall += truth as f64;
            simulated += p.snap(truth) as f64;
        }
        assert!((wall - simulated).abs() / wall < 0.01, "{simulated} vs {wall}");
    }

    #[test]
    fn snap_never_returns_a_non_positive_delta() {
        let mut p = pacer_165();
        let period = 1.0 / 165.0;
        for _ in 0..5_000 {
            let out = p.snap(period * 0.96);
            assert!(out > 0.0, "got {out}");
        }
    }

    /// A browser's deltas: the true period, rounded to whole milliseconds as
    /// `Date.now()` and WebKit's timers report it.
    fn rounded_frames(hz: f64, frames: usize) -> Vec<f32> {
        let mut out = Vec::new();
        let mut last = 0.0_f64;
        for i in 1..=frames {
            let t = (i as f64 * 1000.0 / hz).floor();
            out.push(((t - last) / 1000.0) as f32);
            last = t;
        }
        out
    }

    #[test]
    fn a_browser_display_rate_is_worked_out_from_rounded_frames() {
        for hz in [60.0, 120.0, 144.0, 165.0] {
            let mut est = RefreshEstimate::new();
            let mut rate = None;
            for d in rounded_frames(hz, 64) {
                rate = est.observe(d);
            }
            assert_eq!(rate, Some((hz * 1000.0) as u32), "{hz} Hz");
        }
    }

    #[test]
    fn a_page_that_cannot_keep_up_gets_no_rate() {
        let mut est = RefreshEstimate::new();
        let mut rate = None;
        for i in 0..64 {
            // 23 and 31 ms: no common display rate.
            rate = est.observe(if i % 2 == 0 { 0.023 } else { 0.031 });
        }
        assert_eq!(rate, None);
    }

    #[test]
    fn rounded_browser_frames_pace_to_an_even_step() {
        let mut est = RefreshEstimate::new();
        let mut pacer = FramePacer::new();
        let frames = rounded_frames(60.0, 200);
        let mut paced = Vec::new();
        for d in frames {
            pacer.set_refresh_millihertz(est.observe(d));
            paced.push(pacer.snap(d));
        }
        // Once the rate is known, every frame is one period.
        for d in &paced[100..] {
            assert!((d - 1.0 / 60.0).abs() < 1e-5, "{d}");
        }
    }
}

// ---- The world's time scale ------------------------------------------------

/// `1.0` as f32 bits.
const ONE: u32 = 0x3f80_0000;
static WORLD_SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(ONE);
static AUDIO_FOLLOWS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// How fast the world's time runs - `Time.set_scale`, which every host
/// multiplies its frame `dt` by. Set by the hosts where they apply it, so
/// the engine's own readers (`ecs::time_scale`, audio following time) and
/// `Time.get_scale` see the scale the frame ran at.
pub fn world_scale() -> f32 {
    f32::from_bits(WORLD_SCALE.load(std::sync::atomic::Ordering::Relaxed))
}

/// Set [`world_scale`] (never below 0). Hosts call it beside their own
/// `time_scale`.
pub fn set_world_scale(scale: f32) {
    WORLD_SCALE.store(scale.max(0.0).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Whether music and one-shots bend with the world's time
/// (`Audio.follow_time`). Off by default: `Time.set_scale(0)` is how a game
/// pauses, and a pause menu wants its music.
pub fn audio_follows_time() -> bool {
    AUDIO_FOLLOWS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Set [`audio_follows_time`].
pub fn set_audio_follows_time(on: bool) {
    AUDIO_FOLLOWS.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// What audio's playback rate is multiplied by: the world's time scale when
/// audio follows it - never below a twentieth, so a pause slows the music to
/// a growl rather than a stop - else 1.
pub fn audio_time_pitch() -> f32 {
    if audio_follows_time() { world_scale().max(0.05) } else { 1.0 }
}
