//! **Live ops over real HTTP** - an operator setting a channel up the way
//! the dashboard does, and a game asking the way the engine does: config,
//! an experiment, a live event, a content release and its archive, a
//! rollback and a promotion, every snapshot checked against the public key.

use std::path::PathBuf;

use jengine_liveops::{Envelope, PackEntry, PackManifest, PublicKey, Snapshot};
use jengine_server::{LiveOps, Role, Running, Server};
use serde_json::{Value, json};

const TOKEN: &str = "ops-secret";
const VIEWER: &str = "just-looking";

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jengine-liveops-http-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn start(dir: &PathBuf) -> Running {
    Server::new()
        .admin_token(TOKEN)
        .operator("vera", VIEWER, Role::Viewer)
        .liveops(LiveOps::open(dir).unwrap())
        .start("127.0.0.1:0")
        .unwrap()
}

fn send(req: ureq::Request, body: Option<&Value>) -> (u16, Value) {
    let r = match body {
        Some(b) => req.set("Content-Type", "application/json").send_string(&b.to_string()),
        None => req.call(),
    };
    let resp = match r {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => panic!("no answer: {e}"),
    };
    let status = resp.status();
    let text = resp.into_string().unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

fn admin(running: &Running, method: &str, path: &str, token: &str, body: Option<Value>) -> (u16, Value) {
    let req = ureq::request(method, &format!("{}{path}", running.http_url())).set("Authorization", &format!("Bearer {token}"));
    send(req, body.as_ref())
}

fn ok(running: &Running, method: &str, path: &str, body: Option<Value>) -> Value {
    let (s, v) = admin(running, method, path, TOKEN, body);
    assert_eq!(s, 200, "{method} {path}: {v}");
    v
}

/// What the engine does: ask for a snapshot and believe it only if signed.
fn ask(running: &Running, channel: &str, install: &str, platform: &str, key: &PublicKey) -> Snapshot {
    let url = jengine_liveops::snapshot_url(
        &running.http_url(),
        channel,
        &jengine_liveops::Query { install: install.into(), player: String::new(), platform: platform.into(), app_version: "1.4.0".into() },
    );
    let resp = ureq::get(&url).call().unwrap();
    assert_eq!(resp.header("access-control-allow-origin"), Some("*"), "a web build is served from somewhere else");
    assert_eq!(resp.header("cache-control"), Some("no-store"));
    let env: Envelope = serde_json::from_str(&resp.into_string().unwrap()).unwrap();
    key.verify(&env).expect("signed by the service's key")
}

fn sha(bytes: &[u8]) -> String {
    use sha2::Digest;
    jengine_liveops::to_hex(&sha2::Sha256::digest(bytes))
}

#[test]
fn an_operator_sets_up_a_channel_and_a_game_reads_it() {
    let d = dir("main");
    let running = start(&d);
    let overview = ok(&running, "GET", "/admin/api/liveops", None);
    let key_text = overview["public_key"].as_str().unwrap().to_string();
    let key = PublicKey::parse(&key_text).unwrap();
    assert_eq!(ureq::get(&format!("{}/liveops/v1/key", running.http_url())).call().unwrap().into_string().unwrap(), key_text);
    assert_eq!(overview["channels"].as_array().unwrap().len(), 3);
    let page = ureq::get(&format!("{}/admin/liveops", running.http_url())).call().unwrap().into_string().unwrap();
    assert!(page.contains("<html"), "the dashboard page");

    // Config with a rule, an experiment and an event.
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let save = json!({
        "revision": 0,
        "params": {
            "coins_per_win": { "value": 10, "rules": [{ "name": "phones", "when": { "platforms": ["android", "ios"] }, "value": 12 }] },
            "welcome": { "value": "Hello" },
            "shop_layout": { "value": "grid" }
        },
        "experiments": [{
            "id": "shop", "status": "running",
            "variants": [
                { "name": "grid", "weight": 1, "values": {} },
                { "name": "list", "weight": 1, "values": { "shop_layout": "list" } }
            ]
        }],
        "events": [{ "id": "weekend", "name": "Double coins", "start": now - 60, "end": now + 3600, "values": { "coins_per_win": 20 } }]
    });
    let saved = ok(&running, "PUT", "/admin/api/liveops/channel/live", Some(save.clone()));
    assert_eq!(saved["channel"]["revision"], 1);

    // Somebody else saving over it from the same revision is refused.
    let (s, why) = admin(&running, "PUT", "/admin/api/liveops/channel/live", TOKEN, Some(save.clone()));
    assert_eq!(s, 409, "{why}");
    // A viewer may look and not change.
    assert_eq!(admin(&running, "GET", "/admin/api/liveops/channel/live", VIEWER, None).0, 200);
    let mut again = save.clone();
    again["revision"] = json!(1);
    assert_eq!(admin(&running, "PUT", "/admin/api/liveops/channel/live", VIEWER, Some(again)).0, 403);
    // Nonsense is refused with a reason.
    let (s, why) = admin(&running, "PUT", "/admin/api/liveops/channel/live", TOKEN, Some(json!({ "revision": 1, "events": [{ "id": "x", "start": 5, "end": 1 }] })));
    assert_eq!(s, 400);
    assert!(why["error"].as_str().unwrap().contains("ends before"), "{why}");

    let pc = ask(&running, "live", "install-1", "windows", &key);
    assert_eq!(pc.config["coins_per_win"], json!(20), "the weekend event is on");
    assert_eq!(pc.config["welcome"], json!("Hello"));
    assert_eq!(pc.events[0].id, "weekend");
    assert_eq!(pc.revision, 1);
    let variant = pc.experiments["shop"].clone();
    assert_eq!(pc.config["shop_layout"], json!(variant), "the variant's value");
    assert_eq!(ask(&running, "live", "install-1", "windows", &key).experiments["shop"], variant, "and the same next time");
    // Before the event, as the preview tells an operator: the phone rule.
    let at = (now - 3600).to_string();
    let req = ureq::get(&format!("{}/admin/api/liveops/channel/live/preview?install=i&platform=android&version=1.0", running.http_url()))
        .set("Authorization", &format!("Bearer {TOKEN}"))
        .set("x-liveops-at", &at);
    let (s, preview) = send(req, None);
    assert_eq!(s, 200);
    assert_eq!(preview["config"]["coins_per_win"], json!(12));
    assert_eq!(preview["next_change"], json!(now - 60), "ask again when the event starts");
    // Nobody can read a channel that does not exist.
    assert_eq!(ureq::get(&format!("{}/liveops/v1/nope/snapshot?install=a", running.http_url())).call().map(|_| 200).unwrap_or_else(|e| match e { ureq::Error::Status(c, _) => c, _ => 0 }), 404);

    // A content release: the archive first, then the manifest naming it.
    let archive = b"JPAK pretend archive bytes".to_vec();
    let entry = PackEntry { name: "forest".into(), file: format!("forest-{}.jpak", &sha(&archive)[..12]), size: archive.len() as u64, sha256: sha(&archive), files: vec!["scenes/forest.scene".into()] };
    let manifest = PackManifest { version: 1, url: String::new(), packs: vec![entry.clone()] };
    let (s, why) = admin(&running, "POST", "/admin/api/liveops/channel/live/release", TOKEN, Some(json!({ "manifest": manifest })));
    assert_eq!(s, 400, "an archive nobody uploaded: {why}");
    let up = ureq::put(&format!("{}/admin/api/liveops/blob/{}?sha256={}", running.http_url(), entry.file, entry.sha256))
        .set("Authorization", &format!("Bearer {TOKEN}"))
        .send_bytes(&archive)
        .unwrap();
    assert_eq!(up.status(), 200);
    let wrong = ureq::put(&format!("{}/admin/api/liveops/blob/x.jpak?sha256=00", running.http_url()))
        .set("Authorization", &format!("Bearer {TOKEN}"))
        .send_bytes(&archive);
    assert!(matches!(wrong, Err(ureq::Error::Status(400, _))), "bytes that do not hash to what was said");
    let r1 = ok(&running, "POST", "/admin/api/liveops/channel/live/release", Some(json!({ "manifest": manifest, "notes": "the forest" })));
    assert_eq!(r1["release"]["number"], 1);

    let snap = ask(&running, "live", "install-1", "windows", &key);
    let content = snap.content.expect("a release");
    assert_eq!(content.release, 1);
    assert_eq!(content.manifest.packs, vec![entry.clone()]);
    assert_eq!(content.manifest.url, format!("{}/liveops/v1/packs/", running.http_url()), "fetched from the service");
    let got = ureq::get(&format!("{}{}", content.manifest.url, entry.file)).call().unwrap();
    assert_eq!(got.header("cache-control"), Some("public, max-age=31536000, immutable"));
    assert_eq!(got.header("access-control-allow-origin"), Some("*"));
    let etag = got.header("etag").unwrap().to_string();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut got.into_reader(), &mut bytes).unwrap();
    assert_eq!(bytes, archive);
    assert_eq!(sha(&bytes), entry.sha256, "what the signed manifest says");
    let cached = ureq::get(&format!("{}{}", content.manifest.url, entry.file)).set("If-None-Match", &etag).call().unwrap();
    assert_eq!(cached.status(), 304);

    // Release 2 empties it; a rollback to 1 is release 3, and games take it.
    ok(&running, "POST", "/admin/api/liveops/channel/live/release", Some(json!({ "manifest": { "version": 1, "packs": [] } })));
    assert!(ask(&running, "live", "install-1", "windows", &key).content.unwrap().manifest.packs.is_empty());
    let r3 = ok(&running, "POST", "/admin/api/liveops/channel/live/rollback", Some(json!({ "release": 1 })));
    assert_eq!(r3["release"]["number"], 3);
    let back = ask(&running, "live", "install-1", "windows", &key);
    assert_eq!(back.content.unwrap().manifest.packs, vec![entry.clone()]);
    assert!(back.revision > snap.revision, "every change raises the revision, a rollback too");

    // Promote live's config and content to staging.
    ok(&running, "POST", "/admin/api/liveops/promote", Some(json!({ "from": "live", "to": "staging", "parts": ["config", "content"] })));
    let staged = ask(&running, "staging", "install-1", "windows", &key);
    assert_eq!(staged.config["welcome"], json!("Hello"));
    assert!(staged.events.is_empty(), "events were not promoted");
    assert_eq!(staged.content.unwrap().manifest.packs, vec![entry]);

    // Everything an operator did is in the event log, by name.
    let events = ok(&running, "GET", "/admin/api/events", None);
    let text = events.to_string();
    assert!(text.contains("admin: published release 1 on live"), "{text}");
    assert!(text.contains("admin: rolled live back to release 1"));

    // And it all survives a restart, key included.
    running.stop();
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn without_an_admin_token_games_are_served_but_nothing_can_change() {
    let d = dir("readonly");
    let running = Server::new().liveops(LiveOps::open(&d).unwrap()).start("127.0.0.1:0").unwrap();
    let r = ureq::get(&format!("{}/liveops/v1/live/snapshot?install=a", running.http_url())).call().unwrap();
    let env: Envelope = serde_json::from_str(&r.into_string().unwrap()).unwrap();
    assert!(env.payload.contains("\"channel\":\"live\""));
    let no = ureq::get(&format!("{}/admin/api/liveops", running.http_url())).call();
    assert!(matches!(no, Err(ureq::Error::Status(404, _))), "no admin surface at all");
    running.stop();
    let _ = std::fs::remove_dir_all(&d);
}

/// L4: snapshots share the `/api/` budget per address, so one address asking
/// in a loop is told to wait (429 with `Retry-After`) and a game asking every
/// few minutes never is.
#[test]
fn an_address_asking_in_a_loop_is_told_to_wait() {
    let d = dir("rate");
    let running = Server::new()
        .config(jengine_server::Config { api_requests_per_second: 2.0, ..Default::default() })
        .liveops(LiveOps::open(&d).unwrap())
        .start("127.0.0.1:0")
        .unwrap();
    let url = format!("{}/liveops/v1/live/snapshot?install=a", running.http_url());
    let statuses: Vec<u16> = (0..10)
        .map(|_| match ureq::get(&url).call() {
            Ok(r) => r.status(),
            Err(ureq::Error::Status(code, r)) => {
                assert_eq!(r.header("Retry-After"), Some("1"));
                code
            }
            Err(e) => panic!("{e}"),
        })
        .collect();
    assert_eq!(statuses[0], 200);
    assert!(statuses.contains(&429), "{statuses:?}");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(ureq::get(&url).call().map(|r| r.status()).unwrap_or(0), 200, "and served again after a pause");
    running.stop();
    let _ = std::fs::remove_dir_all(d);
}

/// L5, L6, L9 over HTTP: a rule asks the game's language and attributes;
/// a game reports what an install saw and bought, counted by the variant
/// the service gave it; and an operator limited to `dev` cannot touch
/// `live`.
#[test]
fn targeting_results_and_channel_roles_over_http() {
    let d = dir("targeting");
    let running = start(&d);
    let key = PublicKey::parse(ok(&running, "GET", "/admin/api/liveops", None)["public_key"].as_str().unwrap()).unwrap();
    let rev = ok(&running, "GET", "/admin/api/liveops/channel/live", None)["channel"]["revision"].as_u64().unwrap();
    ok(&running, "PUT", "/admin/api/liveops/channel/live", Some(json!({
        "revision": rev,
        "params": { "greeting": { "value": "hello", "rules": [
            { "when": { "languages": ["ja"] }, "value": "konnichiwa" },
            { "when": { "attributes": { "band": ["20-29"] } }, "value": "welcome back, veteran" },
        ] } },
        "experiments": [{ "id": "price", "status": "running", "variants": [
            { "name": "low", "weight": 1, "values": {} }, { "name": "high", "weight": 1, "values": {} },
        ] }],
    })));
    let q = jengine_liveops::Query { install: "i-7".into(), player: String::new(), platform: "web".into(), app_version: "1.0".into() };
    let snap = |lang: &str, attrs: &[(&str, &str)]| -> Snapshot {
        let url = jengine_liveops::snapshot_url_with(&running.http_url(), "live", &q, lang, "", attrs);
        let env: Envelope = ureq::get(&url).call().unwrap().into_json().unwrap();
        key.verify(&env).unwrap()
    };
    assert_eq!(snap("", &[]).config["greeting"], "hello", "an engine that says nothing more gets the default");
    assert_eq!(snap("ja-JP", &[]).config["greeting"], "konnichiwa");
    assert_eq!(snap("en", &[("band", "20-29")]).config["greeting"], "welcome back, veteran");

    let variant = snap("", &[]).experiments["price"].clone();
    let url = jengine_liveops::exposure_url(&running.http_url(), "live", &q);
    for body in [json!({ "experiment": "price" }), json!({ "experiment": "price", "event": "purchase", "value": 2.5 })] {
        let (status, said) = send(ureq::post(&url), Some(&body));
        assert_eq!((status, said["variant"].clone()), (200, json!(variant)), "{said}");
    }
    let (status, _) = send(ureq::post(&url), Some(&json!({ "experiment": "nothing-running" })));
    assert_eq!(status, 400);
    let results = ok(&running, "GET", "/admin/api/liveops/channel/live/results", None);
    assert_eq!(results["results"]["price"][variant.as_str()]["exposed"]["count"], 1);
    assert_eq!(results["results"]["price"][variant.as_str()]["purchase"]["sum"], 2.5);

    // An operator for dev only.
    let made = ok(&running, "POST", "/admin/api/operators", Some(json!({ "name": "devon", "role": "operator", "channels": ["dev"] })));
    let devon = made["token"].as_str().unwrap().to_string();
    let rev = ok(&running, "GET", "/admin/api/liveops/channel/live", None)["channel"]["revision"].as_u64().unwrap();
    let (status, why) = admin(&running, "PUT", "/admin/api/liveops/channel/live", &devon, Some(json!({ "revision": rev })));
    assert_eq!(status, 403, "{why}");
    let dev_rev = ok(&running, "GET", "/admin/api/liveops/channel/dev", None)["channel"]["revision"].as_u64().unwrap();
    let (status, why) = admin(&running, "PUT", "/admin/api/liveops/channel/dev", &devon, Some(json!({ "revision": dev_rev })));
    assert_eq!(status, 200, "{why}");
    let (status, _) = admin(&running, "POST", "/admin/api/liveops/promote", &devon, Some(json!({ "from": "dev", "to": "live", "parts": ["config"] })));
    assert_eq!(status, 403, "promoting into live is changing live");
    running.stop();
    let _ = std::fs::remove_dir_all(&d);
}
