//! The browser hosts' table of script-spawned entities: spawn handle ->
//! entity, plus the reversed views the hosts read every frame.
//!
//! A script holds a handle for what it spawns (`spawn_prefab` answers one,
//! a million and up), and each browser host keeps `handle -> entity` to
//! resolve it. Every frame the hosts also need it the other way round - the
//! script mirror (`entity -> handle`), the ray and nav snapshots and the
//! physics rows (`entity id -> handle`) - and each rebuilt its view by
//! iterating the table into a fresh `HashMap`, three or four times a frame.
//! With ten thousand spawned lanterns (the showcase's Ten Thousand Lights)
//! that was over a tenth of the frame, hashing the same ten thousand entries
//! again and again although nothing had been spawned since the first frame.
//!
//! Here the views are built once per change: reads go through `Deref` to the
//! table as before, every write goes through a method that counts it, and a
//! view is rebuilt only when the count has moved.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use hecs::Entity;

#[derive(Default)]
pub struct SpawnMap {
    map: HashMap<u32, Entity>,
    generation: u64,
    by_entity: RefCell<Option<(u64, Rc<HashMap<Entity, u32>>)>>,
    by_id: RefCell<Option<(u64, Rc<HashMap<u64, u64>>)>>,
}

impl std::ops::Deref for SpawnMap {
    type Target = HashMap<u32, Entity>;
    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl SpawnMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, handle: u32, entity: Entity) -> Option<Entity> {
        self.generation += 1;
        self.map.insert(handle, entity)
    }

    pub fn retain(&mut self, keep: impl FnMut(&u32, &mut Entity) -> bool) {
        let before = self.map.len();
        self.map.retain(keep);
        if self.map.len() != before {
            self.generation += 1;
        }
    }

    pub fn clear(&mut self) {
        if !self.map.is_empty() {
            self.generation += 1;
        }
        self.map.clear();
    }

    /// How many changes the table has had: equal means nothing a view or a
    /// handle depends on has moved.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Entity -> its handle.
    pub fn by_entity(&self) -> Rc<HashMap<Entity, u32>> {
        let mut cache = self.by_entity.borrow_mut();
        if let Some((g, view)) = cache.as_ref()
            && *g == self.generation
        {
            return view.clone();
        }
        let view = Rc::new(self.map.iter().map(|(h, e)| (*e, *h)).collect::<HashMap<_, _>>());
        *cache = Some((self.generation, view.clone()));
        view
    }

    /// Entity id (`Entity::id`) -> its handle, as the ray and nav snapshots
    /// and the physics rows want it.
    pub fn by_id(&self) -> Rc<HashMap<u64, u64>> {
        let mut cache = self.by_id.borrow_mut();
        if let Some((g, view)) = cache.as_ref()
            && *g == self.generation
        {
            return view.clone();
        }
        let view = Rc::new(self.map.iter().map(|(h, e)| (e.id() as u64, *h as u64)).collect::<HashMap<_, _>>());
        *cache = Some((self.generation, view.clone()));
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_follow_every_change_and_are_reused_between() {
        let mut world = hecs::World::new();
        let a = world.spawn(());
        let b = world.spawn(());
        let mut m = SpawnMap::new();
        m.insert(1_000_000, a);
        let first = m.by_entity();
        assert_eq!(first.get(&a), Some(&1_000_000));
        assert!(Rc::ptr_eq(&first, &m.by_entity()), "rebuilt with nothing changed");
        assert_eq!(m.by_id().get(&(a.id() as u64)), Some(&1_000_000));

        m.insert(1_000_001, b);
        assert_eq!(m.by_entity().get(&b), Some(&1_000_001));
        assert_eq!(m.by_id().len(), 2);

        m.retain(|_, e| *e != a);
        assert!(m.by_entity().get(&a).is_none());
        assert_eq!(m.get(&1_000_001), Some(&b), "reads go through to the table");

        m.clear();
        assert!(m.by_entity().is_empty() && m.by_id().is_empty());
    }
}
