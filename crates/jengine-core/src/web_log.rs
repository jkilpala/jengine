//! The `log` crate's output in a browser console.
//!
//! Native installs `env_logger`; until Oct 2026 the two browser hosts (the web
//! runtime and the editor viewport) set a maximum level and installed **no
//! logger**, so every `log::info!` / `warn!` / `error!` in the engine went
//! nowhere on the web - the adapter line saying WebGPU or WebGL, a shader that
//! fell back, a texture that failed to load. [`init`] is the one logger both
//! call.

use std::cell::RefCell;

use log::{Level, LevelFilter, Log, Metadata, Record};

struct ConsoleLogger {
    level: LevelFilter,
}

thread_local! {
    /// The last line written and how many times it repeated since, so a
    /// message logged every frame costs one console line, not sixty a second.
    static LAST: RefCell<(String, u32)> = const { RefCell::new((String::new(), 0)) };
}

/// wgpu and naga narrate at info; only their warnings are worth a console.
fn is_chatty(target: &str) -> bool {
    target.starts_with("wgpu") || target.starts_with("naga")
}

impl Log for ConsoleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let limit = if is_chatty(metadata.target()) {
            LevelFilter::Warn.min(self.level)
        } else {
            self.level
        };
        metadata.level() <= limit
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!("[{}] {}", record.target(), record.args());
        let repeated = LAST.with(|last| {
            let mut last = last.borrow_mut();
            if last.0 == line {
                last.1 += 1;
                return true;
            }
            if last.1 > 0 {
                web_sys::console::log_1(&format!("(previous line repeated {} more times)", last.1).into());
            }
            *last = (line.clone(), 0);
            false
        });
        if repeated {
            return;
        }
        let text = line.into();
        match record.level() {
            Level::Error => web_sys::console::error_1(&text),
            Level::Warn => web_sys::console::warn_1(&text),
            _ => web_sys::console::log_1(&text),
        }
    }

    fn flush(&self) {}
}

/// Send `log` records at `level` and above to the browser console. Safe to
/// call twice: a second logger is refused by `log`, and the level is set
/// either way.
pub fn init(level: LevelFilter) {
    // `set_boxed_logger` needs `log`'s `std` feature; leaking the box is the
    // same thing, and happens once.
    let _ = log::set_logger(Box::leak(Box::new(ConsoleLogger { level })));
    log::set_max_level(level);
}
