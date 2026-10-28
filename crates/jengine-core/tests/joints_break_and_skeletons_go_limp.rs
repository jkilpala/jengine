//! Breakable joints and ragdolls (Oct 2026), against a real rapier world.
//!
//! * `break_force` is in the units the world weighs things in: a body of
//!   mass `m` hanging still loads its joint with `m * 981` in a project at
//!   the usual `length_unit` - so the number has to be measured, not assumed
//!   from rapier's impulses.
//! * A joint loaded past it is gone: the component is removed (or
//!   `ensure_physics_bodies` would build it again next frame) and the
//!   entity's script is owed `on_joint_break`.
//! * A `ragdoll` turns a skeleton's bones into bodies that fall, collide
//!   with the world, bend no further than their limit, and stand back up as
//!   plain bones when it is switched off.

#![cfg(all(feature = "physics", feature = "ragdoll", feature = "ik"))]

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::ecs::callbacks;
use jengine_core::ecs::components::{
    Bone, BodyType, Collider2D, ColliderShape, IkLookAt, Joint2D, JointType, Name, Parent, RigidBody2D, SkeletonRoot,
    Transform,
};
use jengine_core::ecs::hierarchy::{resolve_world_transforms, world_transform_of};
use jengine_core::ecs::ragdoll::{set_ragdoll_field, Ragdoll};
use jengine_core::ecs::systems::ensure_physics_bodies;
use jengine_core::physics::PhysicsWorld;
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
use serde_json::json;

const DT: f32 = 1.0 / 60.0;
const G: f32 = 981.0;

fn physics() -> PhysicsWorld {
    PhysicsWorld::new(0.0, -9.81, 100.0)
}

fn frame(world: &mut World, physics: &mut PhysicsWorld) {
    ensure_physics_bodies(world, physics);
    physics.step(DT);
    physics.sync_transforms_to_ecs(world);
    resolve_world_transforms(world);
}

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

fn boxed(w: f32, h: f32) -> Collider2D {
    Collider2D { shape: ColliderShape::Box { width: w, height: h }, ..Default::default() }
}

/// A peg and a bob of `mass` hanging 50 below it on a hinge that snaps at
/// `break_force`.
fn pendulum(world: &mut World, mass: f32, break_force: f32) -> Entity {
    world.spawn((
        Name("Peg".into()),
        at(0.0, 0.0),
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        boxed(10.0, 10.0),
    ));
    world.spawn((
        Name("Bob".into()),
        at(0.0, -50.0),
        RigidBody2D { body_type: BodyType::Dynamic, mass, ..Default::default() },
        boxed(20.0, 20.0),
        Joint2D {
            target_entity: "Peg".into(),
            joint_type: JointType::Revolute {
                anchor: [0.0, 50.0],
                anchor_b: [0.0, 0.0],
                limits: None,
                motor_speed: 0.0,
                motor_max_torque: 0.0,
            },
            break_force,
            ..Default::default()
        },
    ))
}

#[test]
fn a_joint_carries_the_weight_hanging_from_it() {
    let mut world = World::new();
    let mut physics = physics();
    let bob = pendulum(&mut world, 2.0, 0.0);
    for _ in 0..120 {
        frame(&mut world, &mut physics);
    }
    let (force, _) = physics.joint_load(bob.id() as u64).expect("the bob hangs by a joint");
    let weight = 2.0 * G;
    assert!((force - weight).abs() < weight * 0.1, "a 2 kg bob should pull {weight}, the joint reports {force}");
}

#[test]
fn a_joint_loaded_past_its_break_force_lets_go() {
    let mut world = World::new();
    let mut physics = physics();
    let _ = callbacks::take();
    // Holds twice its weight, then is given half.
    let strong = pendulum(&mut world, 2.0, 2.0 * 2.0 * G);
    for _ in 0..60 {
        frame(&mut world, &mut physics);
    }
    assert!(world.get::<&Joint2D>(strong).is_ok(), "a joint twice as strong as its load broke");
    assert!(physics.has_joint(strong.id() as u64));

    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    let weak = pendulum(&mut world, 2.0, 0.5 * 2.0 * G);
    // Judged after its grace (`BREAK_GRACE_SECONDS`), then a second to fall.
    for _ in 0..90 {
        frame(&mut world, &mut physics);
    }
    assert!(world.get::<&Joint2D>(weak).is_err(), "the broken joint's component should be gone");
    assert!(!physics.has_joint(weak.id() as u64));
    let y = world.get::<&Transform>(weak).unwrap().position.y;
    assert!(y < -200.0, "the bob should fall once its joint lets go, it is at {y}");
    let owed: Vec<_> = callbacks::take().into_iter().filter(|c| c.handler == "on_joint_break").collect();
    assert_eq!(owed.len(), 1, "the script is told once");
    assert_eq!(owed[0].entity, weak);
}

