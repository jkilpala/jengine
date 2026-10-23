# jengine-physics

2D rigid-body physics on [rapier2d](https://rapier.rs): bodies, colliders,
joints and the kinematic character controller.

This is a **built-in** package: the native runtime, the web runtime and the
editor viewport are compiled with it, so a project never links it by hand. A
project with a `settings.enabled_packages` list must name it there.

## What it registers

| Component | Is |
|---|---|
| `rigid_body` | A body: `Dynamic`, `Kinematic` or `Static`, with mass, damping, gravity scale, `ccd` for fast bodies, and fixed rotation. |
| `collider` | Its shape (box, circle, capsule, segment, triangle, convex polygon, convex decomposition, polyline, heightfield), friction, restitution, sensor flag and collision layers (`collision_membership` / `collision_filter`). |
| `joint` | A fixed, revolute, prismatic, rope or spring joint to another entity, with an anchor on each end (`anchor`, `anchor_b`) and an optional motor. |
| `character_controller` | A kinematic body that walks a level: slopes, steps, one-way platforms, moving platforms, and ground / wall / ceiling reports. |

All four are also **core** components, so they work in v1 and v2 scenes alike.

## How it runs

The simulation itself is `jengine_core::physics::PhysicsWorld`, and each
runtime steps it on its own fixed-timestep path (`physics.fixed_timestep` in
`project.jengine`, 60 Hz by default), then blends dynamic bodies between
steps for drawing. The package also registers a `Physics`-phase system that
steps the world **only if a host shares `PhysicsWorld` as a resource** - no
shipped host does, so it does nothing rather than stepping twice. That is the
resource contract in `docs/writing-packages.md` working as designed.

## From scripts

Scripts use core's `Physics` namespace, available in every runtime:
`Physics.apply_impulse`, `Physics.apply_force`, `Physics.set_velocity`,
`Physics.get_velocity`, `Physics.raycast`, `Physics.overlap_circle`,
`Physics.circle_cast`, `Physics.add_joint`, `Physics.set_motor_speed`,
`Physics.move_character`, `Physics.get_contacts` and more - see the
[Scripting API](/api-reference/scripting-api/) and the
[Physics guide](/guides/physics/).

The package's own `physics_*` script descriptors are **metadata only** (editor
tooltips): no runtime installs them as globals, so do not call
`physics_apply_force` from a script.

## Things to know

- One world unit is about a centimetre: `length_unit` (100 in every project)
  only tunes the solver and scales gravity to ~981 units/s².
- A body faster than its own thickness per step needs `ccd: true`, or it
  passes through walls with no contact reported.
- A `Velocity2D` on a dynamic body is its velocity: written back each
  step, and a write to it sets the body's (Oct 2026; a seed before). The
  motion step leaves dynamic bodies to the physics.
