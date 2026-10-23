//! **What a live ops service and a jEngine game share** (Oct 2026).
//!
//! A live ops service (`jengine-server`'s `liveops`) tells a shipped game
//! three things without a new build: the values of its **remote config**
//! (worked out for this player - their platform, version, rollout bucket,
//! experiment variant and the live events running now), which **live
//! events** are on, and which **content release** - which asset packs - is
//! current. The game asks with a [`Query`] and gets one [`Snapshot`].
//!
//! **A snapshot is signed.** A content release can carry scripts, so a game
//! must not believe one because it came from the right address: the service
//! signs every snapshot with its [`SecretKey`] (Ed25519), the game is built
//! with the matching [`PublicKey`] (`settings.liveops.public_key`), and
//! [`PublicKey::verify`] is the only way to read an [`Envelope`]. A pack
//! archive is then checked against the SHA-256 the signed manifest gives, so
//! whoever serves the bytes - the service, a CDN, a cache - cannot change
//! them.
//!
//! **A key can be replaced without a new build.** The key a game is built
//! with may *endorse* another ([`SecretKey::endorse`]): a signed line saying
//! "this key speaks for me until then". An envelope carries the
//! [`Endorsement`]s from the game's key down to the key that signed it
//! ([`Envelope::chain`]), and [`PublicKey::verify`] walks them - so a
//! service signs with a short-lived key kept on the serving machine while
//! the key games trust (the *root*) stays offline, and a leaked or lost
//! serving key is replaced by endorsing a new one. An endorsement that has
//! run out is not believed: checked against the snapshot's own `issued_at`
//! (a game already refuses one older than it has), and against the clock
//! where there is one ([`PublicKey::verify_at`]).
//!
//! This crate is the part both ends link, as `jengine-netcode` is for
//! prediction: the manifest types (`jengine_core::asset::packs` re-exports
//! them), the snapshot, the envelope and the keys. It has no platform code,
//! so it builds for wasm32.

use std::collections::BTreeMap;

use ed25519_dalek::{Signer, Verifier};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Where the service's routes live under its address.
pub const API_PREFIX: &str = "/liveops/v1";

/// The [`Snapshot`] format this crate writes and reads.
pub const SNAPSHOT_VERSION: u32 = 1;

/// `assets/packs.json`, written by the builder - and, in a [`Content`]
/// release, by the live ops service: every pack, and where to find it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PackManifest {
    #[serde(default = "manifest_version")]
    pub version: u32,
    /// Where the archives are fetched from. In a build, its
    /// `settings.asset_packs.url`; in a release, the service's (or the CDN in
    /// front of it), or empty for the service's own `packs/` route.
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub packs: Vec<PackEntry>,
}

fn manifest_version() -> u32 {
    1
}

/// One built pack.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PackEntry {
    pub name: String,
    /// The archive's file name, which carries its hash - so a changed pack
    /// is a new URL and no cache anywhere can serve the old one.
    pub file: String,
    /// The archive's size in bytes, what a download costs.
    pub size: u64,
    /// The archive's SHA-256, lowercase hex.
    pub sha256: String,
    /// Every asset path in it, relative to `assets/`.
    #[serde(default)]
    pub files: Vec<String>,
}

/// A live event's window, in Unix seconds by the service's clock.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EventWindow {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub start: u64,
    pub end: u64,
}

/// The content release a snapshot names.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Content {
    /// The release's number on its channel; a rollback publishes an older
    /// release's packs under a new number.
    pub release: u64,
    pub manifest: PackManifest,
}

/// Everything the service says to one game at one moment.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    /// `dev`, `staging`, `live` - the channel it was made from.
    pub channel: String,
    /// The install it was worked out for: config is per player, so a
    /// snapshot made for somebody else is refused.
    pub install: String,
    /// When it was made (Unix seconds, the service's clock).
    pub issued_at: u64,
    /// Goes up with every change to the channel - a rollback too - so a game
    /// can refuse an older snapshot replayed at it.
    pub revision: u64,
    /// Every remote config value, resolved.
    #[serde(default)]
    pub config: serde_json::Map<String, Value>,
    /// Experiment id -> the variant this install is in.
    #[serde(default)]
    pub experiments: BTreeMap<String, String>,
    /// The live events on now.
    #[serde(default)]
    pub events: Vec<EventWindow>,
    /// Events that have not started yet, soonest first.
    #[serde(default)]
    pub upcoming: Vec<EventWindow>,
    #[serde(default)]
    pub content: Option<Content>,
    /// How long until the game should ask again.
    #[serde(default)]
    pub refresh_secs: u64,
    /// When something scheduled next starts or ends - ask again then.
    #[serde(default)]
    pub next_change: Option<u64>,
}

