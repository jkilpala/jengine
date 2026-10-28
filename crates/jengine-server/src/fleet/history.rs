//! The console's memory of how things were: a **sample** of every machine and
//! service every [`INTERVAL`] seconds, an hour of them at that grain and a day
//! at one a minute - what the dashboard's charts are drawn from.
//!
//! A sample holds totals (bytes since boot, game traffic since ever, errors
//! since the agent started) as well as levels; rates and "in the last hour"
//! are worked out by the page from two samples, treating a total that went
//! backwards as a counter that started again.
//!
//! A console a host serves keeps the day in `history.json`, so a restart (or
//! a reboot) loses at most a minute of it.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// Seconds between samples.
pub const INTERVAL: u64 = 5;
/// An hour of samples at [`INTERVAL`].
const FINE: usize = (3600 / INTERVAL) as usize;
/// The day, one sample a minute.
const COARSE_EVERY: f64 = 60.0;
const COARSE: usize = 24 * 60;

#[derive(Debug, Default)]
pub struct History {
    fine: VecDeque<Value>,
    coarse: VecDeque<Value>,
    /// Where the day is kept, for a hosted console.
    file: Option<PathBuf>,
    last_saved: f64,
}

/// One sample from the machines' answers to `/v1/status`: the levels and
/// totals the dashboard draws, and nothing an operator would not show.
pub fn sample(time: f64, machines: &[(String, Result<Value, String>)]) -> Value {
    let mut ms = serde_json::Map::new();
    let mut ss = serde_json::Map::new();
    for (name, answer) in machines {
        let Ok(status) = answer else {
            ms.insert(name.clone(), json!({ "up": false }));
            continue;
        };
        let m = &status["machine"];
        ms.insert(
            name.clone(),
            json!({
                "up": true,
                "cpu": m["cpu"], "mem": m["memory_used"], "mem_total": m["memory_total"],
                "disk": m["disk_used"], "disk_total": m["disk_total"],
                "rx": m["net_rx"], "tx": m["net_tx"], "temp": m["temperature"], "load": m["load"][0],
            }),
        );
        for s in status["services"].as_array().into_iter().flatten() {
            let stats = &s["stats"];
            let totals = &stats["totals"];
            ss.insert(
                format!("{name}/{}", s["name"].as_str().unwrap_or("?")),
                json!({
                    "status": s["status"],
                    "players": stats["players"], "conns": stats["connections"],
                    "cpu": stats["cpu"], "mem": stats["memory_mb"],
                    "in": totals["bytes_in"], "out": totals["bytes_out"],
                    "msgs": totals["messages_in"], "frames": totals["frames_out"],
                    "errors": s["log"]["errors"], "warnings": s["log"]["warnings"],
                    "restarts": s["restarts"],
                }),
            );
        }
    }
    json!({ "t": time, "machines": ms, "services": ss })
}

impl History {
    /// A history kept in `file` (read now, written as it grows), or in
    /// memory only.
    pub fn new(file: Option<PathBuf>) -> History {
        let mut history = History { file, ..Default::default() };
        if let Some(saved) = history.file.as_deref().and_then(read) {
            // The hour starts from the day's minutes in it, so a restart (an
            // upgrade) does not leave "the last hour" empty.
            let since = now() - 3600.0;
            history.fine = saved.iter().filter(|s| s["t"].as_f64().unwrap_or(0.0) >= since).cloned().collect();
            history.coarse = saved;
        }
        history
    }

    pub fn push(&mut self, sample: Value) {
        let t = sample["t"].as_f64().unwrap_or(0.0);
        // A Pi reads months back after a boot until the network sets its clock;
        // a sample from before the newest one is from then, and is not kept.
        if self.fine.back().is_some_and(|last| t < last["t"].as_f64().unwrap_or(0.0)) {
            return;
        }
        let due = self.coarse.back().is_none_or(|last| t - last["t"].as_f64().unwrap_or(0.0) >= COARSE_EVERY);
        if due {
            self.coarse.push_back(sample.clone());
            if self.coarse.len() > COARSE {
                self.coarse.pop_front();
            }
        }
        self.fine.push_back(sample);
        if self.fine.len() > FINE {
            self.fine.pop_front();
        }
        if due && t - self.last_saved >= COARSE_EVERY {
            self.save();
            self.last_saved = t;
        }
    }

