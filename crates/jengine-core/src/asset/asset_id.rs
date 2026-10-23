//! An asset's id, interned: `AssetId` is a `Copy` handle that compares by
//! index and reads as the `&str` it was made from. A sprite's texture
//! ([`TextureId`]) and its material ([`MaterialId`]) are both one.
//!
//! The renderer sorts and batches every drawable by texture and material each
//! frame. With the ids owned `String`s per component, that was a string
//! compare per comparison - and in a browser, where the showcase's Ten
//! Thousand Lights sorts ten thousand lanterns of one texture and one
//! material, `memcmp` alone was ~5% of the frame (`docs/architecture-debt.md`,
//! #1). Hashing the ids once a frame to rank them would cost what it saved;
//! only ids interned before the frame pay nothing. So an id is interned when
//! it is made - deserialised, set by a script, written by a package - and
//! equal ids share one string, which is what lets [`same_str`] answer by
//! pointer.
//!
//! **Order is the string's**: two ids compare by index when equal and by
//! their text when not, so anything sorted by them sorts exactly as it did
//! when they were `String`s, and no draw order moved.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// An interned asset id. Serialises as the string it names.
#[derive(Clone, Copy)]
pub struct AssetId {
    index: u32,
    text: &'static str,
}

/// A texture's id: what a sprite, mesh, trail or line draws.
pub type TextureId = AssetId;
/// A material's id: the `MaterialLibrary` entry a `material` component names.
pub type MaterialId = AssetId;

struct Interner {
    by_text: HashMap<&'static str, u32>,
}

fn interner() -> &'static Mutex<Interner> {
    static INTERNER: OnceLock<Mutex<Interner>> = OnceLock::new();
    INTERNER.get_or_init(|| Mutex::new(Interner { by_text: HashMap::new() }))
}

impl AssetId {
    /// The id for `text`, interned: the same text always gives the same
    /// index and the same `&'static str`. Ids are few - one per asset a
    /// project names - so the table is never pruned.
    pub fn new(text: &str) -> Self {
        let mut table = interner().lock().unwrap_or_else(|e| e.into_inner());
        if let Some((&interned, &index)) = table.by_text.get_key_value(text) {
            return AssetId { index, text: interned };
        }
        let interned: &'static str = Box::leak(text.to_owned().into_boxed_str());
        let index = table.by_text.len() as u32;
        table.by_text.insert(interned, index);
        AssetId { index, text: interned }
    }

    /// The text, shared by every id equal to this one.
    pub fn as_str(&self) -> &'static str {
        self.text
    }

    /// The interned index: equal for equal ids, otherwise meaningless.
    pub fn index(&self) -> u32 {
        self.index
    }
}

/// Two strings equal by pointer first - free for interned ids - then by text.
#[inline]
pub fn same_str(a: &str, b: &str) -> bool {
    (a.as_ptr() == b.as_ptr() && a.len() == b.len()) || a == b
}

/// Two strings ordered as `str::cmp` orders them, with the pointer check
/// first: interned ids that are equal never reach `memcmp`.
#[inline]
pub fn cmp_str(a: &str, b: &str) -> std::cmp::Ordering {
    if a.as_ptr() == b.as_ptr() && a.len() == b.len() { std::cmp::Ordering::Equal } else { a.cmp(b) }
}

impl Default for AssetId {
    fn default() -> Self {
        AssetId::new("")
    }
}

impl std::ops::Deref for AssetId {
    type Target = str;
    fn deref(&self) -> &str {
        self.text
    }
}

impl AsRef<str> for AssetId {
    fn as_ref(&self) -> &str {
        self.text
    }
}

impl std::borrow::Borrow<str> for AssetId {
    fn borrow(&self) -> &str {
        self.text
    }
}

impl PartialEq for AssetId {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}
impl Eq for AssetId {}

impl std::hash::Hash for AssetId {
    // As the text hashes, so `Borrow<str>` lookups agree.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.text.hash(state)
    }
}

impl PartialOrd for AssetId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for AssetId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.index == other.index { std::cmp::Ordering::Equal } else { self.text.cmp(other.text) }
    }
}

impl PartialEq<str> for AssetId {
    fn eq(&self, other: &str) -> bool {
        self.text == other
    }
}
impl PartialEq<&str> for AssetId {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}
impl PartialEq<String> for AssetId {
    fn eq(&self, other: &String) -> bool {
        self.text == other.as_str()
    }
}
impl PartialEq<AssetId> for str {
    fn eq(&self, other: &AssetId) -> bool {
        self == other.text
    }
}
impl PartialEq<AssetId> for &str {
    fn eq(&self, other: &AssetId) -> bool {
        *self == other.text
    }
}
impl PartialEq<AssetId> for String {
    fn eq(&self, other: &AssetId) -> bool {
        self.as_str() == other.text
    }
}

impl From<&str> for AssetId {
    fn from(text: &str) -> Self {
        AssetId::new(text)
    }
}
impl From<&String> for AssetId {
    fn from(text: &String) -> Self {
        AssetId::new(text)
    }
}
impl From<String> for AssetId {
    fn from(text: String) -> Self {
        AssetId::new(&text)
    }
}
impl From<AssetId> for String {
    fn from(id: AssetId) -> Self {
        id.text.to_string()
    }
}

impl std::fmt::Debug for AssetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.text, f)
    }
}
impl std::fmt::Display for AssetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.text)
    }
}

impl serde::Serialize for AssetId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.text)
    }
}
impl<'de> serde::Deserialize<'de> for AssetId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = std::borrow::Cow::<'de, str>::deserialize(d)?;
        Ok(AssetId::new(&text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_text_is_one_id_sharing_one_string() {
        let a = AssetId::new("sprites/lanterns/lantern_warm.png");
        let b = AssetId::from(String::from("sprites/lanterns/lantern_warm.png"));
        assert_eq!(a, b);
        assert_eq!(a.as_str().as_ptr(), b.as_str().as_ptr(), "interned: one allocation");
        assert!(same_str(&a, &b));
        assert_eq!(cmp_str(&a, &b), std::cmp::Ordering::Equal);
    }

    #[test]
    fn ids_order_as_their_text() {
        let mut ids: Vec<AssetId> = ["c.png", "a.png", "b.png", "a.png"].iter().map(|s| AssetId::new(s)).collect();
        ids.sort();
        let texts: Vec<&str> = ids.iter().map(|i| i.as_str()).collect();
        assert_eq!(texts, ["a.png", "a.png", "b.png", "c.png"]);
        assert_eq!(cmp_str("a", "b"), "a".cmp("b"));
    }

    #[test]
    fn it_reads_and_writes_as_its_string() {
        let id: AssetId = serde_json::from_str("\"sprites/hero.png\"").unwrap();
        assert_eq!(id, "sprites/hero.png");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"sprites/hero.png\"");
        assert!(AssetId::default().is_empty());
    }
}
