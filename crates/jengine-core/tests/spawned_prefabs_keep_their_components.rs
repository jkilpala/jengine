//! A spawned prefab keeps every component the scene loader would give it.
//!
//! The native `ScriptCommand::SpawnEntity` arm used to be a hand-copied
//! subset of `ComponentBundle` while the editor viewport spawns generically
//! through the registry - so a prefab carrying one of the missing fields
//! worked in the editor and silently lost it on native. Measured Sep 2026:
//! **45 of the 77 fields were missing**, `material` among them, which is part
//! of why no game had ever used a graph material. This test pinned that list
//! so it could only shrink.
//!
//! It is empty now, because there is no list to copy any more: the loader and
//! the spawn arm both call `ComponentBundle::add_to_builder`, the one place
//! the field list lives. What this checks instead is that the method covers
//! every field of the struct, and that both sites still call it.

use std::collections::BTreeSet;

/// Fields the shared method deliberately leaves to its callers, with why.
const HANDLED_BY_THE_CALLERS: &[(&str, &str)] = &[
    (
        "custom",
        "the loader keeps it as data with a warning about nesting; the spawn arm first lifts \
         package components out of it (BUGS #109) - two rules, on purpose",
    ),
];

fn source(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    // LF, whatever git checked out: the scans below look for "\n}\n".
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("reading {}: {e}", p.display())).replace("\r\n", "\n")
}

/// Every `pub <name>:` on `ComponentBundle`.
fn bundle_fields() -> Vec<String> {
    let src = source("src/scene/format.rs");
    let i = src.find("pub struct ComponentBundle").expect("ComponentBundle exists");
    let j = src[i..].find("\n}").expect("the struct closes") + i;
    src[i..j]
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            l.strip_prefix("pub ")?.split(':').next().map(str::to_string)
        })
        .filter(|f| !f.contains(' '))
        .collect()
}

/// Every `self.<name>` the shared method reads.
fn handled_by_add_to_builder() -> BTreeSet<String> {
    let src = source("src/scene/format.rs");
    let i = src.find("pub fn add_to_builder(").expect("the shared method exists");
    let j = src[i..].find("\n    }\n}").expect("the method closes") + i;
    let mut out = BTreeSet::new();
    let mut rest = &src[i..j];
    while let Some(k) = rest.find("self.") {
        rest = &rest[k + "self.".len()..];
        let name: String =
            rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if !name.is_empty() {
            out.insert(name);
        }
    }
    out
}

#[test]
fn the_shared_builder_covers_every_bundle_field() {
    let fields = bundle_fields();
    assert!(fields.len() > 50, "only found {} bundle fields - the parse broke", fields.len());
    let handled = handled_by_add_to_builder();
    let excused: BTreeSet<&str> = HANDLED_BY_THE_CALLERS.iter().map(|(f, _)| *f).collect();
    let missing: Vec<&str> = fields
        .iter()
        .map(String::as_str)
        .filter(|f| !handled.contains(*f) && !excused.contains(f))
        .collect();
    assert!(
        missing.is_empty(),
        "`ComponentBundle::add_to_builder` does not add these fields: {missing:?}\n\
         A prefab carrying one loads from a scene without it and spawns without it, in \
         every runtime that spawns through the bundle. Add it there - not to a call site."
    );
    for (f, _) in HANDLED_BY_THE_CALLERS {
        assert!(fields.iter().any(|x| x == f), "excused field `{f}` no longer exists on the bundle");
        assert!(!handled.contains(*f), "excused field `{f}` is now handled by the shared method; drop the excuse");
    }
}

#[test]
fn both_the_loader_and_the_native_spawn_arm_use_the_shared_builder() {
    let format = source("src/scene/format.rs");
    let loader_at = format.find("pub fn load_scene_with_mapping(").expect("the v1 loader exists");
    let loader = &format[loader_at..loader_at + format[loader_at..].find("\n}\n").unwrap()];
    assert!(
        loader.contains(".add_to_builder(&mut builder)"),
        "the v1 scene loader does not go through `ComponentBundle::add_to_builder`"
    );
    assert!(
        !loader.contains("entity_data.components.sprite {"),
        "the v1 scene loader has grown its own per-field copy again"
    );

    let systems = source("src/ecs/systems.rs");
    // The arm spawns through `spawn_bundle` - the root and every part of a
    // prefab's tree (`EntityData::children`) alike.
    let arm_at = systems.find("ScriptCommand::SpawnEntity { name, components, spawn_id, children }").expect("the spawn arm exists");
    let arm = &systems[arm_at..(arm_at + 1_000).min(systems.len())];
    assert!(arm.contains("spawn_bundle(world,") && arm.contains("spawn_bundle(w,"), "the spawn arm builds its own entity");
    let arm_at = systems.find("fn spawn_bundle(").expect("spawn_bundle exists");
    let arm = &systems[arm_at..(arm_at + 3_000).min(systems.len())];
    assert!(
        arm.contains("bundle.add_to_builder(&mut builder)"),
        "the native spawn arm does not go through `ComponentBundle::add_to_builder`"
    );
    assert!(
        !arm.contains("bundle.sprite {"),
        "the native spawn arm has grown its own per-field copy again"
    );
}
