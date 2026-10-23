//! **A build's packs, published to live ops and installed by a game** - the
//! whole chain with nothing faked: the builder splits a game's packs out,
//! `liveops publish` sends them to a running service, the service names them
//! in a signed snapshot, and the engine's own installer fetches the archive
//! it names and unpacks it.

use std::path::Path;

use jengine_core::asset::packs::{PackDef, PackSettings};
use jengine_liveops::{Envelope, PublicKey, Query};
use jengine_server::{LiveOps, Server};

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn a_published_release_is_what_a_game_downloads() {
    let dir = std::env::temp_dir().join(format!("jengine-liveops-publish-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    // A build with one pack, split as a real build splits it.
    let build = dir.join("build");
    let assets = build.join("assets");
    write(&assets.join("scenes/forest/glade.scene"), b"{\"entities\": []}");
    write(&assets.join("scripts/forest/owl.lua"), b"function update(e, dt) end");
    write(&assets.join("scenes/main.scene"), b"{}");
    let settings = PackSettings { url: String::new(), packs: vec![PackDef { name: "forest".into(), include: vec!["scenes/forest/".into(), "scripts/forest/".into()] }] };
    jengine_builder::asset_packs::split(&assets, &build.join("packs"), &settings, Some("scenes/main.scene")).unwrap();

    let token = "publish-test";
    let running = Server::new().admin_token(token).liveops(LiveOps::open(dir.join("ops")).unwrap()).start("127.0.0.1:0").unwrap();
    let server = running.http_url();

    let first = jengine_builder::liveops_publish::publish(&build, &server, "staging", token, "the forest").unwrap();
    assert_eq!(first.release, 1);
    assert_eq!(first.uploaded.len(), 1);
    let again = jengine_builder::liveops_publish::publish(&build, &server, "staging", token, "").unwrap();
    assert_eq!((again.release, again.uploaded.len(), again.already_there.len()), (2, 0, 1), "nothing sent twice");
    let refused = jengine_builder::liveops_publish::publish(&build, &server, "staging", "wrong", "");
    assert!(format!("{:#}", refused.unwrap_err()).contains("401"));
    assert!(jengine_builder::liveops_publish::publish(&dir.join("nowhere"), &server, "staging", token, "").is_err());

    // The game: a signed snapshot naming the release...
    let key_text = ureq::get(&format!("{server}/liveops/v1/key")).call().unwrap().into_string().unwrap();
    let key = PublicKey::parse(&key_text).unwrap();
    let url = jengine_liveops::snapshot_url(&server, "staging", &Query { install: "game-1".into(), ..Default::default() });
    let env: Envelope = ureq::get(&url).call().unwrap().into_json().unwrap();
    let snapshot = key.verify(&env).unwrap();
    let content = snapshot.content.unwrap();
    assert_eq!(content.release, 2);
    let entry = &content.manifest.packs[0];
    assert_eq!(entry.name, "forest");

    // ...whose archive the engine's installer fetches from the service and
    // checks against the hash the signed manifest gives.
    let fetch = |u: &str| -> anyhow::Result<(Box<dyn std::io::Read + Send>, Option<u64>)> {
        let r = ureq::get(u).call().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok((Box::new(r.into_reader()), None))
    };
    jengine_core::asset::packs::native::set_http(Box::new(fetch));
    let source = format!("{}{}", content.manifest.url, entry.file);
    let installed = jengine_core::asset::packs::native::install(entry, &source, &dir.join("cache"), &dir, &mut |_| {}).unwrap();
    assert_eq!(std::fs::read(installed.join("scripts/forest/owl.lua")).unwrap(), b"function update(e, dt) end");

    running.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
