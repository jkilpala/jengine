//! Who a player is, beyond "whoever holds this token".
//!
//! [`GuestAuth`](crate::GuestAuth) - the default - knows a player only by a
//! secret it handed out, so a player who loses it loses the character. Two
//! more ways to answer a `hello`:
//!
//! - [`PasswordAuth`]: accounts kept by this server. A name and a password
//!   are the player; the password is stored only as a salted, stretched hash.
//! - [`SignedAuth`]: accounts kept by **your** backend. It signs a short
//!   ticket saying who somebody is, with a secret this server shares, and the
//!   server believes any ticket whose signature checks out - it never sees a
//!   password and needs no call to the backend.
//!
//! A `hello` travels in the clear unless the socket is `wss://`, so **the
//! server refuses a password on a socket that crossed the network
//! unencrypted** (`Config::allow_plain_passwords` to allow it on a LAN you
//! trust): give it a certificate (`Server::tls`) or put a TLS proxy in front.
//!
//! A player who forgets their password can get back in without an operator
//! if the game gives [`PasswordAuth::recovery`] a way to reach them (an
//! email, a text): the server makes a short-lived code, hands it to that
//! hook, and takes it in place of the old password. A ticket can be revoked
//! ([`SignedAuth::revoke`], or the dashboard's sign-out): every ticket for
//! that player issued until then stops working, though not yet expired.

use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;

use crate::server::{Authenticator, Identity};
use crate::store::{NS_TOKEN, STORE_DOWN, Store};

/// What a `hello` asking for a recovery code is answered - whether or not
/// the account exists, so the answer says nothing about which names do.
pub const RECOVERY_SENT: &str = "if that account exists, a recovery code is on its way";

/// A recovery code for [`PasswordAuth::recovery`]'s hook to deliver.
#[derive(Debug, Clone)]
pub struct Recovery {
    /// The account's name, as it was registered.
    pub account: String,
    /// The player the account is - for a game that keeps an email address in
    /// the player's state, or in a table of its own.
    pub player: String,
    /// Eight digits, to be typed back in a `hello` with a new password.
    pub code: String,
    /// When it stops working (Unix seconds).
    pub expires: u64,
}

type RecoveryHook = std::sync::Arc<dyn Fn(&Recovery) -> Result<(), String> + Send + Sync>;

/// Where a recovery code waits. Key: the account name, lower-cased; value:
/// `sha256-hex|expires|wrong tries|issued at`.
const NS_RECOVERY: &str = "recovery";
/// How long a recovery code lasts.
const RECOVERY_SECS: u64 = 15 * 60;
/// A new code is not sent sooner than this after the last one - so a
/// stranger cannot flood somebody's inbox.
const RECOVERY_GAP_SECS: u64 = 60;

/// Where [`SignedAuth`] keeps revocations. Key: the player; value: Unix
/// seconds - a ticket issued then or earlier is not believed.
pub const NS_REVOKED: &str = "revoked";

/// Constant-time equality of two byte strings.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn sha256_hex(text: &str) -> String {
    use sha2::Digest;
    hex(&Sha256::digest(text.as_bytes()))
}

/// Where [`PasswordAuth`] keeps accounts. Key: the name, lower-cased.
pub const NS_ACCOUNT: &str = "account";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

fn random(bytes: usize) -> Vec<u8> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("the OS has randomness");
    buf
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---- accounts this server keeps -------------------------------------------------

/// Names and passwords, kept in the server's store.
///
/// ```json
/// {"t":"hello","account":"ada","password":"correct horse","register":true}
/// {"t":"hello","account":"ada","password":"correct horse"}
/// {"t":"hello","token":"<from an earlier welcome>"}
/// ```
///
/// Registering or signing in answers with a session `token`, which is what a
/// client keeps and presents afterwards - so the password is typed once, not
/// stored on the device, and a reconnect needs no password at all.
///
/// **A session ends.** It lasts [`session_secs`](Self::session_secs) from
/// the last time it was used, an account keeps its newest
/// [`max_sessions`](Self::max_sessions), and once signed in a player can end
/// them all:
///
/// ```json
/// {"t":"account","change_password":{"old":"correct horse","new":"battery staple"}}
/// {"t":"account","sign_out_everywhere":true}
/// ```
///
/// Both answer `{"t":"account","ok":true,"token":".."}` - every other session
/// is gone, and this is the new one for the device that asked.
///
/// **Guessing is slowed per account**, not only per connection: five wrong
/// passwords lock the name for a minute, and each further wrong one doubles
/// that, up to a quarter of an hour. The count is kept in the store, so the
/// nodes of a cluster count together.
pub struct PasswordAuth {
    /// How hard the password hash is to compute. More is slower to sign in
    /// and slower to guess; the stored hash remembers what it was made with,
    /// so raising this later does not lock anybody out.
    pub iterations: u32,
    /// Whether a `hello` with neither account nor token makes a guest player,
    /// as [`GuestAuth`](crate::GuestAuth) would.
    pub allow_guests: bool,
    /// Seconds a session lasts after it was last used. Thirty days.
    pub session_secs: u64,
    /// Sessions an account keeps; signing in past this ends the oldest.
    pub max_sessions: usize,
    /// How a recovery code reaches the player ([`PasswordAuth::recovery`]).
    recovery: Option<RecoveryHook>,
}

impl Default for PasswordAuth {
    fn default() -> Self {
        PasswordAuth {
            iterations: 100_000,
            allow_guests: false,
            session_secs: 30 * 24 * 3600,
            max_sessions: 10,
            recovery: None,
        }
    }
}

/// Where a player's account is found from their id. Key: the player id.
const NS_ACCOUNT_OF: &str = "account_of";
/// Wrong passwords in a row, and when the account is locked until. Key: the
/// account name; value: `count|unix-seconds`.
const NS_TRIES: &str = "tries";
/// Wrong passwords before an account is locked for a while.
const TRIES: u32 = 5;

