//! **The console tells somebody.** Every sample the console takes
//! ([`super::history::sample`], every few seconds) is checked against a few
//! rules, and an alert that starts or ends is POSTed to a webhook - a chat
//! channel, a pager - so a server that dies on the Pi at night is heard of
//! before somebody happens to open the page.
//!
//! A level has to hold for a while before it is an alert (a machine missing
//! one sample is a hiccup, not an outage); an event - a restart, error lines
//! in a log - is one at once. The webhook is `"alert_webhook"` in the fleet
//! file, else `JENGINE_ALERT_WEBHOOK`; with neither, alerts are still kept
//! and shown at `/api/alerts`.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// Seconds a machine may not answer before it is an alert.
pub const MACHINE_DOWN_SECS: f64 = 30.0;
/// Seconds a service may be crashed or failed before it is an alert - long
/// enough for the agent's own restart to bring it back.
pub const SERVICE_DOWN_SECS: f64 = 15.0;
/// Seconds a disk may be this full, or a machine this hot, before it is one.
pub const LEVEL_SECS: f64 = 60.0;
/// Used share of a disk that is an alert.
pub const DISK_FULL: f64 = 0.95;
/// Degrees that are an alert (a Pi throttles at 80-85).
pub const HOT: f64 = 80.0;

/// What is wrong now, and what was wrong at the last sample, for telling
/// what started and what ended.
#[derive(Debug, Default)]
pub struct Alerts {
    /// key -> (when it was first seen wrong, what to say).
    pending: BTreeMap<String, (f64, String)>,
    /// Alerts on: key -> what was said.
    on: BTreeMap<String, String>,
    /// Totals as of the last sample: errors and restarts per service.
    last: BTreeMap<String, (f64, f64)>,
}

/// What one sample changed.
#[derive(Debug, Default, PartialEq)]
pub struct Change {
    pub started: Vec<String>,
    pub ended: Vec<String>,
    /// Things that happened (a restart, new error lines): said once.
    pub happened: Vec<String>,
}

impl Change {
    pub fn is_empty(&self) -> bool {
        self.started.is_empty() && self.ended.is_empty() && self.happened.is_empty()
    }
}

impl Alerts {
    /// Check one sample (a `history::sample`).
    pub fn check(&mut self, sample: &Value) -> Change {
        let t = sample["t"].as_f64().unwrap_or(0.0);
        let mut wrong: BTreeMap<String, (f64, String)> = BTreeMap::new();
        let mut change = Change::default();
        for (name, m) in sample["machines"].as_object().into_iter().flatten() {
            if m["up"] == false {
                wrong.insert(format!("down:{name}"), (MACHINE_DOWN_SECS, format!("{name} is not answering")));
                continue;
            }
            let (used, total) = (m["disk"].as_f64().unwrap_or(0.0), m["disk_total"].as_f64().unwrap_or(0.0));
            if total > 0.0 && used / total >= DISK_FULL {
                let pct = (used / total * 100.0).round();
                wrong.insert(format!("disk:{name}"), (LEVEL_SECS, format!("{name}'s disk is {pct}% full")));
            }
            if let Some(temp) = m["temp"].as_f64().filter(|t| *t >= HOT) {
                wrong.insert(format!("hot:{name}"), (LEVEL_SECS, format!("{name} is at {temp:.0} °C")));
            }
        }
        for (name, s) in sample["services"].as_object().into_iter().flatten() {
            let status = s["status"].as_str().unwrap_or("");
            if matches!(status, "crashed" | "failed") {
                wrong.insert(format!("service:{name}"), (SERVICE_DOWN_SECS, format!("{name} is {status}")));
            }
            let (errors, restarts) = (s["errors"].as_f64().unwrap_or(0.0), s["restarts"].as_f64().unwrap_or(0.0));
            if let Some((was_errors, was_restarts)) = self.last.get(name) {
                // A total that went backwards is a counter that started again.
                if errors > *was_errors {
                    change.happened.push(format!("{name} logged {} new error line(s)", errors - was_errors));
                }
                if restarts > *was_restarts {
                    change.happened.push(format!("{name} restarted"));
                }
            }
            self.last.insert(name.clone(), (errors, restarts));
        }
        // Held long enough: on. Gone: off.
        for (key, (hold, text)) in &wrong {
            let since = self.pending.entry(key.clone()).or_insert((t, text.clone())).0;
            if t - since >= *hold && !self.on.contains_key(key) {
                self.on.insert(key.clone(), text.clone());
                change.started.push(text.clone());
            }
        }
        self.pending.retain(|key, _| wrong.contains_key(key));
        let gone: Vec<String> = self.on.keys().filter(|k| !wrong.contains_key(*k)).cloned().collect();
        for key in gone {
            if let Some(text) = self.on.remove(&key) {
                change.ended.push(format!("no longer: {text}"));
            }
        }
        change
    }

