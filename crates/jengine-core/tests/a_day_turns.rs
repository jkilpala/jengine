//! **A day turns** (Oct 2026; `ecs::day_cycle`): a `day_cycle` as a scene
//! authors it drives the sky gradient, the world's ambient light and the
//! sun from keys through the day, places the sun disc on its arc, fades the
//! stars, and tells its script at dawn and dusk. Found by the showcase's A
//! Day on the Island. The sampling is tested in the module.
#![cfg(feature = "day_cycle")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{AmbientLight, Camera2DComponent, DirectionalLight2D, Mesh2D, Name, SpriteComponent, Transform};
use jengine_core::ecs::day_cycle::{DayCycle, tick_day_cycle};

fn world_at(hour: f32) -> (World, hecs::Entity) {
    let mut world = World::new();
    world.spawn((
        Transform::default(),
        Camera2DComponent { is_active: true, viewport_width: 1280.0, viewport_height: 720.0, zoom: 1.0, ..Default::default() },
    ));
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 1.0 },));
    world.spawn((Name("Sun".into()), Transform::default(), DirectionalLight2D::default()));
    world.spawn((Name("SunDisc".into()), Transform::default()));
    world.spawn((Name("Stars".into()), Transform::default(), SpriteComponent::default()));
    let day: DayCycle = serde_json::from_value(serde_json::json!({
        "hour": hour, "hours_per_second": 1.0, "sun": "Sun", "sunrise": 6.0, "sunset": 18.0,
        "keys": [
            { "hour": 0.0, "sky_top": [0.0, 0.0, 0.05], "sky_horizon": [0.02, 0.02, 0.1],
              "ambient": [0.4, 0.5, 0.9], "ambient_intensity": 0.1, "sun_color": [1.0, 0.5, 0.3], "sun_intensity": 0.0 },
            { "hour": 12.0, "sky_top": [0.1, 0.3, 0.9], "sky_horizon": [0.6, 0.8, 1.0],
              "ambient": [1.0, 1.0, 1.0], "ambient_intensity": 0.7, "sun_color": [1.0, 1.0, 0.9], "sun_intensity": 1.2 }
        ],
        "bodies": [{ "name": "SunDisc", "rise": 6.0, "set": 18.0, "height": 0.8 }],
        "night": ["Stars"]
    }))
    .expect("a day_cycle deserializes");
    let e = world.spawn((Transform::default(), day));
    (world, e)
}

fn ambient(world: &World) -> f32 {
    world.query::<&AmbientLight>().iter().next().unwrap().1.intensity
}

fn named(world: &World, name: &str) -> hecs::Entity {
    world.query::<&Name>().iter().find(|(_, n)| n.0 == name).unwrap().0
}

#[test]
fn noon_is_bright_and_blue_midnight_dark_with_stars() {
    let (mut noon, e) = world_at(12.0);
    tick_day_cycle(&mut noon, 0.0);
    assert!((ambient(&noon) - 0.7).abs() < 1e-4);
    let sky = noon.get::<&Mesh2D>(e).unwrap();
    assert!(sky.vertices[0].color[2] > 0.8, "a blue sky at the top: {:?}", sky.vertices[0].color);
    drop(sky);
    let disc = noon.get::<&Transform>(named(&noon, "SunDisc")).unwrap().position;
    assert!(disc.y > 100.0 && disc.x.abs() < 1.0, "the sun disc high and in the middle at noon: {disc}");
    assert_eq!(noon.get::<&SpriteComponent>(named(&noon, "Stars")).unwrap().color[3], 0.0, "no stars by day");

    let (mut night, _) = world_at(0.0);
    tick_day_cycle(&mut night, 0.0);
    assert!((ambient(&night) - 0.1).abs() < 1e-4);
    assert_eq!(night.get::<&DirectionalLight2D>(named(&night, "Sun")).unwrap().intensity, 0.0, "no sun at night");
    assert!(night.get::<&SpriteComponent>(named(&night, "Stars")).unwrap().color[3] > 0.9, "stars by night");
    let below = night.get::<&Transform>(named(&night, "SunDisc")).unwrap().position;
    assert!(below.y < -700.0, "the sun disc below the horizon: {below}");
}

#[test]
fn the_morning_sun_comes_in_low_from_the_east() {
    let (mut world, _) = world_at(8.0);
    tick_day_cycle(&mut world, 0.0);
    let sun = world.get::<&DirectionalLight2D>(named(&world, "Sun")).unwrap();
    let travel = Vec2::from_angle(sun.direction);
    assert!(travel.x < -0.5 && travel.y < 0.0, "the light travels west and down: {travel}");
    assert!(sun.intensity > 0.0);
}
