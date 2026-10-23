//! Sprites swapped by name (Oct 2026; Unity's Sprite Library and Sprite
//! Resolver). A `sprite_library` - on a rig's root, usually - holds the
//! parts a figure can wear, by **category** and **label** (`hat` /
//! `cowboy`, `coat` / `raincoat`); a `sprite_resolver` on each part's sprite
//! says which category it is and which label it shows. Change a label and
//! the sprite becomes that part - texture, atlas region, and the size and
//! anchor the entry gives - while the bone it hangs on, its animation and
//! everything else stay as they are.
//!
//! Resolved by [`resolve`], which `resolve_sprite_regions` runs first - the
//! pass every host already makes before drawing, in Play and in the editor's
//! scene view - so no host was wired. A resolver looks for its library on
//! itself and then up its parents, so one library dresses a whole rig and a
//! prefab variant can swap the library for another.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Parent, SpriteComponent};

/// One part: what a sprite showing it draws.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpriteEntry {
    pub texture_id: String,
    /// A region of that texture's atlas, as a sprite's `region`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// The size to draw it at; the sprite's own when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<[f32; 2]>,
    /// Where it hangs from; the sprite's own when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<[f32; 2]>,
}

/// `sprite_library`: parts by category, then label.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpriteLibrary {
    pub categories: BTreeMap<String, BTreeMap<String, SpriteEntry>>,
}

impl SpriteLibrary {
    pub fn entry(&self, category: &str, label: &str) -> Option<&SpriteEntry> {
        self.categories.get(category)?.get(label)
    }

    /// Every texture the library can show - preloaded with the scene, since
    /// nothing else names a part until it is worn.
    pub fn texture_ids(&self) -> impl Iterator<Item = &str> {
        self.categories.values().flat_map(|c| c.values()).map(|e| e.texture_id.as_str()).filter(|s| !s.is_empty())
    }

    /// A key that changes when anything in the library does.
    fn key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(self).unwrap_or_default().hash(&mut h);
        h.finish()
    }
}

/// `sprite_resolver`: which category this sprite is a part of, and which of
/// its labels it shows.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SpriteResolver {
    pub category: String,
    pub label: String,
    /// What it last wrote into the sprite (library, category and label), so
    /// a sprite is rewritten only when one of them changes. Never saved.
    #[serde(skip)]
    pub applied: u64,
}

/// The library a resolver on `e` reads: on `e`, else its nearest parent's.
fn library_of(world: &World, libs: &HashMap<Entity, u64>, mut e: Entity) -> Option<Entity> {
    for _ in 0..64 {
        if libs.contains_key(&e) {
            return Some(e);
        }
        e = world.get::<&Parent>(e).ok()?.0;
    }
    None
}

/// Point every resolver's sprite at its library's entry, where the library,
/// the category or the label changed since it last did.
pub fn resolve(world: &mut World) {
    if world.query::<&SpriteResolver>().iter().next().is_none() {
        return;
    }
    let libs: HashMap<Entity, u64> = world.query::<&SpriteLibrary>().iter().map(|(e, l)| (e, l.key())).collect();
    let mut writes: Vec<(Entity, u64, Option<SpriteEntry>)> = Vec::new();
    for (e, r) in world.query::<&SpriteResolver>().iter() {
        let Some(lib) = library_of(world, &libs, e) else { continue };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (libs[&lib], lib.to_bits().get(), &r.category, &r.label).hash(&mut h);
        let key = h.finish() | 1;
        if key == r.applied {
            continue;
        }
        let entry = world.get::<&SpriteLibrary>(lib).ok().and_then(|l| l.entry(&r.category, &r.label).cloned());
        if entry.is_none() {
            log::warn!("[sprite_library] no '{}' in category '{}'", r.label, r.category);
        }
        writes.push((e, key, entry));
    }
    for (e, key, entry) in writes {
        if let Some(entry) = entry
            && let Ok(mut sprite) = world.get::<&mut SpriteComponent>(e)
        {
            sprite.texture_id = entry.texture_id.as_str().into();
            // A new region resolves afresh (`resolve_sprite_regions`).
            sprite.region = entry.region;
            sprite.uv_rect = None;
            sprite.region_resolved = false;
            if let Some(size) = entry.size {
                sprite.size = Some(size);
            }
            if let Some(anchor) = entry.anchor {
                sprite.anchor = anchor;
            }
        }
        if let Ok(mut r) = world.get::<&mut SpriteResolver>(e) {
            r.applied = key;
        }
    }
}

