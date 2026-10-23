//! Where things that outlive a process are kept.
//!
//! One small interface - a namespaced key to a string - because that is all
//! the framework needs (a player's record, a login token, whatever a room
//! wants to remember) and it is what every backend can be: a map in memory, a
//! SQLite file on one node, Postgres or Redis behind several.
//!
//! **Calls are synchronous and are made from room and connection tasks**, so
//! an implementation must answer quickly. SQLite in WAL mode does (tens of
//! microseconds); a store a network away should be put behind
//! [`WriteBehind`] (`Server::write_behind`), which takes every write at once
//! and sends it on a thread of its own, answering reads of what it has not
//! yet sent from what it holds - so a room's autosave never waits on a
//! round trip.

use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};

/// The namespace player records live in. Key: the player id.
pub const NS_PLAYER: &str = "player";
/// Login token -> player id.
pub const NS_TOKEN: &str = "token";

/// What a sign-in is refused with when the store cannot be asked. Not "no
/// such player": a store that did not answer has not said anything.
pub const STORE_DOWN: &str = "the server cannot reach its store; try again shortly";

/// One entry of a board: its key, its score and what was saved with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Scored {
    pub key: String,
    pub score: f64,
    pub value: String,
}

/// Which end of a board is the top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// Most points first.
    Highest,
    /// Fastest time first.
    Lowest,
}

/// Where the scores of a namespace are kept: an ordinary namespace beside
/// it, so every store keeps, batches and carries them as it does anything.
pub fn score_ns(ns: &str) -> String {
    format!("{ns}#score")
}

fn by_order(order: Order) -> impl Fn(&(String, f64), &(String, f64)) -> std::cmp::Ordering {
    move |a, b| {
        let by_score = match order {
            Order::Highest => b.1.total_cmp(&a.1),
            Order::Lowest => a.1.total_cmp(&b.1),
        };
        by_score.then_with(|| a.0.cmp(&b.0))
    }
}

/// Every score in a namespace, best first - the slow way, for a store with
/// no better one.
fn all_scores(store: &(impl Store + ?Sized), ns: &str, order: Order) -> Vec<(String, f64)> {
    let sns = score_ns(ns);
    let mut scores: Vec<(String, f64)> = store
        .keys(&sns)
        .into_iter()
        .filter_map(|key| Some((key.clone(), store.load(&sns, &key)?.parse().ok()?)))
        .collect();
    scores.sort_by(by_order(order));
    scores
}

pub trait Store: Send + Sync + 'static {
    fn load(&self, ns: &str, key: &str) -> Option<String>;

    /// [`Store::load`], for a store that can fail to answer. `Ok(None)` is
    /// "there is no such thing"; `Err` is "I could not find out" - and the
    /// difference is a player's whole save, because the caller that takes a
    /// silence for "nothing there" starts them again from nothing and then
    /// saves that over what they had. A store in this process cannot fail,
    /// so the default is `load`.
    fn try_load(&self, ns: &str, key: &str) -> Result<Option<String>, String> {
        Ok(self.load(ns, key))
    }

    /// Save several things at once. One request for a store that is a
    /// network away; a loop for one that is not.
    fn save_many(&self, items: &[(&str, &str, &str)]) {
        for (ns, key, value) in items {
            self.save(ns, key, value);
        }
    }

    /// Writes that could not be sent yet and are being held. Always zero
    /// for a store that cannot fail.
    fn waiting(&self) -> usize {
        0
    }

    /// Try again whatever could not be written earlier. Called every so
    /// often; nothing to do for a store that cannot fail.
    fn flush(&self) {}
    fn save(&self, ns: &str, key: &str, value: &str);
    fn delete(&self, ns: &str, key: &str);
    /// Every key in a namespace, sorted. For tools and for a room that keeps
    /// a list (the zones of a world); never called per frame. A leaderboard
    /// wants [`Store::top`].
    fn keys(&self, ns: &str) -> Vec<String>;

    /// Save `value` under `key` with a `score` a board is ordered by - a lap
    /// time, a high score. The value and the score go in one write.
    fn save_scored(&self, ns: &str, key: &str, value: &str, score: f64) {
        let sns = score_ns(ns);
        self.save_many(&[(ns, key, value), (&sns, key, &score.to_string())]);
    }

    /// Take an entry off its board, and its value with it.
    fn delete_scored(&self, ns: &str, key: &str) {
        self.delete(ns, key);
        self.delete(&score_ns(ns), key);
    }

    /// The best `n` entries of a board - ties by key - with their values.
    /// Only those values are read, however many entries there are: SQLite
    /// answers in one query, a cluster's store in one request.
    fn top(&self, ns: &str, n: usize, order: Order) -> Vec<Scored> {
        all_scores(self, ns, order)
            .into_iter()
            .take(n)
            .filter_map(|(key, score)| Some(Scored { value: self.load(ns, &key)?, key, score }))
            .collect()
    }

    /// Where `key` stands on a board: its place (0 is the top) and how many
    /// entries there are. `None` for a key with no score.
    fn rank(&self, ns: &str, key: &str, order: Order) -> Option<(usize, usize)> {
        let scores = all_scores(self, ns, order);
        scores.iter().position(|(k, _)| k == key).map(|i| (i, scores.len()))
    }
}