/// A signed [`Snapshot`]: the JSON exactly as signed, and the signature.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub payload: String,
    /// Ed25519 over `payload`'s bytes, hex.
    pub signature: String,
    /// The public key that should verify it, as text - for a person reading
    /// a response. A game uses the key it was built with, never this one.
    #[serde(default)]
    pub key: String,
    /// The endorsements from the key a game trusts down to the key that
    /// signed `payload`, first to last; empty when that key signed it itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chain: Vec<Endorsement>,
}

/// One key saying another speaks for it, until a time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Endorsement {
    /// The key endorsed, as [`PublicKey::to_text`] writes it.
    pub key: String,
    /// Unix seconds after which it is not believed; 0 for never - which
    /// leaves only a new build to take it back, so give one.
    #[serde(default)]
    pub not_after: u64,
    /// Ed25519, by the key before it in the chain (or the trusted key),
    /// over [`Endorsement::message`], hex.
    pub signature: String,
}

impl Endorsement {
    /// What an endorsement's signature is over.
    pub fn message(key: &str, not_after: u64) -> Vec<u8> {
        format!("jlo-endorse-v1\n{key}\n{not_after}").into_bytes()
    }
}

/// The longest chain of endorsements believed.
pub const MAX_CHAIN: usize = 4;

/// Why an [`Envelope`] was not believed.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// A key that is not one.
    BadKey(String),
    /// Not signed by the key the game trusts, or changed since.
    BadSignature,
    /// Signed by a key whose endorsement has run out.
    Expired,
    /// Signed, but not a snapshot this crate can read.
    Malformed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::BadKey(why) => write!(f, "not a live ops key: {why}"),
            Error::BadSignature => f.write_str("the snapshot's signature does not match the game's key"),
            Error::Expired => f.write_str("the snapshot is signed by a key whose endorsement has run out"),
            Error::Malformed(why) => write!(f, "the snapshot could not be read: {why}"),
        }
    }
}

impl std::error::Error for Error {}

const SECRET_PREFIX: &str = "jlo-secret-";
const PUBLIC_PREFIX: &str = "jlo-public-";

/// The service's signing key. Kept by the service and nowhere else.
#[derive(Clone)]
pub struct SecretKey(ed25519_dalek::SigningKey);

impl SecretKey {
    /// The key made from 32 random bytes - which the caller must get from a
    /// real source of randomness (the server uses the OS's).
    pub fn from_seed(seed: [u8; 32]) -> Self {
        SecretKey(ed25519_dalek::SigningKey::from_bytes(&seed))
    }

    /// `jlo-secret-<64 hex>`, as [`SecretKey::to_text`] writes it.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let hex = text.trim().strip_prefix(SECRET_PREFIX).ok_or_else(|| Error::BadKey(format!("a secret key starts with {SECRET_PREFIX}")))?;
        Ok(Self::from_seed(bytes32(hex)?))
    }

    pub fn to_text(&self) -> String {
        format!("{SECRET_PREFIX}{}", to_hex(&self.0.to_bytes()))
    }

    /// The 32 bytes the key is made from - for a tool that keeps keys in a
    /// text form of its own.
    pub fn seed(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    /// The key a game is built with to check what this one signs.
    pub fn public(&self) -> PublicKey {
        PublicKey(self.0.verifying_key())
    }

    /// Sign `snapshot` as it serialises now.
    pub fn sign(&self, snapshot: &Snapshot) -> Envelope {
        let payload = serde_json::to_string(snapshot).expect("a snapshot serialises");
        let signature = self.0.sign(payload.as_bytes());
        Envelope { payload, signature: to_hex(&signature.to_bytes()), key: self.public().to_text(), chain: Vec::new() }
    }

    /// [`SecretKey::sign`], carrying the endorsements from a trusted key
    /// down to this one.
    pub fn sign_endorsed(&self, snapshot: &Snapshot, chain: &[Endorsement]) -> Envelope {
        Envelope { chain: chain.to_vec(), ..self.sign(snapshot) }
    }

    /// Ed25519 over any bytes, hex - for whatever else a key vouches for.
    pub fn sign_bytes(&self, message: &[u8]) -> String {
        to_hex(&self.0.sign(message).to_bytes())
    }

    /// Say `key` speaks for this one until `not_after` (Unix seconds; 0 for
    /// never).
    pub fn endorse(&self, key: &PublicKey, not_after: u64) -> Endorsement {
        let text = key.to_text();
        Endorsement { signature: self.sign_bytes(&Endorsement::message(&text, not_after)), key: text, not_after }
    }
}

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the bytes: a key in a log line is a key given away.
        write!(f, "SecretKey({})", self.public().to_text())
    }
}