/// A root at (0, 200) with a torso pointing down and two limbs off its end,
/// over a floor at y = -100.
fn creature(world: &mut World) -> (Entity, Vec<Entity>) {
    world.spawn((
        at(0.0, -120.0),
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        boxed(2000.0, 40.0),
    ));
    let root = world.spawn((at(0.0, 200.0), SkeletonRoot { name: "c".into() }, Ragdoll::default()));
    let down = -std::f32::consts::FRAC_PI_2;
    let torso = world.spawn((
        Transform { rotation: down, ..Default::default() },
        Bone { name: "torso".into(), length: 60.0 },
        Parent(root),
    ));
    let upper = world.spawn((
        Transform { position: Vec2::new(60.0, 0.0), rotation: 0.5, ..Default::default() },
        Bone { name: "upper".into(), length: 40.0 },
        Parent(torso),
    ));
    let lower = world.spawn((
        Transform { position: Vec2::new(40.0, 0.0), rotation: -0.3, ..Default::default() },
        Bone { name: "lower".into(), length: 40.0 },
        Parent(upper),
        IkLookAt { target: [500.0, 500.0], rotation_offset: 0.0 },
    ));
    resolve_world_transforms(world);
    (root, vec![torso, upper, lower])
}

#[test]
fn a_skeleton_goes_limp_falls_and_lands() {
    let mut world = World::new();
    let mut physics = physics();
    let (root, bones) = creature(&mut world);
    let start: Vec<Vec2> = bones.iter().map(|b| world_transform_of(&world, *b).unwrap().position).collect();

    // Not limp: nothing is built and nothing moves.
    for _ in 0..10 {
        frame(&mut world, &mut physics);
    }
    assert_eq!(physics.ragdoll_part_count(root.id() as u64), None);
    let still = world_transform_of(&world, bones[2]).unwrap().position;
    assert!((still - start[2]).length() < 1e-3);

    world.get::<&mut Ragdoll>(root).unwrap().active = true;
    for _ in 0..240 {
        frame(&mut world, &mut physics);
        // The animators keep their hands off a limp skeleton.
        jengine_core::animation::ik::solve_ik(&mut world);
    }
    assert_eq!(physics.ragdoll_part_count(root.id() as u64), Some(3));
    let limit = Ragdoll::default().limit;
    for (i, b) in bones.iter().enumerate() {
        let w = world_transform_of(&world, *b).unwrap();
        assert!(w.position.y < start[i].y - 50.0, "bone {i} should have fallen: {:?} -> {:?}", start[i], w.position);
        assert!(w.position.y > -100.0 - 15.0, "bone {i} went through the floor: {:?}", w.position);
    }
    // The joints held each bone to its parent's end and within its swing.
    let rest = [0.5f32, -0.3];
    for (k, pair) in bones.windows(2).enumerate() {
        let up = world_transform_of(&world, pair[0]).unwrap();
        let child = world_transform_of(&world, pair[1]).unwrap();
        let end = up.position + Vec2::from_angle(up.rotation) * [60.0f32, 40.0][k];
        assert!((child.position - end).length() < 6.0, "joint {k} came apart: {:?} vs {:?}", child.position, end);
        let local = world.get::<&Transform>(pair[1]).unwrap().rotation;
        let bend = (local - rest[k] + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        assert!(bend.abs() < limit + 0.15, "joint {k} bent {bend}, past its limit {limit}");
    }

    // Stood up: the bodies go, the bones keep the pose they fell in.
    world.get::<&mut Ragdoll>(root).unwrap().active = false;
    let fallen = (*world.get::<&Transform>(bones[1]).unwrap()).clone();
    frame(&mut world, &mut physics);
    assert_eq!(physics.ragdoll_part_count(root.id() as u64), None);
    for _ in 0..30 {
        frame(&mut world, &mut physics);
    }
    let kept = (*world.get::<&Transform>(bones[1]).unwrap()).clone();
    assert!((kept.position - fallen.position).length() < 1e-3 && (kept.rotation - fallen.rotation).abs() < 1e-4);
}

#[test]
fn a_bone_kicked_as_it_goes_limp_takes_the_kick() {
    let mut world = World::new();
    let mut physics = physics();
    let (root, bones) = creature(&mut world);
    frame(&mut world, &mut physics);
    // One frame: the script sets it limp and kicks the torso, the kick is
    // applied before the bodies exist.
    world.get::<&mut Ragdoll>(root).unwrap().active = true;
    physics.apply_impulse(bones[0].id() as u64, 600000.0, 0.0);
    frame(&mut world, &mut physics);
    for _ in 0..10 {
        frame(&mut world, &mut physics);
    }
    let x = world_transform_of(&world, bones[0]).unwrap().position.x;
    assert!(x > 20.0, "the torso should fly right, it is at x = {x}");
    let (vx, _) = physics.get_velocity(bones[0].id() as u64).expect("a limp bone answers get_velocity");
    assert!(vx > 0.0);
}

struct Host(World);

impl ScriptWorldHost for Host {
    fn world(&mut self) -> &mut World {
        &mut self.0
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
    }
}

#[test]
fn scripts_set_ragdoll_fields() {
    let mut world = World::new();
    let root = world.spawn((Transform::default(), Ragdoll::default()));
    set_ragdoll_field(&world, root, "active", &json!(true)).unwrap();
    set_ragdoll_field(&world, root, "limit", &json!(0.4)).unwrap();
    assert!(set_ragdoll_field(&world, root, "activ", &json!(true)).is_err(), "a misspelt field is an error");
    {
        let r = world.get::<&Ragdoll>(root).unwrap();
        assert!(r.active);
        assert!((r.limit - 0.4).abs() < 1e-6);
    }
    // The browser hosts, through the shared arm; a 0 from a script is false.
    let mut host = Host(world);
    assert!(handle_world_command(&mut host, "srdf", &JsonArgs::new(json!({ "e": root.id(), "f": "active", "v": 0 }))));
    assert!(!host.0.get::<&Ragdoll>(root).unwrap().active);
}

#[test]
fn a_hinge_at_its_limit_carries_the_torque() {
    // A 2 kg block held 50 out to the side by a hinge at the peg that cannot
    // swing down - it rests on its limit, which carries the weight times that
    // arm.
    let mut world = World::new();
    let mut physics = physics();
    world.spawn((
        Name("Peg".into()),
        at(0.0, 0.0),
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        boxed(10.0, 10.0),
    ));
    let bar = world.spawn((
        at(50.0, 0.0),
        RigidBody2D { body_type: BodyType::Dynamic, mass: 2.0, ..Default::default() },
        boxed(20.0, 20.0),
        Joint2D {
            target_entity: "Peg".into(),
            joint_type: JointType::Revolute {
                anchor: [-50.0, 0.0],
                anchor_b: [0.0, 0.0],
                limits: Some([-0.02, 0.02]),
                motor_speed: 0.0,
                motor_max_torque: 0.0,
            },
            ..Default::default()
        },
    ));
    for _ in 0..120 {
        frame(&mut world, &mut physics);
    }
    let (_, torque) = physics.joint_load(bar.id() as u64).unwrap();
    let moment = 2.0 * G * 50.0;
    assert!((torque - moment).abs() < moment * 0.1, "torque {torque}, expected {moment}");

    // The same hinge with a break_torque under that lets the bar go.
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    world.spawn((
        Name("Peg".into()),
        at(0.0, 0.0),
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        boxed(10.0, 10.0),
    ));
    let weak = world.spawn((
        at(50.0, 0.0),
        RigidBody2D { body_type: BodyType::Dynamic, mass: 2.0, ..Default::default() },
        boxed(20.0, 20.0),
        Joint2D {
            target_entity: "Peg".into(),
            joint_type: JointType::Revolute {
                anchor: [-50.0, 0.0],
                anchor_b: [0.0, 0.0],
                limits: Some([-0.02, 0.02]),
                motor_speed: 0.0,
                motor_max_torque: 0.0,
            },
            break_torque: moment * 0.5,
            ..Default::default()
        },
    ));
    for _ in 0..60 {
        frame(&mut world, &mut physics);
    }
    assert!(world.get::<&Joint2D>(weak).is_err(), "a hinge loaded past its break_torque should let go");
}

#[test]
fn a_new_chain_settles_before_it_is_judged() {
    // Three light links and a 4 kg weight: 4,512 once settled, but the
    // first tenth of a second swings past 6,000 as the soft joints catch it.
    // A chain that holds 5,000 must not snap as the scene begins.
    let mut world = World::new();
    let mut physics = physics();
    world.spawn((
        Name("Beam".into()),
        at(0.0, 190.0),
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        boxed(100.0, 20.0),
    ));
    let hinge = |target: &str, below: f32| Joint2D {
        target_entity: target.into(),
        joint_type: JointType::Revolute {
            anchor: [0.0, 20.0],
            anchor_b: [0.0, below],
            limits: None,
            motor_speed: 0.0,
            motor_max_torque: 0.0,
        },
        break_force: 5000.0,
        ..Default::default()
    };
    let mut held = Vec::new();
    for k in 0..3 {
        let target = if k == 0 { "Beam".to_string() } else { format!("L{}", k - 1) };
        held.push(world.spawn((
            Name(format!("L{k}")),
            at(0.0, 160.0 - 40.0 * k as f32),
            RigidBody2D { body_type: BodyType::Dynamic, mass: 0.2, ..Default::default() },
            boxed(6.0, 40.0),
            hinge(&target, if k == 0 { -10.0 } else { -20.0 }),
        )));
    }
    held.push(world.spawn((
        at(0.0, 40.0),
        RigidBody2D { body_type: BodyType::Dynamic, mass: 4.0, ..Default::default() },
        boxed(40.0, 40.0),
        hinge("L2", -20.0),
    )));
    for _ in 0..120 {
        frame(&mut world, &mut physics);
    }
    for (i, e) in held.iter().enumerate() {
        assert!(world.get::<&Joint2D>(*e).is_ok(), "joint {i} snapped under a load it holds");
    }
}

#[test]
fn a_prefab_fired_in_the_frame_it_spawns_flies() {
    // Native order: the physics commands run, then the spawn is applied,
    // then the bodies are built. The command names a handle no entity has
    // yet; it used to go to a body id that did not exist, every frame.
    use jengine_core::ecs::systems::apply_physics_commands;
    use jengine_core::scripting::api::ScriptCommand;
    use jengine_core::scripting::spawn_aliases;
    let mut world = World::new();
    let mut physics = physics();
    let handle = spawn_aliases::FIRST_SPAWN_ID + 77;
    apply_physics_commands(&[ScriptCommand::SetVelocity { entity_id: handle, vx: 600.0, vy: 300.0 }], &mut physics, &world);
    let ball = world.spawn((
        at(0.0, 0.0),
        RigidBody2D { body_type: BodyType::Dynamic, mass: 8.0, ..Default::default() },
        Collider2D { shape: ColliderShape::Circle { radius: 15.0 }, ..Default::default() },
    ));
    spawn_aliases::record(handle, ball);
    frame(&mut world, &mut physics);
    let (vx, _) = physics.get_velocity(ball.id() as u64).unwrap();
    assert!((vx - 600.0).abs() < 1.0, "the ball should leave at 600, it is going {vx}");
    // And later frames reach it through its handle too.
    apply_physics_commands(&[ScriptCommand::SetVelocity { entity_id: handle, vx: -50.0, vy: 0.0 }], &mut physics, &world);
    let (vx, _) = physics.get_velocity(ball.id() as u64).unwrap();
    assert!((vx + 50.0).abs() < 1e-3, "a later set_velocity by handle went nowhere: {vx}");
    spawn_aliases::clear();
}
