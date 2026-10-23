//! Live ops on a desktop or phone: this install's id and the last snapshot
//! believed, in a `liveops_cache` folder beside the pack cache (so beside the
//! saves - somewhere every platform lets the game write), and a worker
//! thread per fetch through the HTTP client the runtime installs for packs
//! ([`crate::asset::packs::native::set_http`]) - and exposures posted
//! through its POST ([`crate::asset::packs::native::set_http_post`]).

use std::path::PathBuf;

use super::{LiveOpsSettings, native_platform, with};

/// The folder: `JENGINE_LIVEOPS_DIR`, else `liveops_cache` beside the pack
/// cache - not `liveops`, which is what a service's folder is usually
/// called, and a game run from a folder holding one would write into it.
pub fn dir() -> PathBuf {
    if let Ok(d) = std::env::var("JENGINE_LIVEOPS_DIR")
        && !d.is_empty()
    {
        return PathBuf::from(d);
    }
    let packs = crate::asset::packs::native::cache_dir();
    packs.parent().map(|p| p.join("liveops_cache")).unwrap_or_else(|| PathBuf::from("liveops_cache"))
}

/// This install's id: made once, kept in [`dir`]. Rollouts and experiments
/// bucket by it, so it must not change between starts.
pub fn install_id() -> String {
    let path = dir().join("install_id");
    if let Ok(id) = std::fs::read_to_string(&path) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            return id;
        }
    }
    let id = new_id();
    let _ = std::fs::create_dir_all(dir());
    if let Err(e) = std::fs::write(&path, &id) {
        log::warn!("[liveops] could not keep the install id in {}: {e} - it will change next start", path.display());
    }
    id
}

/// An id no other install will have. Not a secret, so not from a
/// cryptographic generator: the time, the process and an address, hashed.
fn new_id() -> String {
    use sha2::Digest;
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let here = &t as *const u128 as usize;
    let h = sha2::Sha256::digest(format!("{t}:{}:{here}", std::process::id()).as_bytes());
    format!("i-{}", jengine_liveops::to_hex(&h[..12]))
}

fn cache_file(channel: &str) -> PathBuf {
    dir().join(format!("snapshot-{channel}.json"))
}

/// Set live ops up from the project's `settings` for a built game (its
/// `channel`), and believe the snapshot cached last time if it checks out.
/// Returns that snapshot's content release, which the packs need before
/// they mount anything (`asset::packs::native::init`).
pub fn init(settings: &serde_json::Value) -> Option<super::Content> {
    let s = LiveOpsSettings::from_settings(settings);
    if s.url.is_empty() {
        with(|l| l.configure(s, "live", "", native_platform(), "", true));
        return None;
    }
    let channel = s.game_channel();
    let version = super::app_version(settings);
    let install = install_id();
    let now = crate::profiling::unix_time();
    with(|l| {
        l.configure(s, &channel, &install, native_platform(), &version, true);
        let cached = std::fs::read_to_string(cache_file(&channel)).ok()?;
        l.load_cached(&cached, now)
    })
}

/// Start a fetch if one is due, and send the exposures scripts queued. The
/// runtime calls it once a frame; the fetch runs on a thread of its own and
/// reports straight into the process's live ops, writing the cache when it
/// was believed.
pub fn sync() {
    let now = crate::profiling::unix_time();
    let (due, exposures) = with(|l| {
        l.follow_locale();
        (l.poll(now).map(|u| (u, l.channel().to_string())), l.take_exposures())
    });
    if !exposures.is_empty() {
        std::thread::spawn(move || {
            for e in exposures {
                if let Err(err) = crate::asset::packs::native::http_post(&e.url, "application/json", e.body.as_bytes()) {
                    log::warn!("[liveops] an exposure was not counted: {err:#}");
                }
            }
        });
    }
    let Some((url, channel)) = due else { return };
    std::thread::spawn(move || {
        let result = crate::asset::packs::native::http_get(&url).and_then(|b| String::from_utf8(b).map_err(|e| anyhow::anyhow!("{e}")));
        let result = result.map_err(|e| format!("{e:#}"));
        let now = crate::profiling::unix_time();
        if let Some(text) = with(|l| l.deliver(result, now)) {
            let path = cache_file(&channel);
            let _ = std::fs::create_dir_all(dir());
            if let Err(e) = std::fs::write(&path, text) {
                log::warn!("[liveops] could not cache the snapshot in {}: {e}", path.display());
            }
            log::info!("[liveops] believed revision {} from the service", with(|l| l.revision()));
        }
    });
}
