//! Filesystem watcher for live asset reloading (native only).
//!
//! Wraps [`notify::RecommendedWatcher`] in a debounced, polled API:
//! - The watcher emits raw events into a channel from a background thread
//!   owned by `notify`.
//! - [`AssetWatcher::drain_changed`] is called once per frame; it returns the
//!   set of paths that have been modified since the last drain, after
//!   suppressing bursts (a 100 ms quiet period collapses rapid sequential
//!   writes into a single change).
//!
//! On `wasm32` the type compiles as a no-op stub so call sites don't need
//! `#[cfg]` gates.

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use notify::{
        event::{ModifyKind, RemoveKind},
        EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    };

    /// Bursts of writes within this window are collapsed into a single event.
    const DEBOUNCE: Duration = Duration::from_millis(100);

    /// Watches a directory tree and reports changed file paths.
    ///
    /// Drop it to stop watching.
    pub struct AssetWatcher {
        // The watcher must be kept alive — dropping it stops the background
        // thread. We never read it after construction.
        _watcher: RecommendedWatcher,
        pending: Arc<Mutex<HashMap<PathBuf, Instant>>>,
        root: PathBuf,
    }

    impl AssetWatcher {
        /// Recursively watch `root`. Returns an error if the path doesn't
        /// exist or the OS watcher couldn't be created.
        pub fn new(root: &Path) -> notify::Result<Self> {
            let pending: Arc<Mutex<HashMap<PathBuf, Instant>>> =
                Arc::new(Mutex::new(HashMap::new()));
            let pending_for_cb = Arc::clone(&pending);

            let mut watcher = notify::recommended_watcher(
                move |res: notify::Result<notify::Event>| {
                    let Ok(event) = res else { return };
                    if !is_relevant(&event.kind) {
                        return;
                    }
                    let now = Instant::now();
                    if let Ok(mut map) = pending_for_cb.lock() {
                        for path in event.paths {
                            map.insert(path, now);
                        }
                    }
                },
            )?;
            watcher.watch(root, RecursiveMode::Recursive)?;
            Ok(Self {
                _watcher: watcher,
                pending,
                root: root.to_path_buf(),
            })
        }

        /// Return paths that have been modified and have been quiet for at
        /// least `DEBOUNCE`. Removes them from the pending set.
        pub fn drain_changed(&self) -> Vec<PathBuf> {
            let now = Instant::now();
            let mut out: Vec<PathBuf> = Vec::new();
            let Ok(mut map) = self.pending.lock() else {
                return out;
            };
            // Collect keys whose last-modified timestamp is older than the
            // debounce window — they're settled.
            let ready: HashSet<PathBuf> = map
                .iter()
                .filter_map(|(p, t)| {
                    if now.duration_since(*t) >= DEBOUNCE {
                        Some(p.clone())
                    } else {
                        None
                    }
                })
                .collect();
            for p in &ready {
                map.remove(p);
            }
            out.extend(ready);
            out
        }

        /// Root directory being watched.
        pub fn root(&self) -> &Path {
            &self.root
        }
    }

    fn is_relevant(kind: &EventKind) -> bool {
        matches!(
            kind,
            EventKind::Create(_)
                | EventKind::Modify(ModifyKind::Data(_))
                | EventKind::Modify(ModifyKind::Name(_))
                | EventKind::Modify(ModifyKind::Any)
                | EventKind::Modify(ModifyKind::Other)
                | EventKind::Remove(RemoveKind::File)
                | EventKind::Remove(RemoveKind::Any)
        )
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::path::{Path, PathBuf};

    /// No-op stub — the browser has no filesystem to watch.
    pub struct AssetWatcher {
        root: PathBuf,
    }

    impl AssetWatcher {
        pub fn new(root: &Path) -> Result<Self, std::io::Error> {
            Ok(Self {
                root: root.to_path_buf(),
            })
        }

        pub fn drain_changed(&self) -> Vec<PathBuf> {
            Vec::new()
        }

        pub fn root(&self) -> &Path {
            &self.root
        }
    }
}

pub use imp::AssetWatcher;

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use std::fs;
    use std::thread::sleep;
    use std::time::Duration;

    use super::*;

    #[test]
    fn detects_modified_file_after_debounce() {
        let dir = tempdir_unique("asset-watcher-mod");
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("a.lua");
        fs::write(&target, b"-- v1").unwrap();

        let w = AssetWatcher::new(&dir).expect("watcher");
        // Give notify a moment to install OS-level hooks before we touch the file.
        sleep(Duration::from_millis(50));

        fs::write(&target, b"-- v2").unwrap();

        // Wait past the debounce window plus a margin.
        sleep(Duration::from_millis(300));
        let changed = w.drain_changed();

        assert!(
            changed.iter().any(|p| p.ends_with("a.lua")),
            "expected a.lua in changed set, got {:?}",
            changed,
        );

        // Cleanup.
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_changes_returns_empty() {
        let dir = tempdir_unique("asset-watcher-empty");
        fs::create_dir_all(&dir).unwrap();
        let w = AssetWatcher::new(&dir).expect("watcher");
        sleep(Duration::from_millis(50));
        assert!(w.drain_changed().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Build a unique temp directory path under `std::env::temp_dir()` without
    /// pulling in the `tempfile` crate.
    fn tempdir_unique(prefix: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("jengine-{}-{}-{}-{}", prefix, pid, nanos, n))
    }
}