impl PasswordAuth {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow_guests(mut self) -> Self {
        self.allow_guests = true;
        self
    }

    /// How many rounds of hashing a password gets. Lower only in tests.
    pub fn iterations(mut self, rounds: u32) -> Self {
        self.iterations = rounds;
        self
    }

    pub fn session_secs(mut self, secs: u64) -> Self {
        self.session_secs = secs;
        self
    }

    pub fn max_sessions(mut self, sessions: usize) -> Self {
        self.max_sessions = sessions;
        self
    }

    /// Let a player who forgot their password back in by themselves. `send`
    /// is given a [`Recovery`] - the account, the player, an eight-digit code
    /// good for fifteen minutes - and delivers it however the game reaches
    /// its players (an email it keeps, a text). Then:
    ///
    /// ```json
    /// {"t":"hello","account":"ada","recover":true}
    /// {"t":"hello","account":"ada","code":"12345678","password":"a new password"}
    /// ```
    ///
    /// The first is always answered [`RECOVERY_SENT`], whether or not the
    /// account exists; the second sets the new password, ends every other
    /// session and signs in. A code is sent at most once a minute per
    /// account, five wrong codes burn it, and a code is refused on a socket
    /// that crossed the network unencrypted, as a password is.
    pub fn recovery(mut self, send: impl Fn(&Recovery) -> Result<(), String> + Send + Sync + 'static) -> Self {
        self.recovery = Some(std::sync::Arc::new(send));
        self
    }

    /// `hello` with `"recover": true`: make a code and hand it to the hook.
    fn send_recovery(&self, store: &dyn Store, key: &str, account: Option<&Value>) -> Result<Identity, String> {
        let Some(send) = &self.recovery else {
            return Err("this server has no way to recover an account by yourself; ask its operator".into());
        };
        let Some(account) = account else { return Err(RECOVERY_SENT.into()) };
        let now = unix_now();
        let issued = store
            .load(NS_RECOVERY, key)
            .and_then(|r| r.split('|').nth(3).and_then(|t| t.parse::<u64>().ok()))
            .unwrap_or(0);
        if issued + RECOVERY_GAP_SECS > now {
            return Err(RECOVERY_SENT.into());
        }
        let n = u64::from_le_bytes(random(8).try_into().expect("eight bytes")) % 100_000_000;
        let code = format!("{n:08}");
        let expires = now + RECOVERY_SECS;
        store.save(NS_RECOVERY, key, &format!("{}|{expires}|0|{now}", sha256_hex(&code)));
        let recovery = Recovery {
            account: account["name"].as_str().unwrap_or(key).to_string(),
            player: account["player"].as_str().unwrap_or("").to_string(),
            code,
            expires,
        };
        if let Err(why) = send(&recovery) {
            log::warn!("[auth] the recovery code for {key} could not be sent: {why}");
        }
        Err(RECOVERY_SENT.into())
    }

    /// `hello` with a `"code"`: the code in place of the old password, and
    /// a new one set.
    fn use_recovery(&self, store: &dyn Store, key: &str, hello: &Value, account: Option<Value>) -> Result<Identity, String> {
        const WRONG: &str = "that recovery code is not right, or has run out";
        let record = store.try_load(NS_RECOVERY, key).map_err(|_| STORE_DOWN.to_string())?.ok_or(WRONG)?;
        let mut parts = record.split('|');
        let (hash, expires, tries, issued) = (
            parts.next().unwrap_or("").to_string(),
            parts.next().and_then(|t| t.parse::<u64>().ok()).unwrap_or(0),
            parts.next().and_then(|t| t.parse::<u32>().ok()).unwrap_or(0),
            parts.next().unwrap_or("0").to_string(),
        );
        if expires < unix_now() {
            store.delete(NS_RECOVERY, key);
            return Err(WRONG.into());
        }
        let given = hello["code"].as_str().unwrap_or("").trim();
        if !same(sha256_hex(given).as_bytes(), hash.as_bytes()) {
            if tries + 1 >= TRIES {
                store.delete(NS_RECOVERY, key);
            } else {
                store.save(NS_RECOVERY, key, &format!("{hash}|{expires}|{}|{issued}", tries + 1));
            }
            return Err(WRONG.into());
        }
        let Some(mut account) = account else { return Err(WRONG.into()) };
        self.set_password(&mut account, hello["password"].as_str().unwrap_or(""))?;
        Self::end_sessions(store, &mut account);
        store.delete(NS_RECOVERY, key);
        store.delete(NS_TRIES, key);
        let player = account["player"].as_str().unwrap_or("").to_string();
        let name = account["name"].as_str().unwrap_or(key).to_string();
        let token = self.session(store, key, &mut account);
        Ok(Identity { token: Some(token), player, name })
    }

    fn stretch(password: &str, salt: &[u8], iterations: u32) -> [u8; 32] {
        pbkdf2::pbkdf2_hmac_array::<Sha256, 32>(password.as_bytes(), salt, iterations)
    }

    /// A session token's record: who, and until when.
    fn session_record(&self, player: &str) -> String {
        format!("{player}|{}", unix_now() + self.session_secs)
    }

    /// A token for a guest, who has no account to list it under.
    fn guest_session(&self, store: &dyn Store, player: &str) -> String {
        let token = hex(&random(16));
        store.save(NS_TOKEN, &token, &self.session_record(player));
        token
    }

    /// A new session for an account, ending its oldest if it has too many.
    fn session(&self, store: &dyn Store, key: &str, account: &mut Value) -> String {
        let token = hex(&random(16));
        let player = account["player"].as_str().unwrap_or("").to_string();
        store.save(NS_TOKEN, &token, &self.session_record(&player));
        let mut sessions: Vec<Value> = account["sessions"].as_array().cloned().unwrap_or_default();
        sessions.push(json!(token));
        while sessions.len() > self.max_sessions.max(1) {
            if let Some(old) = sessions.remove(0).as_str() {
                store.delete(NS_TOKEN, old);
            }
        }
        account["sessions"] = Value::Array(sessions);
        store.save(NS_ACCOUNT, key, &account.to_string());
        token
    }

