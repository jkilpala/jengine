//! **Path morphing** (Oct 2026; `ecs::path_morph`): a drawn shape turning
//! into another. Driven as a host does - `Morph.to` through the shared host
//! command a browser script reaches, frames through `tick_motion` - a square
//! eases into a diamond: half way every point is half way, at the end the
//! path is the diamond's shape (in the square's own frame, though the
//! diamond is drawn elsewhere) and its fill colour the diamond's, and the
//! square's script is told it has finished. Found by the showcase's Shape
//! Shifter.

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::ecs::components::{Name, PathCommand, Transform, VectorPath};
use jengine_core::ecs::path_morph::PathMorph;
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};

struct Host(World);

impl ScriptWorldHost for Host {
    fn world(&mut self) -> &mut World {
        &mut self.0
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
    }
}

fn shape(points: &[(f32, f32)], fill: [f32; 4]) -> VectorPath {
    let mut commands = vec![PathCommand::MoveTo { x: points[0].0, y: points[0].1 }];
    for &(x, y) in &points[1..] {
        commands.push(PathCommand::LineTo { x, y });
    }
    commands.push(PathCommand::Close);
    VectorPath { commands, fill_color: Some(fill), closed: true, ..Default::default() }
}

/// The path's points, as the morph wrote them.
fn points(world: &World, e: Entity) -> Vec<Vec2> {
    world
        .get::<&VectorPath>(e)
        .unwrap()
        .commands
        .iter()
        .filter_map(|c| match *c {
            PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => Some(Vec2::new(x, y)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_square_becomes_a_diamond() {
    let mut world = World::new();
    let square = world.spawn((
        Transform::default(),
        Name("Square".into()),
        shape(&[(-50.0, -50.0), (50.0, -50.0), (50.0, 50.0), (-50.0, 50.0)], [1.0, 0.0, 0.0, 1.0]),
        PathMorph { points: 8, duration: 1.0, ease: jengine_core::ecs::path_morph::MorphEase::Linear, ..Default::default() },
    ));
    // The diamond, drawn far away: only its shape matters.
    world.spawn((
        Transform { position: Vec2::new(900.0, 400.0), ..Default::default() },
        Name("Diamond".into()),
        shape(&[(0.0, -70.0), (70.0, 0.0), (0.0, 70.0), (-70.0, 0.0)], [0.0, 0.0, 1.0, 1.0]),
    ));
    let mut host = Host(world);
    let args = JsonArgs(serde_json::json!({ "e": square.id(), "n": "Diamond", "s": 1.0 }));
    assert!(handle_world_command(&mut host, "mrto", &args), "a shared command");
    let mut world = host.0;
    let _ = jengine_core::ecs::callbacks::take();
    // Half way: each point half way between where it was and where it goes.
    let step = 1.0 / 60.0;
    for _ in 0..30 {
        jengine_core::ecs::behaviors::tick_motion(&mut world, step, None, false);
    }
    let (from, to) = {
        let m = world.get::<&PathMorph>(square).unwrap();
        (m.from.clone(), m.to.clone())
    };
    assert_eq!((from.len(), to.len()), (8, 8));
    let half = points(&world, square);
    let t = world.get::<&PathMorph>(square).unwrap().progress;
    for i in 0..8 {
        let want = from[i] + (to[i] - from[i]) * t;
        assert!(half[i].distance(want) < 1e-3, "point {i} half way: {} vs {want}", half[i]);
    }
    assert!((t - 0.5).abs() < 0.02, "half way through: {t}");
    for _ in 0..40 {
        jengine_core::ecs::behaviors::tick_motion(&mut world, step, None, false);
    }
    // The end: the diamond's eight evenly spaced points, in the square's
    // frame - its corners among them - and its colour.
    let end = points(&world, square);
    for corner in [Vec2::new(0.0, -70.0), Vec2::new(70.0, 0.0), Vec2::new(0.0, 70.0), Vec2::new(-70.0, 0.0)] {
        assert!(end.iter().any(|p| p.distance(corner) < 1e-3), "the diamond's corner {corner} in {end:?}");
    }
    let fill = world.get::<&VectorPath>(square).unwrap().fill_color.unwrap();
    assert!((fill[2] - 1.0).abs() < 1e-4 && fill[0].abs() < 1e-4, "its colour too: {fill:?}");
    let told: Vec<_> = jengine_core::ecs::callbacks::take().into_iter().filter(|c| c.handler == "on_morph_end").collect();
    assert_eq!(told.len(), 1, "told once that it finished");
    assert_eq!(told[0].entity, square);
}
