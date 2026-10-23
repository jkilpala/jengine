//! A parented entity is tested where it is drawn.
//!
//! The renderer places a child at its resolved `WorldTransform`; the camera
//! follow, trigger areas, hitbox knockback, health bars and name tags read
//! `Transform.position` - the child's *local* offset - so a trigger on a
//! child fired around the parent's origin and a camera following a child
//! followed the parent. Nothing in the arcade parents gameplay entities yet,
//! which is why it had not been seen. `hierarchy::world_position` is the one
//! rule now, and this is what would notice a system falling back to the
//! local field.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::behaviors::update_trigger_areas;
use jengine_core::ecs::components::{
    Camera2DComponent, CameraFollow, CustomComponents, Health, HealthBar, HealthBarView, Name,
    NameTagView, Parent, TextComponent, Transform, TriggerArea2D, TriggerOverlaps, TriggerShape,
};
use jengine_core::ecs::hierarchy::resolve_world_transforms;
use jengine_core::ecs::systems::{sync_health_bars, sync_name_tags, update_camera};
use jengine_core::renderer::camera::Camera2D;

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), rotation: 0.0, scale: Vec2::ONE }
}

/// A parent at (300, 0) carrying a child at local (10, 0): the child is at
/// world (310, 0).
fn parent_and_child(world: &mut World) -> (hecs::Entity, hecs::Entity) {
    let parent = world.spawn((Name("Cart".into()), at(300.0, 0.0)));
    let child = world.spawn((Name("Rider".into()), at(10.0, 0.0), Parent(parent)));
    (parent, child)
}

#[test]
fn a_trigger_on_a_child_fires_where_the_child_is_drawn() {
    let mut world = World::new();
    let (_, child) = parent_and_child(&mut world);
    let _ = world.insert_one(
        child,
        TriggerArea2D { shape: TriggerShape::Circle { radius: 5.0 }, match_tags: Vec::new() },
    );
    // Standing at the child's world position, nowhere near its local one.
    let visitor = world.spawn((at(312.0, 0.0),));
    // And one at the child's *local* position, which must not count.
    let decoy = world.spawn((at(12.0, 0.0),));

    resolve_world_transforms(&mut world);
    update_trigger_areas(&mut world);

    let overlaps = world.get::<&TriggerOverlaps>(child).expect("the trigger reports");
    assert!(
        overlaps.staying.contains(&visitor.to_bits().get()),
        "the visitor at the child's world position is inside: {:?}",
        overlaps.staying
    );
    assert!(
        !overlaps.staying.contains(&decoy.to_bits().get()),
        "the decoy at the child's local offset is not"
    );
}

#[test]
fn the_camera_follows_a_child_at_its_world_position() {
    let mut world = World::new();
    let (_, child) = parent_and_child(&mut world);
    let _ = world.insert_one(child, CameraFollow { smoothing: 1.0, ..Default::default() });
    world.spawn((
        at(0.0, 0.0),
        Camera2DComponent { zoom: 1.0, is_active: true, viewport_width: 640.0, viewport_height: 360.0, stretch: false, pixel_size: 0.0, ..Default::default() },
    ));
    resolve_world_transforms(&mut world);

    let mut camera = Camera2D::new(640.0, 360.0);
    update_camera(&world, &mut camera, 1.0 / 60.0);
    assert!(
        (camera.position.x - 310.0).abs() < 0.01,
        "the camera went to the child's world position, not its local one: {:?}",
        camera.position
    );
}

#[test]
fn a_health_bar_and_a_name_tag_float_over_the_child_where_it_is() {
    let mut world = World::new();
    let (_, child) = parent_and_child(&mut world);
    let _ = world.insert_one(child, Health { current: 5.0, max: 10.0, ..Default::default() });
    let _ = world.insert_one(child, HealthBar { y_offset: 20.0, ..Default::default() });
    let mut custom = CustomComponents::default();
    custom.0.insert("NameTag".into(), serde_json::json!({ "text": "Rider" }));
    let _ = world.insert_one(child, custom);
    resolve_world_transforms(&mut world);

    sync_health_bars(&mut world);
    sync_name_tags(&mut world);

    let bar_x = world
        .query::<(&HealthBarView, &Transform)>()
        .iter()
        .find(|(_, (v, _))| v.owner == child && !v.is_fill)
        .map(|(_, (_, t))| t.position.x)
        .expect("a bar backing was spawned");
    assert!((bar_x - 310.0).abs() < 0.01, "the bar sits over the child's world position: {bar_x}");

    let tag_x = world
        .query::<(&NameTagView, &Transform, &TextComponent)>()
        .iter()
        .find(|(_, (v, _, _))| v.owner == child)
        .map(|(_, (_, t, _))| t.position.x)
        .expect("a name tag was spawned");
    assert!((tag_x - 310.0).abs() < 0.01, "the tag sits over the child's world position: {tag_x}");
}
