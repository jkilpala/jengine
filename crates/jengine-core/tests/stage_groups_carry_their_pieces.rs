//! **Stage groups** (Oct 2026; `ecs::stage`): a `stage_group` is a frame in
//! stage space that everything under it - quads, billboards, casters - is
//! placed in, moved and turned as one, nested groups composing; `follow`
//! resolves through groups and other follows, leads with any stage object
//! and finds a nested part by its plain name; and a stage piece is left out
//! of the 2D hierarchy, whose parent's transform used to move the projected
//! picture again, flat, over the scene. Found by jSuite's HD-2D stage
//! editor, whose houses are groups of eighteen pieces.
#![cfg(feature = "stage")]

use std::f32::consts::FRAC_PI_2;

use glam::{Vec2, Vec3};
use hecs::{Entity, World};
use jengine_core::ecs::components::{Mesh2D, Name, Parent, PointLight2D, SpriteComponent, Transform, WorldTransform};
use jengine_core::ecs::stage::{Stage, StageBillboard, StageCaster, StageGroup, StageQuad};
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

/// A frame as a host runs one: motion (the stage), then the hierarchy.
fn tick(world: &mut World) {
    jengine_core::ecs::behaviors::tick_motion(world, 1.0 / 60.0, None, false);
    jengine_core::ecs::hierarchy::resolve_world_transforms(world);
}

fn stage(world: &mut World) -> Entity {
    world.spawn((Name("Stage".into()), Transform::default(), Stage::default()))
}

fn wall(origin: [f32; 3], u: [f32; 3]) -> (Transform, Mesh2D, StageQuad) {
    (
        Transform::default(),
        Mesh2D { texture_id: "wall.png".into(), ..Default::default() },
        StageQuad { origin, u, v: [0.0, 0.0, 3.0], two_sided: true, ..Default::default() },
    )
}

fn figure(position: [f32; 3]) -> (Transform, SpriteComponent, StageBillboard) {
    (
        Transform::default(),
        SpriteComponent { texture_id: "hero.png".into(), size: Some([24.0, 32.0]), anchor: [0.5, 0.0], ..Default::default() },
        StageBillboard { position, ..Default::default() },
    )
}

/// Where a quad's mesh lands on the screen, corner by corner, as drawn.
fn drawn(world: &World, e: Entity) -> Vec<Vec2> {
    let at = world.get::<&WorldTransform>(e).map(|w| w.position).unwrap_or_else(|_| world.get::<&Transform>(e).unwrap().position);
    world.get::<&Mesh2D>(e).unwrap().vertices.iter().map(|v| at + Vec2::from(v.position)).collect()
}

/// Where a billboard is drawn.
fn stands(world: &World, e: Entity) -> Vec2 {
    world.get::<&WorldTransform>(e).map(|w| w.position).unwrap_or_else(|_| world.get::<&Transform>(e).unwrap().position)
}

fn same(a: &[Vec2], b: &[Vec2]) -> bool {
    a.len() == b.len() && !a.is_empty() && a.iter().zip(b).all(|(p, q)| p.distance(*q) < 1e-3)
}

#[test]
fn a_group_carries_its_pieces_and_turns_them() {
    let mut world = World::new();
    stage(&mut world);
    let group = world.spawn((Name("Inn".into()), Transform::default(), StageGroup { position: [3.0, 0.0, 0.0], ..Default::default() }));
    let grouped_wall = world.spawn(wall([-1.0, 4.0, 0.0], [2.0, 0.0, 0.0]));
    let grouped_figure = world.spawn(figure([0.0, 2.0, 0.0]));
    world.insert_one(grouped_wall, Parent(group)).unwrap();
    world.insert_one(grouped_figure, Parent(group)).unwrap();
    let loose_wall = world.spawn(wall([2.0, 4.0, 0.0], [2.0, 0.0, 0.0]));
    let loose_figure = world.spawn(figure([3.0, 2.0, 0.0]));
    tick(&mut world);
    assert!(same(&drawn(&world, grouped_wall), &drawn(&world, loose_wall)), "a group at x 3 is the same as x + 3");
    assert!(stands(&world, grouped_figure).distance(stands(&world, loose_figure)) < 1e-3);

    // A quarter turn about the group's origin: the wall's u edge, east, now
    // runs north, and the corner at (-1, 4) is at (-4, -1).
    world.get::<&mut StageGroup>(group).unwrap().yaw = FRAC_PI_2;
    *world.get::<&mut StageQuad>(loose_wall).unwrap() =
        StageQuad { origin: [3.0 - 4.0, -1.0, 0.0], u: [0.0, 2.0, 0.0], v: [0.0, 0.0, 3.0], two_sided: true, ..Default::default() };
    world.get::<&mut StageBillboard>(loose_figure).unwrap().position = [3.0 - 2.0, 0.0, 0.0];
    tick(&mut world);
    assert!(same(&drawn(&world, grouped_wall), &drawn(&world, loose_wall)), "turned with its group");
    assert!(stands(&world, grouped_figure).distance(stands(&world, loose_figure)) < 1e-3);
}