/// The key a game trusts snapshots from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicKey(ed25519_dalek::VerifyingKey);

impl PublicKey {
    /// `jlo-public-<64 hex>`, as [`PublicKey::to_text`] writes it.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let hex = text.trim().strip_prefix(PUBLIC_PREFIX).ok_or_else(|| Error::BadKey(format!("a public key starts with {PUBLIC_PREFIX}")))?;
        ed25519_dalek::VerifyingKey::from_bytes(&bytes32(hex)?).map(PublicKey).map_err(|e| Error::BadKey(e.to_string()))
    }

    pub fn to_text(&self) -> String {
        format!("{PUBLIC_PREFIX}{}", to_hex(self.0.as_bytes()))
    }

    /// The key's 32 bytes.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    /// A key from its 32 bytes.
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self, Error> {
        ed25519_dalek::VerifyingKey::from_bytes(bytes).map(PublicKey).map_err(|e| Error::BadKey(e.to_string()))
    }

    /// Whether this key signed exactly `message` ([`SecretKey::sign_bytes`]).
    pub fn verify_bytes(&self, message: &[u8], signature: &str) -> bool {
        let Some(sig) = from_hex(signature).and_then(|s| <[u8; 64]>::try_from(s).ok()) else { return false };
        self.0.verify(message, &ed25519_dalek::Signature::from_bytes(&sig)).is_ok()
    }

    /// The snapshot inside `envelope`, if this key signed exactly that
    /// payload - itself, or through the envelope's chain of endorsements.
    /// The only way to read one. An endorsement is held to the snapshot's
    /// own time and, off the web, to this machine's clock as well.
    pub fn verify(&self, envelope: &Envelope) -> Result<Snapshot, Error> {
        #[cfg(not(target_arch = "wasm32"))]
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs());
        #[cfg(target_arch = "wasm32")]
        let now = None;
        self.verify_at(envelope, now)
    }

    /// [`PublicKey::verify`] with the clock given: `Some(unix seconds)` holds
    /// every endorsement to it too (a web build can pass `Date.now()`).
    pub fn verify_at(&self, envelope: &Envelope, now: Option<u64>) -> Result<Snapshot, Error> {
        if envelope.chain.len() > MAX_CHAIN {
            return Err(Error::BadSignature);
        }
        // Down the chain: each key vouched for by the one before.
        let mut signer = *self;
        let mut ends = u64::MAX;
        for e in &envelope.chain {
            if !signer.verify_bytes(&Endorsement::message(&e.key, e.not_after), &e.signature) {
                return Err(Error::BadSignature);
            }
            if e.not_after > 0 {
                ends = ends.min(e.not_after);
            }
            signer = PublicKey::parse(&e.key).map_err(|_| Error::BadSignature)?;
        }
        if !signer.verify_bytes(envelope.payload.as_bytes(), &envelope.signature) {
            return Err(Error::BadSignature);
        }
        let snapshot: Snapshot = serde_json::from_str(&envelope.payload).map_err(|e| Error::Malformed(e.to_string()))?;
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(Error::Malformed(format!("snapshot version {} (this game reads {SNAPSHOT_VERSION})", snapshot.version)));
        }
        if snapshot.issued_at > ends || now.is_some_and(|now| now > ends) {
            return Err(Error::Expired);
        }
        Ok(snapshot)
    }
}

/// Who is asking, for the service to work their config out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Query {
    /// A random id this install made once and keeps - what rollouts and
    /// experiments bucket by.
    pub install: String,
    /// The signed-in player, if the game has one (`LiveOps.set_player`).
    pub player: String,
    /// `windows`, `linux`, `macos`, `android`, `ios`, `web`, `editor`.
    pub platform: String,
    /// The game's version (`settings.app.version`).
    pub app_version: String,
}

