//! Packs on a desktop or phone: a cache on disk that outlives the session,
//! and a worker thread per download.
//!
//! A pack is unpacked into `<cache>/<name>-<hash>/`, and a marker file
//! written last says the copy is whole - so a download cut off half way (the
//! game closed, the phone slept) is a pack that is simply not there yet, and
//! a game started again with the same build mounts what it already has
//! ([`init`]). A new build's pack has a new hash and a new folder; the old
//! one is deleted when the new manifest no longer names it.
//!
//! The archive comes from `http(s)://` through the fetcher the runtime
//! installs ([`set_http`] - core links no HTTP client), or from a folder on
//! disk (relative to the game, or `file://`), which is what a fresh build
//! does by default: its packs are in `packs/` beside it. Either way it is
//! checked against the manifest's size and SHA-256 before a byte is
//! unpacked.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use super::{PackEntry, PackManifest, PackSettings, Request, archive};

/// An HTTP GET: the body to read, and its length if the server said.
pub type HttpFetch = dyn Fn(&str) -> anyhow::Result<(Box<dyn Read + Send>, Option<u64>)> + Send + Sync;

static HTTP: OnceLock<Box<HttpFetch>> = OnceLock::new();

/// An HTTP POST of `body` as `content_type`: the answer's body, or why there
/// is none (a 4xx or 5xx answer is an error).
pub type HttpPost = dyn Fn(&str, &str, &[u8]) -> anyhow::Result<Vec<u8>> + Send + Sync;

static HTTP_POST: OnceLock<Box<HttpPost>> = OnceLock::new();

/// Where this game's packs are cached, and the folder a relative pack URL is
/// read against - set by [`init`].
static PLACES: Mutex<Option<(PathBuf, PathBuf)>> = Mutex::new(None);

/// The marker written into a pack's folder once it is whole.
pub const COMPLETE: &str = ".jpak-complete";

/// Install the HTTP client downloads use. The runtime calls it once.
pub fn set_http(fetch: Box<HttpFetch>) {
    let _ = HTTP.set(fetch);
}

/// Install the POST live ops tells the service of exposures with
/// (`crate::liveops::native`). The runtime calls it once, beside [`set_http`].
pub fn set_http_post(post: Box<HttpPost>) {
    let _ = HTTP_POST.set(post);
}

/// POST `body` to `url` through the installed client.
pub fn http_post(url: &str, content_type: &str, body: &[u8]) -> anyhow::Result<Vec<u8>> {
    let Some(post) = HTTP_POST.get() else {
        anyhow::bail!("this runtime has no HTTP client to POST to {url}");
    };
    post(url, content_type, body)
}

/// GET `url` whole through the installed HTTP client - also what live ops
/// fetches its snapshot with (`crate::liveops::native`).
pub fn http_get(url: &str) -> anyhow::Result<Vec<u8>> {
    let Some(http) = HTTP.get() else {
        anyhow::bail!("this runtime has no HTTP client for {url}");
    };
    let (mut body, _) = http(url)?;
    let mut out = Vec::new();
    body.read_to_end(&mut out)?;
    Ok(out)
}

/// Where packs are cached: `JENGINE_PACK_DIR`, else a `pack_cache` folder
/// beside the save folder (`JENGINE_SAVE_DIR`, else `./saves`) - which every
/// platform already puts somewhere writable (an app's data folder on
/// Android, Application Support for a bundle).
pub fn cache_dir() -> PathBuf {
    if let Ok(d) = std::env::var("JENGINE_PACK_DIR")
        && !d.is_empty()
    {
        return PathBuf::from(d);
    }
    let saves = std::env::var("JENGINE_SAVE_DIR").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "saves".into());
    let saves = PathBuf::from(saves);
    saves.parent().map(|p| p.join("pack_cache")).unwrap_or_else(|| PathBuf::from("pack_cache"))
}

/// The folder a pack is unpacked into.
pub fn pack_dir(cache: &Path, entry: &PackEntry) -> PathBuf {
    let hash = &entry.sha256[..entry.sha256.len().min(16)];
    cache.join(format!("{}-{hash}", entry.name))
}