    fn end_sessions(store: &dyn Store, account: &mut Value) {
        for token in account["sessions"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            store.delete(NS_TOKEN, token);
        }
        account["sessions"] = json!([]);
    }

    fn set_password(&self, account: &mut Value, password: &str) -> Result<(), String> {
        if password.chars().count() < 8 {
            return Err("a password needs at least 8 characters".into());
        }
        let salt = random(16);
        account["salt"] = json!(hex(&salt));
        account["hash"] = json!(hex(&Self::stretch(password, &salt, self.iterations)));
        account["iterations"] = json!(self.iterations);
        Ok(())
    }

    /// Whether `password` is the account's. Takes as long for an account
    /// that does not exist as for one that does.
    fn matches(&self, account: Option<&Value>, password: &str) -> bool {
        let (salt, stored, iterations) = match account {
            Some(a) => (
                unhex(a["salt"].as_str().unwrap_or("")).unwrap_or_default(),
                unhex(a["hash"].as_str().unwrap_or("")).unwrap_or_default(),
                a["iterations"].as_u64().unwrap_or(u64::from(self.iterations)) as u32,
            ),
            None => (vec![0u8; 16], Vec::new(), self.iterations),
        };
        let given = Self::stretch(password, &salt, iterations);
        stored.len() == given.len() && stored.iter().zip(given.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
    }

    /// (wrong tries in a row, locked until - unix seconds, 0 for not locked).
    fn tries(store: &dyn Store, key: &str) -> (u32, u64) {
        let record = store.load(NS_TRIES, key).unwrap_or_default();
        let (count, until) = record.split_once('|').unwrap_or(("0", "0"));
        (count.parse().unwrap_or(0), until.parse().unwrap_or(0))
    }

    fn account(store: &dyn Store, key: &str) -> Result<Option<Value>, String> {
        Ok(store.try_load(NS_ACCOUNT, key).map_err(|_| STORE_DOWN.to_string())?.and_then(|t| serde_json::from_str(&t).ok()))
    }

    /// The account a signed-in player belongs to, with its key.
    fn account_of(store: &dyn Store, player: &str) -> Result<(String, Value), String> {
        let key = store
            .try_load(NS_ACCOUNT_OF, player)
            .map_err(|_| STORE_DOWN.to_string())?
            .ok_or_else(|| "a guest has no account".to_string())?;
        let account = Self::account(store, &key)?.ok_or_else(|| "that account is gone".to_string())?;
        Ok((key, account))
    }
}

/// A name a person can type and another can read: letters, digits, `_` and
/// `-`, three to twenty-four of them.
fn account_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    let fits = (3..=24).contains(&name.chars().count())
        && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-');
    if fits { Ok(name.to_string()) } else { Err("an account name is 3 to 24 letters, digits, _ or -".into()) }
}

impl Authenticator for PasswordAuth {
    fn authenticate(&self, hello: &Value, store: &dyn Store) -> Result<Identity, String> {
        let text = |key: &str| hello.get(key).and_then(Value::as_str).unwrap_or("");
        if !text("token").is_empty() {
            let token = text("token");
            let record = store.try_load(NS_TOKEN, token).map_err(|_| STORE_DOWN.to_string())?;
            // A session that has run out is, to the client, a token nobody
            // knows: it throws it away and signs in again.
            let Some(record) = record else { return Err("unknown token".into()) };
            let (player, until) = record.split_once('|').unwrap_or((&record, ""));
            let until: u64 = until.parse().unwrap_or(u64::MAX);
            if until < unix_now() {
                store.delete(NS_TOKEN, token);
                return Err("unknown token".into());
            }
            // Used: it lasts from now. Rewritten only once half its life is
            // gone, so a sign-in is not always a write.
            if until != u64::MAX && until - unix_now() < self.session_secs / 2 {
                store.save(NS_TOKEN, token, &self.session_record(player));
            }
            return Ok(Identity {
                player: player.to_string(),
                name: text("name").to_string(),
                token: Some(token.to_string()),
            });
        }
        if text("account").is_empty() {
            return if self.allow_guests {
                let player = format!("p_{}", hex(&random(8)));
                Ok(Identity { token: Some(self.guest_session(store, &player)), player, name: text("name").to_string() })
            } else {
                Err("sign in with an account".into())
            };
        }

        let name = account_name(text("account"))?;
        let key = name.to_lowercase();
        let password = text("password");
        let existing = Self::account(store, &key)?;

        if hello["recover"] == true {
            return self.send_recovery(store, &key, existing.as_ref());
        }
        if hello.get("code").and_then(Value::as_str).is_some_and(|c| !c.is_empty()) {
            return self.use_recovery(store, &key, hello, existing);
        }

        if hello["register"].as_bool().unwrap_or(false) {
            if existing.is_some() {
                return Err("that name is taken".into());
            }
            let player = format!("p_{}", hex(&random(8)));
            let mut account = json!({ "player": player, "name": name, "sessions": [] });
            self.set_password(&mut account, password)?;
            store.save(NS_ACCOUNT_OF, &player, &key);
            let token = self.session(store, &key, &mut account);
            return Ok(Identity { token: Some(token), player, name });
        }

        // Locked, after too many wrong tries: not even the right password
        // gets in until it has passed, or guessing would only have to wait
        // for the guess that works.
        let (wrong, locked_until) = Self::tries(store, &key);
        if locked_until > unix_now() {
            return Err("too many wrong tries; wait a while and try again".into());
        }

        // One answer for "no such account" and "wrong password": which names
        // exist is not something to tell a stranger.
        if !self.matches(existing.as_ref(), password) {
            // Kept in the store, so every node of a cluster counts the same
            // tries: a guesser cannot have five on each.
            let wrong = wrong + 1;
            let until = if wrong >= TRIES {
                let minutes = 1u64 << (wrong - TRIES).min(4);
                unix_now() + 60 * minutes.min(15)
            } else {
                0
            };
            store.save(NS_TRIES, &key, &format!("{wrong}|{until}"));
            return Err("wrong account or password".to_string());
        }
        if wrong > 0 {
            store.delete(NS_TRIES, &key);
        }
        let Some(mut account) = existing else { return Err("wrong account or password".into()) };
        let Some(player) = account["player"].as_str().map(str::to_string) else {
            return Err("wrong account or password".into());
        };
        // Accounts made before sessions were listed have no way back from
        // their player; give them one.
        store.save(NS_ACCOUNT_OF, &player, &key);
        let token = self.session(store, &key, &mut account);
        Ok(Identity { token: Some(token), player, name: account["name"].as_str().unwrap_or(&name).to_string() })
    }