/// The snapshot route for `channel` on the service at `base`
/// (`https://ops.example.com`, no trailing path).
pub fn snapshot_url(base: &str, channel: &str, q: &Query) -> String {
    format!(
        "{}{API_PREFIX}/{}/snapshot?install={}&player={}&platform={}&version={}",
        base.trim_end_matches('/'),
        encode(channel),
        encode(&q.install),
        encode(&q.player),
        encode(&q.platform),
        encode(&q.app_version)
    )
}

/// [`snapshot_url`], saying more of the game than a [`Query`] does: its
/// language (`ja-JP`), region and attributes of its own, which a service's
/// rules can target (`lang`, `region`, `a.<name>`). An empty one is left out.
pub fn snapshot_url_with(base: &str, channel: &str, q: &Query, language: &str, region: &str, attributes: &[(&str, &str)]) -> String {
    let mut url = snapshot_url(base, channel, q);
    if !language.is_empty() {
        url.push_str(&format!("&lang={}", encode(language)));
    }
    if !region.is_empty() {
        url.push_str(&format!("&region={}", encode(region)));
    }
    for (name, value) in attributes {
        url.push_str(&format!("&a.{}={}", encode(name), encode(value)));
    }
    url
}

/// Where a game says an install saw its variant, or did something worth
/// counting in an experiment: POST `{"experiment","event","value"}` here.
pub fn exposure_url(base: &str, channel: &str, q: &Query) -> String {
    format!(
        "{}{API_PREFIX}/{}/exposure?install={}&player={}",
        base.trim_end_matches('/'),
        encode(channel),
        encode(&q.install),
        encode(&q.player)
    )
}

/// Where the service at `base` serves pack archives - a release manifest's
/// `url` when it names none.
pub fn packs_url(base: &str) -> String {
    format!("{}{API_PREFIX}/packs/", base.trim_end_matches('/'))
}