/// Read the game's manifest (`<assets_root>/packs.json`), or with none the
/// project's declared packs; lay over it the live ops release the game last
/// believed (`live`, from its cache - see `crate::liveops`); mount every pack
/// already cached whole - the current copy, or the one a release replaced,
/// `outdated`; and delete cached copies nothing names any more.
/// `base_roots` are the game's own asset roots ([`super::Packs::set_base_roots`]).
pub fn init(assets_root: &Path, settings: &PackSettings, base_roots: Vec<PathBuf>, live: Option<&jengine_liveops::Content>) {
    let manifest = read_manifest(assets_root);
    let cache = cache_dir();
    let game_root = assets_root.parent().map(Path::to_path_buf).unwrap_or_default();
    *PLACES.lock().unwrap_or_else(|p| p.into_inner()) = Some((cache.clone(), game_root));
    let built = manifest.is_some();
    super::configure(manifest, settings);
    super::set_base_roots(base_roots);
    if let Some(c) = live {
        super::apply_live(c.release, &c.manifest);
    }
    if !built && live.is_none() {
        if !settings.packs.is_empty() {
            log::info!("[packs] no {} - {} declared pack(s) are local", super::MANIFEST_FILE, settings.packs.len());
        }
        return;
    }
    let mut keep = Vec::new();
    let mut ours = Vec::new();
    for name in super::names() {
        let (Some(entry), previous) = super::with(|p| (p.entry(&name), p.previous(&name))) else { continue };
        let dir = pack_dir(&cache, &entry);
        if dir.join(COMPLETE).is_file() {
            super::set_installed(&name, &entry.sha256, Some(&dir), true);
            log::info!("[packs] '{name}' is installed ({})", dir.display());
        } else if let Some((old, _)) = &previous {
            let old_dir = pack_dir(&cache, old);
            if old_dir.join(COMPLETE).is_file() {
                super::set_installed(&name, &old.sha256, Some(&old_dir), true);
                log::info!("[packs] '{name}' is outdated - the live release has a newer copy ({})", old_dir.display());
            }
            keep.push(old_dir);
        }
        keep.push(dir);
        ours.push(name);
    }
    prune(&cache, &ours, &keep);
}

fn read_manifest(assets_root: &Path) -> Option<PackManifest> {
    let path = assets_root.join(super::MANIFEST_FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str(&text) {
        Ok(m) => Some(m),
        Err(e) => {
            log::warn!("[packs] {} is not valid: {e}", path.display());
            None
        }
    }
}

/// Delete the cache's copies of this game's packs that are not `keep` -
/// older builds' and releases' - and any half-finished download. Only names
/// that start with one of `names` (this game's packs) are touched.
fn prune(cache: &Path, names: &[String], keep: &[PathBuf]) {
    let Ok(entries) = std::fs::read_dir(cache) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        let ours = names.iter().any(|p| name.starts_with(&format!("{p}-")));
        if !ours || keep.contains(&path) {
            continue;
        }
        let gone = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        match gone {
            Ok(()) => log::info!("[packs] deleted the stale copy {}", path.display()),
            Err(e) => log::warn!("[packs] could not delete {}: {e}", path.display()),
        }
    }
}

/// A mounted pack's copy of asset `rel`, read from disk.
pub fn read(rel: &str) -> Option<Vec<u8>> {
    super::locate(rel).and_then(|p| std::fs::read(p).ok())
}

/// What the packs mounted since the last call brought for the runtime to
/// keep in memory ([`super::content::take_arrived`]).
pub fn arrived() -> Vec<super::content::PackContent> {
    super::content::take_arrived(read)
}

/// Start what scripts asked for. The runtime calls it once a frame; each
/// download runs on a thread of its own and reports straight into the
/// process's packs.
pub fn sync() {
    let requests = super::take_requests();
    if requests.is_empty() {
        return;
    }
    let Some((cache, game_root)) = PLACES.lock().unwrap_or_else(|p| p.into_inner()).clone() else {
        for r in requests {
            if let Request::Download { name } = r {
                super::set_failed(&name, "packs were never initialised");
            }
        }
        return;
    };
    for req in requests {
        match req {
            Request::Download { name } => {
                let (entry, source) = super::with(|p| (p.entry(&name), p.source(&name)));
                let (Some(entry), Some(source)) = (entry, source) else {
                    super::set_failed(&name, "not in this build's manifest");
                    continue;
                };
                let (cache, game_root) = (cache.clone(), game_root.clone());
                std::thread::spawn(move || {
                    log::info!("[packs] downloading '{name}' from {source} ({} bytes)", entry.size);
                    let mut report = |f: f32| super::set_progress(&name, f);
                    match install(&entry, &source, &cache, &game_root, &mut report) {
                        Ok(dir) => {
                            super::set_installed(&name, &entry.sha256, Some(&dir), false);
                            log::info!("[packs] '{name}' installed ({})", dir.display());
                        }
                        Err(e) => super::set_failed(&name, &format!("{e:#}")),
                    }
                });
            }
            Request::Remove { name } => {
                let Some(entry) = super::with(|p| p.entry(&name)) else { continue };
                let dir = pack_dir(&cache, &entry);
                std::thread::spawn(move || {
                    if dir.exists()
                        && let Err(e) = std::fs::remove_dir_all(&dir)
                    {
                        log::warn!("[packs] could not delete {}: {e}", dir.display());
                    }
                    log::info!("[packs] '{name}' removed");
                });
            }
        }
    }
}