/// A store that forgets when the process ends. Tests, and a relay.
#[derive(Default)]
pub struct MemoryStore {
    map: Mutex<BTreeMap<(String, String), String>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Store for MemoryStore {
    fn load(&self, ns: &str, key: &str) -> Option<String> {
        self.map.lock().unwrap().get(&(ns.to_string(), key.to_string())).cloned()
    }
    fn save(&self, ns: &str, key: &str, value: &str) {
        self.map
            .lock()
            .unwrap()
            .insert((ns.to_string(), key.to_string()), value.to_string());
    }
    fn delete(&self, ns: &str, key: &str) {
        self.map.lock().unwrap().remove(&(ns.to_string(), key.to_string()));
    }
    fn keys(&self, ns: &str) -> Vec<String> {
        self.map
            .lock()
            .unwrap()
            .keys()
            .filter(|(n, _)| n == ns)
            .map(|(_, k)| k.clone())
            .collect()
    }
}

/// One SQLite file: the store for a single node, and enough for a small MMO.
///
/// WAL mode with `synchronous = NORMAL`: a write is durable against a crash of
/// the server and survives everything but power loss in the last moment, at a
/// fraction of the cost of a full sync per save.
#[cfg(feature = "sqlite")]
pub struct SqliteStore {
    conn: Mutex<rusqlite::Connection>,
}

#[cfg(feature = "sqlite")]
impl SqliteStore {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        let conn = rusqlite::Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS kv (
                 ns TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL,
                 PRIMARY KEY (ns, key)
             ) WITHOUT ROWID;",
        )
        .map_err(|e| e.to_string())?;
        Ok(SqliteStore { conn: Mutex::new(conn) })
    }
}

#[cfg(feature = "sqlite")]
impl Store for SqliteStore {
    fn load(&self, ns: &str, key: &str) -> Option<String> {
        self.conn
            .lock()
            .unwrap()
            .query_row("SELECT value FROM kv WHERE ns = ?1 AND key = ?2", [ns, key], |r| r.get(0))
            .ok()
    }
    fn save(&self, ns: &str, key: &str, value: &str) {
        if let Err(e) = self.conn.lock().unwrap().execute(
            "INSERT INTO kv (ns, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT (ns, key) DO UPDATE SET value = excluded.value",
            [ns, key, value],
        ) {
            log::error!("[store] could not save {ns}/{key}: {e}");
        }
    }
    fn delete(&self, ns: &str, key: &str) {
        if let Err(e) = self
            .conn
            .lock()
            .unwrap()
            .execute("DELETE FROM kv WHERE ns = ?1 AND key = ?2", [ns, key])
        {
            log::error!("[store] could not delete {ns}/{key}: {e}");
        }
    }
    fn keys(&self, ns: &str) -> Vec<String> {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) = conn.prepare("SELECT key FROM kv WHERE ns = ?1 ORDER BY key") else {
            return Vec::new();
        };
        stmt.query_map([ns], |r| r.get(0))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }
    fn save_many(&self, items: &[(&str, &str, &str)]) {
        // One transaction: a value and its score are written together.
        let mut conn = self.conn.lock().unwrap();
        let Ok(tx) = conn.transaction() else {
            return log::error!("[store] could not begin a write of {} item(s)", items.len());
        };
        for (ns, key, value) in items {
            if let Err(e) = tx.execute(
                "INSERT INTO kv (ns, key, value) VALUES (?1, ?2, ?3)
                 ON CONFLICT (ns, key) DO UPDATE SET value = excluded.value",
                [ns, key, value],
            ) {
                log::error!("[store] could not save {ns}/{key}: {e}");
            }
        }
        if let Err(e) = tx.commit() {
            log::error!("[store] could not save {} item(s): {e}", items.len());
        }
    }
    fn top(&self, ns: &str, n: usize, order: Order) -> Vec<Scored> {
        let dir = if order == Order::Highest { "DESC" } else { "ASC" };
        let sql = format!(
            "SELECT s.key, CAST(s.value AS REAL), v.value FROM kv s JOIN kv v ON v.ns = ?2 AND v.key = s.key
             WHERE s.ns = ?1 ORDER BY CAST(s.value AS REAL) {dir}, s.key LIMIT ?3"
        );
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) = conn.prepare(&sql) else { return Vec::new() };
        let limit = i64::try_from(n).unwrap_or(i64::MAX);
        stmt.query_map(rusqlite::params![score_ns(ns), ns, limit], |r| {
            Ok(Scored { key: r.get(0)?, score: r.get(1)?, value: r.get(2)? })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }
    fn rank(&self, ns: &str, key: &str, order: Order) -> Option<(usize, usize)> {
        let better = if order == Order::Highest { ">" } else { "<" };
        let sql = format!(
            "SELECT
               (SELECT COUNT(*) FROM kv o WHERE o.ns = ?1 AND (CAST(o.value AS REAL) {better} CAST(m.value AS REAL)
                  OR (CAST(o.value AS REAL) = CAST(m.value AS REAL) AND o.key < m.key))),
               (SELECT COUNT(*) FROM kv a WHERE a.ns = ?1)
             FROM kv m WHERE m.ns = ?1 AND m.key = ?2"
        );
        let conn = self.conn.lock().unwrap();
        conn.query_row(&sql, [&score_ns(ns), key], |r| Ok((r.get::<_, i64>(0)? as usize, r.get::<_, i64>(1)? as usize)))
            .ok()
    }
}

