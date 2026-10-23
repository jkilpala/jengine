//! **The engine's own live ops client against a real service** -
//! `jengine_core::liveops::native` asking over HTTP as a shipped game does:
//! the defaults before the answer, the service's values after it, the
//! content release laid over the build's packs, the answer cached and
//! believed again at the next start, and a service with another key never
//! believed at all. And what the game says of itself (L13): its language,
//! region and attributes reach the service's rules, and `LiveOps.expose`
//! is counted in the experiment's results.

use std::time::{Duration, Instant};

use jengine_core::asset::packs;
use jengine_core::liveops::{self, Source};
use jengine_server::{LiveOps, Server};
use serde_json::{Value, json};

const TOKEN: &str = "game-test";

fn admin(base: &str, method: &str, path: &str, body: Value) {
    let r = ureq::request(method, &format!("{base}{path}"))
        .set("Authorization", &format!("Bearer {TOKEN}"))
        .send_string(&body.to_string());
    assert!(r.is_ok(), "{method} {path}: {:?}", r.err().map(|e| e.to_string()));
}

/// Run the client's per-frame sync until `done` says so.
fn sync_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        liveops::native::sync();
        assert!(Instant::now() < deadline, "timed out waiting for {what}: {}", liveops::with(|l| l.error().to_string()));
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_game_reads_its_values_and_release_from_the_service() {
    let dir = std::env::temp_dir().join(format!("jengine-liveops-game-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let ops = LiveOps::open(dir.join("ops")).unwrap();
    let key = ops.public_key();
    let running = Server::new().admin_token(TOKEN).liveops(ops).start("127.0.0.1:0").unwrap();
    let base = running.http_url();

    // The service: a value, a rule for this game's version, and a release.
    admin(&base, "PUT", "/admin/api/liveops/channel/live", json!({
        "revision": 0,
        "params": {
            "coins": { "value": 25 },
            "banner": { "value": "old builds", "rules": [{ "when": { "min_version": "2.0" }, "value": "new builds" }] },
            "greeting": { "value": "hello", "rules": [{ "when": { "languages": ["ja"] }, "value": "konnichiwa" }] },
            "nordic": { "value": false, "rules": [{ "when": { "regions": ["FI"] }, "value": true }] },
            "veteran": { "value": false, "rules": [{ "when": { "attributes": { "band": ["20-29"] } }, "value": true }] },
            "mode": { "value": "normal", "rules": [{ "when": { "attributes": { "difficulty": ["hard"] } }, "value": "hard" }] },
        },
        "experiments": [{ "id": "price", "status": "running", "variants": [
            { "name": "low", "weight": 1, "values": {} }, { "name": "high", "weight": 1, "values": {} },
        ] }],
    }));
    let archive = b"pretend archive".to_vec();
    let sha = {
        use sha2::Digest;
        jengine_liveops::to_hex(&sha2::Sha256::digest(&archive))
    };
    let file = format!("winter-{}.jpak", &sha[..12]);
    let r = ureq::put(&format!("{base}/admin/api/liveops/blob/{file}?sha256={sha}")).set("Authorization", &format!("Bearer {TOKEN}")).send_bytes(&archive);
    assert!(r.is_ok());
    admin(&base, "POST", "/admin/api/liveops/channel/live/release", json!({
        "manifest": { "version": 1, "packs": [{ "name": "winter", "file": file, "size": archive.len(), "sha256": sha, "files": ["scenes/winter.scene"] }] }
    }));

    // The game, as the native runner sets it up.
    // SAFETY: this test binary reads these variables only here.
    unsafe {
        std::env::set_var("JENGINE_LIVEOPS_DIR", dir.join("game-liveops"));
        std::env::set_var("JENGINE_PACK_DIR", dir.join("game-packs"));
    }
    packs::native::set_http(Box::new(|url: &str| {
        let r = ureq::get(url).call().map_err(|e| anyhow::anyhow!("GET {url}: {e}"))?;
        Ok((Box::new(r.into_reader()) as Box<dyn std::io::Read + Send>, None))
    }));
    packs::native::set_http_post(Box::new(|url: &str, content_type: &str, body: &[u8]| {
        let r = ureq::post(url).set("Content-Type", content_type).send_bytes(body).map_err(|e| anyhow::anyhow!("POST {url}: {e}"))?;
        Ok(r.into_string()?.into_bytes())
    }));
    // The player's language, as a game that called `Locale.set` has it.
    jengine_core::locale::set_locale("ja-JP");
    let settings = json!({
        "app": { "version": "2.1.0" },
        "liveops": {
            "url": base, "public_key": key, "defaults": { "coins": 10, "only_here": true },
            "region": "fi", "attributes": { "band": "20-29" },
        }
    });
    assert!(liveops::native::init(&settings).is_none(), "nothing cached the first time");
    packs::configure(None, &Default::default());
    assert_eq!(liveops::with(|l| (l.source(), l.get("coins"))), (Source::Defaults, Some(json!(10))));

    sync_until("the first answer", || liveops::with(|l| l.source() == Source::Live));
    let install = liveops::with(|l| {
        assert_eq!(l.get("coins"), Some(json!(25)));
        assert_eq!(l.get("banner"), Some(json!("new builds")), "the rule saw the game's version");
        assert_eq!(l.get("only_here"), Some(json!(true)), "a default the service does not set");
        assert_eq!(l.release(), 1);
        l.install().to_string()
    });
    assert!(install.starts_with("i-"));
    assert_eq!(packs::status("winter").name(), "remote", "the release's pack is there to download");
    assert_eq!(packs::with(|p| p.source("winter")).unwrap(), format!("{base}/liveops/v1/packs/{file}"));
    // What a script sees, through the op every runtime shares.
    assert_eq!(liveops::script_op("get", "coins"), liveops::ScriptAnswer::Json(json!(25)));
    assert_eq!(liveops::script_op("status", ""), liveops::ScriptAnswer::Text("live".into()));

    // L13: what the game said of itself reached the service's rules - the
    // language from `locale`, the region and attributes from the settings.
    liveops::with(|l| {
        assert_eq!(l.get("greeting"), Some(json!("konnichiwa")), "the language (ja-JP) reached a `languages: [ja]` rule");
        assert_eq!(l.get("nordic"), Some(json!(true)), "the region reached a `regions` rule");
        assert_eq!(l.get("veteran"), Some(json!(true)), "the settings' attribute reached its rule");
        assert_eq!(l.get("mode"), Some(json!("normal")));
    });
    // An attribute a script sets asks again, and reaches its rule too.
    liveops::script_call("set_attribute", &["difficulty", "hard"]);
    sync_until("the script's attribute", || liveops::with(|l| l.get("mode") == Some(json!("hard"))));
    // A new language asks again by itself.
    jengine_core::locale::set_locale("en");
    sync_until("the new language", || liveops::with(|l| l.get("greeting") == Some(json!("hello"))));

    // `LiveOps.expose`: counted for the variant the service gives this install.
    let variant = liveops::with(|l| l.variant("price"));
    assert!(!variant.is_empty(), "the install is in the experiment");
    assert_eq!(liveops::script_call("expose", &["price"]), liveops::ScriptAnswer::Bool(true));
    assert_eq!(liveops::script_call("expose", &["price", "purchase", "2.5"]), liveops::ScriptAnswer::Bool(true));
    assert_eq!(liveops::script_call("expose", &["price", "purchase", "1.5"]), liveops::ScriptAnswer::Bool(true));
    let results = || -> Value {
        ureq::get(&format!("{base}/admin/api/liveops/channel/live/results"))
            .set("Authorization", &format!("Bearer {TOKEN}"))
            .call()
            .unwrap()
            .into_json::<Value>()
            .unwrap()["results"]["price"][variant.as_str()]
            .clone()
    };
    sync_until("the exposures counted", || results()["purchase"]["count"] == json!(2));
    let counted = results();
    assert_eq!(counted["exposed"]["count"], json!(1), "{counted}");
    assert_eq!(counted["purchase"]["sum"], json!(4.0), "{counted}");

    // A change on the service reaches the game at its next ask.
    admin(&base, "PUT", "/admin/api/liveops/channel/live", json!({ "revision": 2, "params": { "coins": { "value": 30 } } }));
    liveops::with(|l| l.fetch());
    sync_until("the change", || liveops::with(|l| l.get("coins") == Some(json!(30))));

    // The next start: the answer is believed from the cache before any
    // fetch, with the same install id - and its release laid over the packs.
    let cached = liveops::native::init(&settings).expect("the cached release");
    assert_eq!(cached.release, 1);
    liveops::with(|l| {
        assert_eq!(l.source(), Source::Cached);
        assert_eq!(l.get("coins"), Some(json!(30)));
        assert_eq!(l.install(), install, "an install keeps its id, so its buckets");
    });

    // A game built with another key believes nothing this service says.
    let other = jengine_liveops::SecretKey::from_seed([9; 32]).public().to_text();
    let mut wrong = settings.clone();
    wrong["liveops"]["public_key"] = json!(other);
    wrong["liveops"]["channel"] = json!("live");
    let _ = std::fs::remove_dir_all(dir.join("game-liveops"));
    liveops::native::init(&wrong);
    liveops::with(|l| l.fetch());
    sync_until("a refusal", || liveops::with(|l| !l.error().is_empty()));
    liveops::with(|l| {
        assert!(l.error().contains("signature"), "{}", l.error());
        assert_eq!(l.source(), Source::Defaults);
        assert_eq!(l.get("coins"), Some(json!(10)));
    });

    running.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
