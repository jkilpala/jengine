//! Live ops in a browser - a web build and the editor's Play: this install's
//! id and the last snapshot believed in `localStorage`, and a `fetch` per
//! ask. The service answers any origin (`Access-Control-Allow-Origin: *`),
//! since a web build and the editor are served from elsewhere.
//!
//! `localStorage` is per origin, so its keys carry the service's address
//! and the channel: two games on one origin keep their own answers (they
//! share an install id, which only buckets).

use js_sys::Reflect;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use super::{LiveOpsSettings, with};

fn storage() -> Option<JsValue> {
    let s = Reflect::get(&js_sys::global(), &JsValue::from_str("localStorage")).ok()?;
    (!s.is_undefined() && !s.is_null()).then_some(s)
}

fn call(obj: &JsValue, method: &str, args: &[JsValue]) -> Option<JsValue> {
    let f: js_sys::Function = Reflect::get(obj, &JsValue::from_str(method)).ok()?.dyn_into().ok()?;
    let arr = js_sys::Array::new();
    for a in args {
        arr.push(a);
    }
    Reflect::apply(&f, obj, &arr).ok()
}

fn load(key: &str) -> Option<String> {
    // A private window may refuse storage outright; then nothing is kept.
    call(&storage()?, "getItem", &[JsValue::from_str(key)])?.as_string()
}

fn keep(key: &str, value: &str) {
    if let Some(s) = storage() {
        let _ = call(&s, "setItem", &[JsValue::from_str(key), JsValue::from_str(value)]);
    }
}

fn cache_key(url: &str, channel: &str) -> String {
    format!("jengine-liveops:{url}:{channel}")
}

/// This browser's install id, made once and kept.
pub fn install_id() -> String {
    const KEY: &str = "jengine-liveops-install";
    if let Some(id) = load(KEY).filter(|s| !s.is_empty()) {
        return id;
    }
    let mut hex = String::new();
    for _ in 0..6 {
        hex.push_str(&format!("{:04x}", (js_sys::Math::random() * 65536.0) as u32));
    }
    let id = format!("i-{hex}");
    keep(KEY, &id);
    id
}

/// Set live ops up from the project's `settings`. `editor`: the editor's
/// Play - its channel, platform `editor`, and no content release, since its
/// packs are the project's own files. Believes the snapshot cached last time
/// if it checks out, and returns its content release for the packs.
pub fn init(settings: &serde_json::Value, editor: bool) -> Option<super::Content> {
    let s = LiveOpsSettings::from_settings(settings);
    let platform = if editor { "editor" } else { "web" };
    if s.url.is_empty() {
        with(|l| l.configure(s, "live", "", platform, "", !editor));
        return None;
    }
    let channel = if editor { s.editor_channel() } else { s.game_channel() };
    let version = super::app_version(settings);
    let install = install_id();
    let key = cache_key(&s.url, &channel);
    let now = crate::profiling::unix_time();
    with(|l| {
        l.configure(s, &channel, &install, platform, &version, !editor);
        let cached = load(&key)?;
        l.load_cached(&cached, now)
    })
}

/// Start a fetch if one is due, and send the exposures scripts queued.
/// Each browser host calls it once a frame.
pub fn sync() {
    let now = crate::profiling::unix_time();
    let (due, exposures) = with(|l| {
        l.follow_locale();
        (l.poll(now).map(|u| (u, cache_key(&l.settings.url, l.channel()))), l.take_exposures())
    });
    for e in exposures {
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(err) = post_json(&e.url, &e.body).await {
                log::warn!("[liveops] an exposure was not counted: {err}");
            }
        });
    }
    let Some((url, key)) = due else { return };
    wasm_bindgen_futures::spawn_local(async move {
        let result = fetch_text(&url).await;
        let now = crate::profiling::unix_time();
        if let Some(text) = with(|l| l.deliver(result, now)) {
            keep(&key, &text);
            log::info!("[liveops] believed revision {} from the service", with(|l| l.revision()));
        }
    });
}

/// POST `body` as JSON - the service answers a browser's preflight for it.
async fn post_json(url: &str, body: &str) -> Result<(), String> {
    let describe = |e: JsValue| e.as_string().unwrap_or_else(|| format!("{e:?}"));
    let init = js_sys::Object::new();
    let headers = js_sys::Object::new();
    let _ = Reflect::set(&headers, &JsValue::from_str("Content-Type"), &JsValue::from_str("application/json"));
    let _ = Reflect::set(&init, &JsValue::from_str("method"), &JsValue::from_str("POST"));
    let _ = Reflect::set(&init, &JsValue::from_str("headers"), &headers);
    let _ = Reflect::set(&init, &JsValue::from_str("body"), &JsValue::from_str(body));
    let p = call(&js_sys::global(), "fetch", &[JsValue::from_str(url), init.into()]).ok_or("no fetch here")?;
    let resp = JsFuture::from(js_sys::Promise::from(p)).await.map_err(describe)?;
    let ok = Reflect::get(&resp, &JsValue::from_str("ok")).ok().and_then(|v| v.as_bool()).unwrap_or(false);
    if !ok {
        let status = Reflect::get(&resp, &JsValue::from_str("status")).ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
        let text = match call(&resp, "text", &[]) {
            Some(t) => JsFuture::from(js_sys::Promise::from(t)).await.ok().and_then(|v| v.as_string()).unwrap_or_default(),
            None => String::new(),
        };
        return Err(format!("HTTP {status}: {}", text.chars().take(160).collect::<String>()));
    }
    Ok(())
}

async fn fetch_text(url: &str) -> Result<String, String> {
    let describe = |e: JsValue| e.as_string().unwrap_or_else(|| format!("{e:?}"));
    let p = call(&js_sys::global(), "fetch", &[JsValue::from_str(url)]).ok_or("no fetch here")?;
    let resp = JsFuture::from(js_sys::Promise::from(p)).await.map_err(describe)?;
    let ok = Reflect::get(&resp, &JsValue::from_str("ok")).ok().and_then(|v| v.as_bool()).unwrap_or(false);
    let status = Reflect::get(&resp, &JsValue::from_str("status")).ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
    let text = call(&resp, "text", &[]).ok_or("no body")?;
    let text = JsFuture::from(js_sys::Promise::from(text)).await.map_err(describe)?.as_string().unwrap_or_default();
    if !ok {
        return Err(format!("HTTP {status}: {}", text.chars().take(160).collect::<String>()));
    }
    Ok(text)
}