/// A write waiting to go out: the value, or `None` for a delete.
type Pending = BTreeMap<(String, String), Option<String>>;

#[derive(Default)]
struct Queue {
    /// Taken and not yet handed to the store.
    waiting: Pending,
    /// Handed to the store, which has not finished with them.
    sending: Pending,
    stopping: bool,
}

/// A store in front of another that **takes every write at once** and sends
/// it on a thread of its own - the latest value for each key, batched -
/// while reads of anything not yet sent are answered from what it holds,
/// so this server sees its own writes the moment it makes them.
///
/// For a store a network away (a cluster's coordinator, a database
/// elsewhere): a room's autosave and a player's leave cost no round trip.
/// What it does not change is a read of something this server never wrote -
/// a sign-in still asks the store, as it must to see another node's save.
/// [`Store::flush`] waits until everything taken has been handed over, which
/// is what the server does before it sends a player to another node or lets
/// another node have them. A write the inner store cannot make waits there
/// as it would without this ([`Store::waiting`] counts both).
pub struct WriteBehind {
    inner: Arc<dyn Store>,
    queue: Arc<(Mutex<Queue>, Condvar)>,
    writer: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl WriteBehind {
    pub fn new(inner: Arc<dyn Store>) -> Self {
        let queue: Arc<(Mutex<Queue>, Condvar)> = Arc::default();
        let (q, store) = (queue.clone(), inner.clone());
        let writer = std::thread::Builder::new()
            .name("jengine-store-writer".into())
            .spawn(move || Self::write_forever(&q, &*store))
            .expect("a thread for the store's writes");
        WriteBehind { inner, queue, writer: Mutex::new(Some(writer)) }
    }

    fn write_forever(queue: &(Mutex<Queue>, Condvar), store: &dyn Store) {
        let (lock, wake) = queue;
        loop {
            let batch = {
                let mut q = lock.lock().unwrap();
                while q.waiting.is_empty() && !q.stopping {
                    q = wake.wait(q).unwrap();
                }
                if q.waiting.is_empty() && q.stopping {
                    return;
                }
                let batch = std::mem::take(&mut q.waiting);
                q.sending = batch.clone();
                batch
            };
            let saves: Vec<(&str, &str, &str)> = batch
                .iter()
                .filter_map(|((ns, key), value)| value.as_deref().map(|v| (ns.as_str(), key.as_str(), v)))
                .collect();
            if !saves.is_empty() {
                store.save_many(&saves);
            }
            for ((ns, key), value) in &batch {
                if value.is_none() {
                    store.delete(ns, key);
                }
            }
            let mut q = lock.lock().unwrap();
            q.sending.clear();
            wake.notify_all();
        }
    }

    fn put(&self, items: impl IntoIterator<Item = ((String, String), Option<String>)>) {
        let (lock, wake) = &*self.queue;
        lock.lock().unwrap().waiting.extend(items);
        wake.notify_all();
    }

    /// What this store holds for a key it has not finished sending:
    /// `Some(None)` is a delete on its way.
    fn held(&self, ns: &str, key: &str) -> Option<Option<String>> {
        let q = self.queue.0.lock().unwrap();
        let k = (ns.to_string(), key.to_string());
        q.waiting.get(&k).or_else(|| q.sending.get(&k)).cloned()
    }
}

impl Drop for WriteBehind {
    fn drop(&mut self) {
        {
            let (lock, wake) = &*self.queue;
            lock.lock().unwrap().stopping = true;
            wake.notify_all();
        }
        if let Some(writer) = self.writer.lock().unwrap().take() {
            let _ = writer.join();
        }
    }
}

impl Store for WriteBehind {
    fn load(&self, ns: &str, key: &str) -> Option<String> {
        match self.held(ns, key) {
            Some(held) => held,
            None => self.inner.load(ns, key),
        }
    }
    fn try_load(&self, ns: &str, key: &str) -> Result<Option<String>, String> {
        match self.held(ns, key) {
            Some(held) => Ok(held),
            None => self.inner.try_load(ns, key),
        }
    }
    fn save(&self, ns: &str, key: &str, value: &str) {
        self.put([((ns.to_string(), key.to_string()), Some(value.to_string()))]);
    }
    fn save_many(&self, items: &[(&str, &str, &str)]) {
        self.put(items.iter().map(|(ns, key, value)| ((ns.to_string(), key.to_string()), Some(value.to_string()))));
    }
    fn delete(&self, ns: &str, key: &str) {
        self.put([((ns.to_string(), key.to_string()), None)]);
    }
    fn keys(&self, ns: &str) -> Vec<String> {
        let mut keys: std::collections::BTreeSet<String> = self.inner.keys(ns).into_iter().collect();
        let q = self.queue.0.lock().unwrap();
        // What was sent first, then what came after it: the later decides.
        for pending in [&q.sending, &q.waiting] {
            for ((n, key), value) in pending.iter().filter(|((n, _), _)| n == ns) {
                let _ = n;
                match value {
                    Some(_) => keys.insert(key.clone()),
                    None => keys.remove(key),
                };
            }
        }
        keys.into_iter().collect()
    }
    /// Boards are read whole by the inner store, so what is waiting goes
    /// first: a lap just saved is on the board asked for after it.
    fn top(&self, ns: &str, n: usize, order: Order) -> Vec<Scored> {
        self.flush();
        self.inner.top(ns, n, order)
    }
    fn rank(&self, ns: &str, key: &str, order: Order) -> Option<(usize, usize)> {
        self.flush();
        self.inner.rank(ns, key, order)
    }
    fn waiting(&self) -> usize {
        let q = self.queue.0.lock().unwrap();
        q.waiting.len() + q.sending.len() + self.inner.waiting()
    }
    /// Wait - five seconds at most - until everything taken has been handed
    /// to the inner store, then have it send whatever it is holding.
    fn flush(&self) {
        let (lock, wake) = &*self.queue;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut q = lock.lock().unwrap();
        while !(q.waiting.is_empty() && q.sending.is_empty()) {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                log::warn!("[store] {} write(s) still waiting after five seconds", q.waiting.len() + q.sending.len());
                break;
            }
            q = wake.wait_timeout(q, left).unwrap().0;
        }
        drop(q);
        self.inner.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store a slow network away: every write takes 60 ms.
    struct Slow(MemoryStore, std::sync::atomic::AtomicUsize);
    impl Store for Slow {
        fn load(&self, ns: &str, key: &str) -> Option<String> {
            self.0.load(ns, key)
        }
        fn save(&self, ns: &str, key: &str, value: &str) {
            std::thread::sleep(std::time::Duration::from_millis(60));
            self.1.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.0.save(ns, key, value)
        }
        fn save_many(&self, items: &[(&str, &str, &str)]) {
            std::thread::sleep(std::time::Duration::from_millis(60));
            self.1.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            for (ns, key, value) in items {
                self.0.save(ns, key, value);
            }
        }
        fn delete(&self, ns: &str, key: &str) {
            self.0.delete(ns, key)
        }
        fn keys(&self, ns: &str) -> Vec<String> {
            self.0.keys(ns)
        }
    }

