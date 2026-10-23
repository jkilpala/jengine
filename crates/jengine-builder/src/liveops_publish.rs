//! `jengine liveops publish <build> --server <url> --channel <name>`: a
//! build's asset packs, published to a live ops service as a content release
//! (`jengine-server`'s `liveops`) - so games already out there take the new
//! packs without being rebuilt.
//!
//! It reads the build's `packs.json` (the builder writes it into the build's
//! `assets/`) and the archives beside it (`packs/`), uploads every archive
//! the service does not already hold - checked against its SHA-256 on both
//! ends - and publishes the manifest as the channel's next release. The
//! manifest's `url` is cleared: a release's archives are fetched from the
//! service (or the CDN it names), whatever the build said.
//!
//! The admin token is `--token`, else `JENGINE_ADMIN_TOKEN`.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use jengine_core::asset::packs::{MANIFEST_FILE, PackManifest};

/// What a publish did.
#[derive(Debug, Clone, PartialEq)]
pub struct Published {
    pub release: u64,
    pub uploaded: Vec<String>,
    pub already_there: Vec<String>,
}

/// Where a build keeps its manifest and archives: `<build>/assets/packs.json`
/// (or `<build>/packs.json`) and `<build>/packs/`.
pub fn find(build: &Path) -> anyhow::Result<(PathBuf, PathBuf)> {
    let manifest = [build.join("assets").join(MANIFEST_FILE), build.join(MANIFEST_FILE)]
        .into_iter()
        .find(|p| p.is_file())
        .with_context(|| format!("no {MANIFEST_FILE} in {} or its assets/ - is it a build with asset packs?", build.display()))?;
    let archives = build.join("packs");
    if !archives.is_dir() {
        bail!("no packs/ folder in {}", build.display());
    }
    Ok((manifest, archives))
}

/// Publish the build at `build` to `channel` on the service at `server`.
pub fn publish(build: &Path, server: &str, channel: &str, token: &str, notes: &str) -> anyhow::Result<Published> {
    let (manifest_path, archives) = find(build)?;
    let mut manifest: PackManifest = serde_json::from_str(&std::fs::read_to_string(&manifest_path)?)
        .with_context(|| format!("{} is not a packs manifest", manifest_path.display()))?;
    manifest.url.clear();
    let server = server.trim_end_matches('/');
    let auth = format!("Bearer {token}");
    let agent = ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(20)).build();
    let call = |r: Result<ureq::Response, ureq::Error>, what: &str| -> anyhow::Result<serde_json::Value> {
        match r {
            Ok(resp) => Ok(resp.into_json()?),
            Err(ureq::Error::Status(code, resp)) => {
                let body: serde_json::Value = resp.into_json().unwrap_or_default();
                bail!("{what}: HTTP {code} - {}", body["error"].as_str().unwrap_or("no reason given"))
            }
            Err(e) => bail!("{what}: {e}"),
        }
    };
    let overview = call(agent.get(&format!("{server}/admin/api/liveops")).set("Authorization", &auth).call(), "asking the service")?;
    let held: Vec<(String, String)> = overview["blobs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| Some((b["file"].as_str()?.to_string(), b["sha256"].as_str()?.to_string())))
        .collect();
    let mut out = Published { release: 0, uploaded: Vec::new(), already_there: Vec::new() };
    for p in &manifest.packs {
        if held.iter().any(|(f, s)| *f == p.file && *s == p.sha256) {
            out.already_there.push(p.file.clone());
            continue;
        }
        let path = archives.join(&p.file);
        let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        {
            use sha2::Digest;
            let got: String = sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
            if got != p.sha256 {
                bail!("{} does not match the SHA-256 its manifest gives - rebuild", path.display());
            }
        }
        crate::progress::step(&format!("Uploading {} ({:.1} MB)", p.file, bytes.len() as f64 / 1_048_576.0));
        call(
            agent
                .put(&format!("{server}/admin/api/liveops/blob/{}?sha256={}", p.file, p.sha256))
                .set("Authorization", &auth)
                .send_bytes(&bytes),
            &format!("uploading {}", p.file),
        )?;
        out.uploaded.push(p.file.clone());
    }
    let r = call(
        agent
            .post(&format!("{server}/admin/api/liveops/channel/{channel}/release"))
            .set("Authorization", &auth)
            .send_json(serde_json::json!({ "manifest": manifest, "notes": notes })),
        "publishing the release",
    )?;
    out.release = r["release"]["number"].as_u64().unwrap_or(0);
    Ok(out)
}