/// Fetch `entry`'s archive from `source`, check it, and unpack it into its
/// folder under `cache`. Returns the folder. Already there, it is returned
/// at once. `progress` is told 0..0.95 as the bytes arrive.
pub fn install(
    entry: &PackEntry,
    source: &str,
    cache: &Path,
    game_root: &Path,
    progress: &mut dyn FnMut(f32),
) -> anyhow::Result<PathBuf> {
    use sha2::Digest;
    anyhow::ensure!(super::valid_name(&entry.name), "'{}' is not a pack name", entry.name);
    let dir = pack_dir(cache, entry);
    if dir.join(COMPLETE).is_file() {
        return Ok(dir);
    }
    std::fs::create_dir_all(cache)?;
    let part = cache.join(format!("{}-download.part", entry.name));
    let mut reader = open(source, game_root)?;
    let mut out = std::fs::File::create(&part)?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut got: u64 = 0;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        got += n as u64;
        anyhow::ensure!(got <= entry.size, "{source} is larger than the {} bytes the manifest says", entry.size);
        progress(got as f32 / entry.size.max(1) as f32 * 0.95);
    }
    out.flush()?;
    drop(out);
    let check = || -> anyhow::Result<()> {
        anyhow::ensure!(got == entry.size, "{source} is {got} bytes, the manifest says {}", entry.size);
        let hash = super::hex(&hasher.finalize());
        anyhow::ensure!(hash == entry.sha256, "{source} does not match its SHA-256 (a stale or corrupt copy?)");
        Ok(())
    };
    if let Err(e) = check() {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    let tmp = cache.join(format!("{}-unpacking.tmp", entry.name));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp)?;
    }
    let written = archive::extract(&part, &tmp);
    let _ = std::fs::remove_file(&part);
    let written = written?;
    for f in &entry.files {
        anyhow::ensure!(written.iter().any(|w| w == &super::normalize(f)), "the archive is missing '{f}'");
    }
    std::fs::write(tmp.join(COMPLETE), &entry.sha256)?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::rename(&tmp, &dir)?;
    Ok(dir)
}

/// The archive's bytes: over HTTP, or from a file (`file://`, absolute, or
/// relative to the game).
fn open(source: &str, game_root: &Path) -> anyhow::Result<Box<dyn Read + Send>> {
    if source.starts_with("http://") || source.starts_with("https://") {
        let Some(http) = HTTP.get() else {
            anyhow::bail!("this runtime has no HTTP client for {source}");
        };
        return Ok(http(source)?.0);
    }
    let path = source.strip_prefix("file://").unwrap_or(source);
    let path = Path::new(path);
    let path = if path.is_absolute() { path.to_path_buf() } else { game_root.join(path) };
    let f = std::fs::File::open(&path).map_err(|e| anyhow::anyhow!("opening {}: {e}", path.display()))?;
    Ok(Box::new(std::io::BufReader::new(f)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jengine-packs-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn built(dir: &Path) -> PackEntry {
        let bytes = archive::encode(&[
            ("sprites/forest/tree.png".to_string(), vec![9; 300]),
            ("scenes/forest/a.scene".to_string(), b"{\"version\":1}".to_vec()),
        ]);
        let sha = super::super::sha256_hex(&bytes);
        let file = format!("forest-{}.jpak", &sha[..12]);
        std::fs::create_dir_all(dir.join("packs")).unwrap();
        std::fs::write(dir.join("packs").join(&file), &bytes).unwrap();
        PackEntry {
            name: "forest".into(),
            file,
            size: bytes.len() as u64,
            sha256: sha,
            files: vec!["sprites/forest/tree.png".into(), "scenes/forest/a.scene".into()],
        }
    }

    #[test]
    fn a_pack_installs_from_a_folder_and_is_found_again() {
        let game = tmp("install");
        let entry = built(&game);
        let cache = game.join("cache");
        let mut seen = Vec::new();
        let source = format!("packs/{}", entry.file);
        let dir = install(&entry, &source, &cache, &game, &mut |f| seen.push(f)).unwrap();
        assert_eq!(std::fs::read(dir.join("sprites/forest/tree.png")).unwrap(), vec![9; 300]);
        assert!(dir.join(COMPLETE).is_file());
        assert!(seen.last().is_some_and(|&f| f > 0.9), "progress reached the end: {seen:?}");
        assert!(!cache.join("forest-download.part").exists(), "no download left behind");
        // A second time is the copy already there.
        let again = install(&entry, "nowhere/at/all.jpak", &cache, &game, &mut |_| {}).unwrap();
        assert_eq!(again, dir);
        let _ = std::fs::remove_dir_all(&game);
    }

    #[test]
    fn a_corrupt_archive_is_refused_and_leaves_nothing() {
        let game = tmp("corrupt");
        let mut entry = built(&game);
        entry.sha256 = "f".repeat(64);
        let cache = game.join("cache");
        let err = install(&entry, &format!("packs/{}", entry.file), &cache, &game, &mut |_| {}).unwrap_err();
        assert!(format!("{err:#}").contains("SHA-256"), "{err:#}");
        assert!(!pack_dir(&cache, &entry).exists());
        let mut short = built(&game);
        short.size += 1;
        assert!(install(&short, &format!("packs/{}", short.file), &cache, &game, &mut |_| {}).is_err());
        let _ = std::fs::remove_dir_all(&game);
    }

    #[test]
    fn a_missing_archive_fails_with_its_path() {
        let game = tmp("missing");
        let entry = built(&game);
        let err = install(&entry, "packs/none.jpak", &game.join("cache"), &game, &mut |_| {}).unwrap_err();
        assert!(format!("{err:#}").contains("none.jpak"));
        let _ = std::fs::remove_dir_all(&game);
    }
}