    /// S8/K2: a write is taken at once and sent behind; this server reads it
    /// back at once; a flush waits for it; many writes in a burst go as few
    /// requests; and it behaves as any store does.
    #[test]
    fn a_write_behind_store_takes_writes_at_once_and_sends_them_after() {
        let slow = Arc::new(Slow(MemoryStore::new(), Default::default()));
        let store = WriteBehind::new(slow.clone());
        let started = std::time::Instant::now();
        for n in 0..50 {
            store.save("player", &format!("p{n}"), &format!("{{\"n\":{n}}}"));
        }
        assert!(started.elapsed() < std::time::Duration::from_millis(50), "fifty saves waited on nothing: {:?}", started.elapsed());
        assert_eq!(store.load("player", "p49").as_deref(), Some("{\"n\":49}"), "its own write, read back at once");
        store.delete("player", "p3");
        assert_eq!(store.load("player", "p3"), None, "and a delete on its way");
        assert!(!store.keys("player").contains(&"p3".to_string()));
        store.flush();
        assert_eq!(store.waiting(), 0);
        assert_eq!(slow.0.load("player", "p49").as_deref(), Some("{\"n\":49}"), "there once flushed");
        assert_eq!(slow.0.load("player", "p3"), None);
        assert!(slow.1.load(std::sync::atomic::Ordering::Relaxed) < 10, "fifty saves went as a handful of batches");
        exercise(&WriteBehind::new(Arc::new(MemoryStore::new())));
        exercise_boards(&WriteBehind::new(Arc::new(MemoryStore::new())));
        // Dropped with writes waiting: they are written, not lost.
        let inner = Arc::new(MemoryStore::new());
        {
            let store = WriteBehind::new(inner.clone());
            store.save("player", "last", "{}");
        }
        assert_eq!(inner.load("player", "last").as_deref(), Some("{}"));
    }