#[test]
fn groups_nest_through_plain_parents() {
    let mut world = World::new();
    stage(&mut world);
    let outer = world.spawn((Transform::default(), StageGroup { position: [2.0, 0.0, 0.0], ..Default::default() }));
    // A plain entity between the two groups is passed through.
    let plain = world.spawn((Transform::default(), Parent(outer)));
    let inner = world.spawn((Transform::default(), StageGroup { position: [1.0, 0.0, 0.0], ..Default::default() }, Parent(plain)));
    let nested = world.spawn(figure([0.0, 3.0, 0.0]));
    world.insert_one(nested, Parent(inner)).unwrap();
    let loose = world.spawn(figure([3.0, 3.0, 0.0]));
    tick(&mut world);
    assert!(stands(&world, nested).distance(stands(&world, loose)) < 1e-3, "inner at 1 in outer at 2 puts it at x + 3");
}

#[test]
fn a_plain_parents_2d_transform_does_not_move_a_stage_piece() {
    let mut world = World::new();
    stage(&mut world);
    // The bug: a group's 2D transform slid its projected pieces across the
    // screen, flat, with no perspective.
    let parent = world.spawn((Transform { position: Vec2::new(200.0, 0.0), rotation: 0.4, scale: Vec2::splat(2.0) },));
    let child_wall = world.spawn(wall([-1.0, 4.0, 0.0], [2.0, 0.0, 0.0]));
    let child_figure = world.spawn(figure([0.0, 2.0, 0.0]));
    world.insert_one(child_wall, Parent(parent)).unwrap();
    world.insert_one(child_figure, Parent(parent)).unwrap();
    let wall_alone = world.spawn(wall([-1.0, 4.0, 0.0], [2.0, 0.0, 0.0]));
    let figure_alone = world.spawn(figure([0.0, 2.0, 0.0]));
    tick(&mut world);
    assert!(same(&drawn(&world, child_wall), &drawn(&world, wall_alone)), "drawn where no parent would put it");
    assert!(stands(&world, child_figure).distance(stands(&world, figure_alone)) < 1e-3);
    let ws = world.get::<&WorldTransform>(child_figure).unwrap().scale;
    assert!((ws - world.get::<&Transform>(figure_alone).unwrap().scale).length() < 1e-4, "nor scaled by it");
}

#[test]
fn a_script_moves_a_group_and_each_piece_sorts_on_its_own() {
    let mut world = World::new();
    stage(&mut world);
    let group = world.spawn((Name("Cart".into()), Transform::default(), StageGroup::default()));
    let front = world.spawn(wall([-1.0, 1.0, 0.0], [2.0, 0.0, 0.0]));
    let back = world.spawn(wall([-1.0, 3.0, 0.0], [2.0, 0.0, 0.0]));
    world.insert_one(front, Parent(group)).unwrap();
    world.insert_one(back, Parent(group)).unwrap();
    let hero = world.spawn(figure([0.0, 2.0, 0.0]));
    tick(&mut world);
    let z = |w: &World, e: Entity| w.get::<&Mesh2D>(e).unwrap().z_order;
    let hz = |w: &World| w.get::<&SpriteComponent>(hero).unwrap().z_order;
    assert!(z(&world, front) > hz(&world) && hz(&world) > z(&world, back), "the hero stands between the cart's walls");
    let before = (drawn(&world, front), z(&world, front), z(&world, back));

    let mut host = Host(world);
    let v = serde_json::json!({ "e": group.id(), "x": 0.0, "y": 5.0, "z": 0.0 });
    assert!(handle_world_command(&mut host, "stgp", &JsonArgs(v)));
    let v = serde_json::json!({ "e": group.id(), "f": "yaw", "v": 0.0 });
    assert!(handle_world_command(&mut host, "stgs", &JsonArgs(v)), "Stage.set reaches a group");
    tick(&mut host.0);
    let world = host.0;
    assert!(!same(&drawn(&world, front), &before.0), "Stage.place on the group moved its pieces");
    assert!(z(&world, front) < before.1 && z(&world, back) < before.2, "both farther now, so lower");
    assert!(z(&world, front) > z(&world, back), "and still sorted apart");
    assert!(hz(&world) > z(&world, front), "the hero is in front of the whole cart now");
    assert!(jengine_core::ecs::stage::place(&mut World::new(), group, 0.0, 0.0, 0.0).is_err());
}