    fn manage(&self, player: &str, request: &Value, store: &dyn Store) -> Result<Value, String> {
        let (key, mut account) = Self::account_of(store, player)?;
        if let Some(change) = request.get("change_password") {
            if !self.matches(Some(&account), change["old"].as_str().unwrap_or("")) {
                return Err("that is not the current password".into());
            }
            self.set_password(&mut account, change["new"].as_str().unwrap_or(""))?;
        } else if request["sign_out_everywhere"] != true {
            return Err("an account request is change_password or sign_out_everywhere".into());
        }
        // Either way every session ends, and the one who asked gets a new one.
        Self::end_sessions(store, &mut account);
        let token = self.session(store, &key, &mut account);
        Ok(json!({ "ok": true, "token": token }))
    }

    fn reset_password(&self, account: &str, password: &str, store: &dyn Store) -> Result<(), String> {
        let key = account.trim().to_lowercase();
        let mut found = Self::account(store, &key)?.ok_or_else(|| "no such account".to_string())?;
        self.set_password(&mut found, password)?;
        Self::end_sessions(store, &mut found);
        store.save(NS_ACCOUNT, &key, &found.to_string());
        store.delete(NS_TRIES, &key);
        Ok(())
    }

    fn delete_account(&self, account: &str, store: &dyn Store) -> Result<String, String> {
        let key = account.trim().to_lowercase();
        let mut found = Self::account(store, &key)?.ok_or_else(|| "no such account".to_string())?;
        Self::end_sessions(store, &mut found);
        let player = found["player"].as_str().unwrap_or("").to_string();
        store.delete(NS_ACCOUNT, &key);
        store.delete(NS_ACCOUNT_OF, &player);
        store.delete(NS_RECOVERY, &key);
        Ok(player)
    }

    fn sign_out(&self, player: &str, store: &dyn Store) -> Result<(), String> {
        let (key, mut account) = Self::account_of(store, player)?;
        Self::end_sessions(store, &mut account);
        store.save(NS_ACCOUNT, &key, &account.to_string());
        Ok(())
    }
}

// ---- accounts somebody else keeps -----------------------------------------------

/// Believes tickets signed with a shared secret.
///
/// Your backend - the thing with the real accounts - makes one with
/// [`SignedAuth::ticket`] (or the same few lines in any language: the payload
/// is JSON, hex-encoded, then `.`, then the hex HMAC-SHA256 of that hex) and
/// hands it to the game, which presents it as the `token` of its `hello`. A
/// ticket names the player, so the same person is the same player on every
/// device, and it expires, so a stolen one is not forever.
///
/// **A standard JWT is a ticket too**: HS256 over the same secret, the
/// player in `sub` (or `player`), the name in `name` (or
/// `preferred_username`), `exp` required and `nbf` honoured - what most
/// account backends already issue. [`SignedAuth::issuer`] and
/// [`SignedAuth::audience`] make `iss` and `aud` count too. Only HS256 is
/// taken: a token naming any other algorithm, `none` above all, is refused.
///
/// **A ticket can be revoked** before it expires: [`SignedAuth::revoke`]
/// (or the dashboard's sign-out of a player) refuses every ticket for that
/// player issued (`iat`) until that moment, in every node sharing the store.
pub struct SignedAuth {
    secret: Vec<u8>,
    issuer: Option<String>,
    audience: Option<String>,
}

impl SignedAuth {
    pub fn new(secret: &str) -> Self {
        SignedAuth { secret: secret.as_bytes().to_vec(), issuer: None, audience: None }
    }

    /// Take only JWTs whose `iss` is this.
    pub fn issuer(mut self, iss: &str) -> Self {
        self.issuer = Some(iss.to_string());
        self
    }

    /// Take only JWTs whose `aud` is (or includes) this.
    pub fn audience(mut self, aud: &str) -> Self {
        self.audience = Some(aud.to_string());
        self
    }

    fn sign(secret: &[u8], payload: &str) -> Vec<u8> {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC takes a key of any length");
        mac.update(payload.as_bytes());
        mac.finalize().into_bytes().to_vec()
    }

    /// A ticket saying `player` (called `name`) may sign in for `valid_secs`.
    pub fn ticket(secret: &str, player: &str, name: &str, valid_secs: u64) -> String {
        let now = unix_now();
        let payload =
            hex(json!({ "player": player, "name": name, "iat": now, "exp": now + valid_secs }).to_string().as_bytes());
        let signature = hex(&Self::sign(secret.as_bytes(), &payload));
        format!("{payload}.{signature}")
    }