    fn exercise(store: &dyn Store) {
        assert_eq!(store.load("a", "k"), None);
        store.save("a", "k", "1");
        store.save("a", "k", "2");
        store.save("b", "k", "other");
        assert_eq!(store.load("a", "k").as_deref(), Some("2"), "a save replaces");
        assert_eq!(store.keys("a"), vec!["k".to_string()], "namespaces do not mix");
        store.delete("a", "k");
        assert_eq!(store.load("a", "k"), None);
        assert_eq!(store.load("b", "k").as_deref(), Some("other"));
    }

    /// A board: the best first, ties by key, only the asked-for few read;
    /// a place and a count for anyone on it; re-scoring moves an entry and
    /// taking it off takes its value too.
    pub(crate) fn exercise_boards(store: &dyn Store) {
        for (key, time) in [("ada", 61.5), ("bo", 59.25), ("cy", 70.0), ("di", 59.25), ("ed", 64.0)] {
            store.save_scored("stage:1", key, &format!("{{\"who\":\"{key}\"}}"), time);
        }
        let top = store.top("stage:1", 3, Order::Lowest);
        let keys: Vec<&str> = top.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["bo", "di", "ada"], "fastest first, a tie by key");
        assert_eq!((top[0].score, top[0].value.as_str()), (59.25, "{\"who\":\"bo\"}"));
        let most: Vec<String> = store.top("stage:1", 2, Order::Highest).into_iter().map(|s| s.key).collect();
        assert_eq!(most, ["cy", "ed"]);
        assert_eq!(store.rank("stage:1", "ada", Order::Lowest), Some((2, 5)));
        assert_eq!(store.rank("stage:1", "di", Order::Lowest), Some((1, 5)));
        assert_eq!(store.rank("stage:1", "nobody", Order::Lowest), None);
        store.save_scored("stage:1", "cy", "{}", 50.0);
        assert_eq!(store.rank("stage:1", "cy", Order::Lowest), Some((0, 5)), "a better time moves her up");
        store.delete_scored("stage:1", "bo");
        assert_eq!(store.load("stage:1", "bo"), None);
        assert_eq!(store.top("stage:1", 10, Order::Lowest).len(), 4);
        assert!(store.top("stage:2", 10, Order::Lowest).is_empty(), "boards do not mix");
    }

    #[test]
    fn memory_store_keeps_and_forgets() {
        exercise(&MemoryStore::new());
        exercise_boards(&MemoryStore::new());
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn sqlite_store_keeps_across_a_reopen() {
        let dir = std::env::temp_dir().join(format!("jengine-server-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("store.db");
        let _ = std::fs::remove_file(&path);
        exercise(&SqliteStore::open(&path).unwrap());
        exercise_boards(&SqliteStore::open(&path).unwrap());
        SqliteStore::open(&path).unwrap().save("p", "ada", "{}");
        assert_eq!(SqliteStore::open(&path).unwrap().load("p", "ada").as_deref(), Some("{}"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