/// A query-string value: unreserved bytes as they are, the rest `%XX`.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Lowercase hex.
pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Hex back to bytes; `None` for anything else.
pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn bytes32(hex: &str) -> Result<[u8; 32], Error> {
    from_hex(hex).and_then(|v| v.try_into().ok()).ok_or_else(|| Error::BadKey("expected 64 hex digits".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> SecretKey {
        SecretKey::from_seed([7; 32])
    }

    fn snapshot() -> Snapshot {
        let mut config = serde_json::Map::new();
        config.insert("coins_per_win".into(), Value::from(25));
        Snapshot {
            version: SNAPSHOT_VERSION,
            channel: "live".into(),
            install: "i-1".into(),
            issued_at: 1_800_000_000,
            revision: 4,
            config,
            refresh_secs: 300,
            ..Default::default()
        }
    }

    #[test]
    fn a_signed_snapshot_reads_back_with_the_public_key() {
        let k = key();
        let env = k.sign(&snapshot());
        assert_eq!(k.public().verify(&env).unwrap(), snapshot());
        assert_eq!(env.key, k.public().to_text());
    }

    #[test]
    fn a_changed_payload_or_another_key_is_refused() {
        let k = key();
        let mut env = k.sign(&snapshot());
        env.payload = env.payload.replace("25", "2500");
        assert_eq!(k.public().verify(&env), Err(Error::BadSignature), "a richer reward is not believed");

        let env = k.sign(&snapshot());
        let other = SecretKey::from_seed([8; 32]).public();
        assert_eq!(other.verify(&env), Err(Error::BadSignature));
        let mut env2 = env.clone();
        env2.signature = "zz".into();
        assert_eq!(k.public().verify(&env2), Err(Error::BadSignature));
    }

    #[test]
    fn keys_round_trip_as_text_and_refuse_nonsense() {
        let k = key();
        let back = SecretKey::parse(&k.to_text()).unwrap();
        assert_eq!(back.public(), k.public());
        assert_eq!(PublicKey::parse(&k.public().to_text()).unwrap(), k.public());
        assert!(PublicKey::parse("jlo-public-1234").is_err());
        assert!(PublicKey::parse(&k.to_text()).is_err(), "a secret key is not a public one");
        assert!(SecretKey::parse(&k.public().to_text()).is_err());
        assert!(!format!("{k:?}").contains(&to_hex(&[7; 32])), "Debug never prints the secret");
    }

    /// L3: a game built with one key believes a key that key endorsed - for
    /// as long as the endorsement says - and nothing a stranger endorsed.
    #[test]
    fn a_key_the_trusted_one_endorsed_is_believed_until_it_runs_out() {
        let root = key();
        let serving = SecretKey::from_seed([9; 32]);
        let until = 1_800_000_500;
        let chain = vec![root.endorse(&serving.public(), until)];
        let env = serving.sign_endorsed(&snapshot(), &chain);
        assert_eq!(root.public().verify_at(&env, Some(until - 1)).unwrap(), snapshot(), "endorsed: believed");
        assert_eq!(root.public().verify_at(&env, None).unwrap(), snapshot(), "issued before it runs out");
        assert_eq!(root.public().verify_at(&env, Some(until + 1)), Err(Error::Expired), "past it, by the clock");
        let mut late = snapshot();
        late.issued_at = until + 10;
        assert_eq!(root.public().verify_at(&serving.sign_endorsed(&late, &chain), None), Err(Error::Expired), "or by the snapshot's own time");

        // Without the chain the serving key is nobody to the game.
        assert_eq!(root.public().verify(&serving.sign(&snapshot())), Err(Error::BadSignature));
        // A chain made by somebody else's key.
        let stranger = SecretKey::from_seed([10; 32]);
        let forged = serving.sign_endorsed(&snapshot(), &[stranger.endorse(&serving.public(), until)]);
        assert_eq!(root.public().verify(&forged), Err(Error::BadSignature));
        // An endorsement whose time was moved.
        let mut moved = chain.clone();
        moved[0].not_after = until + 1_000_000;
        assert_eq!(root.public().verify(&serving.sign_endorsed(&snapshot(), &moved)), Err(Error::BadSignature));
        // Two links: the root endorses a yearly key, which endorses a weekly one.
        let yearly = SecretKey::from_seed([11; 32]);
        let two = vec![root.endorse(&yearly.public(), 0), yearly.endorse(&serving.public(), until)];
        assert!(root.public().verify(&serving.sign_endorsed(&snapshot(), &two)).is_ok());
        let long = vec![root.endorse(&root.public(), 0); MAX_CHAIN + 1];
        assert_eq!(root.public().verify(&root.sign_endorsed(&snapshot(), &long)), Err(Error::BadSignature));
        // An envelope with no chain reads as it always did, and still parses
        // where a chain is absent from the JSON.
        let plain: Envelope = serde_json::from_str(&serde_json::to_string(&root.sign(&snapshot())).unwrap()).unwrap();
        assert!(!serde_json::to_string(&plain).unwrap().contains("chain"));
        assert!(root.public().verify(&plain).is_ok());
    }

    #[test]
    fn bytes_sign_and_keys_round_trip_as_bytes() {
        let k = key();
        let sig = k.sign_bytes(b"jengine-release-v1\nabc");
        assert!(k.public().verify_bytes(b"jengine-release-v1\nabc", &sig));
        assert!(!k.public().verify_bytes(b"jengine-release-v1\nabd", &sig));
        assert!(!k.public().verify_bytes(b"x", "not hex"));
        assert_eq!(PublicKey::from_bytes(&k.public().to_bytes()).unwrap(), k.public());
        assert_eq!(SecretKey::from_seed(k.seed()).public(), k.public());
    }

    #[test]
    fn an_unknown_snapshot_version_is_refused() {
        let k = key();
        let mut s = snapshot();
        s.version = 99;
        assert!(matches!(k.public().verify(&k.sign(&s)), Err(Error::Malformed(_))));
    }

    #[test]
    fn urls_are_built_and_escaped() {
        let q = Query { install: "a b".into(), player: "p&1".into(), platform: "web".into(), app_version: "1.2.0".into() };
        assert_eq!(
            snapshot_url("https://ops.example.com/", "live", &q),
            "https://ops.example.com/liveops/v1/live/snapshot?install=a%20b&player=p%261&platform=web&version=1.2.0"
        );
        assert_eq!(packs_url("http://127.0.0.1:9001"), "http://127.0.0.1:9001/liveops/v1/packs/");
        assert_eq!(
            snapshot_url_with("https://ops.example.com", "live", &q, "ja-JP", "", &[("band", "20-29")]),
            "https://ops.example.com/liveops/v1/live/snapshot?install=a%20b&player=p%261&platform=web&version=1.2.0&lang=ja-JP&a.band=20-29"
        );
        assert_eq!(exposure_url("https://ops.example.com", "live", &q), "https://ops.example.com/liveops/v1/live/exposure?install=a%20b&player=p%261");
    }
}