/// `Sprite.set_label(e, label, category)`: on a part, show `label` (in
/// `category`, if given, else its own); on a library's entity, every part
/// under it whose category has `label` (only `category`'s, if given) shows
/// it - an outfit in one call. The number of parts changed, or why none.
pub fn set_label(world: &World, e: Entity, label: &str, category: Option<&str>) -> Result<usize, String> {
    if let Ok(mut r) = world.get::<&mut SpriteResolver>(e) {
        if let Some(c) = category.filter(|c| !c.is_empty()) {
            r.category = c.to_string();
        }
        r.label = label.to_string();
        return Ok(1);
    }
    let lib = world.get::<&SpriteLibrary>(e).map_err(|_| "the entity has no sprite_resolver or sprite_library".to_string())?;
    let libs: HashMap<Entity, u64> = world.query::<&SpriteLibrary>().iter().map(|(le, _)| (le, 0)).collect();
    let mut n = 0;
    for (part, r) in world.query::<&mut SpriteResolver>().iter() {
        if library_of(world, &libs, part) != Some(e) {
            continue;
        }
        if category.is_some_and(|c| !c.is_empty() && c != r.category) {
            continue;
        }
        if lib.entry(&r.category, label).is_some() {
            r.label = label.to_string();
            n += 1;
        }
    }
    if n == 0 {
        return Err(format!("no part under it has a '{label}'"));
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::Transform;

    fn entry(tex: &str) -> SpriteEntry {
        SpriteEntry { texture_id: tex.into(), ..Default::default() }
    }

    fn doll() -> (World, Entity, Entity, Entity) {
        let mut lib = SpriteLibrary::default();
        let hats = lib.categories.entry("hat".into()).or_default();
        hats.insert("cowboy".into(), entry("hats/cowboy.png"));
        hats.insert("top".into(), SpriteEntry { size: Some([60.0, 80.0]), anchor: Some([0.5, 0.0]), ..entry("hats/top.png") });
        hats.insert("winter".into(), entry("hats/beanie.png"));
        let coats = lib.categories.entry("coat".into()).or_default();
        coats.insert("rain".into(), SpriteEntry { region: Some("rain".into()), ..entry("coats.atlas") });
        coats.insert("winter".into(), entry("coats/parka.png"));
        let mut w = World::new();
        let root = w.spawn((Transform::default(), lib));
        let torso = w.spawn((Transform::default(), Parent(root)));
        let sprite = |tex: &str| SpriteComponent { texture_id: tex.into(), size: Some([40.0, 40.0]), ..Default::default() };
        let hat = w.spawn((Transform::default(), Parent(torso), sprite("x.png"),
                           SpriteResolver { category: "hat".into(), label: "cowboy".into(), applied: 0 }));
        let coat = w.spawn((Transform::default(), Parent(root), sprite("x.png"),
                            SpriteResolver { category: "coat".into(), label: "rain".into(), applied: 0 }));
        (w, root, hat, coat)
    }

    fn tex(w: &World, e: Entity) -> String {
        w.get::<&SpriteComponent>(e).unwrap().texture_id.to_string()
    }

    #[test]
    fn a_part_shows_its_label_from_the_library_up_its_parents() {
        let (mut w, _, hat, coat) = doll();
        resolve(&mut w);
        assert_eq!(tex(&w, hat), "hats/cowboy.png", "the library is two parents up");
        assert_eq!(tex(&w, coat), "coats.atlas");
        assert_eq!(w.get::<&SpriteComponent>(coat).unwrap().region.as_deref(), Some("rain"));
        assert_eq!(w.get::<&SpriteComponent>(hat).unwrap().size, Some([40.0, 40.0]), "no size in the entry keeps the sprite's");

        set_label(&w, hat, "top", None).unwrap();
        resolve(&mut w);
        let s = w.get::<&SpriteComponent>(hat).unwrap();
        assert_eq!((s.texture_id.as_str(), s.size, s.anchor), ("hats/top.png", Some([60.0, 80.0]), [0.5, 0.0]));
    }

    #[test]
    fn an_outfit_is_one_call_on_the_library() {
        let (mut w, root, hat, coat) = doll();
        resolve(&mut w);
        assert_eq!(set_label(&w, root, "winter", None), Ok(2));
        resolve(&mut w);
        assert_eq!((tex(&w, hat), tex(&w, coat)), ("hats/beanie.png".into(), "coats/parka.png".into()));
        assert_eq!(set_label(&w, root, "cowboy", Some("hat")), Ok(1), "one category only");
        assert!(set_label(&w, root, "tuxedo", None).is_err(), "nothing has it");
    }

    #[test]
    fn a_sprite_is_rewritten_only_when_something_changed() {
        let (mut w, root, hat, _) = doll();
        resolve(&mut w);
        // Something else moves the sprite's texture (an animation, a script):
        // the resolver leaves it alone until its own inputs change.
        w.get::<&mut SpriteComponent>(hat).unwrap().texture_id = "painted.png".into();
        resolve(&mut w);
        assert_eq!(tex(&w, hat), "painted.png");
        // Editing the library is a change.
        w.get::<&mut SpriteLibrary>(root).unwrap().categories.get_mut("hat").unwrap().get_mut("cowboy").unwrap().texture_id =
            "hats/cowboy2.png".into();
        resolve(&mut w);
        assert_eq!(tex(&w, hat), "hats/cowboy2.png");
    }
}