    /// The alerts on now.
    pub fn on(&self) -> Vec<String> {
        self.on.values().cloned().collect()
    }
}

/// POST a change to `url`, on a thread of its own, tried three times.
pub fn send(url: &str, change: &Change) {
    let body = json!({ "from": "jengine-fleet console", "started": change.started, "ended": change.ended, "happened": change.happened })
        .to_string();
    let url = url.to_string();
    std::thread::spawn(move || {
        for wait in [0u64, 5, 30] {
            std::thread::sleep(std::time::Duration::from_secs(wait));
            let sent = ureq::post(&url)
                .timeout(std::time::Duration::from_secs(5))
                .set("Content-Type", "application/json")
                .send_string(&body);
            match sent {
                Ok(_) => return,
                Err(e) => log::warn!("[console] the alert webhook did not take it: {e}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(t: f64, up: bool, disk: f64, status: &str, errors: f64, restarts: f64) -> Value {
        json!({
            "t": t,
            "machines": { "pi": { "up": up, "disk": disk, "disk_total": 100.0, "temp": 50.0 } },
            "services": { "pi/chess": { "status": status, "errors": errors, "restarts": restarts } },
        })
    }

    /// A machine that misses one sample is not an outage; one that stays
    /// away is, once; and its coming back is said too.
    #[test]
    fn a_level_must_hold_and_its_end_is_said() {
        let mut a = Alerts::default();
        assert!(a.check(&sample(0.0, true, 10.0, "running", 0.0, 0.0)).is_empty());
        assert!(a.check(&sample(5.0, false, 0.0, "running", 0.0, 0.0)).started.is_empty(), "one missed sample");
        assert!(a.check(&sample(10.0, true, 10.0, "running", 0.0, 0.0)).is_empty(), "back: nothing to say");
        for t in [15.0, 20.0, 40.0] {
            a.check(&sample(t, false, 0.0, "running", 0.0, 0.0));
        }
        let c = a.check(&sample(50.0, false, 0.0, "running", 0.0, 0.0));
        assert_eq!(c.started, ["pi is not answering"], "thirty seconds away");
        assert!(a.check(&sample(55.0, false, 0.0, "running", 0.0, 0.0)).is_empty(), "said once");
        assert_eq!(a.on(), ["pi is not answering"]);
        let c = a.check(&sample(60.0, true, 10.0, "running", 0.0, 0.0));
        assert_eq!(c.ended, ["no longer: pi is not answering"]);
    }

    /// A crashed service, a full disk; a restart and new errors are events.
    #[test]
    fn services_disks_restarts_and_errors() {
        let mut a = Alerts::default();
        a.check(&sample(0.0, true, 96.0, "running", 2.0, 1.0));
        let c = a.check(&sample(5.0, true, 96.0, "crashed", 5.0, 2.0));
        assert_eq!(c.happened, ["pi/chess logged 3 new error line(s)", "pi/chess restarted"]);
        let c = a.check(&sample(25.0, true, 96.0, "crashed", 5.0, 2.0));
        assert_eq!(c.started, ["pi/chess is crashed"]);
        let c = a.check(&sample(65.0, true, 96.0, "running", 5.0, 2.0));
        assert_eq!(c.started, ["pi's disk is 96% full"]);
        assert_eq!(c.ended, ["no longer: pi/chess is crashed"]);
        // A counter that starts again (the agent restarted) is no news.
        assert!(a.check(&sample(70.0, true, 96.0, "running", 0.0, 0.0)).happened.is_empty());
    }
}
