//! What an agent watches besides its servers: **the machine** - CPU, memory,
//! the disk its data is on, network totals, load, temperature - and **each
//! server's log**, counted for errors and warnings as it grows. Both end up
//! in `/v1/status`, which is what the console samples for its dashboard.
//!
//! Every number that grows is a total (bytes since boot, errors since the
//! agent started): a reader takes rates and "in the last hour" from two
//! samples, and a total that went backwards means its counter started again.

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tokio::sync::watch;

fn unix_now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Whether an interface is this machine talking to itself: the gateway and
/// its servers speak over loopback, which would count every byte twice.
fn loopback(name: &str) -> bool {
    name == "lo" || name.starts_with("lo0") || name.to_ascii_lowercase().contains("loopback")
}

/// The hottest thermal zone, in degrees: on a Raspberry Pi the SoC, which
/// throttles at 80-85. None where the system says nothing (Windows).
fn temperature() -> Option<f64> {
    let zones = std::fs::read_dir("/sys/class/thermal").ok()?;
    zones
        .flatten()
        .filter(|z| z.file_name().to_string_lossy().starts_with("thermal_zone"))
        .filter_map(|z| std::fs::read_to_string(z.path().join("temp")).ok())
        .filter_map(|t| t.trim().parse::<f64>().ok())
        .map(|milli| milli / 1000.0)
        .reduce(f64::max)
}

/// Measure the machine every two seconds on a thread of its own (CPU use is
/// the difference between two askings) and keep the last answer.
pub fn watch_machine(data: PathBuf, stop: watch::Receiver<bool>) -> Arc<Mutex<Value>> {
    use sysinfo::{Disks, Networks, System};
    let latest = Arc::new(Mutex::new(Value::Null));
    let out = latest.clone();
    std::thread::spawn(move || {
        let mut system = System::new();
        system.refresh_cpu_usage();
        let mut disks = Disks::new_with_refreshed_list();
        let mut networks = Networks::new_with_refreshed_list();
        let data = std::path::absolute(&data).unwrap_or(data);
        let mut looks: u32 = 0;
        // The first CPU reading needs a second asking to mean anything.
        std::thread::sleep(Duration::from_millis(300));
        while !*stop.borrow() {
            system.refresh_cpu_usage();
            system.refresh_memory();
            disks.refresh(looks.is_multiple_of(30));
            networks.refresh(looks.is_multiple_of(30));
            looks = looks.wrapping_add(1);
            let (rx, tx) = networks
                .iter()
                .filter(|(name, _)| !loopback(name))
                .fold((0u64, 0u64), |(rx, tx), (_, d)| (rx + d.total_received(), tx + d.total_transmitted()));
            let disk = disks
                .iter()
                .filter(|d| data.starts_with(d.mount_point()))
                .max_by_key(|d| d.mount_point().as_os_str().len());
            let load = System::load_average();
            let now = json!({
                "time": unix_now(),
                "host": System::host_name(),
                "os": System::long_os_version(),
                "cores": system.cpus().len(),
                "cpu": (system.global_cpu_usage() * 10.0).round() / 10.0,
                "load": [load.one, load.five, load.fifteen],
                "memory_used": system.used_memory(),
                "memory_total": system.total_memory(),
                "disk_used": disk.map(|d| d.total_space().saturating_sub(d.available_space())),
                "disk_total": disk.map(|d| d.total_space()),
                "net_rx": rx,
                "net_tx": tx,
                "uptime_secs": System::uptime(),
                "temperature": temperature().map(|t| (t * 10.0).round() / 10.0),
            });
            *out.lock().unwrap() = now;
            for _ in 0..20 {
                if *stop.borrow() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
    latest
}

/// How bad a log line is, if it is bad at all. Servers log through
/// `env_logger` (`[time LEVEL target] ..`); a Rust panic says so in words.
pub fn severity(line: &str) -> Option<&'static str> {
    if line.contains(" ERROR ") || line.contains("panicked at") {
        Some("error")
    } else if line.contains(" WARN ") {
        Some("warning")
    } else {
        None
    }
}

/// Most of a new log read at one look: past this, the oldest is skipped.
const MAX_READ: u64 = 2 << 20;
/// Bad lines kept per service, newest last.
const RECENT: usize = 20;

/// One server's log, read from where the last look stopped.
#[derive(Debug, Default)]
pub struct LogWatch {
    /// None until the first look, which starts at the end: what was in the
    /// log before the agent started is not counted.
    offset: Option<u64>,
    pub errors: u64,
    pub warnings: u64,
    pub recent: VecDeque<Value>,
}

impl LogWatch {
    /// Read what was added to `path` since the last look.
    pub fn look(&mut self, path: &Path) {
        let Ok(mut file) = std::fs::File::open(path) else { return };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let from = match self.offset {
            None => len,
            // Shorter than it was: set aside and begun again.
            Some(at) if at > len => 0,
            Some(at) => at.max(len.saturating_sub(MAX_READ)),
        };
        if from >= len {
            self.offset = Some(len);
            return;
        }
        let mut text = Vec::new();
        if file.seek(SeekFrom::Start(from)).is_err() || file.take(len - from).read_to_end(&mut text).is_err() {
            return;
        }
        // Only whole lines: a line being written is read next time.
        let whole = text.iter().rposition(|&b| b == b'\n').map(|i| i + 1).unwrap_or(0);
        self.offset = Some(from + whole as u64);
        let now = unix_now();
        for line in String::from_utf8_lossy(&text[..whole]).lines() {
            let Some(level) = severity(line) else { continue };
            match level {
                "error" => self.errors += 1,
                _ => self.warnings += 1,
            }
            let line: String = line.chars().take(400).collect();
            self.recent.push_back(json!({ "time": now, "level": level, "line": line }));
            if self.recent.len() > RECENT {
                self.recent.pop_front();
            }
        }
    }

    pub fn json(&self) -> Value {
        json!({ "errors": self.errors, "warnings": self.warnings, "recent": self.recent })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn errors_and_warnings_are_counted_as_the_log_grows() {
        let dir = std::env::temp_dir().join(format!("jengine-logwatch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("service.log");
        std::fs::write(&path, "[t ERROR x] before the agent looked\n").unwrap();
        let mut watch = LogWatch::default();
        watch.look(&path);
        assert_eq!((watch.errors, watch.warnings), (0, 0), "what was there before is not counted");

        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(f, "[t INFO x] fine\n[t WARN x] slow tick\n[t ERROR x] room crashed\nthread 'x' panicked at src/a.rs\n[t ERROR x] half a li").unwrap();
        watch.look(&path);
        assert_eq!((watch.errors, watch.warnings), (2, 1), "the half-written line waits");
        writeln!(f, "ne").unwrap();
        watch.look(&path);
        assert_eq!(watch.errors, 3);
        assert_eq!(watch.recent.back().unwrap()["line"], "[t ERROR x] half a line");

        // Set aside and started again: read from the top.
        std::fs::write(&path, "[t WARN x] new log\n").unwrap();
        watch.look(&path);
        assert_eq!(watch.warnings, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn loopback_is_not_the_network() {
        assert!(loopback("lo") && loopback("Loopback Pseudo-Interface 1") && loopback("lo0"));
        assert!(!loopback("eth0") && !loopback("wlan0") && !loopback("Wi-Fi"));
    }
}