    /// The same as a standard HS256 JWT - `sub`, `name`, `iat`, `exp` - for
    /// a backend that would rather issue those (or a test of one).
    pub fn jwt(secret: &str, player: &str, name: &str, valid_secs: u64) -> String {
        let now = unix_now();
        let header = b64url(br#"{"alg":"HS256","typ":"JWT"}"#);
        let claims = b64url(json!({ "sub": player, "name": name, "iat": now, "exp": now + valid_secs }).to_string().as_bytes());
        let signed = format!("{header}.{claims}");
        let signature = b64url(&Self::sign(secret.as_bytes(), &signed));
        format!("{signed}.{signature}")
    }

    /// Refuse every ticket for `player` issued until now. Kept in the store,
    /// so every node of a cluster refuses them; a ticket issued afterwards
    /// is believed as usual.
    pub fn revoke(store: &dyn Store, player: &str) {
        store.save(NS_REVOKED, player, &unix_now().to_string());
    }

    /// The claims of one of our own tickets (`hex.hex`), if its signature is ours.
    fn own_claims(&self, ticket: &str) -> Result<Value, String> {
        let Some((payload, signature)) = ticket.split_once('.') else {
            return Err("sign in with a ticket".into());
        };
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).expect("HMAC takes a key of any length");
        mac.update(payload.as_bytes());
        // `verify_slice` compares in constant time.
        if unhex(signature).is_none_or(|s| mac.verify_slice(&s).is_err()) {
            return Err("that ticket is not ours".into());
        }
        unhex(payload).and_then(|bytes| serde_json::from_slice(&bytes).ok()).ok_or_else(|| "that ticket is not ours".into())
    }

    /// The claims of an HS256 JWT, if its signature is ours and its
    /// issuer and audience are the ones asked for.
    fn jwt_claims(&self, token: &str) -> Result<Value, String> {
        const NOT_OURS: &str = "that ticket is not ours";
        let mut parts = token.split('.');
        let (Some(header), Some(claims), Some(signature), None) = (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(NOT_OURS.into());
        };
        let head: Value = unb64url(header).and_then(|b| serde_json::from_slice(&b).ok()).ok_or(NOT_OURS)?;
        if head["alg"] != "HS256" {
            return Err("a ticket is taken signed with HS256 only".into());
        }
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).expect("HMAC takes a key of any length");
        mac.update(header.as_bytes());
        mac.update(b".");
        mac.update(claims.as_bytes());
        if unb64url(signature).is_none_or(|s| mac.verify_slice(&s).is_err()) {
            return Err(NOT_OURS.into());
        }
        let claims: Value = unb64url(claims).and_then(|b| serde_json::from_slice(&b).ok()).ok_or(NOT_OURS)?;
        if let Some(iss) = &self.issuer
            && claims["iss"].as_str() != Some(iss.as_str())
        {
            return Err("that ticket is from another issuer".into());
        }
        if let Some(aud) = &self.audience {
            let ours = match &claims["aud"] {
                Value::String(one) => one == aud,
                Value::Array(many) => many.iter().any(|a| a.as_str() == Some(aud.as_str())),
                _ => false,
            };
            if !ours {
                return Err("that ticket is for somebody else".into());
            }
        }
        if claims["nbf"].as_u64().is_some_and(|nbf| nbf > unix_now() + 60) {
            return Err("that ticket is not good yet".into());
        }
        Ok(claims)
    }
}