#[test]
fn a_casters_shadow_moves_with_its_group() {
    let mut world = World::new();
    stage(&mut world);
    world.spawn((
        Transform::default(),
        PointLight2D { intensity: 1.0, ..Default::default() },
        StageBillboard { position: [0.0, 0.0, 3.0], light_radius: 12.0, ..Default::default() },
    ));
    let square = vec![[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
    let group = world.spawn((Transform::default(), StageGroup { position: [3.0, 2.0, 0.0], ..Default::default() }));
    let grouped = world.spawn((StageCaster { footprint: square.clone(), height: 2.0, ..Default::default() }, Parent(group)));
    let moved: Vec<[f32; 2]> = square.iter().map(|p| [p[0] + 3.0, p[1] + 2.0]).collect();
    let loose = world.spawn((StageCaster { footprint: moved, height: 2.0, ..Default::default() },));
    tick(&mut world);
    let shadow = |w: &World, e: Entity| w.get::<&StageCaster>(e).unwrap().shadow.positions.clone();
    assert!(!shadow(&world, loose).is_empty(), "the lamp throws a shadow");
    assert!(same(&shadow(&world, grouped), &shadow(&world, loose)), "thrown from the moved footprint");
}

#[test]
fn follows_resolve_through_chains_quads_and_part_names() {
    let mut world = World::new();
    stage(&mut world);
    // C stands in a group; B follows C; A follows B.
    let party = world.spawn((Name("Party".into()), Transform::default(), StageGroup { position: [1.0, 1.0, 0.0], ..Default::default() }));
    let c = world.spawn((Name("Party/Hero".into()), Parent(party)));
    world.insert(c, figure([1.0, 0.0, 0.0])).unwrap();
    // `Hero`: the plain part name finds the nested one.
    let b = world.spawn((Name("Lamp".into()), Transform::default(), StageBillboard { position: [0.0, 1.0, 0.0], follow: "Hero".into(), ..Default::default() }));
    let a = world.spawn((Transform::default(), StageBillboard { position: [0.0, 0.0, 1.0], follow: "Lamp".into(), ..Default::default() }));
    let want = world.spawn(figure([2.0, 2.0, 1.0]));
    // A quad following a quad: a door on a wall.
    let wall_at = world.spawn((Name("Wall".into()),));
    world.insert(wall_at, wall([4.0, 5.0, 0.0], [2.0, 0.0, 0.0])).unwrap();
    let door = world.spawn(wall([0.5, 0.0, 0.0], [1.0, 0.0, 0.0]));
    world.get::<&mut StageQuad>(door).unwrap().follow = "Wall".into();
    let door_alone = world.spawn(wall([4.5, 5.0, 0.0], [1.0, 0.0, 0.0]));
    // The stage's camera follows the nested hero by its part name too.
    world.query_mut::<&mut Stage>().into_iter().for_each(|(_, s)| s.follow = "Hero".into());
    tick(&mut world);
    let pa = world.get::<&Transform>(a).unwrap().position;
    let pw = world.get::<&Transform>(want).unwrap().position;
    assert!(pa.distance(pw) < 1e-3, "A is at C + B's offset + A's: {pa} against {pw}");
    assert!(same(&drawn(&world, door), &drawn(&world, door_alone)), "a quad follows a quad");
    let target = Vec3::from(world.query::<&Stage>().iter().next().unwrap().1.target);
    assert!(target.distance(Vec3::new(2.0, 1.0, 0.0)) < 1e-4, "the camera found Party/Hero as Hero: {target}");
    let _ = b;
}

#[test]
fn a_loop_of_follows_does_not_hang() {
    let mut world = World::new();
    stage(&mut world);
    world.spawn((Name("A".into()), Transform::default(), StageBillboard { follow: "B".into(), ..Default::default() }));
    world.spawn((Name("B".into()), Transform::default(), StageBillboard { follow: "A".into(), ..Default::default() }));
    let g = world.spawn((Transform::default(), StageGroup::default()));
    let h = world.spawn((Transform::default(), StageGroup::default(), Parent(g)));
    world.insert_one(g, Parent(h)).unwrap();
    tick(&mut world);
}
