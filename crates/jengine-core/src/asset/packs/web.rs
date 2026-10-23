//! Packs in a browser: the archive is fetched whole, kept as a JavaScript
//! buffer - outside wasm memory, which never shrinks - and each file is
//! sliced out of it when the runtime reads it ([`read`]). The archive is
//! also put in the Cache API under its URL, so a page opened again mounts it
//! at boot ([`init`]) with no download; its URL carries its hash, so a new
//! build's pack can never be served from an old copy, and the old copy is
//! deleted once no pack names it.
//!
//! The browser APIs are reached through `Reflect` rather than web-sys
//! bindings, as `display::web` and the haptics are, so a web-sys release
//! that renames a binding cannot break the build. The Cache API and
//! `crypto.subtle` exist only in a secure context (`https`, `localhost`);
//! without them a pack still downloads and mounts, unverified and for the
//! session only, and says so in the log.

use std::cell::RefCell;
use std::collections::HashMap;

use js_sys::{Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use super::{PackEntry, PackManifest, PackSettings, Request, archive};

const CACHE_NAME: &str = "jengine-packs";

struct Mounted {
    pack: String,
    archive: Uint8Array,
    offset: u32,
    len: u32,
}

thread_local! {
    /// Asset path -> where its bytes are, for every mounted pack.
    static FILES: RefCell<HashMap<String, Mounted>> = RefCell::new(HashMap::new());
    /// Blob URLs made for videos, kept so each is made once.
    static BLOBS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

fn get(obj: &JsValue, key: &str) -> JsValue {
    Reflect::get(obj, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

fn call(obj: &JsValue, method: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let f: js_sys::Function = get(obj, method).dyn_into().map_err(|_| JsValue::from_str(&format!("no {method}")))?;
    let arr = js_sys::Array::new();
    for a in args {
        arr.push(a);
    }
    Reflect::apply(&f, obj, &arr)
}

async fn wait(p: JsValue) -> Result<JsValue, JsValue> {
    JsFuture::from(js_sys::Promise::from(p)).await
}

fn window() -> JsValue {
    js_sys::global().into()
}

/// The page's pack cache, if this is a secure context.
async fn open_cache() -> Option<JsValue> {
    let caches = get(&window(), "caches");
    if caches.is_undefined() || caches.is_null() {
        return None;
    }
    wait(call(&caches, "open", &[JsValue::from_str(CACHE_NAME)]).ok()?).await.ok()
}

fn describe(e: &JsValue) -> String {
    e.as_string().or_else(|| get(e, "message").as_string()).unwrap_or_else(|| format!("{e:?}"))
}

/// Set this game's packs from its manifest (the text of `assets/packs.json`,
/// if the build has one) or, with none, the project's declared packs; lay
/// over it the live ops release the game last believed (`live`); mount every
/// pack the Cache API still holds - the current copy, or the one a release
/// replaced, `outdated`; and delete cached archives no pack names any more.
/// A host awaits this at boot, before scripts run, so a script never sees a
/// cached pack as remote.
pub async fn init(manifest_text: Option<String>, settings: &PackSettings, live: Option<&jengine_liveops::Content>) {
    let manifest: Option<PackManifest> = manifest_text.and_then(|t| match serde_json::from_str(&t) {
        Ok(m) => Some(m),
        Err(e) => {
            log::warn!("[packs] {} is not valid: {e}", super::MANIFEST_FILE);
            None
        }
    });
    let built = manifest.is_some();
    super::configure(manifest, settings);
    if let Some(c) = live {
        super::apply_live(c.release, &c.manifest);
    }
    if !built && live.is_none() {
        return;
    }
    let Some(cache) = open_cache().await else {
        log::info!("[packs] no Cache API here (not a secure context?) - packs last the session only");
        return;
    };
    let mut wanted = Vec::new();
    for name in super::names() {
        let (Some(entry), Some(url), previous) = super::with(|p| (p.entry(&name), p.source(&name), p.previous(&name))) else { continue };
        wanted.push(url.clone());
        if restore(&cache, &entry, &url).await {
            log::info!("[packs] '{name}' is installed (cached)");
            continue;
        }
        if let Some((old, old_url)) = previous {
            wanted.push(old_url.clone());
            if restore(&cache, &old, &old_url).await {
                log::info!("[packs] '{name}' is outdated - the live release has a newer copy");
            }
        }
    }
    prune(&cache, &wanted).await;
}

/// Mount `entry` from the Cache API's copy at `url`, if it has one.
async fn restore(cache: &JsValue, entry: &PackEntry, url: &str) -> bool {
    let Ok(found) = wait(call(cache, "match", &[JsValue::from_str(url)]).unwrap_or(JsValue::UNDEFINED)).await else { return false };
    if found.is_undefined() || found.is_null() {
        return false;
    }
    let Ok(buf) = wait(call(&found, "arrayBuffer", &[]).unwrap_or(JsValue::UNDEFINED)).await else { return false };
    match mount(entry, Uint8Array::new(&buf)) {
        Ok(()) => {
            super::set_installed(&entry.name, &entry.sha256, None, true);
            true
        }
        Err(e) => {
            log::warn!("[packs] the cached '{}' is unusable: {e:#}", entry.name);
            false
        }
    }
}

/// Delete cached archives whose URL no pack of this build names.
async fn prune(cache: &JsValue, wanted: &[String]) {
    let Ok(keys) = wait(call(cache, "keys", &[]).unwrap_or(JsValue::UNDEFINED)).await else { return };
    let keys = js_sys::Array::from(&keys);
    for req in keys.iter() {
        let url = get(&req, "url").as_string().unwrap_or_default();
        if !url.ends_with(".jpak") || wanted.iter().any(|w| url.ends_with(w.trim_start_matches("./"))) {
            continue;
        }
        let _ = call(cache, "delete", &[req]);
        log::info!("[packs] deleted the stale cached {url}");
    }
}

/// Mount `bytes`, the whole archive of `entry`.
fn mount(entry: &PackEntry, bytes: Uint8Array) -> anyhow::Result<()> {
    let total = bytes.length() as u64;
    anyhow::ensure!(total >= archive::HEADER_LEN as u64, "not a pack archive (too short)");
    let head = bytes.subarray(0, archive::HEADER_LEN as u32).to_vec();
    let end = archive::index_end(&head)?;
    anyhow::ensure!(end as u64 <= total, "pack archive index cut short");
    let index_bytes = bytes.subarray(0, end as u32).to_vec();
    let (index, data) = archive::read_index(&index_bytes, total)?;
    FILES.with(|files| {
        let mut files = files.borrow_mut();
        files.retain(|_, m| m.pack != entry.name);
        for f in index.files {
            files.insert(
                super::normalize(&f.path),
                Mounted { pack: entry.name.clone(), archive: bytes.clone(), offset: (data + f.offset) as u32, len: f.len as u32 },
            );
        }
    });
    Ok(())
}

fn unmount(name: &str) {
    FILES.with(|f| f.borrow_mut().retain(|_, m| m.pack != name));
}

/// Start what scripts asked for. Each host calls it once a frame.
pub fn sync() {
    for req in super::take_requests() {
        match req {
            Request::Download { name } => {
                let (entry, url) = super::with(|p| (p.entry(&name), p.source(&name)));
                let (Some(entry), Some(url)) = (entry, url) else {
                    super::set_failed(&name, "not in this build's manifest");
                    continue;
                };
                wasm_bindgen_futures::spawn_local(async move {
                    log::info!("[packs] downloading '{name}' from {url} ({} bytes)", entry.size);
                    match download(&entry, &url).await {
                        Ok(()) => {
                            super::set_installed(&name, &entry.sha256, None, false);
                            log::info!("[packs] '{name}' installed");
                        }
                        Err(e) => super::set_failed(&name, &e),
                    }
                });
            }
            Request::Remove { name } => {
                unmount(&name);
                let url = super::with(|p| p.source(&name));
                wasm_bindgen_futures::spawn_local(async move {
                    if let (Some(cache), Some(url)) = (open_cache().await, url) {
                        let _ = call(&cache, "delete", &[JsValue::from_str(&url)]);
                    }
                    log::info!("[packs] '{name}' removed");
                });
            }
        }
    }
}

async fn download(entry: &PackEntry, url: &str) -> Result<(), String> {
    let resp = wait(call(&window(), "fetch", &[JsValue::from_str(url)]).map_err(|e| describe(&e))?)
        .await
        .map_err(|e| format!("fetch {url}: {}", describe(&e)))?;
    if !get(&resp, "ok").as_bool().unwrap_or(false) {
        return Err(format!("fetch {url}: HTTP {}", get(&resp, "status").as_f64().unwrap_or(0.0)));
    }
    let size = entry.size as u32;
    let buffer = Uint8Array::new_with_length(size);
    let body = get(&resp, "body");
    let mut got: u32 = 0;
    if body.is_null() || body.is_undefined() {
        // No streams: the whole body at once, with no progress on the way.
        let buf = wait(call(&resp, "arrayBuffer", &[]).map_err(|e| describe(&e))?).await.map_err(|e| describe(&e))?;
        let all = Uint8Array::new(&buf);
        if all.length() != size {
            return Err(format!("{url} is {} bytes, the manifest says {size}", all.length()));
        }
        buffer.set(&all, 0);
        got = size;
    } else {
        let reader = call(&body, "getReader", &[]).map_err(|e| describe(&e))?;
        loop {
            let chunk = wait(call(&reader, "read", &[]).map_err(|e| describe(&e))?).await.map_err(|e| format!("reading {url}: {}", describe(&e)))?;
            if get(&chunk, "done").as_bool().unwrap_or(true) {
                break;
            }
            let value: Uint8Array = get(&chunk, "value").unchecked_into();
            let n = value.length();
            if got as u64 + n as u64 > size as u64 {
                return Err(format!("{url} is larger than the {size} bytes the manifest says"));
            }
            buffer.set(&value, got);
            got += n;
            super::set_progress(&entry.name, got as f32 / size.max(1) as f32 * 0.95);
        }
    }
    if got != size {
        return Err(format!("{url} is {got} bytes, the manifest says {size}"));
    }
    match sha256(&buffer).await {
        Some(hash) if hash != entry.sha256 => return Err(format!("{url} does not match its SHA-256 (a stale or corrupt copy?)")),
        Some(_) => {}
        None => log::info!("[packs] no crypto.subtle here - '{}' is not verified", entry.name),
    }
    mount(entry, buffer.clone()).map_err(|e| format!("{e:#}"))?;
    if let Some(cache) = open_cache().await {
        let ctor: js_sys::Function = get(&window(), "Response").unchecked_into();
        let response = js_sys::Reflect::construct(&ctor, &js_sys::Array::of1(&buffer));
        if let Ok(response) = response {
            if let Ok(p) = call(&cache, "put", &[JsValue::from_str(url), response]) {
                let _ = wait(p).await;
            }
        }
    }
    Ok(())
}

/// The SHA-256 of `bytes` by `crypto.subtle`, or `None` where there is none.
async fn sha256(bytes: &Uint8Array) -> Option<String> {
    let subtle = get(&get(&window(), "crypto"), "subtle");
    if subtle.is_undefined() || subtle.is_null() {
        return None;
    }
    let digest = wait(call(&subtle, "digest", &[JsValue::from_str("SHA-256"), bytes.clone().into()]).ok()?).await.ok()?;
    Some(super::hex(&Uint8Array::new(&digest).to_vec()))
}

/// A mounted pack's copy of asset `path` (an id, or an `assets/...` URL).
/// The web runtime's fetch helpers ask this before the network.
pub fn read(path: &str) -> Option<Vec<u8>> {
    FILES.with(|f| {
        let f = f.borrow();
        let m = f.get(&super::normalize(path))?;
        Some(m.archive.subarray(m.offset, m.offset + m.len).to_vec())
    })
}

/// What the packs mounted since the last call brought for the runtime to
/// keep in memory ([`super::content::take_arrived`]).
pub fn arrived() -> Vec<super::content::PackContent> {
    super::content::take_arrived(read)
}

/// A Blob URL for a mounted file, for what the browser loads by URL itself
/// (a `<video>`). Made once per file.
pub fn blob_url(path: &str) -> Option<String> {
    let key = super::normalize(path);
    if let Some(url) = BLOBS.with(|b| b.borrow().get(&key).cloned()) {
        return Some(url);
    }
    let part = FILES.with(|f| {
        let f = f.borrow();
        let m = f.get(&key)?;
        Some(m.archive.subarray(m.offset, m.offset + m.len))
    })?;
    let blob = web_sys::Blob::new_with_u8_array_sequence(&js_sys::Array::of1(&part)).ok()?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).ok()?;
    BLOBS.with(|b| b.borrow_mut().insert(key, url.clone()));
    Some(url)
}
