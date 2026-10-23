//! An untextured sprite draws as a coloured quad, not as nothing.
//!
//! The renderer buckets sprites by texture id and the hosts' flush callbacks
//! *discard the whole batch* when that id is not a registered texture. An
//! empty `texture_id` never is — so a sprite authored as "just a colour"
//! silently drew nothing, with one console warning about a missing texture
//! named `''` to explain it.
//!
//! Found the honest way: scene 61 reported 16,000 boids at 5 ms/frame and an
//! empty screen. Scenes 58, 59 and 60 had the same fault and nobody had
//! noticed, because "did it draw?" is the one question the test suite cannot
//! ask.

use jengine_core::asset::{resolve_texture_id, WHITE_TEXTURE_ID};

#[test]
fn an_empty_texture_id_resolves_to_the_white_pixel() {
    assert_eq!(resolve_texture_id(""), WHITE_TEXTURE_ID);
}

#[test]
fn a_real_texture_id_is_left_alone() {
    // Including one that merely *looks* odd: only empty is special, so a
    // typo'd path still fails loudly instead of being silently recoloured.
    assert_eq!(resolve_texture_id("sprites/white32.png"), "sprites/white32.png");
    assert_eq!(resolve_texture_id("  "), "  ");
    assert_eq!(resolve_texture_id(WHITE_TEXTURE_ID), WHITE_TEXTURE_ID);
}

#[test]
fn the_white_id_matches_what_the_hosts_register() {
    // Each runtime registers this id itself (three separate call sites), so a
    // rename here without renaming there would bring the invisible sprites
    // straight back.
    assert_eq!(WHITE_TEXTURE_ID, "__white");
}

#[test]
fn bulk_entity_is_registered_so_it_can_be_authored() {
    // It has to be in the registry the scene loader consults, or a scene
    // marking a bulk entity would have the key silently dropped and quietly
    // keep paying the per-frame mirror cost.
    use jengine_core::package::component_registry::ComponentRegistry;
    use jengine_core::package::core_registration::register_core_components;

    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    assert!(registry.get("bulk_entity").is_some());
}

#[test]
fn a_query_without_the_marker_excludes_marked_entities() {
    // The mechanism the hydration loops rely on.
    use jengine_core::ecs::components::{BulkEntity, Transform};

    let mut world = hecs::World::new();
    world.spawn((Transform::default(),));
    world.spawn((Transform::default(), BulkEntity));
    world.spawn((Transform::default(), BulkEntity));

    assert_eq!(world.query::<&Transform>().iter().count(), 3);
    assert_eq!(
        world.query::<&Transform>().without::<&BulkEntity>().iter().count(),
        1,
        "marked entities are still being mirrored"
    );
}

#[test]
fn the_bulk_marker_covers_both_costs_it_claims_to() {
    // `BulkEntity` governs two separate things: the per-frame script mirror
    // and whether the editor's Scene panel lists the entity. It was named
    // `NoScriptSync` when it only did the first. Both hosts and the viewport's
    // entity listing must actually consult it, or the name is a lie and the
    // 68 ms of DOM work comes straight back.
    const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
    const WEB: &str = include_str!("../../jengine-web/src/script_host.rs");

    // The script mirror is one implementation both hosts call since Oct 2026
    // (`adapters::js::mirror`), so the exclusion lives there.
    const MIRROR: &str = include_str!("../src/scripting/bridge/adapters/js/mirror.rs");
    assert!(
        MIRROR.contains("without::<&crate::ecs::components::BulkEntity>"),
        "the shared mirror does not exclude BulkEntity"
    );
    for (name, src) in [("viewport", VIEWPORT), ("web", WEB)] {
        assert!(src.contains("mirror::sync_world_to_play_namespace("), "{name} does not use the shared mirror");
    }
    assert!(
        VIEWPORT.contains("app.world.get::<&BulkEntity>(entity).is_ok()"),
        "the Scene panel listing does not exclude BulkEntity"
    );
}