    /// `hour`: every sample of the last hour; `day`: one a minute.
    pub fn range(&self, range: &str) -> Value {
        let (samples, interval) = match range {
            "day" => (&self.coarse, COARSE_EVERY),
            _ => (&self.fine, INTERVAL as f64),
        };
        json!({ "interval": interval, "samples": samples })
    }

    fn save(&self) {
        let Some(file) = &self.file else { return };
        let text = serde_json::to_string(&self.coarse).unwrap_or_default();
        let tmp = file.with_extension("json.tmp");
        if std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, file)).is_err() {
            log::warn!("[console] could not write {}", file.display());
        }
    }
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn read(file: &Path) -> Option<VecDeque<Value>> {
    serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> Value {
        json!({
            "machine": { "cpu": 12.5, "memory_used": 1000, "memory_total": 4000, "disk_used": 5, "disk_total": 10,
                         "net_rx": 700, "net_tx": 300, "temperature": 51.2, "load": [0.4, 0.3, 0.2] },
            "services": [{ "name": "chess", "status": "running", "restarts": 1,
                           "stats": { "players": 3, "connections": 4, "cpu": 2.0, "memory_mb": 12,
                                      "totals": { "bytes_in": 10, "bytes_out": 90, "messages_in": 2, "frames_out": 9 } },
                           "log": { "errors": 1, "warnings": 2 } }],
        })
    }

    #[test]
    fn a_restart_starts_the_hour_from_the_days_minutes() {
        let file = std::env::temp_dir().join(format!("jengine-history-restart-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&file);
        let mut h = History::new(Some(file.clone()));
        let start = now() - 7200.0;
        for i in 0..1440 {
            h.push(json!({ "t": start + i as f64 * 5.0 }));
        }
        let again = History::new(Some(file.clone()));
        let fine = again.range("hour")["samples"].as_array().unwrap().len();
        assert!((55..=61).contains(&fine), "the last hour's minutes: {fine}");
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn a_sample_from_before_the_newest_is_dropped() {
        let mut h = History::new(None);
        for t in [6000.0, 6005.0, 1500.0, 6015.0] {
            h.push(json!({ "t": t }));
        }
        let ts: Vec<f64> = h.range("hour")["samples"].as_array().unwrap().iter().map(|s| s["t"].as_f64().unwrap()).collect();
        assert_eq!(ts, [6000.0, 6005.0, 6015.0]);
    }

    #[test]
    fn a_sample_carries_levels_and_totals_and_nothing_else() {
        let s = sample(100.0, &[("pi".into(), Ok(status())), ("gone".into(), Err("unreached".into()))]);
        assert_eq!(s["machines"]["pi"]["temp"], 51.2);
        assert_eq!(s["machines"]["pi"]["load"], 0.4);
        assert_eq!(s["machines"]["gone"]["up"], false);
        let k = &s["services"]["pi/chess"];
        assert_eq!((k["players"].as_u64(), k["out"].as_u64(), k["errors"].as_u64()), (Some(3), Some(90), Some(1)));
        assert!(!s.to_string().contains("token"));
    }

    #[test]
    fn an_hour_is_fine_and_a_day_is_one_a_minute_and_the_day_is_kept() {
        let file = std::env::temp_dir().join(format!("jengine-history-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&file);
        let mut h = History::new(Some(file.clone()));
        for i in 0..1000 {
            h.push(json!({ "t": 1000.0 + i as f64 * 5.0 }));
        }
        assert_eq!(h.range("hour")["samples"].as_array().unwrap().len(), FINE);
        // 5000 seconds of samples, one a minute kept.
        assert_eq!(h.range("day")["samples"].as_array().unwrap().len(), 84);
        let again = History::new(Some(file.clone()));
        assert!(again.coarse.len() >= 83, "the day was written down as it went: {}", again.coarse.len());
        let _ = std::fs::remove_file(&file);
    }
}
