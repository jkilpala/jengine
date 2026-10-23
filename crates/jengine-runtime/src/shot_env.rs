//! `JENGINE_SHOT`: screenshots for a script or an agent to look at, then quit.
//!
//! ```text
//! JENGINE_SHOT=hub JENGINE_SHOT_AFTER=1.5,3 jengine-runtime --scene assets/scenes/hub.scene
//! ```
//!
//! takes a screenshot 1.5 s and 3 s (wall time) after the first frame,
//! written as `<JENGINE_SAVE_DIR>/screenshots/hub-0.png` and `hub-1.png` (a
//! single time writes `hub.png`), and closes the window once the last is on
//! disk. `JENGINE_SHOT_AFTER` defaults to `2`.
//!
//! It asks the engine's own screenshot path (`renderer::screenshot`, the
//! one `Screenshot.take` uses), so what lands in the file is the presented
//! frame - world, post effects and UI. Capturing the window from outside
//! (`PrintWindow`) answered a white rectangle on some machines; this cannot.
//!
//! `JENGINE_PERSISTENT=mines.seed=4;mines.demo=won` writes those keys into
//! the `Persistent` store before the first script runs (a number when it
//! parses as one, else a string) - how a screenshot reaches a state past the
//! title screen without anyone pressing a key: the game reads its own
//! `<game>.demo` key, as the headless tests' `Play::persist` does.

use jengine_core::renderer::screenshot;

pub(crate) struct ShotEnv {
    name: String,
    times: Vec<f32>,
    next: usize,
    elapsed: f32,
    pending: Vec<u64>,
}

/// Apply `JENGINE_PERSISTENT` (see the module doc). Called once at start-up.
pub(crate) fn seed_persistent_from_env() {
    let Ok(spec) = std::env::var("JENGINE_PERSISTENT") else { return };
    for pair in spec.split(';').filter(|p| !p.trim().is_empty()) {
        let Some((k, v)) = pair.split_once('=') else {
            log::warn!("[persistent] `{pair}` in JENGINE_PERSISTENT has no `=`");
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        match v.parse::<f64>() {
            Ok(n) => jengine_core::scripting::persistent::set_number(k, n),
            Err(_) => jengine_core::scripting::persistent::set_string(k, v),
        }
        log::info!("[persistent] {k} = {v} (JENGINE_PERSISTENT)");
    }
}

impl ShotEnv {
    pub(crate) fn from_env() -> Option<ShotEnv> {
        let name = std::env::var("JENGINE_SHOT").ok().filter(|s| !s.is_empty())?;
        let mut times: Vec<f32> = std::env::var("JENGINE_SHOT_AFTER")
            .unwrap_or_else(|_| "2".into())
            .split(',')
            .filter_map(|t| t.trim().parse::<f32>().ok())
            .filter(|t| t.is_finite() && *t >= 0.0)
            .collect();
        if times.is_empty() {
            times.push(2.0);
        }
        times.sort_by(|a, b| a.total_cmp(b));
        log::info!("[shot] {} screenshot(s) named `{name}` at {times:?} s, then quit", times.len());
        Some(ShotEnv { name, times, next: 0, elapsed: 0.0, pending: Vec::new() })
    }

    /// Advance by one frame's wall time. True once every screenshot is
    /// written - time to quit.
    pub(crate) fn tick(&mut self, wall_dt: f32) -> bool {
        self.elapsed += wall_dt;
        while self.next < self.times.len() && self.elapsed >= self.times[self.next] {
            let file = if self.times.len() == 1 {
                self.name.clone()
            } else {
                format!("{}-{}", self.name, self.next)
            };
            self.pending.push(screenshot::request(&serde_json::json!({ "file": file })));
            self.next += 1;
        }
        self.pending.retain(|id| !screenshot::is_done(*id));
        self.next == self.times.len() && self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshots_are_asked_for_at_their_times() {
        let mut s = ShotEnv { name: "x".into(), times: vec![0.5, 1.0], next: 0, elapsed: 0.0, pending: Vec::new() };
        assert!(!s.tick(0.4));
        assert_eq!(s.next, 0);
        assert!(!s.tick(0.2), "the first is asked for but not yet written");
        assert_eq!(s.next, 1);
        assert_eq!(s.pending.len(), 1);
    }
}
