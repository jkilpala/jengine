//! Performance metrics collection and reporting.
//!
//! Tracks FPS, frame times, draw calls, and sprite counts. Supports JSON output
//! for CI regression detection.

use std::collections::VecDeque;

/// Rolling window size for FPS smoothing.
const FPS_WINDOW: usize = 60;

/// Performance metrics collector.
pub struct BenchmarkStats {
    /// Rolling window of frame times (seconds).
    frame_times: VecDeque<f32>,
    /// All recorded frame times for final report.
    all_frame_times: Vec<f32>,
    /// Current bunny count.
    pub bunny_count: u32,
    /// Draw calls in the last frame.
    pub draw_calls: u32,
    /// Sprites rendered in the last frame.
    pub sprite_count: u32,
    /// Total frames rendered.
    pub total_frames: u64,
    /// Highest bunny count where FPS stayed >= 60.
    pub max_bunnies_at_60fps: u32,
    /// Highest bunny count where FPS stayed >= 30.
    pub max_bunnies_at_30fps: u32,
    /// Accumulator for per-second log output.
    log_accumulator: f32,
}

impl Default for BenchmarkStats {
    fn default() -> Self {
        Self::new()
    }
}

impl BenchmarkStats {
    pub fn new() -> Self {
        Self {
            frame_times: VecDeque::with_capacity(FPS_WINDOW + 1),
            all_frame_times: Vec::with_capacity(10_000),
            bunny_count: 0,
            draw_calls: 0,
            sprite_count: 0,
            total_frames: 0,
            max_bunnies_at_60fps: 0,
            max_bunnies_at_30fps: 0,
            log_accumulator: 0.0,
        }
    }

    /// Record a frame. `dt` is delta time in seconds.
    pub fn record_frame(&mut self, dt: f32, draw_calls: u32, sprite_count: u32, bunny_count: u32) {
        self.frame_times.push_back(dt);
        if self.frame_times.len() > FPS_WINDOW {
            self.frame_times.pop_front();
        }
        self.all_frame_times.push(dt);
        self.draw_calls = draw_calls;
        self.sprite_count = sprite_count;
        self.bunny_count = bunny_count;
        self.total_frames += 1;

        // Track high-water marks
        let fps = self.fps();
        if fps >= 60.0 && bunny_count > self.max_bunnies_at_60fps {
            self.max_bunnies_at_60fps = bunny_count;
        }
        if fps >= 30.0 && bunny_count > self.max_bunnies_at_30fps {
            self.max_bunnies_at_30fps = bunny_count;
        }
    }

    /// Smoothed FPS over the rolling window.
    pub fn fps(&self) -> f32 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        let avg_dt: f32 = self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32;
        if avg_dt > 0.0 { 1.0 / avg_dt } else { 0.0 }
    }

    /// Current frame time in milliseconds.
    pub fn frame_time_ms(&self) -> f32 {
        self.frame_times.back().copied().unwrap_or(0.0) * 1000.0
    }

    /// Average frame time in milliseconds over all recorded frames.
    pub fn avg_frame_time_ms(&self) -> f32 {
        if self.all_frame_times.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.all_frame_times.iter().sum();
        (sum / self.all_frame_times.len() as f32) * 1000.0
    }

    /// 95th percentile frame time in milliseconds.
    pub fn p95_frame_time_ms(&self) -> f32 {
        if self.all_frame_times.is_empty() {
            return 0.0;
        }
        let mut sorted = self.all_frame_times.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((sorted.len() as f32 * 0.95) as usize).min(sorted.len() - 1);
        sorted[idx] * 1000.0
    }

    /// Minimum FPS observed (from worst frame time).
    pub fn min_fps(&self) -> f32 {
        let max_dt = self.all_frame_times.iter().cloned().fold(0.0f32, f32::max);
        if max_dt > 0.0 { 1.0 / max_dt } else { 0.0 }
    }

    /// Maximum FPS observed (from best frame time).
    pub fn max_fps(&self) -> f32 {
        let min_dt = self
            .all_frame_times
            .iter()
            .cloned()
            .filter(|&dt| dt > 0.0)
            .fold(f32::MAX, f32::min);
        if min_dt < f32::MAX { 1.0 / min_dt } else { 0.0 }
    }

    /// Log stats to console once per second. Returns true if it logged.
    pub fn maybe_log(&mut self, dt: f32) -> bool {
        self.log_accumulator += dt;
        if self.log_accumulator >= 1.0 {
            self.log_accumulator -= 1.0;
            log::info!(
                "[BunnyMark] Bunnies: {} | FPS: {:.1} | Frame: {:.2}ms | Draw calls: {} | Sprites: {}",
                self.bunny_count,
                self.fps(),
                self.frame_time_ms(),
                self.draw_calls,
                self.sprite_count,
            );
            return true;
        }
        false
    }

    /// Generate a JSON report string for CI consumption.
    pub fn json_report(&self) -> String {
        let report = serde_json::json!({
            "engine": "jEngine",
            "benchmark": "bunnymark",
            "timestamp": chrono_timestamp(),
            "results": {
                "final_bunny_count": self.bunny_count,
                "max_bunnies_at_60fps": self.max_bunnies_at_60fps,
                "max_bunnies_at_30fps": self.max_bunnies_at_30fps,
                "avg_fps": self.fps(),
                "min_fps": self.min_fps(),
                "max_fps": self.max_fps(),
                "avg_frame_time_ms": self.avg_frame_time_ms(),
                "p95_frame_time_ms": self.p95_frame_time_ms(),
                "total_draw_calls": self.draw_calls,
                "total_sprites_rendered": self.sprite_count,
                "frames_rendered": self.total_frames,
            }
        });
        serde_json::to_string_pretty(&report).unwrap_or_default()
    }

    /// Check if FPS is below the target threshold (for autoramp exit).
    pub fn is_below_target(&self, target_fps: f32) -> bool {
        // Need at least a full window of samples before deciding
        self.frame_times.len() >= FPS_WINDOW && self.fps() < target_fps
    }
}

/// Simple timestamp without pulling in chrono.
#[cfg(not(target_arch = "wasm32"))]
fn chrono_timestamp() -> String {
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}s", dur.as_secs())
}

#[cfg(target_arch = "wasm32")]
fn chrono_timestamp() -> String {
    let ms = js_sys::Date::now();
    format!("{}s", (ms / 1000.0) as u64)
}

/// HUD text lines for on-screen display.
#[allow(dead_code)]
pub fn hud_text(stats: &BenchmarkStats, mode: &str) -> String {
    format!(
        "jEngine BunnyMark [{}]\nBunnies: {}\nFPS: {:.1}\nFrame: {:.2}ms\nDraw calls: {}\nSprites: {}",
        mode,
        stats.bunny_count,
        stats.fps(),
        stats.frame_time_ms(),
        stats.draw_calls,
        stats.sprite_count,
    )
}
