//! Split screen as one field (Oct 2026): a camera's `screen_rect` makes it a
//! view with a target of its own, sized to its share of the screen, shown by
//! an entity-UI image over that share - under every other UI.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::Camera2DComponent;
use jengine_core::ecs::views::{SCREEN_RECT_Z, ScreenRectImage, render_target_cameras, sync_screen_rects};
use jengine_core::ui::{UIElement, UIImage};

fn half(left: bool) -> Camera2DComponent {
    Camera2DComponent {
        is_active: true,
        screen_rect: if left { [0.0, 0.0, 0.5, 1.0] } else { [0.5, 0.0, 0.5, 1.0] },
        ..Default::default()
    }
}

fn images(world: &World) -> Vec<(UIElement, String)> {
    let mut out: Vec<(UIElement, String)> = world
        .query::<(&UIElement, &UIImage, &ScreenRectImage)>()
        .iter()
        .map(|(_, (e, i, _))| (e.clone(), i.texture_id.clone()))
        .collect();
    out.sort_by(|a, b| a.0.offset.x.total_cmp(&b.0.offset.x));
    out
}

#[test]
fn two_cameras_split_the_screen() {
    let mut world = World::new();
    let left = world.spawn((half(true),));
    let right = world.spawn((half(false),));
    let main = world.spawn((Camera2DComponent { is_active: true, ..Default::default() },));

    sync_screen_rects(&mut world, 1280.0, 720.0);
    // Each is a view now, with a target of its own; the plain camera is not.
    let target = |e| world.get::<&Camera2DComponent>(e).unwrap().target.clone();
    assert_eq!(target(left), format!("__screen/{}", left.id()));
    assert_eq!(target(right), format!("__screen/{}", right.id()));
    assert!(target(main).is_empty());

    // An image over each half of the UI, beneath every other element.
    let shown = images(&world);
    assert_eq!(shown.len(), 2);
    let (l, r) = (&shown[0], &shown[1]);
    assert_eq!((l.0.offset, l.0.size), (Vec2::new(0.0, 0.0), Vec2::new(640.0, 720.0)));
    assert_eq!((r.0.offset, r.0.size), (Vec2::new(640.0, 0.0), Vec2::new(640.0, 720.0)));
    assert_eq!(l.1, target(left));
    assert!(l.0.z_order < SCREEN_RECT_Z && l.0.visible, "under every other UI");
    let (min, max) = r.0.screen_rect(1280.0, 720.0);
    assert_eq!((min, max), (Vec2::new(640.0, 0.0), Vec2::new(1280.0, 720.0)));

    // Each view draws its share of the surface - in surface pixels, which
    // need not be the UI's.
    let views = render_target_cameras(&world, (1920, 1080));
    assert_eq!(views.len(), 2);
    assert!(views.iter().all(|v| v.size == (960, 1080)), "{:?}", views.iter().map(|v| v.size).collect::<Vec<_>>());
    // And shows half the authored width at the full screen's scale, not the
    // whole of it squeezed in.
    assert!(views.iter().all(|v| { let (w, h) = v.camera.viewport_size(); (w - 640.0).abs() < 0.5 && (h - 720.0).abs() < 0.5 }),
        "{:?}", views.iter().map(|v| v.camera.viewport_size()).collect::<Vec<_>>());

    // Run again: the same two images, moved with the UI's size, not more.
    sync_screen_rects(&mut world, 1000.0, 500.0);
    let shown = images(&world);
    assert_eq!(shown.len(), 2);
    assert_eq!(shown[1].0.offset, Vec2::new(500.0, 0.0));

    // An inactive camera's image hides; a camera that stops splitting, or
    // goes, takes its image with it.
    world.get::<&mut Camera2DComponent>(left).unwrap().is_active = false;
    sync_screen_rects(&mut world, 1000.0, 500.0);
    assert!(!images(&world)[0].0.visible);
    world.get::<&mut Camera2DComponent>(left).unwrap().screen_rect = [0.0; 4];
    world.despawn(right).unwrap();
    sync_screen_rects(&mut world, 1000.0, 500.0);
    assert!(images(&world).is_empty());
}

#[test]
fn a_screen_rect_round_trips_and_is_omitted_when_unset() {
    let json = serde_json::to_value(half(false)).unwrap();
    assert_eq!(json["screen_rect"], serde_json::json!([0.5, 0.0, 0.5, 1.0]));
    let plain = serde_json::to_value(Camera2DComponent::default()).unwrap();
    assert!(plain.get("screen_rect").is_none());
    let back: Camera2DComponent = serde_json::from_value(json).unwrap();
    assert_eq!(back.screen_rect, [0.5, 0.0, 0.5, 1.0]);
}

struct Host(World);

impl jengine_core::scripting::host_commands::ScriptWorldHost for Host {
    fn world(&mut self) -> &mut World {
        &mut self.0
    }
    fn resolve_entity(&mut self, id: u32) -> Option<hecs::Entity> {
        self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
    }
}

#[test]
fn scripts_split_and_join_the_screen() {
    use jengine_core::scripting::host_commands::{JsonArgs, handle_world_command};
    let mut host = Host(World::new());
    let cam = host.0.spawn((Camera2DComponent { is_active: true, ..Default::default() },));
    // The browser hosts' shared arm (`Camera.set_screen_rect`).
    assert!(handle_world_command(
        &mut host,
        "scsr",
        &JsonArgs::new(serde_json::json!({ "e": cam.id(), "x": 0.0, "y": 0.5, "w": 1.0, "h": 0.5 }))
    ));
    sync_screen_rects(&mut host.0, 800.0, 600.0);
    let shown = images(&host.0);
    assert_eq!((shown[0].0.offset, shown[0].0.size), (Vec2::new(0.0, -300.0), Vec2::new(800.0, 300.0)));
    // Cleared, the camera gives its implied target back and is a plain
    // camera again - the main one, if it is the active one.
    assert!(handle_world_command(
        &mut host,
        "scsr",
        &JsonArgs::new(serde_json::json!({ "e": cam.id(), "x": 0, "y": 0, "w": 0, "h": 0 }))
    ));
    sync_screen_rects(&mut host.0, 800.0, 600.0);
    assert!(images(&host.0).is_empty());
    assert!(host.0.get::<&Camera2DComponent>(cam).unwrap().target.is_empty());
    assert!(render_target_cameras(&host.0, (800, 600)).is_empty());
}

#[test]
fn an_inset_draws_over_the_view_it_is_set_into() {
    let mut world = World::new();
    world.spawn((Camera2DComponent { is_active: true, screen_rect: [0.0, 0.0, 1.0, 1.0], ..Default::default() },));
    world.spawn((Camera2DComponent { is_active: true, screen_rect: [0.7, 0.05, 0.25, 0.25], ..Default::default() },));
    sync_screen_rects(&mut world, 1280.0, 720.0);
    let shown = images(&world);
    let (full, inset) = (&shown[0].0, &shown[1].0);
    assert!(inset.z_order > full.z_order, "the inset ({}) must draw over the whole view ({})", inset.z_order, full.z_order);
}