/// Base64url without padding, as a JWT writes it.
fn b64url(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(ABC[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// Base64url (padded or not) back to bytes; `None` for anything else.
fn unb64url(text: &str) -> Option<Vec<u8>> {
    let text = text.trim_end_matches('=');
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        } as u32)
    };
    if text.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    for chunk in text.as_bytes().chunks(4) {
        let n = chunk.iter().enumerate().try_fold(0u32, |n, (i, c)| Some(n | (value(*c)? << (18 - 6 * i))))?;
        for i in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

impl Authenticator for SignedAuth {
    fn authenticate(&self, hello: &Value, store: &dyn Store) -> Result<Identity, String> {
        let ticket = hello.get("token").and_then(Value::as_str).unwrap_or("");
        let claims = match ticket.matches('.').count() {
            2 => self.jwt_claims(ticket)?,
            _ => self.own_claims(ticket)?,
        };
        if claims["exp"].as_u64().unwrap_or(0) < unix_now() {
            return Err("that ticket has expired".into());
        }
        let player = claims["player"].as_str().or_else(|| claims["sub"].as_str()).filter(|p| !p.is_empty());
        let Some(player) = player else {
            return Err("that ticket names nobody".into());
        };
        // Revoked: every ticket issued until a moment is refused. A ticket
        // that says nothing of when it was issued is as old as can be.
        let revoked = store.try_load(NS_REVOKED, player).map_err(|_| STORE_DOWN.to_string())?;
        if let Some(until) = revoked.and_then(|t| t.parse::<u64>().ok())
            && claims["iat"].as_u64().unwrap_or(0) <= until
        {
            return Err("that ticket was revoked".into());
        }
        let name = claims["name"].as_str().or_else(|| claims["preferred_username"].as_str()).unwrap_or("");
        Ok(Identity {
            player: player.to_string(),
            name: name.to_string(),
            // The ticket is what the client presents again; there is nothing
            // of ours for it to keep instead.
            token: Some(ticket.to_string()),
        })
    }

    fn sign_out(&self, player: &str, store: &dyn Store) -> Result<(), String> {
        Self::revoke(store, player);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemoryStore;

    fn quick() -> PasswordAuth {
        // Fewer rounds than anyone should ship: these tests are about the
        // rules, and a debug build is slow at hashing.
        PasswordAuth::new().iterations(50)
    }

    #[test]
    fn an_account_is_the_same_player_from_anywhere() {
        let (auth, store) = (quick(), MemoryStore::new());
        let made = auth
            .authenticate(&json!({ "account": "Ada", "password": "correct horse", "register": true }), &store)
            .unwrap();
        let again = auth.authenticate(&json!({ "account": "ada", "password": "correct horse" }), &store).unwrap();
        assert_eq!(made.player, again.player, "the name, in any case, is the player");
        assert_eq!(again.name, "Ada", "spelled as it was registered");
        assert_ne!(made.token, again.token, "each sign-in is its own session");

        // Either session token is her, with no password.
        for token in [made.token.unwrap(), again.token.unwrap()] {
            assert_eq!(auth.authenticate(&json!({ "token": token }), &store).unwrap().player, made.player);
        }
    }

    #[test]
    fn a_wrong_password_and_a_name_nobody_has_get_the_same_answer() {
        let (auth, store) = (quick(), MemoryStore::new());
        auth.authenticate(&json!({ "account": "ada", "password": "correct horse", "register": true }), &store).unwrap();
        let wrong = auth.authenticate(&json!({ "account": "ada", "password": "battery staple" }), &store).unwrap_err();
        let nobody = auth.authenticate(&json!({ "account": "bob", "password": "battery staple" }), &store).unwrap_err();
        assert_eq!(wrong, nobody);
        assert_eq!(wrong, "wrong account or password");
    }

    #[test]
    fn what_is_refused_at_registration() {
        let (auth, store) = (quick(), MemoryStore::new());
        let try_it = |hello: Value| auth.authenticate(&hello, &store).unwrap_err();
        assert!(try_it(json!({ "account": "a", "password": "long enough", "register": true })).contains("3 to 24"));
        assert!(try_it(json!({ "account": "ada/..", "password": "long enough", "register": true })).contains("3 to 24"));
        assert!(try_it(json!({ "account": "ada", "password": "short", "register": true })).contains("8 characters"));
        auth.authenticate(&json!({ "account": "ada", "password": "long enough", "register": true }), &store).unwrap();
        assert_eq!(try_it(json!({ "account": "ADA", "password": "another one", "register": true })), "that name is taken");
        assert_eq!(try_it(json!({ "name": "guest" })), "sign in with an account");
        assert!(quick().allow_guests().authenticate(&json!({ "name": "guest" }), &store).is_ok());
    }

    #[test]
    fn the_password_itself_is_never_kept() {
        let (auth, store) = (quick(), MemoryStore::new());
        auth.authenticate(&json!({ "account": "ada", "password": "correct horse", "register": true }), &store).unwrap();
        let kept = store.load(NS_ACCOUNT, "ada").unwrap();
        assert!(!kept.contains("correct horse"), "{kept}");
        let other = MemoryStore::new();
        auth.authenticate(&json!({ "account": "ada", "password": "correct horse", "register": true }), &other).unwrap();
        let hash = |text: &str| serde_json::from_str::<Value>(text).unwrap()["hash"].clone();
        assert_ne!(hash(&kept), hash(&other.load(NS_ACCOUNT, "ada").unwrap()), "the same password, salted, hashes differently");
    }

    #[test]
    fn changing_the_password_ends_every_other_session() {
        let (auth, store) = (quick(), MemoryStore::new());
        let hello = |hello: Value| auth.authenticate(&hello, &store);
        let phone = hello(json!({ "account": "ada", "password": "correct horse", "register": true })).unwrap();
        let laptop = hello(json!({ "account": "ada", "password": "correct horse" })).unwrap();

        let refused = auth.manage(&phone.player, &json!({ "change_password": { "old": "wrong", "new": "battery staple" } }), &store);
        assert_eq!(refused.unwrap_err(), "that is not the current password");
        let changed = auth
            .manage(&phone.player, &json!({ "change_password": { "old": "correct horse", "new": "battery staple" } }), &store)
            .unwrap();

        for old in [phone.token.unwrap(), laptop.token.unwrap()] {
            assert_eq!(hello(json!({ "token": old })).unwrap_err(), "unknown token", "a session from before is over");
        }
        assert_eq!(hello(json!({ "token": changed["token"] })).unwrap().player, phone.player, "the one who asked has a new one");
        assert!(hello(json!({ "account": "ada", "password": "correct horse" })).is_err());
        assert!(hello(json!({ "account": "ada", "password": "battery staple" })).is_ok());

        let out = auth.manage(&phone.player, &json!({ "sign_out_everywhere": true }), &store).unwrap();
        assert_eq!(hello(json!({ "token": changed["token"] })).unwrap_err(), "unknown token");
        assert!(hello(json!({ "token": out["token"] })).is_ok());
    }

    #[test]
    fn a_session_runs_out_and_an_account_keeps_only_its_newest() {
        let store = MemoryStore::new();
        let brief = PasswordAuth::new().iterations(50).session_secs(0);
        let made = brief.authenticate(&json!({ "account": "ada", "password": "correct horse", "register": true }), &store).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_eq!(brief.authenticate(&json!({ "token": made.token }), &store).unwrap_err(), "unknown token");

        let few = PasswordAuth::new().iterations(50).max_sessions(2);
        let tokens: Vec<String> = (0..3)
            .map(|_| few.authenticate(&json!({ "account": "ada", "password": "correct horse" }), &store).unwrap().token.unwrap())
            .collect();
        assert_eq!(few.authenticate(&json!({ "token": tokens[0] }), &store).unwrap_err(), "unknown token", "the oldest gave way");
        assert!(few.authenticate(&json!({ "token": tokens[2] }), &store).is_ok());
    }

    #[test]
    fn guessing_locks_the_account_even_for_the_right_password() {
        let (auth, store) = (quick(), MemoryStore::new());
        auth.authenticate(&json!({ "account": "ada", "password": "correct horse", "register": true }), &store).unwrap();
        for n in 0..5 {
            let why = auth.authenticate(&json!({ "account": "ada", "password": format!("guess {n}") }), &store).unwrap_err();
            assert_eq!(why, "wrong account or password");
        }
        let locked = auth.authenticate(&json!({ "account": "ada", "password": "correct horse" }), &store).unwrap_err();
        assert!(locked.starts_with("too many wrong tries"), "{locked}");
        // An operator's reset is also the way back in.
        auth.reset_password("Ada", "a new password", &store).unwrap();
        assert!(auth.authenticate(&json!({ "account": "ada", "password": "a new password" }), &store).is_ok());
    }

    #[test]
    fn two_servers_sharing_a_store_count_wrong_tries_together() {
        let store = MemoryStore::new();
        let (one, two) = (quick(), quick());
        one.authenticate(&json!({ "account": "ada", "password": "correct horse", "register": true }), &store).unwrap();
        for n in 0..5 {
            // Alternating nodes, as a guesser behind a load balancer would.
            let node = if n % 2 == 0 { &one } else { &two };
            let _ = node.authenticate(&json!({ "account": "ada", "password": format!("guess {n}") }), &store);
        }
        for node in [&one, &two] {
            let locked = node.authenticate(&json!({ "account": "ada", "password": "correct horse" }), &store).unwrap_err();
            assert!(locked.starts_with("too many wrong tries"), "{locked}");
        }
    }

    #[test]
    fn an_account_can_be_deleted() {
        let (auth, store) = (quick(), MemoryStore::new());
        let made = auth.authenticate(&json!({ "account": "ada", "password": "correct horse", "register": true }), &store).unwrap();
        assert_eq!(auth.delete_account("ada", &store).unwrap(), made.player);
        assert_eq!(auth.authenticate(&json!({ "token": made.token }), &store).unwrap_err(), "unknown token");
        assert_eq!(auth.authenticate(&json!({ "account": "ada", "password": "correct horse" }), &store).unwrap_err(), "wrong account or password");
        assert!(auth.delete_account("ada", &store).is_err());
    }

    #[test]
    fn a_signed_ticket_is_believed_and_nothing_else_is() {
        let (auth, store) = (SignedAuth::new("shared-secret"), MemoryStore::new());
        let ticket = SignedAuth::ticket("shared-secret", "user-42", "Ada", 60);
        let who = auth.authenticate(&json!({ "token": ticket }), &store).unwrap();
        assert_eq!((who.player.as_str(), who.name.as_str()), ("user-42", "Ada"));
        assert_eq!(who.token.as_deref(), Some(ticket.as_str()), "the ticket is what the client keeps");

        let try_it = |token: &str| auth.authenticate(&json!({ "token": token }), &store).unwrap_err();
        assert_eq!(try_it(&SignedAuth::ticket("another-secret", "user-42", "Ada", 60)), "that ticket is not ours");
        // The payload changed, the signature kept: user-42 claiming to be user-43.
        let (payload, signature) = ticket.split_once('.').unwrap();
        let forged = payload.replace(&hex(b"user-42"), &hex(b"user-43"));
        assert_ne!(forged, payload);
        assert_eq!(try_it(&format!("{forged}.{signature}")), "that ticket is not ours");
        assert_eq!(try_it(""), "sign in with a ticket");
    }

    /// S13: a player who forgot their password gets back in by a code the
    /// game delivers, and nothing about it helps a stranger.
    #[test]
    fn a_forgotten_password_is_recovered_by_a_code_the_game_delivers() {
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Recovery>::new()));
        let outbox = sent.clone();
        let auth = quick().recovery(move |r| {
            outbox.lock().unwrap().push(r.clone());
            Ok(())
        });
        let store = MemoryStore::new();
        let ada = auth.authenticate(&json!({ "account": "Ada", "password": "correct horse", "register": true }), &store).unwrap();

        // Asked for: the same answer for an account that is and one that is not.
        let asked = auth.authenticate(&json!({ "account": "ada", "recover": true }), &store).unwrap_err();
        let nobody = auth.authenticate(&json!({ "account": "bob", "recover": true }), &store).unwrap_err();
        assert_eq!((asked.as_str(), nobody.as_str()), (RECOVERY_SENT, RECOVERY_SENT));
        let code = {
            let sent = sent.lock().unwrap();
            assert_eq!(sent.len(), 1, "only the real account was sent a code");
            assert_eq!((sent[0].account.as_str(), sent[0].player.as_str()), ("Ada", ada.player.as_str()));
            assert_eq!(sent[0].code.len(), 8);
            sent[0].code.clone()
        };
        // Asked for again at once: no second message to flood an inbox with.
        let _ = auth.authenticate(&json!({ "account": "ada", "recover": true }), &store);
        assert_eq!(sent.lock().unwrap().len(), 1);
        assert!(!store.load(NS_RECOVERY, "ada").unwrap().contains(&code), "the code itself is never kept");

        let wrong = code.chars().rev().collect::<String>();
        let wrong = if wrong == code { "00000000".to_string() } else { wrong };
        let refused = auth.authenticate(&json!({ "account": "ada", "code": wrong, "password": "battery staple" }), &store);
        assert_eq!(refused.unwrap_err(), "that recovery code is not right, or has run out");
        let back = auth
            .authenticate(&json!({ "account": "ada", "code": code, "password": "battery staple" }), &store)
            .unwrap();
        assert_eq!(back.player, ada.player, "the same player, signed in");
        assert_eq!(auth.authenticate(&json!({ "token": ada.token }), &store).unwrap_err(), "unknown token", "every other session ended");
        assert!(auth.authenticate(&json!({ "account": "ada", "password": "battery staple" }), &store).is_ok());
        let again = auth.authenticate(&json!({ "account": "ada", "code": code, "password": "a third one!" }), &store);
        assert!(again.is_err(), "a code works once");

        // Five wrong codes burn one.
        store.delete(NS_RECOVERY, "ada");
        let _ = auth.authenticate(&json!({ "account": "ada", "recover": true }), &store);
        let code = sent.lock().unwrap().last().unwrap().code.clone();
        for n in 0..5 {
            let guess = format!("{:08}", (code.parse::<u64>().unwrap() + 1 + n) % 100_000_000);
            let _ = auth.authenticate(&json!({ "account": "ada", "code": guess, "password": "battery staple" }), &store);
        }
        assert!(auth.authenticate(&json!({ "account": "ada", "code": code, "password": "battery staple" }), &store).is_err());

        // Without a way to deliver one, there is none.
        assert!(quick().authenticate(&json!({ "account": "ada", "recover": true }), &store).unwrap_err().contains("operator"));
    }

    #[test]
    fn an_operator_signs_a_player_out_everywhere() {
        let (auth, store) = (quick(), MemoryStore::new());
        let phone = auth.authenticate(&json!({ "account": "ada", "password": "correct horse", "register": true }), &store).unwrap();
        let laptop = auth.authenticate(&json!({ "account": "ada", "password": "correct horse" }), &store).unwrap();
        auth.sign_out(&phone.player, &store).unwrap();
        for token in [phone.token, laptop.token] {
            assert_eq!(auth.authenticate(&json!({ "token": token }), &store).unwrap_err(), "unknown token");
        }
        assert!(auth.authenticate(&json!({ "account": "ada", "password": "correct horse" }), &store).is_ok());
    }

    /// S13: a ticket can be taken back before it expires - every ticket for
    /// that player issued until then - and a fresh one is believed.
    #[test]
    fn a_ticket_can_be_revoked_before_it_runs_out() {
        let (auth, store) = (SignedAuth::new("shared-secret"), MemoryStore::new());
        let old = SignedAuth::ticket("shared-secret", "user-42", "Ada", 3600);
        let old_jwt = SignedAuth::jwt("shared-secret", "user-42", "Ada", 3600);
        let other = SignedAuth::ticket("shared-secret", "user-43", "Bob", 3600);
        assert!(auth.authenticate(&json!({ "token": old }), &store).is_ok());
        auth.sign_out("user-42", &store).unwrap();
        for ticket in [&old, &old_jwt] {
            assert_eq!(auth.authenticate(&json!({ "token": ticket }), &store).unwrap_err(), "that ticket was revoked");
        }
        assert!(auth.authenticate(&json!({ "token": other }), &store).is_ok(), "nobody else's");
        // A ticket from before `iat` existed is as old as can be.
        let payload = hex(json!({ "player": "user-42", "name": "Ada", "exp": unix_now() + 60 }).to_string().as_bytes());
        let ancient = format!("{payload}.{}", hex(&SignedAuth::sign(b"shared-secret", &payload)));
        assert!(auth.authenticate(&json!({ "token": ancient }), &store).is_err());
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let fresh = SignedAuth::ticket("shared-secret", "user-42", "Ada", 3600);
        assert_eq!(auth.authenticate(&json!({ "token": fresh }), &store).unwrap().player, "user-42", "issued after: believed");
    }

    /// S13: a standard HS256 JWT signs a player in; anything else that
    /// looks like one - another key, another algorithm, `none`, another
    /// issuer or audience - does not.
    #[test]
    fn a_standard_jwt_is_a_ticket() {
        let store = MemoryStore::new();
        let auth = SignedAuth::new("shared-secret");
        let jwt = SignedAuth::jwt("shared-secret", "user-42", "Ada", 60);
        let who = auth.authenticate(&json!({ "token": jwt }), &store).unwrap();
        assert_eq!((who.player.as_str(), who.name.as_str()), ("user-42", "Ada"));
        let try_it = |auth: &SignedAuth, token: &str| auth.authenticate(&json!({ "token": token }), &store);
        assert_eq!(try_it(&auth, &SignedAuth::jwt("another", "user-42", "Ada", 60)).unwrap_err(), "that ticket is not ours");

        // Made by hand, as another backend would: a known JWT library's shape.
        let make = |header: &str, claims: Value, secret: &str| {
            let signed = format!("{}.{}", b64url(header.as_bytes()), b64url(claims.to_string().as_bytes()));
            format!("{signed}.{}", b64url(&SignedAuth::sign(secret.as_bytes(), &signed)))
        };
        let claims = json!({ "sub": "user-7", "preferred_username": "Cy", "exp": unix_now() + 60, "iss": "acct", "aud": ["game", "web"] });
        let good = make(r#"{"alg":"HS256","typ":"JWT"}"#, claims.clone(), "shared-secret");
        assert_eq!(try_it(&auth, &good).unwrap().name, "Cy");
        let strict = SignedAuth::new("shared-secret").issuer("acct").audience("game");
        assert!(try_it(&strict, &good).is_ok());
        assert!(try_it(&SignedAuth::new("shared-secret").issuer("other"), &good).is_err());
        assert!(try_it(&SignedAuth::new("shared-secret").audience("admin"), &good).is_err());
        let none = format!("{}.{}.", b64url(br#"{"alg":"none"}"#), b64url(claims.to_string().as_bytes()));
        assert!(try_it(&auth, &none).unwrap_err().contains("HS256"), "an unsigned token is never believed");
        let hs512 = make(r#"{"alg":"HS512"}"#, claims.clone(), "shared-secret");
        assert!(try_it(&auth, &hs512).is_err());
        let expired = make(r#"{"alg":"HS256"}"#, json!({ "sub": "user-7", "exp": unix_now() - 5 }), "shared-secret");
        assert_eq!(try_it(&auth, &expired).unwrap_err(), "that ticket has expired");
        let early = make(r#"{"alg":"HS256"}"#, json!({ "sub": "user-7", "exp": unix_now() + 900, "nbf": unix_now() + 600 }), "shared-secret");
        assert_eq!(try_it(&auth, &early).unwrap_err(), "that ticket is not good yet");
        // The base64url both ways, on lengths that leave every remainder.
        for bytes in [&b""[..], b"a", b"ab", b"abc", b"abcd", &[0xff, 0xfe, 0xfd, 0x00]] {
            assert_eq!(unb64url(&b64url(bytes)).unwrap(), bytes);
        }
    }

    #[test]
    fn a_ticket_runs_out() {
        let (auth, store) = (SignedAuth::new("shared-secret"), MemoryStore::new());
        // Signed an hour ago for a minute: made by hand, since `ticket` only
        // makes ones that start now.
        let payload = hex(json!({ "player": "user-42", "name": "Ada", "exp": unix_now() - 3600 }).to_string().as_bytes());
        let ticket = format!("{payload}.{}", hex(&SignedAuth::sign(b"shared-secret", &payload)));
        assert_eq!(auth.authenticate(&json!({ "token": ticket }), &store).unwrap_err(), "that ticket has expired");
    }
}
