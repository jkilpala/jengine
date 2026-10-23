# jengine-particles

Particle emitters: fire, smoke, sparks, rain.

This is a **built-in** package: the native runtime, the web runtime and the
editor viewport are compiled with it. It registers the component; the
simulation is core's `particles::tick_particles`, which every runtime calls
once a frame, so emitters behave the same everywhere.

## What it registers

| Component | Is |
|---|---|
| `particle_emitter` | An emitter: `emission_rate`, `max_particles`, `lifetime`, `speed`, `angle`, start / end colour and size, `gravity_modifier`, `emit_shape`, `blend_mode`, `world_space`, `z_order` - and the newer fields: `drag`, `turbulence`, colour and size curves, `stretch`, `collision` with colliders and `sub_emitters`. |

`particle_emitter` is also a **core** component, so it works in v1 and v2
scenes. An emitter can name a shared definition instead of carrying every
field: `{ "particle_source": "particles/fire.particle" }` loads one.

For hundreds of thousands of particles drawn by the GPU, use core's
`gpu_particles` component instead.

## From scripts

Scripts use core's `Particles` namespace in every runtime:
`Particles.emit_burst`, `Particles.set_active`, `Particles.set_rate`,
`Particles.get_count`, and `Particles.set(e, field, value)` for any emitter
field by name. See the [Particles guide](/guides/particles/).

The package's own `particle_*` script descriptors are **metadata only**; no
runtime installs them as globals.
