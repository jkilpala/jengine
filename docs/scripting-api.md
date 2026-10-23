# jEngine Scripting API Reference

> **Authoritative source.** This document tracks the live native scripting bridge surface (Luau + host JavaScript adapters in `crates/jengine-core/src/scripting/bridge/adapters/`). It is regenerated from those modules whenever the API changes. See [Maintaining This Document](#maintaining-this-document).
>
> **Four runtimes, one surface.** Native runs Luau; the editor and web builds run Lua 5.3 through Fengari over the JS preamble in `adapters/js/browser.rs`. Where they differ is in [Browser Adapter Parity](#browser-adapter-parity) and, for every known gap, `docs/parity-gaps.md`. Lua scripts call the **namespaced** names (`Transform.get_position`); JavaScript scripts call the flat ones.

---

## Three-Tier Scripting Model

JEngine uses a deliberate three-tier model. Pick the lowest (cheapest) tier that solves your problem.

| Tier | Mechanism | Cost model | Use for |
|------|-----------|------------|--------|
| **1 — Entity scripts** | `ScriptComponent` — per-entity `init` / `update(dt)` | High per-entity VM cost. **Hundreds, not tens of thousands.** | Bosses, doors, cutscenes, quests, unique objects |
| **2 — Tick-rate scripts** | Same `ScriptComponent` with a `tick_rate` header — engine schedules at ≤N Hz | Bounded by event rate, not entity count | NPC AI, pickups, spawners, damage reactions |
| **3 — Batch script systems** | `BatchScriptComponent` — typed-array views over component columns, one VM call for all matching entities. **Native desktop only**: the editor and web builds load a `batch_script` entity and leave it alone | A handful of host crossings per frame regardless of entity count | Custom movers, particle steering, crowd flocking, custom physics integrators |

> **Rule of thumb:** Rust ECS owns continuous simulation. Scripts orchestrate behaviour. Batch scripts are for high-volume custom systems. Per-entity scripts are for unique objects, not crowds.

Native ECS components and systems (see the generated Components Reference on the documentation site) handle the hot inner loops — movement, bouncing, lifetime, oscillators, patrol, follow, animation state machines, trigger areas. Scripts react to events and drive logic; they do not need to run every frame at 60 Hz for most use cases.

See also: [Batch Scripts](#batch-scripts-step-4) · [Behavior Graph](#behavior-graph-step-7)

---

## Contents

- [Three-Tier Scripting Model](#three-tier-scripting-model)
- [Script Lifecycle](#script-lifecycle)
- [Calling Conventions](#calling-conventions)
- [Namespaces](#namespaces)
- [API Index](#api-index)
- [Transform](#transform)
- [Sprite & Material](#sprite--material)
- [Entity Lifecycle](#entity-lifecycle)
- [Tags](#tags)
- [Custom Components](#custom-components)
- [Data Tables (data-oriented design)](#data-tables-data-oriented-design)
- [Input — Keyboard, Mouse, Actions](#input--keyboard-mouse-actions)
- [Animation](#animation)
- [Tweens](#tweens)
- [Timers](#timers)
- [Events](#events)
- [Visibility notifier](#visibility-notifier)
- [Path follow](#path-follow)
- [Sorting groups](#sorting-groups)
- [Lines](#lines)
- [Specular highlights](#specular-highlights)
- [Canvas groups](#canvas-groups)
- [Video](#video)
- [Timelines](#timelines)
- [Ragdolls and breaking joints](#ragdolls-and-breaking-joints)
- [Wheel, target, motor and friction joints](#wheel-target-motor-and-friction-joints)
- [Audio](#audio)
- [Physics](#physics)
- [Tilemap](#tilemap)
- [Particles](#particles)
- [UI Elements](#ui-elements)
- [GUI Documents](#gui-documents)
- [Scene Management](#scene-management)
- [Save Slots](#save-slots)
- [Lighting & Post-Processing](#lighting--post-processing)
- [Camera](#camera)
- [Debug Drawing & Overlay](#debug-drawing--overlay)
- [Utility](#utility)
- [Noise](#noise)
- [Key Names](#key-names)
- [UI Anchors](#ui-anchors)
- [Batch Scripts](#batch-scripts-step-4)
- [Behavior Graph](#behavior-graph-step-7)
- [Browser Adapter Parity](#browser-adapter-parity)
- [Maintaining This Document](#maintaining-this-document)

---

## Script Lifecycle

Every script defines two top-level functions that the engine invokes automatically:

```lua
-- Lua
function init(entity)
    -- Called once when the script is attached / scene loads.
    -- entity: number — the entity ID this script is attached to.
end

function update(entity, dt)
    -- Called every frame.
    -- dt: number — frame delta time in seconds.
end
```

```javascript
// JavaScript
function init(entity) { /* once */ }
function update(entity, dt) { /* every frame */ }
```

**A script file is loaded once and shared by every entity it is attached
to**, in every runtime: a file-level `local` is one variable for all of them,
not one each. Ten torches running `local t = 0 ... t = t + dt` add ten
`dt`s a frame to the same `t`. Keep what belongs to one entity in a table
keyed by `entity` (or in a custom component):

```lua
local flicker = {}            -- entity -> its own phase
function init(entity) flicker[entity] = math.random() * 6.28 end
function update(entity, dt) flicker[entity] = flicker[entity] + dt * 13 end
```

### Fixed update

A Lua script may also define `fixed_update` (Oct 2026; Godot's
`_physics_process`, Unity's `FixedUpdate`):

```lua
function fixed_update(entity, dt)
    -- Called once before each physics step: dt is the step (1/60 by
    -- default, `physics.fixed_timestep`), not the frame.
    local _, y = Transform.get_position(drone)
    local _, vy = Physics.get_velocity(drone)
    Physics.apply_force(drone, 0, 981 + (60 - y) * 40 - vy * 9)
end
```

`update` runs once a frame at the display's rate; `fixed_update` runs as
many times as the physics steps that frame, none on a frame too short for a
step, and each call reads the world - positions and rapier's velocities - as
the step before it left it. So a controller in it steps with the physics,
and a run replayed from the same start comes out the same, bit for bit,
whatever the frame rate. Its commands apply before the step that follows,
and a force it sets pushes for that step. Messages to other scripts
(`Event.send_to`, script method calls) belong in `update`. The host steps the
physics one step at a time only when some script defines `fixed_update`, so
a game without one pays nothing. Lua scripts only; JavaScript scripts keep
`update`. Test Lab scene 187; the showcase's Moon Lander.

Scripts may also declare **named callbacks** invoked by `Timer.after`, `Timer.every`, `Event.on`, and `GUI.on_click`. Each named callback receives the entity id as its first argument:

```lua
function on_score_increased(entity)
    log("score went up")
end
Event.on("score_increased", entity) -- inside init/update
```

### Engine Lifecycle Events (opt-in via `events = [...]`)

Beyond `init` / `update` and the named callbacks above, scripts can
subscribe to engine-driven lifecycle events by declaring an `events`
header at the top of the file. The engine reads the header at script
load time, indexes the script in a per-event subscription map, and
only dispatches the matching handlers — unsubscribed scripts pay
nothing for events they don't use.

```lua
-- Lua / Luau
script = { events = { "on_spawn", "on_despawn", "on_collision_enter" } }

function on_spawn(entity)        end  -- once, frame after init
function on_despawn(entity)      end  -- once, when the entity is removed
function on_collision_enter(entity, other) end
function on_collision_exit(entity, other)  end
function on_trigger(entity)      end  -- TriggerArea2D enter/exit changed
```

```javascript
// JavaScript
export const events = ["on_spawn", "on_despawn", "on_collision_enter"];

function on_spawn(entity) {}
function on_collision_enter(entity, other) {}
```

Recognised event names: `on_spawn`, `on_despawn`,
`on_collision_enter`, `on_collision_exit`, `on_trigger`. Unknown
names in the header are ignored (logged at `debug` level).
Collision/trigger events fire from the bridge snapshot of the previous
frame's physics step + trigger overlap diff.

The engine fires `on_spawn` exactly once on the first frame *after* `init`
returns successfully — by which point every other entity in the scene exists,
which is the reason it is a separate callback from `init`. `on_despawn` fires
when the entity leaves the world: `Entity.destroy`, an expired `Lifetime`, a
bounds despawn, **and a scene transition**, where every scripted entity is
notified before the world is replaced.

`on_despawn` is the teardown hook — the one place a script can release what it
owns but the world does not. Looping audio is the canonical case, because the
audio backend outlives every world:

```lua
script = { events = { "on_despawn" } }

function on_despawn(entity)
    Audio.stop_music()   -- otherwise it plays on into the next scene
end
```

Commands issued from `on_despawn` are applied like any other script command,
including at scene teardown. Handlers must not assume the entity still exists:
on the per-frame path it does, but a handler reached through a teardown may see
last-known values from the snapshot instead.

### Budget Caps (host-configured)

The script bridge runners enforce an optional per-frame budget:

```rust
runner.set_budget(BudgetConfig {
    max_script_ms_per_frame: 4.0,      // 0.0 = unlimited
    max_invocations_per_frame: 5_000,  // 0 = unlimited
    overflow_policy: BudgetPolicy::Defer,  // Warn | Drop | Defer
    deferred_cap: 1_024,
});
```

This is a host-side API (Rust), not a script-facing function — the
runtime configures the budget once at startup. When a frame exceeds
the cap, `Warn` logs once and lets the call run, `Drop` skips the
call and bumps `FrameCounters.events_dropped_or_deferred`, and
`Defer` queues the call (up to `deferred_cap`) for the next frame.
Default budget is unlimited + `Warn` — i.e. pre-Step 3 behaviour.

### Tick-Rate Throttling (Tier 2)

Per-entity scripts can declare a `tick_rate` to cap how often `update` is
called. The engine calls `update` once every `60 / tick_rate` **frames**
(rounded down, at least 1) - so it is hertz only at 60 frames a second, and
`tick_rate = 60` (or no header) means every frame at any refresh rate.
Between calls the script pays nothing - and sees nothing: a package's
one-frame `events` in a published view are missed on the frames it skips. Combine with `events = [...]` for
zero-cost idle behaviour that still reacts to collisions or timer ticks.

```lua
-- Lua / Luau
script = {
    tick_rate = 10,                              -- update at most 10 Hz
    events = { "on_collision_enter" },           -- still fires on contact
}

function update(entity, dt) -- called ~10× per second instead of 60×
end

function on_collision_enter(entity, other)
end
```

```javascript
// JavaScript
export const tick_rate = 10;
export const events = ["on_collision_enter"];

function update(entity, dt) {}
function on_collision_enter(entity, other) {}
```

A default tick rate can be set project-wide via the `JENGINE_SCRIPT_TICK_RATE`
env variable; per-script headers override the default.

### Inheritance: `extends` and `super`

A script can build on a module. Name it in the header, and whatever the
script does not define itself - `init`, `update`, a collision or animation
handler, a helper - is looked up on the table that module **returns**, then
on that module's own parent, and so on. `super` is that table, for calling
the parent explicitly.

```lua
-- scripts/lib/creature.lua - a module: it returns a table
local Creature = {}
Creature.species = "creature"

function Creature.update(entity, dt)          -- on the table, so it is inherited
    Transform.set_rotation(entity, Transform.get_rotation(entity) + dt)
end

function Creature:greet()
    log("I am a " .. self.species)
end

return Creature
```

```lua
-- scripts/lib/npc.lua - a module can extend another the same way
script = { extends = "lib/creature" }
local NPC = {}
NPC.species = "npc"
return NPC
```

```lua
-- scripts/goblin.lua - attached to an entity
script = { extends = "lib/npc" }

function init(entity)
    super:greet()                             -- "I am a npc": creature's greet, NPC's species
end
-- no `update`: the goblin turns, with creature's
```

- **Only the returned table is inherited.** A parent's `function update(...)`
  written as a plain global belongs to that module alone - every module runs
  in its own environment, as a `require`d one does - and nothing that extends
  it will see it. Write `function M.update(...)`.
- **Use `.`, not `:`, for anything the engine calls.** `init`, `update` and
  the handlers are called with the entity id first; `function M:update(dt)`
  would receive the entity as `self`.
- The path resolves like `require`: under `assets/scripts/`, `.lua` or
  `.luau`. A module is loaded once and shared by every script that extends or
  requires it, so a field on it (`Creature.tick_count`) is shared too.
- A parent that cannot be found, does not return a table, or extends its own
  descendant is an error at load, not a script that silently does nothing.
- Event handlers are inherited like everything else: a parent's
  `M.on_score_up` answers `Event.send("score_up")` for the child. The
  engine's own `on_event` global (the registration function) is never mistaken for
  one.

Works the same in every runtime since Sep 2026 - the editor and web builds
ignored `extends` before that. Engine Test Lab scene 103 is a worked
example.

---

## Calling Conventions

- **Functions live in namespaces.** Scripts call `Transform.set_position(entity, 100, 200)`, `Sprite.set_color(e, 1, 0, 0, 1)`, `Audio.play_sound("sounds/hit.wav")`. Namespaces are capitalised; see [Namespaces](#namespaces) below for why, and for the flat names every function still answers to.
- **Entity IDs are plain numbers.** The `entity` argument passed to `init` / `update` is the entity ID of the entity the script is attached to; pass it directly into other API calls.
- **Mutations are deferred.** Setters such as `Transform.set_position`, `Physics.apply_force`, `Animation.play`, `Entity.spawn`, etc. queue commands that the engine applies after every script in the frame has run. Reads are against a per-frame snapshot taken before any script runs.
- **Multi-value returns:** in Lua, functions like `Transform.get_position` return multiple values (`local x, y = Transform.get_position(e)`). In JS, which keeps the flat names, the same calls return arrays (`var [x, y] = get_position(e)`). The transpiler bridges Lua scripts onto the JS adapter so destructuring stays equivalent.
- **Pre-allocated handles.** Functions that allocate engine resources (`Entity.spawn`, `Entity.spawn_prefab`, `Audio.play_sound`, `Tween.*`, `Timer.after`, `Event.on`, `UI.create_*`) return a numeric handle synchronously, even though the underlying side-effect is deferred. You can store and use the handle in the same frame.
- **Numeric domain (host JS):** the host JS adapter (`rquickjs`) marshals all numbers as `f64`. `Entity.get_custom` therefore returns `Float`, never `Int`/`Bool`. Luau retains full Number/Bool/String fidelity.

---

## Namespaces

Every function belongs to one namespace - `Transform`, `Sprite`, `Entity`,
`Input`, `Physics`, `Audio` and so on, 28 in all; the [API Index](#api-index)
lists them. Inside a namespace the name drops the noun the namespace already
says: `set_sprite_color` is `Sprite.set_color`, `audio_create_bus` is
`Audio.create_bus`, `screen_flash` is `Camera.flash`.

```lua
function update(entity, dt)
    local x, y = Transform.get_position(entity)
    if Input.is_action_just_pressed("jump") then
        Physics.apply_impulse(entity, 0, 400)
        Audio.play_sound("sounds/jump.wav")
    end
    Sprite.set_color(entity, 1, x > 0 and 0.5 or 1, 1, 1)
end
```

- **Capitalised on purpose.** Lowercase names collide with what scripts call
  their variables: `entity` is the parameter of nearly every callback, so an
  `entity` table would be hidden inside `update(entity, dt)`, and `text`,
  `camera`, `timer`, `data` and `script` are all common. `math` and `debug`
  are Lua's own libraries, read-only on native. `Math.lerp`, `Math.clamp`,
  `Math.distance` and `Math.angle_between` are ours.
- **Global:** `log` (and `print`, which goes to the same place in every
  runtime), and `require`.
- **Packages** each get one namespace, from the crate name: `jengine-quest`
  is `Quest`. Where all of a package's commands share one prefix it is
  dropped - `quest_talk` is `Quest.talk` - otherwise the names stay whole,
  `Shooter.ship_fire`. A package whose namespace is taken (by core, or by
  another package) keeps only its flat names, and logs why.
- **The flat names still work.** Each namespaced function is the same
  function as the older flat global - `Sprite.set_color == set_sprite_color`
  - so nothing old breaks, but game scripts use the namespaced names, and a
  test (`shipped_scripts_use_namespaces`) holds the shipped games to it.
  **JavaScript scripts keep the flat names**; the namespaces are the Lua API.
- **Namespace tables are read-only natively** - every script shares them there
  - so assign nothing into them.
- To check whether a package is linked, test its namespace: `if Realm == nil
  then return end`. Testing a field (`Realm.talk == nil`) raises when the
  whole table is missing.

The list lives once, in `crates/jengine-core/src/scripting/namespaces.rs`;
every runtime builds its tables from it. Adding a script function means
adding it there too, or `script_namespaces_reach_every_runtime` fails.

---

## API Index

> **`Runtimes` says where a function is *installed*, not that it does
> anything.** The four `set_material_uniform_*` entries below were the
> standing example: installed everywhere and **inert** until Sep 2026, their
> value landing on `MaterialComponent.uniforms` where no render path read
> it. They reach a graph's `MaterialParam` nodes now - see
> [Sprite & Material](#sprite--material) and
> [`docs/parity-gaps.md`](parity-gaps.md).
>
> This note lives outside the generated block on purpose; the table itself is
> rebuilt by `cargo run -p jengine-core --example script_api_index` and
> `script_api_index_is_current` fails on any hand edit to it.

<!-- BEGIN GENERATED API INDEX -->

<!-- Generated by `cargo run -p jengine-core --example script_api_index`. Do not edit by hand. -->

442 functions.

**Runtimes** says where a function is actually installed:

- `all` — the Luau adapter and the Fengari runtime the editor and web builds share.
- `native only` — set on the Luau VM but absent from `BRIDGE_FUNCTIONS` in `editor/src/lib/lua-runtime.ts`, so it is `nil` in the editor and in every web build (0 of these).
- `editor + web` — registered in `core_script_api.rs` for editor tooltips but never installed on the Luau VM, because `install_script_api` skips descriptors without a handler and no core descriptor has one. Callable in the browser runtimes, `nil` on native (0 of these).
- `unreachable` — documented and installed nowhere.

**Function** is the name to call - namespaced, as `docs/script-namespaces-plan.md` describes and `scripting::namespaces` lists. **Flat name** is the older global the same function is still installed under; game scripts use the namespaced name (`shipped_scripts_use_namespaces`), JavaScript scripts the flat one. `Global` rows have no namespace; an `Alias` is a second flat name for a function listed elsewhere.

| Namespace | Function | Flat name | Returns | Side effect | Runtimes |
|---|---|---|---|---|---|
| Entity | `Entity.add_tag(entity_id, tag)` | `add_tag` | — | deferred | all |
| Entity | `Entity.destroy(entity_id)` | `destroy_entity` | — | deferred | all |
| Entity | `Entity.find_by_name(name)` | `find_by_name` | `entity_id \| nil` | — | all |
| Entity | `Entity.find_by_tag(tag)` | `find_by_tag` | array of entity ids | — | all |
| Entity | `Entity.find_child(parent, path)` | `find_child` | `entity_id \| nil` | — | all |
| Entity | `Entity.find_in_radius(x, y, radius, tag)` | `find_in_radius` | array of entity ids | — | all |
| Entity | `Entity.get_custom(entity_id, component, field)` | `get_custom` | `float` | — | all |
| Entity | `Entity.get_health(entity_id)` | `get_health` | `float` | — | all |
| Entity | `Entity.get_name(entity_id)` | `get_name` | `string \| nil` | — | all |
| Entity | `Entity.has_tag(entity_id, tag)` | `has_tag` | `bool` | — | all |
| Entity | `Entity.remove_tag(entity_id, tag)` | `remove_tag` | — | deferred | all |
| Entity | `Entity.set_custom(entity_id, component, field, value)` | `set_custom` | — | deferred | all |
| Entity | `Entity.set_fields(entity_id, component, fields)` | `set_fields` | — | — | all |
| Entity | `Entity.set_health(entity_id, current, max)` | `set_health` | — | deferred | all |
| Entity | `Entity.set_hitbox(entity_id, damage, knockback, retrigger)` | `set_hitbox` | — | deferred | all |
| Entity | `Entity.set_parent(child, parent)` | `set_parent` | — | — | all |
| Entity | `Entity.spawn(name)` | `spawn_entity` | `entity_id` | deferred | all |
| Entity | `Entity.spawn_prefab(prefab_name)` | `spawn_prefab` | `entity_id` | deferred | all |
| Transform | `Transform.get_position(entity_id)` | `get_position` | `x, y` | — | all |
| Transform | `Transform.get_rotation(entity_id)` | `get_rotation` | `float` | — | all |
| Transform | `Transform.get_scale(entity_id)` | `get_scale` | `x, y` | — | all |
| Transform | `Transform.get_world_position(entity_id)` | `get_world_position` | `x, y` | — | all |
| Transform | `Transform.get_world_rotation(entity_id)` | `get_world_rotation` | `float` | — | all |
| Transform | `Transform.get_world_scale(entity_id)` | `get_world_scale` | `x, y` | — | all |
| Transform | `Transform.set_position(entity_id, x, y)` | `set_position` | — | deferred | all |
| Transform | `Transform.set_rotation(entity_id, radians)` | `set_rotation` | — | deferred | all |
| Transform | `Transform.set_scale(entity_id, x, y)` | `set_scale` | — | deferred | all |
| Sprite | `Sprite.carve(entity_id, x, y, radius)` | `carve_sprite` | — | deferred | all |
| Sprite | `Sprite.get_anchor(entity_id)` | `get_sprite_anchor` | `x, y` | — | all |
| Sprite | `Sprite.get_color(entity_id)` | `get_sprite_color` | `r, g, b, a` | — the sprite's tint, else a `mesh`'s | all |
| Sprite | `Sprite.get_point(entity_id, point_name)` | `get_sprite_point` | `x, y` | — | all |
| Sprite | `Sprite.paint(entity_id, x, y, size, r, g, b, a, shape, rotation, blend)` | `paint_sprite` | — | deferred | all |
| Sprite | `Sprite.press(entity_id, x, y, size, depth, shape, rotation)` | `press_sprite` | — | deferred | all |
| Sprite | `Sprite.set_anchor(entity_id, x, y)` | `set_sprite_anchor` | — | deferred | all |
| Sprite | `Sprite.set_color(entity_id, r, g, b, a)` | `set_sprite_color` | — | deferred; tints the sprite, else a `mesh` | all |
| Sprite | `Sprite.set_corners(entity_id, x0, y0, x1, y1, x2, y2, x3, y3)` | `set_sprite_corners` | — | — | all |
| Sprite | `Sprite.set_fill(entity_id, amount)` | `set_sprite_fill` | — | deferred | all |
| Sprite | `Sprite.set_label(entity_id, label, category)` | `set_sprite_label` | — | deferred | all |
| Sprite | `Sprite.set_region(entity_id, atlas_id, region_name)` | `set_sprite_region` | — | deferred | all |
| Sprite | `Sprite.set_size(entity_id, w, h)` | `set_sprite_size` | — | deferred | all |
| Sprite | `Sprite.set_z_order(entity_id, z)` | `set_z_order` | — | deferred; sets the sprite's, the text's and the mesh's `z_order`, whichever the entity has | all |
| Material | `Material.get(entity_id)` | `get_material` | `string` | — | all |
| Material | `Material.set(entity_id, material_id)` | `set_material` | — | deferred | all |
| Material | `Material.set_float(entity_id, name, value)` | `set_material_uniform_float` | — | deferred | all |
| Material | `Material.set_int(entity_id, name, value)` | `set_material_uniform_int` | — | — | all |
| Material | `Material.set_normal_map(entity_id, normal_map_id)` | `set_normal_map` | — | deferred | all |
| Material | `Material.set_vec2(entity_id, name, x, y)` | `set_material_uniform_vec2` | — | — | all |
| Material | `Material.set_vec4(entity_id, name, x, y, z, w)` | `set_material_uniform_vec4` | — | deferred | all |
| Text | `Text.set(entity_id, text)` | `set_text` | — | — | all |
| Text | `Text.set_color(entity_id, r, g, b, a)` | `set_text_color` | — | deferred; the colour of the entity's world text (`a` defaults to 1) | all |
| Trail | `Trail.clear(entity_id)` | `clear_trail` | — | deferred; forget every point of the entity's `trail`, so a teleport leaves no streak | all |
| Trail | `Trail.set(entity_id, field, value)` | `set_trail_field` | — | deferred; any `trail` field by name - `"color"`, `"color_end"`, `"width"`, `"lifetime"` - keeping the points it has recorded, so a pooled ribbon takes the colour of what it is flying for. A name the trail has not got is logged | all |
| Trail | `Trail.set_emitting(entity_id, on)` | `set_trail_emitting` | — | deferred; whether the entity's `trail` records new points - off, the ones there age out (a sword's trail during the swing only) | all |
| Mesh | `Mesh.set(entity_id, vertices, indices)` | `set_mesh` | — | deferred; replace the entity's `mesh` shape, adding a `mesh` (white texture) if it had none; texture, colour and z are kept. Each vertex is `{ x, y, u, v }` (a fifth number is the perspective weight `q`) or keyed `{ x =, y =, u =, v =, q =, r =, g =, b =, a = }`; `indices` are triangles counted **from 1**, and none is a fan over the vertices in order. A triangle naming a vertex that does not exist is dropped | all |
| Animation | `Animation.crossfade(entity_id, clip, seconds)` | `crossfade_animation` | — | deferred | all |
| Animation | `Animation.get_current(entity_id)` | `get_current_animation` | `string \| nil` | — | all |
| Animation | `Animation.get_state(entity_id)` | `get_anim_state` | `string` | — | all |
| Animation | `Animation.get_time(entity_id)` | `get_animation_time` | `float` | — | all |
| Animation | `Animation.is_playing(entity_id)` | `is_animation_playing` | `bool` | — | all |
| Animation | `Animation.play(entity_id, clip_name)` | `play_animation` | — | deferred | all |
| Animation | `Animation.play_layer(entity_id, layer, clip, seconds, loop)` | `animation_play_layer` | — | deferred | all |
| Animation | `Animation.set_blend(entity_id, clip, weight)` | `set_animation_blend` | — | deferred | all |
| Animation | `Animation.set_blend_position(entity_id, x, y)` | `set_animation_blend_position` | — | deferred | all |
| Animation | `Animation.set_bool(entity_id, param, value)` | `set_anim_bool` | — | deferred | all |
| Animation | `Animation.set_float(entity_id, param, value)` | `set_anim_float` | — | deferred | all |
| Animation | `Animation.set_layer_weight(entity_id, layer, weight, seconds)` | `animation_set_layer_weight` | — | deferred | all |
| Animation | `Animation.set_speed(entity_id, speed)` | `set_animation_speed` | — | deferred | all |
| Animation | `Animation.set_time(entity_id, time)` | `set_animation_time` | — | — | all |
| Animation | `Animation.set_trigger(entity_id, param)` | `set_anim_trigger` | — | deferred | all |
| Animation | `Animation.start_state_machine(entity_id, state)` | `start_state_machine` | — | deferred | all |
| Animation | `Animation.stop(entity_id)` | `stop_animation` | — | deferred | all |
| Animation | `Animation.stop_layer(entity_id, layer, seconds)` | `animation_stop_layer` | — | deferred | all |
| Skeleton | `Skeleton.find(name)` | `find_skeleton` | `entity_id` | — | all |
| Skeleton | `Skeleton.find_bone(root, bone_name)` | `find_bone` | `entity_id` | — | all |
| Skeleton | `Skeleton.set_ik_target(entity_id, x, y)` | `set_ik_target` | — | — | all |
| Skeleton | `Skeleton.set_ik_weight(entity_id, weight)` | `set_ik_weight` | — | deferred | all |
| Skeleton | `Skeleton.spawn_rig(rig_id)` | `spawn_rig` | `entity_id` | — | all |
| Tween | `Tween.cancel(tween_id)` | `cancel_tween` | — | deferred | all |
| Tween | `Tween.cancel_all(entity_id)` | `cancel_all_tweens` | — | deferred | all |
| Tween | `Tween.color(entity_id, r, g, b, a, duration, easing)` | `tween_color` | `int` | deferred | all |
| Tween | `Tween.position(entity_id, x, y, duration, easing)` | `tween_position` | `int` | deferred | all |
| Tween | `Tween.rotation(entity_id, radians, duration, easing)` | `tween_rotation` | `int` | deferred | all |
| Tween | `Tween.scale(entity_id, sx, sy, duration, easing)` | `tween_scale` | `int` | deferred | all |
| Timer | `Timer.after(entity_id, delay, callback)` | `set_timer` | `int` | deferred | all |
| Timer | `Timer.cancel(timer_id)` | `cancel_timer` | — | deferred | all |
| Timer | `Timer.every(entity_id, interval, callback)` | `set_interval` | `int` | deferred | all |
| Event | `Event.off(event_name, entity_id)` | `remove_event_listener` | — | deferred | all |
| Event | `Event.on(event_name, entity_id)` | `on_event` | `int` | deferred | all |
| Event | `Event.send(event_name, sender, data)` | `send_event` | — | deferred | all |
| Event | `Event.send_to(event_name, sender, target, data)` | `send_event_to` | — | — | all |
| Input | `Input.add_binding(action, key)` | `input_add_binding` | — | deferred | all |
| Input | `Input.clear_bindings(action)` | `input_clear_bindings` | — | deferred | all |
| Input | `Input.get_action_axis(action)` | `get_action_axis` | `float` | — | all |
| Input | `Input.get_actions()` | `input_get_actions` | `...` | — | all |
| Input | `Input.get_bindings(action)` | `input_get_bindings` | `...` | — | all |
| Input | `Input.get_dead_zone(action)` | `input_get_dead_zone` | `float` | — | all |
| Input | `Input.get_gamepad_axis(pad, axis)` | `get_gamepad_axis` | `float` | — | all |
| Input | `Input.get_gamepad_count()` | `get_gamepad_count` | `int` | — | all |
| Input | `Input.get_gamepad_name(pad)` | `get_gamepad_name` | `string` | — | all |
| Input | `Input.get_gamepad_type(pad)` | `get_gamepad_type` | `string` | — | all |
| Input | `Input.get_gestures()` | `get_gestures` | `any` | this frame's taps, swipes, pans, pinches and twists | all |
| Input | `Input.get_hold_time(action)` | `get_action_hold_time` | `float` | — | all |
| Input | `Input.get_just_pressed()` | `input_get_just_pressed` | `...` | — | all |
| Input | `Input.get_last_device()` | `get_last_input_device` | `string` | — | all |
| Input | `Input.get_last_gamepad()` | `get_last_gamepad` | `int` | — | all |
| Input | `Input.get_mouse_delta()` | `get_mouse_delta` | `x, y` | — | all |
| Input | `Input.get_mouse_position()` | `get_mouse_position` | `x, y` (screen px) | — | all |
| Input | `Input.get_mouse_wheel()` | `get_mouse_wheel` | `float` | — | all |
| Input | `Input.get_mouse_world_position()` | `get_mouse_world_position` | `x, y` | — | all |
| Input | `Input.get_prompt(action, style)` | `get_action_prompt` | `any` | — | all |
| Input | `Input.get_touches()` | `get_touches` | `any` | every finger on a touch screen | all |
| Input | `Input.get_typed()` | `input_get_typed` | `string` | — | all |
| Input | `Input.is_action_double_tapped(action, seconds)` | `is_action_double_tapped` | `bool` | — | all |
| Input | `Input.is_action_held(action, seconds)` | `is_action_held` | `bool` | — | all |
| Input | `Input.is_action_just_pressed(action)` | `is_action_just_pressed` | `bool` | — | all |
| Input | `Input.is_action_just_released(action)` | `is_action_just_released` | `bool` | — | all |
| Input | `Input.is_action_pressed(action)` | `is_action_pressed` | `bool` | — | all |
| Input | `Input.is_action_tapped(action, seconds)` | `is_action_tapped` | `bool` | — | all |
| Input | `Input.is_gamepad_button_pressed(pad, button)` | `is_gamepad_button_pressed` | `bool` | — | all |
| Input | `Input.is_gamepad_connected(pad)` | `is_gamepad_connected` | `bool` | — | all |
| Input | `Input.is_key_just_pressed(key)` | `is_key_just_pressed` | `bool` | — | all |
| Input | `Input.is_key_pressed(key)` | `is_key_pressed` | `bool` | — | all |
| Input | `Input.is_mouse_left_just_pressed()` | `is_mouse_left_just_pressed` | `bool` | — | all |
| Input | `Input.is_mouse_left_just_released()` | `is_mouse_left_just_released` | `bool` | release edge, for drag-drop | all |
| Input | `Input.is_mouse_left_pressed()` | `is_mouse_left_pressed` | `bool` | held state, for drag interactions | all |
| Input | `Input.is_mouse_middle_just_pressed()` | `is_mouse_middle_just_pressed` | `bool` | — | all |
| Input | `Input.is_mouse_middle_just_released()` | `is_mouse_middle_just_released` | `bool` | — | all |
| Input | `Input.is_mouse_middle_pressed()` | `is_mouse_middle_pressed` | `bool` | — | all |
| Input | `Input.is_mouse_right_just_pressed()` | `is_mouse_right_just_pressed` | `bool` | — | all |
| Input | `Input.is_mouse_right_just_released()` | `is_mouse_right_just_released` | `bool` | — | all |
| Input | `Input.is_mouse_right_pressed()` | `is_mouse_right_pressed` | `bool` | — | all |
| Input | `Input.remove_binding(action, key)` | `input_remove_binding` | — | deferred | all |
| Input | `Input.reset_bindings()` | `input_reset_bindings` | — | deferred | all |
| Input | `Input.rumble(pad, strong, weak, seconds)` | `input_rumble` | — | deferred | all |
| Input | `Input.set_bindings(action, keys)` | `input_set_bindings` | — | deferred | all |
| Input | `Input.set_dead_zone(action, value)` | `input_set_dead_zone` | — | deferred | all |
| Input | `Input.vibrate(seconds, strength)` | `input_vibrate` | — | deferred | all |
| Physics | `Physics.add_joint(entity_name, target_name, joint_json)` | `add_joint` | — | deferred | all |
| Physics | `Physics.apply_force(entity_id, fx, fy)` | `apply_force` | — | deferred | all |
| Physics | `Physics.apply_impulse(entity_id, ix, iy)` | `apply_impulse` | — | deferred | all |
| Physics | `Physics.circle_cast(ox, oy, dx, dy, radius, max_dist)` | `circle_cast` | `...` | — | all |
| Physics | `Physics.contact_count()` | `contact_count` | `int` | — | all |
| Physics | `Physics.get_angular_velocity(entity_id)` | `get_angular_velocity` | `float` | — | all |
| Physics | `Physics.get_character(entity_id)` | `get_character` | `any` | — | all |
| Physics | `Physics.get_contact(index)` | `get_contact` | `entity_a, entity_b, started` | — | all |
| Physics | `Physics.get_contacts()` | `get_contacts` | `...` | — | all |
| Physics | `Physics.get_gravity()` | `get_gravity` | `x, y` | — | all |
| Physics | `Physics.get_velocity(entity_id)` | `get_velocity` | `x, y` | — | all |
| Physics | `Physics.move_character(entity_id, vx, vy, drop_through)` | `move_character` | — | deferred | all |
| Physics | `Physics.overlap_box(x, y, width, height, rotation)` | `overlap_box` | `...` | — | all |
| Physics | `Physics.overlap_circle(x, y, radius)` | `overlap_circle` | `...` | — | all |
| Physics | `Physics.overlap_point(x, y)` | `overlap_point` | `...` | — | all |
| Physics | `Physics.raycast(ox, oy, dx, dy, max_dist)` | `raycast` | `...` | nil/null on a miss | all |
| Physics | `Physics.remove_joint(entity_name)` | `remove_joint` | — | deferred | all |
| Physics | `Physics.set_angular_velocity(entity_id, omega)` | `set_angular_velocity` | — | deferred | all |
| Physics | `Physics.set_body_type(entity_id, body_type)` | `set_body_type` | — | deferred | all |
| Physics | `Physics.set_conveyor_speed(entity_id, speed)` | `set_conveyor_speed` | — | deferred | all |
| Physics | `Physics.set_gravity(gx, gy)` | `set_gravity` | — | deferred | all |
| Physics | `Physics.set_motor_speed(entity_name, speed)` | `set_motor_speed` | — | deferred | all |
| Physics | `Physics.set_velocity(entity_id, vx, vy)` | `set_velocity` | — | deferred | all |
| Physics | `Physics.slice(x0, y0, x1, y1)` | `physics_slice` | — | deferred | all |
| Audio | `Audio.create_bus(name, parent, volume)` | `audio_create_bus` | — | deferred | all |
| Audio | `Audio.crossfade_music(clip, seconds, volume, looping)` | `audio_crossfade_music` | — | deferred | all |
| Audio | `Audio.follow_time(on)` | `audio_follow_time` | — | deferred | all |
| Audio | `Audio.get_bus_volume(name)` | `audio_get_bus_volume` | `...` | — | all |
| Audio | `Audio.get_level()` | `audio_get_level` | `float` | — | all |
| Audio | `Audio.get_music_clip()` | `audio_get_music_clip` | `string` | — | all |
| Audio | `Audio.get_music_layer(index)` | `audio_get_music_layer` | `float` | — | all |
| Audio | `Audio.get_music_position()` | `get_music_position` | `float` | — | all |
| Audio | `Audio.get_spectrum(bands)` | `audio_get_spectrum` | `...` | — | all |
| Audio | `Audio.is_bus_muted(name)` | `audio_is_bus_muted` | `bool` | — | all |
| Audio | `Audio.next_track()` | `audio_next_track` | — | deferred | all |
| Audio | `Audio.pause_all()` | `pause_all_sounds` | — | deferred | all |
| Audio | `Audio.pause_music()` | `pause_music` | — | deferred | all |
| Audio | `Audio.play_music(clip_id, volume, looping)` | `play_music` | — | deferred | all |
| Audio | `Audio.play_music_layers(clips, volumes, seconds)` | `audio_play_music_layers` | — | deferred | all |
| Audio | `Audio.play_sound(clip_id, volume, pitch, pan, loop)` | `play_sound` | `int` | deferred | all |
| Audio | `Audio.play_sound_at(clip_id, x, y, volume, pitch)` | `play_sound_at` | `int` | — | all |
| Audio | `Audio.remove_bus(name)` | `audio_remove_bus` | — | deferred | all |
| Audio | `Audio.resume_all()` | `resume_all_sounds` | — | deferred | all |
| Audio | `Audio.resume_music()` | `resume_music` | — | deferred | all |
| Audio | `Audio.seek_music(seconds)` | `audio_seek_music` | — | deferred | all |
| Audio | `Audio.set_bus_effect(name, effect, value)` | `audio_set_bus_effect` | — | deferred | all |
| Audio | `Audio.set_bus_muted(name, muted)` | `audio_set_bus_muted` | — | deferred | all |
| Audio | `Audio.set_bus_solo(name, solo)` | `audio_set_bus_solo` | — | deferred | all |
| Audio | `Audio.set_bus_volume(name, volume)` | `audio_set_bus_volume` | — | deferred | all |
| Audio | `Audio.set_master_volume(volume)` | `set_master_volume` | — | deferred | all |
| Audio | `Audio.set_music_layer(index, volume, seconds)` | `audio_set_music_layer` | — | deferred | all |
| Audio | `Audio.set_music_pitch(pitch)` | `audio_set_music_pitch` | — | deferred | all |
| Audio | `Audio.set_music_volume(volume)` | `set_music_volume` | — | deferred | all |
| Audio | `Audio.set_playlist(clips, options)` | `audio_set_playlist` | — | deferred | all |
| Audio | `Audio.set_sound_pitch(sound, pitch)` | `audio_set_sound_pitch` | — | deferred | all |
| Audio | `Audio.set_sound_volume(handle_id, volume)` | `set_sound_volume` | — | deferred | all |
| Audio | `Audio.set_variation(clip, pitch, volume)` | `audio_set_variation` | — | deferred | all |
| Audio | `Audio.stop_all()` | `stop_all_sounds` | — | deferred | all |
| Audio | `Audio.stop_music()` | `stop_music` | — | deferred | all |
| Audio | `Audio.stop_sound(handle_id)` | `stop_sound` | — | deferred | all |
| Audio | `Audio.stop_sound_by_clip(clip_id)` | `stop_sound_by_clip` | — | deferred | all |
| Audio | `Audio.synth(opts)` | `audio_synth` | — | deferred | all |
| Scene | `Scene.fade_in(duration_s, r, g, b)` | `fade_in` | — | deferred | all |
| Scene | `Scene.fade_out(duration_s, r, g, b)` | `fade_out` | — | deferred | all |
| Scene | `Scene.get_name()` | `get_current_scene_name` | `string` | — | all |
| Scene | `Scene.load(path)` | `load_scene` | — | deferred | all |
| Scene | `Scene.load_additive(path, x, y)` | `scene_load_additive` | `int` | — | all |
| Scene | `Scene.pop()` | `pop_scene` | — | deferred | all |
| Scene | `Scene.preload(path)` | `scene_preload` | — | deferred | all |
| Scene | `Scene.progress(of)` | `scene_progress` | `float` | — | all |
| Scene | `Scene.push(path)` | `push_scene` | — | deferred | all |
| Scene | `Scene.quit()` | `quit_game` | — | deferred; native closes the window, the editor stops Play, **web is a logged no-op** (a tab cannot close itself) | all |
| Scene | `Scene.status(of)` | `scene_status` | `string` | — | all |
| Scene | `Scene.transition(path, style, seconds, texture)` | `scene_transition` | — | deferred | all |
| Scene | `Scene.unload(handle)` | `scene_unload` | — | deferred | all |
| Packs | `Packs.download(name)` | `pack_download` | — | deferred | all |
| Packs | `Packs.error(name)` | `pack_error` | `string` | — | all |
| Packs | `Packs.list()` | `pack_list` | `any` | — | all |
| Packs | `Packs.of(path)` | `pack_of` | `string` | — | all |
| Packs | `Packs.progress(name)` | `pack_progress` | `float` | — | all |
| Packs | `Packs.remove(name)` | `pack_remove` | — | deferred | all |
| Packs | `Packs.set_url(url)` | `pack_set_url` | — | deferred | all |
| Packs | `Packs.size(name)` | `pack_size` | `float` | — | all |
| Packs | `Packs.status(name)` | `pack_status` | `string` | — | all |
| LiveOps | `LiveOps.error()` | `liveops_error` | `string` | — | all |
| LiveOps | `LiveOps.event_active(id)` | `liveops_event_active` | `bool` | — | all |
| LiveOps | `LiveOps.event_ends(id)` | `liveops_event_ends` | `float` | — | all |
| LiveOps | `LiveOps.event_name(id)` | `liveops_event_name` | `string` | — | all |
| LiveOps | `LiveOps.event_starts(id)` | `liveops_event_starts` | `float` | — | all |
| LiveOps | `LiveOps.events()` | `liveops_events` | `any` | — | all |
| LiveOps | `LiveOps.expose(experiment, event, value)` | `liveops_expose` | `bool` | — | all |
| LiveOps | `LiveOps.fetch()` | `liveops_fetch` | — | deferred | all |
| LiveOps | `LiveOps.fetching()` | `liveops_fetching` | `bool` | — | all |
| LiveOps | `LiveOps.get(key, default)` | `liveops_get` | `any` | — | all |
| LiveOps | `LiveOps.keys()` | `liveops_keys` | `any` | — | all |
| LiveOps | `LiveOps.release()` | `liveops_release` | `float` | — | all |
| LiveOps | `LiveOps.revision()` | `liveops_revision` | `float` | — | all |
| LiveOps | `LiveOps.server_time()` | `liveops_server_time` | `float` | — | all |
| LiveOps | `LiveOps.set_attribute(name, value)` | `liveops_set_attribute` | — | deferred | all |
| LiveOps | `LiveOps.set_player(id)` | `liveops_set_player` | — | deferred | all |
| LiveOps | `LiveOps.status()` | `liveops_status` | `string` | — | all |
| LiveOps | `LiveOps.updated()` | `liveops_updated` | `float` | — | all |
| LiveOps | `LiveOps.variant(experiment)` | `liveops_variant` | `string` | — | all |
| Camera | `Camera.flash(duration, r, g, b, a)` | `screen_flash` | — | deferred | all |
| Camera | `Camera.get_position()` | `get_camera_position` | `x, y` | — | all |
| Camera | `Camera.get_screen_size()` | `get_screen_size` | `x, y` | — | all |
| Camera | `Camera.get_zoom()` | `get_camera_zoom` | `float` | — | all |
| Camera | `Camera.set(entity_id, field, value)` | `set_camera_field` | — | deferred | all |
| Camera | `Camera.set_follow(entity_id, field, value)` | `set_camera_follow` | — | deferred | all |
| Camera | `Camera.set_pixel_size(size)` | `set_camera_pixel_size` | — | deferred; world units per art pixel for a pixel-perfect camera (the world drawn at the art's resolution and scaled up), 0 to draw at the screen's | all |
| Camera | `Camera.set_screen_rect(entity_id, x, y, w, h)` | `set_camera_screen_rect` | — | deferred | all |
| Camera | `Camera.set_zoom(zoom)` | `set_camera_zoom` | — | deferred | all |
| Camera | `Camera.shake(amplitude, duration, frequency)` | `camera_shake` | — | deferred | all |
| Time | `Time.get_delta()` | `get_delta_time` | `float` | — | all |
| Time | `Time.get_scale()` | `get_time_scale` | `float` | — | all |
| Time | `Time.get_unix()` | `get_unix_time` | `float` | — | all |
| Time | `Time.set_scale(scale)` | `set_time_scale` | — | deferred; multiplies the delta time fed to the whole world tick. `0.0` is a true pause — behaviours, velocity, `Lifetime`, damage-over-time and animation all stop, because they all derive from that one `dt`. Scripts still run (at `dt = 0`), so a pause menu can read input and resume. Survives a `load_scene`, so a scene that may be entered from a paused one should set it back to `1.0` itself | all |
| Tilemap | `Tilemap.distance(entity_id, ax, ay, bx, by)` | `tilemap_distance` | `int` | — | all |
| Tilemap | `Tilemap.find_path(entity_id, layer, start_x, start_y, goal_x, goal_y, mode)` | `tilemap_find_path` | `...` | — | all |
| Tilemap | `Tilemap.find_path_world(entity_id, layer, start_x, start_y, goal_x, goal_y, mode)` | `tilemap_find_path_world` | `...` | — | all |
| Tilemap | `Tilemap.get_tile(entity_id, layer, x, y)` | `get_tile` | `int` | — | all |
| Tilemap | `Tilemap.get_tile_at_world(entity_id, layer, wx, wy)` | `get_tile_at_world` | `int` | — | all |
| Tilemap | `Tilemap.get_tile_data(entity_id, layer, x, y, field)` | `get_tile_data` | `any` | — | all |
| Tilemap | `Tilemap.neighbors(entity_id, tx, ty)` | `tilemap_neighbors` | `...` | — | all |
| Tilemap | `Tilemap.set_tile(entity_id, layer, x, y, tile_id)` | `set_tile` | — | deferred | all |
| Tilemap | `Tilemap.tile_to_world(entity_id, tx, ty)` | `tile_to_world` | `x, y` | — | all |
| Tilemap | `Tilemap.world_to_tile(entity_id, wx, wy)` | `world_to_tile` | `x, y` | — | all |
| Particles | `Particles.emit_burst(entity_id, count)` | `emit_burst` | — | deferred | all |
| Particles | `Particles.get_count()` | `get_particle_count` | `int` | — | all |
| Particles | `Particles.set(entity_id, field, value)` | `set_emitter_field` | — | deferred; any emitter field by name - `"angle"`, `"speed"`, `"gravity_modifier"`, `"turbulence.strength"` (a dot reaches into a group, creating it with its defaults). The value is what a scene file would give the field: a number, a flag, a string or a list of numbers. A name the emitter does not have is logged, not ignored | all |
| Particles | `Particles.set_active(entity_id, active)` | `set_emitter_active` | — | deferred | all |
| Particles | `Particles.set_rate(entity_id, rate)` | `set_emitter_rate` | — | deferred | all |
| Cloth | `Cloth.set(entity_id, field, value)` | `set_cloth_field` | — | deferred; any field of the entity's `cloth` or `rope` by name - `"wind"`, `"wave"`, `"gust"`, `"length"`, `"end"`. The simulation carries on from where it was. A name the component has not got is logged, not ignored | all |
| PathFollow | `PathFollow.set(entity_id, field, value)` | `set_path_follow_field` | — | deferred | all |
| Line | `Line.set(entity_id, field, value)` | `set_line_field` | — | deferred | all |
| Fluid | `Fluid.set(entity_id, field, value)` | `set_fluid_field` | — | deferred | all |
| Mirror | `Mirror.set(entity_id, field, value)` | `set_mirror_field` | — | deferred | all |
| PixelSim | `PixelSim.paint(entity_id, x, y, radius, material)` | `pixel_paint` | — | deferred | all |
| Smoke | `Smoke.puff(entity_id, x, y, radius, amount, vx, vy)` | `smoke_puff` | — | deferred | all |
| Smoke | `Smoke.set(entity_id, field, value)` | `set_smoke_field` | — | deferred | all |
| Rewind | `Rewind.set(entity_id, field, value)` | `set_rewind_field` | — | deferred | all |
| Fog | `Fog.set(entity_id, field, value)` | `set_fog_field` | — | deferred | all |
| Wfc | `Wfc.generate(entity_id, x, y, w, h)` | `wfc_generate` | — | deferred | all |
| Vision | `Vision.set(entity_id, field, value)` | `set_vision_field` | — | deferred | all |
| DayCycle | `DayCycle.set(entity_id, field, value)` | `set_day_field` | — | deferred | all |
| FloatingOrigin | `FloatingOrigin.set(entity_id, field, value)` | `set_origin_field` | — | deferred | all |
| TimeScale | `TimeScale.set(entity_id, field, value)` | `set_time_scale_field` | — | deferred | all |
| CastShadow | `CastShadow.set(entity_id, field, value)` | `set_cast_shadow_field` | — | deferred | all |
| AutoNormal | `AutoNormal.set(entity_id, field, value)` | `set_auto_normal_field` | — | deferred | all |
| TileLight | `TileLight.set(entity_id, field, value)` | `set_tile_light_field` | — | deferred | all |
| Trajectory | `Trajectory.set(entity_id, field, value)` | `set_trajectory_field` | — | deferred | all |
| TextPath | `TextPath.set(entity_id, field, value)` | `set_text_path_field` | — | deferred | all |
| BeatClock | `BeatClock.set(entity_id, field, value)` | `set_beat_clock_field` | — | deferred | all |
| Terrain | `Terrain.edit(entity_id, x, y, radius, amount)` | `smooth_terrain_edit` | — | deferred | all |
| Dialogue | `Dialogue.advance(entity_id)` | `dialogue_advance` | — | deferred | all |
| Dialogue | `Dialogue.choose(entity_id, n)` | `dialogue_choose` | — | deferred | all |
| Dialogue | `Dialogue.start(entity_id, node)` | `dialogue_start` | — | deferred | all |
| Dialogue | `Dialogue.stop(entity_id)` | `dialogue_stop` | — | deferred | all |
| UI | `UI.create_panel(anchor, ox, oy, w, h)` | `create_ui_panel` | `entity_id` | deferred | all |
| UI | `UI.create_text(text, anchor, ox, oy)` | `create_ui_text` | `entity_id` | deferred | all |
| UI | `UI.drop(entity_id, slot)` | `ui_drop` | — | deferred | all |
| UI | `UI.get_prop(entity_id, property)` | `get_ui_prop` | `any` | — | all |
| UI | `UI.set_position(entity_id, anchor, ox, oy)` | `set_ui_position` | — | deferred | all |
| UI | `UI.set_prop(entity_id, property, value)` | `set_ui_prop` | — | deferred | all |
| UI | `UI.set_text(entity_id, text)` | `set_ui_text` | — | deferred | all |
| UI | `UI.set_visible(entity_id, visible)` | `set_ui_visible` | — | deferred | all |
| Mode7 | `Mode7.place(entity_id, x, y, angle)` | `mode7_place` | — | deferred | all |
| Mode7 | `Mode7.set(entity_id, field, value)` | `mode7_set` | — | deferred | all |
| SpriteTilt | `SpriteTilt.set(entity_id, field, value)` | `sprite_tilt_set` | — | deferred | all |
| Afterimage | `Afterimage.set(entity_id, field, value)` | `afterimage_set` | — | deferred | all |
| Portal | `Portal.set(entity_id, field, value)` | `portal_set` | — | deferred | all |
| Jelly | `Jelly.poke(entity_id, x, y, strength, radius)` | `jelly_poke` | — | deferred | all |
| Jelly | `Jelly.set(entity_id, field, value)` | `jelly_set` | — | deferred | all |
| Marker | `Marker.set(entity_id, field, value)` | `marker_set` | — | deferred | all |
| Radial | `Radial.set(entity_id, field, value)` | `radial_set` | — | deferred | all |
| Bubble | `Bubble.say(entity_id, text, seconds)` | `bubble_say` | — | deferred | all |
| Bubble | `Bubble.set(entity_id, field, value)` | `bubble_set` | — | deferred | all |
| Outline | `Outline.set(entity_id, field, value)` | `outline_set` | — | deferred | all |
| Lightning | `Lightning.set(entity_id, field, value)` | `lightning_set` | — | deferred | all |
| Lightning | `Lightning.strike(entity_id, x, y)` | `lightning_strike` | — | deferred | all |
| Flow | `Flow.set(entity_id, field, value)` | `flow_set` | — | deferred | all |
| NavLink | `NavLink.set(entity_id, field, value)` | `nav_link_set` | — | deferred | all |
| Morph | `Morph.set(entity_id, field, value)` | `morph_set` | — | deferred | all |
| Morph | `Morph.to(entity_id, target, seconds)` | `morph_to` | — | deferred | all |
| Toast | `Toast.set(entity_id, field, value)` | `toast_set` | — | deferred | all |
| Toast | `Toast.show(entity_id, title, text, icon, seconds)` | `toast_show` | — | deferred | all |
| Replay | `Replay.clear()` | `replay_clear` | — | deferred | all |
| Replay | `Replay.frames()` | `replay_frames` | `int` | — | all |
| Replay | `Replay.live_key(key)` | `replay_live_key` | `bool` | — | all |
| Replay | `Replay.play()` | `replay_play` | `bool` | — | all |
| Replay | `Replay.progress()` | `replay_progress` | `float` | — | all |
| Replay | `Replay.record()` | `replay_record` | — | deferred | all |
| Replay | `Replay.status()` | `replay_status` | `string` | — | all |
| Replay | `Replay.stop()` | `replay_stop` | — | deferred | all |
| Snow | `Snow.set(entity_id, field, value)` | `snow_set` | — | deferred | all |
| FogOfWar | `FogOfWar.reset(entity_id)` | `fog_of_war_reset` | — | deferred; every cell unseen and unexplored again (a new run starts dark) | all |
| FogOfWar | `FogOfWar.set(entity_id, field, value)` | `fog_of_war_set` | — | deferred; a new `size` / `cell` makes a new, unseen grid | all |
| State | `State.forget(slot)` | `state_forget` | — | deferred | all |
| State | `State.has(slot)` | `state_has` | `bool` | — | all |
| State | `State.load(slot)` | `state_load` | — | deferred | all |
| State | `State.save(slot)` | `state_save` | — | deferred | all |
| Water | `Water.set(entity_id, field, value)` | `set_water_field` | — | deferred | all |
| Water | `Water.splash(entity_id, x, strength)` | `water_splash` | — | deferred | all |
| CanvasGroup | `CanvasGroup.set(entity_id, field, value)` | `set_canvas_group_field` | — | deferred | all |
| Video | `Video.set(entity_id, field, value)` | `set_video_field` | — | deferred; `"playing"`, `"looping"`, `"speed"`, `"seek"` (seconds) or `"source"` | all |
| Ragdoll | `Ragdoll.set(entity_id, field, value)` | `set_ragdoll_field` | — | deferred | all |
| Timeline | `Timeline.set(entity_id, field, value)` | `set_timeline_field` | — | deferred | all |
| BehaviorTree | `BehaviorTree.set(entity_id, field, value)` | `set_behavior_tree_field` | — | deferred | all |
| Area | `Area.set(entity_id, field, value)` | `set_area_field` | — | deferred | all |
| Nav | `Nav.closest_point(x, y)` | `nav_closest_point` | `...` | — | all |
| Nav | `Nav.find_path(x0, y0, x1, y1)` | `nav_find_path` | `...` | — | all |
| Nav | `Nav.is_moving(entity_id)` | `nav_is_moving` | `bool` | — | all |
| Nav | `Nav.set(entity_id, field, value)` | `set_nav_field` | — | deferred | all |
| Nav | `Nav.set_target(entity_id, x, y)` | `set_nav_target` | — | deferred | all |
| Nav | `Nav.stop(entity_id)` | `stop_nav` | — | deferred | all |
| Locale | `Locale.current()` | `locale_current` | `string` | — | all |
| Locale | `Locale.get(key, args)` | `locale_get` | `string` | — | all |
| Locale | `Locale.list()` | `locale_list` | `...` | — | all |
| Locale | `Locale.name(code)` | `locale_name` | `string` | — | all |
| Locale | `Locale.set(code)` | `locale_set` | — | deferred | all |
| Display | `Display.get_max_fps()` | `display_get_max_fps` | `int` | — | all |
| Display | `Display.get_mode()` | `display_get_mode` | `string` | — | all |
| Display | `Display.get_resolutions()` | `display_get_resolutions` | `{ {w, h, hz}, ... }` | — | all |
| Display | `Display.get_size()` | `display_get_size` | `w, h` | — | all |
| Display | `Display.get_vsync()` | `display_get_vsync` | `bool` | — | all |
| Display | `Display.set_max_fps(fps)` | `display_set_max_fps` | — | deferred | all |
| Display | `Display.set_mode(mode)` | `display_set_mode` | — | deferred | all |
| Display | `Display.set_size(w, h)` | `display_set_size` | — | deferred | all |
| Display | `Display.set_vsync(on)` | `display_set_vsync` | — | deferred | all |
| Display | `Display.supports(feature)` | `display_supports` | `bool` | — | all |
| Cursor | `Cursor.set_confined(on)` | `cursor_set_confined` | — | deferred | all |
| Cursor | `Cursor.set_icon(name)` | `cursor_set_icon` | — | deferred | all |
| Cursor | `Cursor.set_image(texture_id, hot_x, hot_y)` | `cursor_set_image` | — | deferred | all |
| Cursor | `Cursor.set_locked(on)` | `cursor_set_locked` | — | deferred | all |
| Cursor | `Cursor.set_visible(on)` | `cursor_set_visible` | — | deferred | all |
| Noise | `Noise.cellular(x, y, seed)` | `noise_cellular` | `...` | — | all |
| Noise | `Noise.fractal(x, y, opts)` | `noise_fractal` | `float` | — | all |
| Noise | `Noise.grid(x0, y0, w, h, step, opts)` | `noise_grid` | `...` | — | all |
| Noise | `Noise.perlin(x, y, seed)` | `noise_perlin` | `float` | — | all |
| Noise | `Noise.perlin3(x, y, z, seed)` | `noise_perlin3` | `float` | — | all |
| Noise | `Noise.simplex(x, y, seed)` | `noise_simplex` | `float` | — | all |
| Screenshot | `Screenshot.ready(id)` | `screenshot_ready` | `bool` | — | all |
| Screenshot | `Screenshot.take(opts)` | `screenshot_take` | `int` | — | all |
| Accessibility | `Accessibility.announce(text)` | `accessibility_announce` | — | deferred | all |
| Accessibility | `Accessibility.speech()` | `accessibility_speech` | `string` | — | all |
| Lighting | `Lighting.set_ambient(r, g, b, intensity)` | `set_ambient_light` | — | deferred | all |
| Lighting | `Lighting.set_color(entity_id, r, g, b)` | `set_light_color` | — | deferred | all |
| Lighting | `Lighting.set_global_illumination(strength, scale)` | `set_global_illumination` | — | deferred | all |
| Lighting | `Lighting.set_height(entity_id, height)` | `set_light_height` | — | deferred; how high a point, spot or directional light stands - the tangent of its elevation. Normal maps are lit from there, and a sun's shadows are `shadow_length / height` long, so lowering it lengthens them | all |
| Lighting | `Lighting.set_intensity(entity_id, intensity)` | `set_light_intensity` | — | deferred | all |
| Lighting | `Lighting.set_mask(entity_id, mask)` | `set_light_mask` | — | deferred | all |
| Lighting | `Lighting.set_occlusion(radius, strength)` | `set_ambient_occlusion` | — | deferred; the scene's `ambient_occlusion` (defaults 48, 0.6); a strength of 0 removes it, which also skips its pass | all |
| Lighting | `Lighting.set_radius(entity_id, radius)` | `set_light_radius` | — | deferred | all |
| Lighting | `Lighting.set_toon(bands, softness, dither)` | `set_toon_lighting` | — | deferred; the scene's `toon_lighting`: light stepped into `bands` (0 removes it), each edge `softness` of a band wide - or, with `dither` (a cell size in world units), that width drawn as a 4x4 ordered-dither pattern anchored to the world. Writes the first `toon_lighting` or spawns one; nil softness and dither are the defaults (0.08, 0) | all |
| PostProcess | `PostProcess.set_enabled(effect, enabled)` | `set_post_process_enabled` | — | deferred | all |
| PostProcess | `PostProcess.set_float(effect, param, value)` | `set_post_process_float` | — | deferred | all |
| PostProcess | `PostProcess.set_lut(texture_id, strength)` | `set_color_lut` | — | deferred | all |
| PostProcess | `PostProcess.set_vec4(effect, param, x, y, z, w)` | `set_post_process_vec4` | — | deferred | all |
| GUI | `GUI.focus(gui_id, node_id)` | `gui_focus` | — | command (browser: queued, no-op runtime) | all |
| GUI | `GUI.get(gui_id, node_id, property)` | `gui_get` | `string` | — | all |
| GUI | `GUI.hide(gui_id)` | `gui_hide` | — | deferred | all |
| GUI | `GUI.on_click(entity_id, gui_id, node_id, callback)` | `gui_on_click` | — | per-handler `callback_name` resolved at click time on all targets | all |
| GUI | `GUI.set(gui_id, node_id, property, value)` | `gui_set` | — | deferred | all |
| GUI | `GUI.show(gui_id)` | `gui_show` | — | deferred | all |
| Save | `Save.clear(slot)` | `save_clear` | — | deferred | all |
| Save | `Save.commit(slot)` | `save_commit` | — | deferred | all |
| Save | `Save.get(slot, key)` | `save_get` | `...` | — | all |
| Save | `Save.set(slot, key, value)` | `save_set` | — | deferred | all |
| Persistent | `Persistent.clear()` | `persistent_clear` | — | wipes both namespaces | all |
| Persistent | `Persistent.get_number(key)` | `persistent_get_number` | `float` | nil/null if absent | all |
| Persistent | `Persistent.get_string(key)` | `persistent_get_string` | `string?` | nil/null if absent | all |
| Persistent | `Persistent.has(key)` | `persistent_has` | `bool` | true if either namespace has the key | all |
| Persistent | `Persistent.set_number(key, value)` | `persistent_set_number` | — | immediate, survives scene transitions | all |
| Persistent | `Persistent.set_string(key, value)` | `persistent_set_string` | — | immediate, survives scene transitions; the same store Rust code (`scripting::persistent`, a package) reads and writes, in every runtime - a Rust write reaches scripts the next frame | all |
| Debug | `Debug.draw_circle(x, y, radius, r, g, b)` | `draw_debug_circle` | — | deferred | all |
| Debug | `Debug.draw_line(x1, y1, x2, y2, r, g, b)` | `draw_debug_line` | — | deferred | all |
| Debug | `Debug.draw_rect(x, y, w, h, r, g, b)` | `draw_debug_rect` | — | deferred | all |
| Debug | `Debug.get_draw_calls()` | `get_draw_calls` | `int` | — | all |
| Debug | `Debug.get_entity_count()` | `get_entity_count` | `int` | — | all |
| Debug | `Debug.get_fps()` | `get_fps` | `float` | — | all |
| Debug | `Debug.get_frame_time()` | `get_frame_time` | `float` | — | all |
| Debug | `Debug.log_stats()` | `log_stats` | — | deferred | all |
| Debug | `Debug.set_overlay(visible)` | `set_debug_overlay` | — | deferred | all |
| Data | `Data.get(table, id, field)` | `get_data` | native (Luau) / `Float` (host JS) | — | all |
| Script | `Script.call(caller, target, method, args)` | `call` | — | — | all |
| Script | `Script.load(entity_id, path)` | `load_script` | — | — | all |
| Math | `Math.angle_between(x1, y1, x2, y2)` | `angle_between` | `float` | — | all |
| Math | `Math.clamp(value, min, max)` | `clamp` | `float` | — | all |
| Math | `Math.distance(x1, y1, x2, y2)` | `distance` | `float` | — | all |
| Math | `Math.lerp(a, b, t)` | `lerp` | `float` | — | all |
| Platform | `Platform.game_ready()` | `platform_game_ready` | — | deferred | all |
| Platform | `Platform.gameplay_start()` | `platform_gameplay_start` | — | deferred | all |
| Platform | `Platform.gameplay_stop()` | `platform_gameplay_stop` | — | deferred | all |
| Platform | `Platform.happy_time()` | `platform_happy_time` | — | deferred | all |
| Platform | `Platform.name()` | `platform_name` | `string` | — | all |
| Platform | `Platform.show_ad(kind, reward)` | `platform_show_ad` | — | deferred | all |
| Platform | `Platform.submit_score(value, board)` | `platform_submit_score` | — | deferred | all |
| Global | `log(message)` | — | — | immediate | all |
| Global | `print(…)` | — | — | — | all |
| Global | `require(…)` | — | — | — | all |
| Alias | `get_entities_by_tag(tag)` | → `Entity.find_by_tag` | `...` | — | all |
| Alias | `get_entity_by_name(name)` | → `Entity.find_by_name` | `entity_id` | — | all |
| Alias | `register_event_listener(…)` | → `Event.on` | — | — | all |
| Alias | `set_post_process_param_float(…)` | → `PostProcess.set_float` | — | — | all |
| Alias | `set_post_process_param_vec4(…)` | → `PostProcess.set_vec4` | — | — | all |

<!-- END GENERATED API INDEX -->

## Transform

```lua
local x, y = Transform.get_position(entity)
Transform.set_position(entity, 100, 200)

local angle = Transform.get_rotation(entity)
Transform.set_rotation(entity, math.pi / 4)

local sx, sy = Transform.get_scale(entity)
Transform.set_scale(entity, 2.0, 2.0)
```

Reads come from the per-frame snapshot. Writes are deferred — calling `Transform.get_position` immediately after `Transform.set_position` returns the **previous** value until the next frame.

An entity that does not exist - despawned, or spawned this frame and not created yet - reads as **`nil`** from every `Transform.get_*`, in every runtime, so `if Transform.get_position(e) == nil` asks whether it is still there. (Until Sep 2026 native threw `no position for entity N`, killing the calling `update`, while the editor and web answered `0, 0` or the last position of whatever had the id - so the same question was an error in one runtime and "yes, forever" in the others.)

On a **dynamic physics body** `Transform.set_position` / `Transform.set_rotation` teleport the body; its velocity is kept, so call `Physics.set_velocity(e, 0, 0)` too for a dead stop. (Until Sep 2026 they did nothing on one - the physics sync wrote the body's own pose back over them - BUGS #158.)

### Hierarchy

| Function | Returns | Notes |
|---|---|---|
| `Entity.set_parent(child, parent)` | — | `nil` parent detaches. Self-parenting is ignored. |
| `Transform.get_world_position(entity)` | `x, y` | Composed down the parent chain. `nil` for an entity that does not exist, like the local getters. |
| `Transform.get_world_rotation(entity)` | radians | |
| `Transform.get_world_scale(entity)` | `sx, sy` | |

`Transform.set_position` and friends are always **local**. For an entity with no parent
the local and world values are identical, which is what the world getters
return in that case.

Available in every runtime as of Sep 2026; earlier builds registered them on
native only.

---

## Sprite & Material

```lua
-- Sprite RGBA tint (an entity whose picture is a `mesh` takes it too, since
-- Oct 2026 - recolour a territory without resending its vertices)
local r, g, b, a = Sprite.get_color(entity)
Sprite.set_color(entity, 1.0, 0.0, 0.0, 1.0)

-- Anchor point (0,0 = bottom-left, 0.5,0.5 = center, 0.5,0 = bottom-centre)
Sprite.set_anchor(entity, 0.5, 0.5)

-- The size it is drawn at, in world units (the transform's scale still
-- applies on top). Sizes a sprite without scaling its children, and works
-- on an `online_slot`, whose own scale jengine-online owns (Oct 2026).
Sprite.set_size(entity, 120, 80)

-- To swap a sprite's *texture*, give it a `sprite_library` and call
-- `Sprite.set_label(entity, label)`: every part of a library is preloaded,
-- and a texture nothing names is not (a native build loads what the scene
-- names when it starts).

-- Material switching
Material.set(entity, "materials/glow.material")

-- Material parameters: read by a graph's `MaterialParam` node of the same
-- name, per entity (a float arrives as (v, 0, 0, 0)).
Material.set_float(entity, "intensity", 1.5)
Material.set_vec4(entity, "tint", 1.0, 0.5, 0.0, 1.0)

-- Normal map: lights shade the sprite per pixel by it. nil takes it away.
Material.set_normal_map(entity, "sprites/env/bricks_n.png")
Material.set_normal_map(entity, nil)
```

**`Material.set_normal_map`** (Sep 2026) changes only
`MaterialComponent.normal_map_id` - the material and its parameters stay -
and creates the component on a sprite that has none. Name a normal map the
scene already references (some entity's `normal_map_id`): textures are loaded
with the scene, so an id nothing named draws flat, with a warning, where it
has not been loaded. The showcase's Crypt uses it for its N key; before it
existed, the scene authored every surface twice.

**To drive a graph material from the game**, give the graph a
`MaterialParam` node and set it per entity with `set_material_uniform_*`.
Up to **four** per graph, each a `vec4`; they take slots in sorted name
order. Each flush of the sprite batch carries them, and sprites batch by
`(material, texture, parameters)` - so sprites whose parameters are equal
still draw together, and only a different value splits a batch. Before Sep
2026 these setters wrote `MaterialComponent.uniforms` and nothing read it.
Test Lab scene 21's Tint swatches.

The sprite's **colour** is the other per-entity channel, read with a
`Vertex Color` node and costing no split at all:

```lua
Sprite.set_color(entity, r, g, b, a)
```

A searchlight game's reveal shader uses that one:
the script writes how lit an attacker is into its colour and the graph turns
that into an emissive rim. Web builds draw graph materials too since Sep
2026 - each `.material` and its `.shader` are fetched the first time
something draws with it.

**A graph can move its vertices** (Oct 2026): a `Vertex Offset` node (under
*Output* in the shader editor) takes a world-space offset that is added to
every vertex before it is drawn - grass and banners in the wind, a jelly's
wobble - with no script and no per-frame cost on the CPU. The nodes feeding
it run once per vertex and see that vertex's UV, colour, world position, the
time and the material parameters, so a parameter set from a script reaches
it as it reaches the colour. A sprite has four vertices - an offset times
`1 - uv.y` leans its top and keeps its foot - and a `mesh` has as many as it
is given, so a banner made as a grid ripples. A texture read there takes the
full-size mip; `Scene Color` cannot feed it. Test Lab scene 178; the
showcase's Windswept Meadow.

---

## Entity Lifecycle

```lua
-- Spawn
local enemy = Entity.spawn("Enemy")
local boss  = Entity.spawn_prefab("prefabs/boss.prefab")

-- Find
local player = Entity.find_by_name("Player")  -- returns entity_id or nil
local coins  = Entity.find_by_tag("coin")     -- returns array of entity ids

-- Find all entities within a radius of a point (optionally tag-filtered).
-- Read-only: returns immediately from the current frame snapshot.
local px, py = Transform.get_position(player)
local nearby_enemies = Entity.find_in_radius(px, py, 80, "enemy")  -- array of ids
local anything_close = Entity.find_in_radius(px, py, 32)           -- omit tag = any

-- Destroy
Entity.destroy(enemy)

-- Get the name string
local name = Entity.get_name(entity)
```

`Entity.spawn` / `Entity.spawn_prefab` return a pre-allocated handle you can store and
use immediately. **Placing a freshly spawned entity works on the next line:**

```lua
local crate = Entity.spawn_prefab("PhysCrate")
Transform.set_position(crate, 220, 120)   -- applies to the crate, not to nothing
Sprite.set_color(crate, 1, 0.5, 0, 1)
```

That is worth stating because it was not true until Sep 2026. The handle is
allocated in the script's own runtime and the entity is not created until the
engine drains the command queue, after the script returns — so `Transform.set_position`,
`Transform.set_rotation`, `Transform.set_scale` and `Sprite.set_color` had nothing to write to and
were **silently discarded** (BUGS #52). They now fall back to a queued command
when the entity is not addressable yet, which the engine applies after the
spawn. Ordinary entities still take the fast path, so nothing got slower.

*Reading* back is still a frame behind: `Transform.get_position` on a handle spawned this
frame returns `nil`, because the entity does not exist to read from. Store what
you set rather than asking for it.

### Prefabs with parts

A `.prefab` can be a hierarchy (Oct 2026): `children` hangs parts under it,
each placed in its parent's frame and with parts of its own, and a part can
be an instance of another prefab - so a cart's wheels are one wheel prefab.

```json
{ "name": "Cart", "components": { "sprite": { "texture_id": "cart.png", "size": [120, 34] } },
  "children": [
    { "name": "WheelL", "components": { "transform": { "position": [-40, -22] },
        "prefab_instance": { "prefab_id": "Wheel" } } },
    { "name": "Mast", "components": { "transform": { "position": [0, 50] } },
      "children": [ { "name": "Flag", "components": { "sprite": { "texture_id": "flag.png" } } } ] } ] }
```

```lua
local cart = Entity.spawn_prefab("Cart")    -- the cart, its wheels, its mast and flag
-- next frame:
local flag = Entity.find_child(cart, "Flag")        -- breadth first: children, then theirs
local same = Entity.find_child(cart, "Mast/Flag")   -- or a path
Entity.destroy(cart)                                -- its parts go with it
```

- `Entity.find_child(e, name)` answers `nil` for a part it has not. It
  matches the part's own name - in a scene a part is named `Cart/WheelL`
  (so two carts never share one), and the part after the last `/` is what
  is matched.
- Parts exist from the frame **after** the spawn, like any spawned entity.
- A **variant** (a prefab whose components carry a `prefab_instance`) keeps
  its base's parts and adds its own.
- A **scene instance** brings its parts as entities named `Cart/WheelL`,
  `Cart/Mast/Flag`. To change one part in a scene, author an entity with
  that name and `"parent": "Cart"`: it is laid over the part, component by
  component, rather than added beside it.
- Destroying an entity destroys everything parented to it, by whatever
  means it was destroyed (a `lifetime`, `despawn_offscreen`, a graph).

Engine Test Lab scene 203; the showcase's Siege Workshop.

---

## Tags

```lua
Entity.add_tag(entity, "enemy")
Entity.remove_tag(entity, "enemy")
if Entity.has_tag(entity, "boss") then ... end
```

---

## Custom Components

```lua
-- get_custom returns the field value as a number (host JS) or native type (Luau)
local hp = Entity.get_custom(entity, "Health", "current")
Entity.set_custom(entity, "Health", "current", hp - 10)
```

`Entity.set_fields(entity, "Game", { score = 10, state = "over" })` writes
several fields of one custom component in a single command - what a
controller publishing its state each frame wants. Like `set_custom`, it
writes only to an entity that already has custom components, so author the
component in the scene (`"Game": { "state": "title" }`).

The host JS adapter only marshals numbers; for booleans use `0` / `1`. Luau and
the editor/web runtimes keep numbers, bools and strings, and **tables** in both
directions: an array field reads back as a 1-indexed table, an object field as a
keyed table, and `Entity.set_custom(e, "C", "list", { 1, 2, 3 })` stores an array. This
is how a package publishes a list for scripts to read (a ball package's
`BallState.events`, say, or `jengine-net`'s `inbox`). Until Sep 2026 native Luau
returned nil for a table field and stored a table as null, while the editor
passed both through.

---

## Data Tables (data-oriented design)

Author typed **data schemas** (`.dataschema`) and **tables** of rows (`.data`)
in the editor's data editor, then read them at runtime. A table is a collection
of rows conforming to one schema; each row has a unique string `id`. Reference
fields point at a single row in another table.

```lua
-- Data.get(table, id, field) reads one field from a loaded .data table row.
-- `table` is the path relative to assets/ (with or without the .data extension).
local hp    = Data.get("data/enemies", "goblin", "health")   -- number
local name  = Data.get("data/enemies", "goblin", "display_name")

-- Reference fields return a { table, id } table on Luau; follow them with a
-- second get_data call:
local drop  = Data.get("data/enemies", "goblin", "drop")     -- { table=..., id=... }
if drop then
  local value = Data.get(drop.table, drop.id, "value")
end

-- Nested object fields and arrays return Luau tables:
local stats = Data.get("data/enemies", "goblin", "stats")    -- { attack=..., defense=... }
```

| Runtime | `Data.get` support |
|---------|--------------------|
| Native (Luau) | ✅ full fidelity — numbers, strings, bools, arrays, nested objects, `{table,id}` references |
| Editor viewport (Play mode) | ✅ full fidelity (browser shim reads `window.__jengine_play.data_tables`) |
| Web (`jengine-web` packaged runtime) | ✅ full fidelity — the builder lists `.data` tables and the runtime fetches + seeds them |
| Native JS (`rquickjs`) | numbers only (like `Entity.get_custom`) |

`Data.get` is **read-only**; there is no `set_data`. Tables are static data
authored in the editor, loaded once at startup.

---

## Input — Keyboard, Mouse, Actions

```lua
-- Keys (case-sensitive — see Key Names)
if Input.is_key_pressed("Space") then ... end
if Input.is_key_just_pressed("Escape") then ... end

-- Actions (configured in InputState)
if Input.is_action_pressed("jump") then ... end
if Input.is_action_just_pressed("fire") then ... end
if Input.is_action_just_released("crouch") then ... end

-- Mouse
local mx, my = Input.get_mouse_position()
local wheel  = Input.get_mouse_wheel()
local dx, dy = Input.get_mouse_delta()       -- moved this frame (x right, y down); see below
if Input.is_mouse_left_just_pressed() then ... end   -- press edge
if Input.is_mouse_left_pressed() then ... end        -- held (drag in progress)
if Input.is_mouse_left_just_released() then ... end  -- release edge (drop)
if Input.is_mouse_right_just_pressed() then ... end  -- and the right button, the same three ways
if Input.is_mouse_middle_pressed() then ... end      -- and the middle one (the wheel pressed)
```

`Input.get_mouse_delta()` is how far the mouse moved this frame, from the
device's own motion (natively winit's raw `MouseMotion`, about a pixel a
count; in a browser `movementX` / `movementY`, in the same pixels as the
position). Read motion from it - a spinner, a trackball, a mouse-look -
rather than from the difference of two positions, which stops when the
pointer reaches the window edge and jumps when it first enters the window.
It keeps counting while the pointer is locked (`Cursor.set_locked`). Engine
Test Lab scene 290.

The right button has been tracked by the engine all along - behaviour
graphs read it - but had no script surface until Sep 2026, so a game that
wanted right-click to cancel had to pick another key. The **middle** button
(Oct 2026) has the same three readings; no runtime tracked it before, and
the browser hosts stop the page's autoscroll on it.

### Touches - every finger

```lua
for _, t in ipairs(Input.get_touches()) do
    -- t.id        stable while the finger is down
    -- t.x, t.y    where it is, in get_mouse_position's units (window px, y down)
    -- t.start_x, t.start_y   where it landed
    -- t.wx, t.wy  the world point under it (get_mouse_world_position's arithmetic)
    -- t.phase     "began" | "moved" | "held" | "ended"
    -- t.on_control  it landed on an on-screen touch control
end
```

The first finger that lands on nothing is still the left mouse button, so a
game that only taps needs none of this. `Input.get_touches` is for the
second finger: a pinch, a two-thumb game, a chord. A finger is reported as
`"ended"` for exactly one frame after it lifts, and then it is gone - so a tap
shorter than a frame is seen only as `"ended"`. A finger on an on-screen
control (`touch_controls`) is listed with `on_control = true`; it is pressing
keys, and most games skip it.

Packages read the same list as `InputState::touches`. It is kept in one
place, `input::touch_controls::TouchControlsState::tick`, which every host
calls with its finger events. Before Sep 2026 no script or package could see
a second finger at all. The desktop has no touches, so a game should keep a
mouse or keyboard way to do the same thing. Test Lab scene 108 shows one ring
per finger.

### Gestures - what the fingers mean

```lua
for _, g in ipairs(Input.get_gestures()) do
    -- g.kind     "tap" | "double_tap" | "long_press" | "swipe" | "pan" | "pinch" | "rotate"
    -- g.x, g.y   where (window px, y down): the finger, or the middle of two
    -- g.wx, g.wy the world point there
    -- g.dx, g.dy a pan's move this frame, a swipe's whole move (screen px)
    -- g.wdx, g.wdy  the same move in world units, y up - add it to a dragged thing
    -- g.scale    a pinch's factor this frame (1 is no change)
    -- g.angle    a twist this frame, radians, anticlockwise as seen
    -- g.speed    a swipe's speed, px a second
    -- g.phase    "began" | "changed" | "ended" for pan, pinch and rotate
end
```

Oct 2026 (`input::gestures`). The engine recognises gestures from the
touches once, inside `InputState::update_interactions` (which every host
calls before scripts, on the wall clock), so a game reads meanings rather
than tracking fingers itself. A **tap** is a lift within 0.3 s that never
wandered past 12 px; a second within 0.35 s and 30 px is a **double tap**
(instead of a second tap); a finger held still 0.5 s is a **long press** (once,
`"began"`), and it is no longer a tap. A finger that wanders is a **pan** -
`"began"` on the frame it passes 12 px, `"changed"` while it moves, `"ended"`
when it lifts - and a lift after at least 50 px at 300 px/s or more is also a
**swipe**, reported the same frame as the pan's end. Two fingers down make a
**pinch** and a **rotate** about their middle, both `"began"` when the second
lands; a finger that was ever part of two is never a tap or swipe. Fingers on
on-screen touch controls are left out.

On a desktop the left mouse button is a finger while no real finger is down,
and the **wheel is a pinch** about the pointer (`1.1` per notch, phase
`"changed"` with no began or end) - so everything but a twist can be tried
without a phone. Packages read the same list as `InputState::gestures`.
Engine Test Lab scene 257; the showcase's Photo Album.

### Analog input and gamepads

```lua
-- How far an action is pushed, dead-zoned, 0.0 .. 1.0.
-- A key or button binding reads 1.0 while held, so the same action works
-- whether the player is on a keyboard or a stick. Every axis binding names a
-- direction (`Pad0_LeftStickX+` / `Pad0_LeftStickX-`), so an action is
-- one-sided: a signed axis is two actions subtracted.
local x = Input.get_action_axis("move_right") - Input.get_action_axis("move_left")

-- Raw device access, when a stick really is a stick.
if Input.is_gamepad_connected(0) then
    local lx = Input.get_gamepad_axis(0, 0)          -- left stick X
    local ly = Input.get_gamepad_axis(0, 1)          -- left stick Y, up is +
    if Input.is_gamepad_button_pressed(0, 0) then ... end
end
local pads = Input.get_gamepad_count()               -- slots seen, connected or not
```

Before Sep 2026 a `-` binding returned a *negative* value while a key on the
same action returned +1.0, so `right - left` sent a stick and a D-pad in
opposite directions. `Input.is_action_just_released` also ignored gamepad bindings;
it now sees a button release and an axis returning inside its dead zone.

When the window or tab loses focus, every held key and mouse button is
released - with a `just_released` edge - because the platform delivers no
key-up to an unfocused window. Native builds also no longer quit on Escape;
a game quits with `Scene.quit()`.

**Prefer `Input.get_action_axis`.** It goes through the binding layer, so the player
can rebind it and a keyboard still drives it; the `gamepad_*` four bypass all
of that. `Input.get_gamepad_axis` is deliberately **not** dead-zoned — a dead zone
belongs to a binding, not to a device, so a script reading a stick directly
picks its own threshold.

**Stick Y is positive up**, in every runtime, like the world - so `move_up`
binds `Pad0_LeftStickY+`. Until Sep 2026 the runtimes reported it positive
*down* (the browser's convention, which the desktop copied by flipping gilrs)
while every game bound up to `+`: a real controller moved every game
upside down. Nothing had been played with one.

Pad indices are stable across a disconnect (the slot is kept), which is why
`Input.get_gamepad_count` counts slots ever seen and `Input.is_gamepad_connected` is a
separate question. Check it before trusting an axis: a stick reading 0.0
because nothing is plugged in looks exactly like one held at centre.

> Until Sep 2026 pads reached scripts only as thresholded action *presses*, and
> in the browser runtimes not even that: the preamble resolved actions by
> testing their bindings against a keyboard-only key map, so `Pad0_A` was never
> found and gamepad actions were silently dead in the editor and in web builds
> while working natively. Actions are now resolved once, in Rust, and the hosts
> publish the answers (BUGS #9).

### Rebinding at runtime

```lua
local SLOT, current = "controls", {}

local function bind(action, keys)
    current[action] = keys                 -- kept here: see below
    Input.set_bindings(action, keys)
end

function init(entity)
    local saved = Save.get(SLOT, "bindings")
    if type(saved) == "table" then
        for action, keys in pairs(saved) do bind(action, keys) end
    end
end

-- In update, once the player has picked an action to rebind:
local pressed = Input.get_just_pressed()   -- e.g. { "K" } or { "Pad0_A" }
if #pressed > 0 then
    bind("jump", { pressed[1] })
    Save.set(SLOT, "bindings", current)
    Save.commit(SLOT)                      -- holds after a restart
end
```

**`Input.get_just_pressed()`** (Oct 2026) is every input that went down this
frame, as the name an action binds it by: keys by their canonical name
(`K`, `Space`, `Up`), pad buttons as `Pad0_A`, and a stick pushed past the
dead zone this frame as `Pad0_LeftStickX+` / `-` - sorted, one table, empty
on a frame with no press. Hand one to `Input.set_bindings` and the action
answers to it. Two things to know. **`Input.set_bindings` is deferred**:
`Input.get_bindings` answers the old keys until the frame is over, so save
from a table the script keeps (as above), not by asking back. And a screen
that listens should stop its own navigation keys while it does, or the key
the player meant to bind does what it always did. Test Lab scene 195; the
showcase's The Controls Room.

**`Input.get_typed()`** (Oct 2026) is the text typed this frame, as the
keyboard **layout** made it - so a command that is a character works on
any keyboard. `is_key_just_pressed` names a physical key: `"Slash"` with
shift is `?` on a US keyboard and `_` on a Finnish one, where `?` is shift
and the key beside 0. Roguelike commands (`?`, `<`, `>`) want this one:

```lua
local typed = Input.get_typed()          -- "" most frames; "?" when it was typed
if typed:find("?", 1, true) then show_help() end
```

Control keys (Enter, Escape, Backspace, arrows) are not text - read them
with `is_key_just_pressed`. While a text field has the keys it consumes
them, and this is empty.

### Hold, tap, double tap - and button prompts

```lua
if Input.is_action_tapped("jump") then hop() end              -- released before 0.25 s
if Input.is_action_held("charge", 0.5) then tier(1) end       -- the frame a press reaches 0.5 s
if Input.is_action_held("charge", 1.5) then tier(2) end       -- ... and 1.5 s, of the same press
local t = Input.get_hold_time("charge")                       -- seconds held so far, 0 while up
if Input.is_action_double_tapped("dash") then dash() end      -- second press within 0.3 s

local p = Input.get_prompt("jump")      -- for the device in hand
-- p.label "Space" / "A" / "Cross" / "B", p.glyph "keyboard/space" / "xbox/a" /
-- "playstation/cross" / "switch/b", p.binding "Pad0_A", p.style "xbox"
Sprite.set_label(icon, p.glyph)         -- an icon per glyph, in a sprite_library
Input.get_prompt("jump", "switch")      -- in a named style: keyboard, xbox, playstation, switch, generic

Input.get_last_device()                 -- "keyboard", "mouse", "gamepad", "touch" or "none"
Input.get_last_gamepad()                -- the pad used last, or -1
Input.get_gamepad_type(0)               -- "xbox", "playstation", "switch", "generic", "" for none
Input.get_gamepad_name(0)               -- "DualSense Wireless Controller"
```

**Interactions** (Oct 2026) are asked of an action rather than kept in a
script: the engine times each action's presses on the wall clock - input is
the player's time, so a paused game does not stop a hold - and every
threshold is an argument, so one press can be asked about several holds.
`is_action_held(a, s)` is true on the **one frame** a press reaches `s`
(default 0.5); `is_action_tapped(a, s)` on a release before `s` (default
0.25), so tap-or-hold on one button never fires both for the same `s`;
`is_action_double_tapped(a, s)` on a press within `s` (default 0.3) of the
previous one, counted in pairs - a fourth quick press is a second double
tap. A press and release inside one frame is still a tap. An action held by
two bindings is one press: the key coming up while the pad's button is down
releases nothing.

**Prompts** name the binding in the family of the pad in hand. Buttons are
read positionally (`Pad0_A` is the south button on every pad), so the label
is the family's for that place: A, Cross, or **B** on a Switch. A prompt
shows the action's first binding of the device in use - its first key on a
keyboard, its first pad binding on a pad - or, with none, its first binding
of any kind, drawn as what it is. `glyph` is `<style>/<button>` (`xbox/rb`,
`playstation/r1`, `keyboard/shift_left`, `xbox/left_stick_up`): give each
icon you draw that name as a `sprite_library` label and
`Sprite.set_label(icon, p.glyph)` is the whole swap. A pad's family comes
from its USB vendor, or failing that its name; anything else is `generic`
(labelled as an Xbox pad). The **last device** is the last key, mouse
button, pad press or finger - moving the mouse does not change it. Test Lab
scene 207; the showcase's Button Prompts.

### Patrol routes (no script needed)

A scene can declare named waypoint routes, and an entity walks one by carrying
a `patrol_path` component. The engine does the walking — no script ticks.

```json
{
  "name": "Level 1",
  "paths": {
    "guard_route": { "points": [[-200, 0], [200, 0], [200, 120]], "looped": true }
  },
  "entities": [
    { "name": "Guard", "components": {
        "transform": { "position": [-200, 0] },
        "patrol_path": { "path_id": "guard_route", "speed": 90.0, "mode": "PingPong" }
    }}
  ]
}
```

`mode` is `"Loop"` (wrap to the first waypoint), `"PingPong"` (reverse at each
end) or `"Once"` (stop on the last). Many entities can share one route by naming
the same id. An unknown `path_id` is skipped silently, so an asset typo leaves
one guard standing still rather than crashing the level.

Routes live in the scene because a patrol route is level geometry — it is placed
against the walls of one level and means nothing in another. That also means
every runtime gets them from the scene parsing it already does.

> This was inert until Sep 2026. `Path2D`, the library, the component and the
> walk system all existed, with tests covering all three modes, and no runtime
> ever filled the library — so `PatrolPathRef` moved nothing anywhere and there
> was no way to author a route at all (BUGS #7). Working example: Engine Test
> Lab scene 64.

### Action-driven components (no script needed)

For the common case of "translate input into motion", attach these
components in the scene JSON instead of writing a per-entity script.
They run before `velocity_system` each frame, so the impulse takes
effect on the same tick.

```json
{
  "input_axis": {
    "x_neg": "move_left", "x_pos": "move_right",
    "y_neg": "move_down", "y_pos": "move_up",
    "scale": 200.0,
    "normalize": true,
    "drive": "velocity"
  },
  "input_action_impulse": {
    "action": "jump",
    "impulse_y": 350.0
  }
}
```

- `InputAxis2D` writes `current: Vec2` from any combination of the four
  optional action names; `scale` multiplies the result; `normalize`
  caps diagonals at unit length before scaling. `drive` applies it with
  no script: `"velocity"` sets the entity's `velocity` to it every frame,
  `"acceleration"` its `acceleration` (so the entity has momentum, and
  `drag2d` or `max_speed` shape it). The default, `"none"`, leaves
  `current` for a script, which was the only option before Sep 2026 -
  an axis on its own moved nothing.
- `InputActionImpulse` overwrites `Velocity2D.x` / `.y` only on the
  rising edge of the action. Either axis may be omitted (`null`) to
  leave that axis untouched.

### Forces (no script needed)

Every runtime integrates these into `velocity` each frame, in the one
shared motion step (`behaviors::tick_motion`), with unit mass:

| Component | Each frame |
|---|---|
| `wind2d` / `force2d` | `vel += (x, y) * dt` - two, so an entity can carry its own push and the weather's |
| `impulse2d` | `vel += (x, y)` once, then it is zeroed |
| `drag2d` | `vel *= 1 - drag * dt` per axis, never below zero - a body settles at force / drag |
| `torque2d` | `angular_velocity.omega += x * dt` |

Until Sep 2026 none of the five was integrated anywhere: they were batch
script vocabulary, authorable and documented, and a `drag2d` on a sprite
did nothing. An entity with a running **batch script** is left to that
script - it integrates its own velocity and position, and the native
integrators used to move it a second time. Test Lab scene 101; a whole game
can be made of these, with no script.

---

## Animation

```lua
Animation.play(entity, "run")
Animation.stop(entity)
Animation.set_speed(entity, 1.5)

if Animation.is_playing(entity) then
    local clip = Animation.get_current(entity)
end

-- State machine parameters
Animation.set_bool(entity, "is_grounded", true)
Animation.set_float(entity, "speed", 4.2)
Animation.set_trigger(entity, "jump")

-- Force a state transition
Animation.start_state_machine(entity, "Idle")

-- Read current state
local state = Animation.get_state(entity)
```

`Animation.play` / `stop` / `set_speed` drive whatever the entity has - a
sprite animation, a skeletal player (`skeletal_player`), or both. Until Sep
2026 they reached only the sprite animator in every runtime, so no script
could start a skeletal clip.

### Spawning a rig

```lua
local arm = Skeleton.spawn_rig("rigs/arm.rig")   -- a handle naming the root bone
local wrist = Skeleton.find_bone(arm, "wrist")   -- from the next frame
```

A `.rig` file is a skeleton as JSON - `{ "name": "arm", "bones": [{ "name",
"parent", "position", "rotation" (radians), "scale", "length" }] }` - and
`spawn_rig` makes each bone an entity with its bind-pose transform, parented
by name, the first bone with no parent carrying `skeleton_root`. It is named
by its path under `assets/`, as a clip is. Bones have no sprites; attach
them, or a `skinned_mesh`, to draw one. Every runtime loads every `.rig` in
the project since Oct 2026 - before that none did, so the call spawned
nothing anywhere. Test-lab scene 34.

### Skeletal clips: blend, crossfade, foot IK

```lua
Animation.play(cat, "cr_walk")               -- a clip by its name or its path
Animation.crossfade(cat, "cr_idle", 0.5)     -- fade the current pose out over 0.5 s
Animation.set_blend(cat, "cr_run", 0.4)      -- mix a second clip in (0..1)
Animation.set_blend_position(ranger, 1, 0.5) -- move a 2D blend space
Skeleton.set_ik_weight(paw, 0)               -- how much of a two-bone IK solve applies
```

A skeletal player **blends two clips** (Sep 2026): `blend_clip` at `blend`,
both sampled at the **same phase** - the same fraction of each clip's length
- so a walk of 1 s and a run of 0.6 s blend step for step, and the cycle's
pace moves between theirs with the weight. Author both clips with each
leg's footfall at the same phase and a walk turns into a run by one number.
A channel only one clip animates is taken from that clip.

**`Animation.crossfade`** starts a clip from its beginning and fades the
old pose out over the seconds given; the clip being left keeps playing (its
blend with it) while it fades. `Animation.play` still snaps. A clip is found
by its asset path (`animations/walk.skelanim`) or, failing that, by its
`name`.

**A 2D blend space** (Oct 2026; Godot's `AnimationNodeBlendSpace2D`,
Unity's 2D Freeform blend tree) places clips on two axes:

```json
"skeletal_player": { "clip": "", "blend_space": {
  "points": [ { "clip": "animations/idle.skelanim", "at": [0, 0] },
              { "clip": "animations/walk_fwd.skelanim", "at": [1, 0] },
              { "clip": "animations/walk_left.skelanim", "at": [0, 1] } ],
  "position": [0, 0] } }
```

A player whose `clip` is empty plays it: every clip with a weight, all at
the same phase, the cycle's pace between theirs by the weights (`time` is
then the phase, 0 to 1). The weights are gradient band interpolation
(Unity's Freeform Cartesian) - a point's clip is the whole pose on that
point, the mix is continuous between points, and a position outside them is
still defined. `Animation.set_blend_position(e, x, y)` moves it - a walk in
eight directions is the velocity turned into the character's own frame.
`Animation.play` / `crossfade` to a clip leave the space, and to `""` come
back to it, faded like any crossfade. Test Lab scene 189, showcase scene 70.

**Layers** (Oct 2026; Unity's Animator layers and avatar masks, Godot's
blend-tree filters) lay clips over the pose above, on part of the skeleton:

```json
"skeletal_player": { "clip": "animations/walk.skelanim", "layers": [
  { "name": "upper", "clip": "", "mask": ["torso"], "weight": 0 },
  { "name": "breath", "clip": "animations/breathe.skelanim", "mode": "additive", "mask": ["torso"], "weight": 0.4 } ] }
```

```lua
Animation.play_layer(archer, "upper", "animations/draw.skelanim", 0.15)        -- (e, layer, clip, fade?, loop?)
Animation.play_layer(archer, "upper", "animations/aim.skelanim", 0.2, true)    -- looping
Animation.stop_layer(archer, "upper", 0.3)                                     -- fade it out
Animation.set_layer_weight(archer, "breath", 1.0, 0.5)                         -- fade to a weight
function on_layer_end(entity) end                                              -- on the rig's own script
```

Each layer has its own clip, playhead and weight, and drives only its
`mask`: each bone named **and every bone below it** (`["torso"]` is the
torso, head and arms), every bone when empty. Layers apply in order, each
over everything before it - `override` (the default) mixes its pose in by
its weight, `additive` adds its motion away from its own first frame, so a
breath or a recoil sits on any base clip (it moves only bones the pose
under it animates). `play_layer` starts the clip from its beginning and
fades the layer in over the seconds given (0.2 by default); a **one-shot**
(`loop` false, the default) fades itself out over its `release` (0.25 s)
when the clip ends - 0 holds its last pose - and calls `on_layer_end` on the
rig's own script. A layer `play_layer` names that the scene did not author
is made, over every bone. A stopped base clip (`Animation.stop`) leaves its
layers playing. Test Lab scene 209; the showcase's Archer's Range.

**Foot IK on uneven ground**: an `ik_two_bone` on a paw can carry a
`ground` - `{ "floor": -104, "reach": 160, "mask": 65535 }` - and then its
target is not a point a script sets but the paw **where the clip put it**,
moved onto the first collider under it by however far the ground there is
above or below the clip's own floor (`floor`, a height in the skeleton
root's frame). A walk authored on flat ground climbs a slope and steps onto
a rock; the body is the game's to raise and pitch. The colliders are read as
outlines on the CPU (`ecs::outlines`, as particles do), so it needs no
physics. `weight` (0..1, `Skeleton.set_ik_weight`) blends any two-bone
solve over the animated pose. Showcase scene 21, Test Lab scene 135.

**IK chains** (Oct 2026; FABRIK - Godot's `SkeletonModification2DFABRIK`,
Unity's 2D IK Chain):

```json
"ik_chain": { "target": [120, 80], "bones": 11, "iterations": 12, "tolerance": 0.5, "max_bend": 0.45, "weight": 1 }
```

On a chain's **end** bone, it bends that bone and the `bones - 1` above it
so the end bone's tip reaches `target` - or the entity named
`target_name`, followed wherever it goes. A tail, a tentacle, a long arm, a
rope of bones. Out of reach, the chain points straight at the target.
`max_bend` limits each joint after the first to that many radians either
way from its pose (the clip's, or the authored one), which is what keeps a
tail from folding back on itself; `weight` blends the solve over the pose.
`Skeleton.set_ik_target` and `Skeleton.set_ik_weight` reach it as they reach
`ik_two_bone`. Chains are solved after two-bone IK and before look-at. Test
Lab scene 188, showcase scene 69.

**Jiggle bones** (Oct 2026; Godot's `SkeletonModification2DJiggle`):

```json
"jiggle": { "stiffness": 0.12, "damping": 0.88, "gravity": [0, -200], "max_angle": 1.3 }
```

On a `bone`, its tip lags the pose and springs back - a tail trailing a
running creature, ears that flop, a jellyfish's tentacles, kelp in a
current. `stiffness` is how much of the way back to the pose the tip is
pulled each frame (at 60 fps; 1 is rigid), `damping` how much speed it
keeps, `gravity` a steady pull in world units a second squared (down for a
tail, up for something buoyant, sideways for a current), and `max_angle`
the furthest it may swing from its pose, in radians. The pose is the clip's
when a skeletal player animates the bone, and the authored rotation when
nothing does - so a script that turns a bone (`Transform.set_rotation`) sets
where it springs to. A chain of jiggle bones whips, since each link's pose
follows its parent as the parent is this frame; keep `max_angle` small on
the links of a long soft chain (0.3 or so), or it folds into kinks. There
are no script functions: move what the bones hang from, and they follow.
Test Lab scene 169; the showcase's Jellyfish Bloom.

---

## Tweens

```lua
local tw = Tween.position(entity, 200, 300, 1.0, "ease_in_out")
Tween.rotation(entity, math.pi, 0.5, "linear")
Tween.scale(entity, 2.0, 2.0, 0.3, "ease_out")
Tween.color(entity, 1, 0, 0, 1, 0.4, "ease_in")

Tween.cancel(tw)
Tween.cancel_all(entity)
```

`Tween.color` fades an entity's sprite, or its **world text** when it has
text and no sprite - a "PERFECT" that pops and fades is one text entity,
`Text.set_color` to show it and `Tween.color` to its transparent self.
Before Sep 2026 text had no colour setter at all and a colour tween on it
did nothing, so a fading label took one entity per colour.

`easing` is any name accepted by `EasingFunction::from_name`: `linear`, `ease_in` / `ease_out` / `ease_in_out` (the quadratic curves), and the `quad` / `cubic` / `sine` families as `ease_in_quad` / `ease_out_quad` / `ease_in_out_quad` (likewise `_cubic`, `_sine`). The CamelCase spellings (`EaseInQuad`) work too. There is **no** bounce or elastic curve — an unknown name logs a warning and silently falls back to `Linear`, so a typo is a wrong curve rather than an error.

---

## Timers

```lua
function on_explode(entity) Entity.destroy(entity) end
local t = Timer.after(entity, 2.0, "on_explode")  -- one-shot
local i = Timer.every(entity, 0.5, "on_tick")  -- repeating
Timer.cancel(t)
```

The callback function must be defined at the script's top level (visible in its environment) and takes `(entity)`.

---

## Events

```lua
-- Listen
Event.on("score_changed", entity)
function on_score_changed(entity, data)
    -- handler — called on the entity that registered
end

-- Or one handler for every event this entity subscribed to. Both spellings
-- work in all four runtimes since 2026-09-21; before that each runtime knew
-- only one of them, so a handler written the "wrong" way was silently never
-- called (BUGS #150). `on_<name>` wins when a script defines both.
function on_event(entity, name, sender, data)
    if name == "score_changed" then ... end
end

-- Send. Broadcast reaches every entity that subscribed with Event.on;
-- Event.send_to targets one. Handlers run before the frame's commands are
-- applied, so a handler can move things, stop audio, spawn — anything a
-- script can do. The payload is optional: a table, number, boolean or string.
Event.send("score_changed", entity, { score = 42 })

Event.off("score_changed", entity)
```

The payload travels as JSON text and the handler gets the **value** - a table
arrives as a table, `7` as a number, other text as the string it is - in all
four runtimes. Natively a table payload used to throw ("error converting Lua
table to String") and JSON text reached the handler as text (fixed Oct 2026).
An event sent from a callback dispatched after the world is stepped -
`on_collision_enter`, `on_animation_event`, `on_screen_entered` - reaches its
listeners on the next frame; natively it used to reach none.

## Visibility notifier

```json
"visibility_notifier": { "size": [0, 0], "offset": [0, 0], "margin": 0, "pause_offscreen": false }
```

Tells the entity's own script when it comes on screen and when it leaves
(Oct 2026; Godot's `VisibleOnScreenNotifier2D`, Unity's `OnBecameVisible`):

```lua
function on_screen_entered(entity)
    -- wake up: start an animation, play a sound, spawn its enemies
end

function on_screen_exited(entity)
    -- go to sleep
end
```

The box is `size` (local units, about the entity plus `offset`), or the
sprite's quad when `size` is zero, turned and scaled with the entity. `margin`
grows the camera's view by that many world units: positive notices something
a little before it shows, negative only once it is well inside. The first
frame reports what starts on screen and nothing about what does not.
`pause_offscreen` (Godot's `VisibleOnScreenEnabler2D`) holds the entity's
sprite animation, skeletal player and particle emission while it is out of
view, and restarts only what it stopped. Test Lab scene 170; the showcase's
Night Bloom.

## Path follow

```json
"path_follow": { "path": "Railway", "speed": 120, "progress": 0, "mode": "loop", "rotate": true, "lateral": 0 }
```

Rides another entity's `vector_path` (Oct 2026; Godot's `PathFollow2D`):
`path` is that entity's name, `speed` world units a second along the curve
(negative rides backwards), `progress` how far along it is. Curves are
measured, so the speed is even round them, and the path is read in world
space - move or turn the path's entity and its riders go with it.

- `mode`: `loop` (round again; seamless on a closed path), `ping_pong` (turn
  round at each end), or `once` - stop at the end and call the script's
  `on_path_end(entity)`.
- `rotate` turns it to face along the path, plus `rotation_offset` radians
  (for art that points up, `-math.pi / 2`).
- `lateral` rides that far to the **left of the way it is going** (negative
  is right), so traffic that keeps left uses one sign in both directions.
- `playing` stops and starts it.

```lua
PathFollow.set(train, "speed", 200)        -- any field by name
PathFollow.set(shuttle, "path", "Lower")   -- switch tracks
PathFollow.set(ferry, "progress", 0)       -- jump
PathFollow.set(ferry, "playing", true)

function on_path_end(entity)               -- a `once` rider got there
end
```

A train is one path and several riders at different `progress`, all at the
same speed. Test Lab scene 171; the showcase's Model Railway.

## Sorting groups

```json
"sorting_group": { "z_order": 0, "y_sort": true }
```

On a parent, makes its whole hierarchy sort as **one** drawable (Oct 2026;
Unity's Sorting Group): everything under it sorts at the group's `z_order`
(and `sort_layer`), and with `y_sort` by where the parent stands - lower on
screen in front - against other groups and y-sorted sprites of that z. Inside
the group the parts keep their own order by their own `z_order`. A character
of parts - legs, body, head, a front arm at z 10 - then passes in front of
and behind other characters whole, instead of every front arm in the scene
drawing over every body. A group inside a group is part of the outer one.
There are no script functions. Test Lab scene 172; the showcase's Village
Square.

## Lines

```json
"line": { "points": [[0, 0], [100, 40], [200, 0]], "width": 10, "joint": "round",
          "begin_cap": "round", "end_cap": "box",
          "width_curve": [[0, 0.2], [0.5, 1], [1, 0.2]],
          "gradient": [[0, 1, 0.3, 0.3, 1], [1, 0.3, 0.5, 1, 1]],
          "texture_id": "sprites/arrow.png", "texture_mode": "tile", "tile_length": 24 }
```

A styled polyline (Oct 2026; Godot's `Line2D`, Unity's `LineRenderer`):
`points` in the entity's local space, `closed` to join the ends. Along the
line, `t` runs 0 to 1 by distance: `width_curve` multiplies `width` and
`gradient` (`[t, r, g, b, a]` stops) colours it, times `color`. A
`texture_id` lies along it `stretch`ed once or `tile`d every `tile_length`
units (the line is cut where the texture repeats, so tiles stay whole).
`joint` is `sharp` (cut to a bevel past `sharp_limit` half-widths), `bevel`
or `round`; `begin_cap` / `end_cap` are `none`, `box` or `round`. It draws
in the sprite pass by `z_order`, lit unless `unlit`, with its `blend_mode`.

```lua
Line.set(laser, "points", { { 0, 0 }, { 200, 80 }, { 320, -40 } })  -- reshape it
Line.set(laser, "joint", "round")                                  -- any field by name
```

`Line.set` writes any field; setting `points` every frame is the way to
draw something that moves - a beam traced through mirrors, a bolt of
lightning, a rope. Test Lab scene 173; the showcase's Tesla Lab.

## Specular highlights

```json
"material": { "material_id": "lit_sprite", "normal_map_id": "sprites/coin_n.png",
              "specular": 1.0, "shininess": 40 }
```

A material's `specular` (0..1, default 0) makes point and spot lights glint
off it (Oct 2026): a highlight where the light's reflection would meet the
eye, in the light's colour and **added** over the surface, so a dark shiny
thing still glints. `shininess` (1..128, default 24) is how tight it is - 4
a broad sheen, 120 a pin-point. With a normal map the highlights follow its
bumps, and its **alpha** masks them: paint it white where the sprite is
metal and dark where it is cloth, leather or stone. Each light is treated as
hanging `height * radius` above the plane for its highlight, so a lower or
smaller light gives a tighter spot. Highlights can pass 1.0, so they bloom
in an HDR scene; in an LDR one, leave the diffuse some headroom. There are
no script functions. Test Lab scene 174; the showcase's Dragon's Hoard.

## Canvas groups

```json
"canvas_group": { "opacity": 0.5, "tint": [1, 1, 1, 1], "z_order": 10 }
```

On a parent, draws it and everything under it as **one picture** (Oct 2026;
Godot's `CanvasGroup`, Unity's `CanvasGroup` alpha) and then draws that
picture with `opacity` and `tint` at the group's `z_order`. Fading a figure
made of parts by fading each part shows the parts through each other - the
arm behind the body shows through the body - and a canvas group fades it as
a picture of it fades. The members are drawn lit and in their own `z_order`
inside the group; a group inside a group is part of the outer one; opacity 0
hides the group and skips its drawing.

```lua
CanvasGroup.set(ghost, "opacity", 0.3)            -- fade it
CanvasGroup.set(ghost, "tint", { 0.7, 0.9, 1, 1 }) -- colour it
```

Each group costs a pass over the world a frame (only its members are drawn),
so use it for what fades - a ghost, a dissolving enemy, a panel in the
world - not to organise a scene. Test Lab scene 175; the showcase's Haunted
Hall.

A **`material`** on the group's entity (Oct 2026) draws the whole picture
through it - an effect on the group, not on each part: an outline round a
ghost's silhouette rather than round every limb where they overlap, a
shimmer over the whole of it. The picture is premultiplied, so the material
should blend `PremultipliedAlpha` (a graph's output is then taken as
premultiplied); its `uniforms` are read as on any sprite. The material is
the group's alone: the group entity's own sprite, if it has one, draws
plain inside the picture. Test Lab scene 213 (`lab213_ghost_glow`); the
showcase's Ghost Lantern, whose `ghost_glow` graph (`tools/gen_shaders.py`)
is a worked example.

## Video

```json
"video_player": { "source": "videos/intro.mp4", "texture_id": "", "playing": true,
                  "looping": true, "speed": 1.0 }
```

Plays a video file into a texture (Oct 2026): `texture_id`, or the source's
own path when that is empty. Anything that names the texture shows the film
- a sprite, a mesh, a UI image - and two sprites naming one texture show the
same frame, which is how a screen and a monitor show one film. The texture is
written each frame a new picture is due, never loaded from a file.

```lua
Video.set(screen, "playing", false)   -- pause (true to carry on)
Video.set(screen, "seek", 0)          -- back to the start, in seconds
Video.set(screen, "speed", 0.5)       -- slow motion
Video.set(screen, "looping", false)

function on_video_end(entity)         -- a film that does not loop, at its end
end
```

The film is **silent**: play its soundtrack with `Audio.play_music`. It plays
on Windows (Media Foundation), in the editor and in web builds (the
browser's own decoder); on Android it is a black texture and a warning in the
log. Use H.264 (baseline profile, `yuv420p`) in an `.mp4` - every one of them
plays that. Test Lab scene 176; the showcase's Picture House.

---

## Timelines

```json
"timeline": {
  "clips": { "act1": {
    "duration": 8.0,
    "tracks": [
      { "target": "Knight", "component": "transform", "field": "position",
        "keys": [[0.0, [-560, -168]], [4.0, [-150, -168]]], "ease": "in_out" },
      { "target": "Spot", "component": "spot_light", "field": "color",
        "keys": [[0.0, [1, 0.85, 0.6]], [6.0, [0.45, 0.55, 1.0]]] },
      { "target": "Caption", "component": "text", "field": "text",
        "keys": [[0.0, ""], [1.0, "Act I"]] }
    ],
    "events": [[6.0, "thunder"]]
  } },
  "clip": "act1", "playing": true, "speed": 1.0, "looping": false
}
```

A **keyframe timeline** (Oct 2026; Godot's AnimationPlayer, Unity's
Timeline). Each track animates one field of one component on one entity:

- `target` - the entity's name, found under the timeline's entity first;
  empty for the timeline's own entity.
- `component` - the component's scene key: `transform`, `sprite`, `text`,
  `point_light`, `ambient_light`, or a package's.
- `field` - the field, dotted into lists and objects (`"position.0"`,
  `"color.3"`).

Numbers and same-length lists of numbers move between keys by the track's
`ease` (`linear`, `step`, `in`, `out`, `in_out`). Anything else - a texture
id, a flag, a string - holds until its next key. An integer field takes the
rounded value. A track writes only when its value changes, so a script may
move the field between keys.

An event key calls `on_<name>(entity)` on the timeline entity's own script
as the playhead passes it. A clip that does not loop calls
`on_timeline_end(entity)` when it finishes. `duration` 0 takes the last key.
`clip` empty plays the first clip by name.

```lua
Timeline.set(stage, "clip", "act2")    -- play another clip from its start
Timeline.set(stage, "playing", false)  -- pause (true to carry on)
Timeline.set(stage, "time", 3.5)       -- seek; the events skipped do not fire
Timeline.set(stage, "speed", 0.5)
Timeline.set(stage, "looping", true)

function on_thunder(entity) Camera.shake(7, 0.35, 18) end
function on_timeline_end(entity) end
```

A track writes through the component's own serde, so a component that keeps
unsaved runtime state (a cloth's points, a jiggle's swing) restarts that
state when a track writes it. Drive those with an event key and their own
setter. Test Lab scene 185; the showcase's Puppet Theatre.

---

## Ragdolls and breaking joints

```json
"ragdoll": { "active": false, "thickness": 12.0, "limit": 1.2, "density": 1.0,
             "friction": 0.7, "damping": 0.4 }
"joint":   { "target_entity": "Peg", "joint_type": { "Revolute": { ... } },
             "break_force": 5000.0, "break_torque": 0.0 }
```

A `ragdoll` sits on a skeleton's root (Oct 2026). While it is `active` every
`bone` under the root is a capsule in the physics world, `thickness` across,
pinned to its parent bone where it starts by a hinge that bends at most
`limit` radians either way from the angle it had when the skeleton went limp.
Physics poses the bones; the skin, a sprite on a hand and the hierarchy
follow. The skeleton's clips, IK and jiggle leave it alone while it is limp.
Switched off, the bodies go and the bones keep the pose they fell in until a
clip moves them. The parts of one ragdoll do not collide with each other.

A joint's `break_force` snaps it when the pull through its pin passes that
(Unity's `breakForce`), and `break_torque` when its **limit or motor** carries
more than that turn - a hinge held open on its stop. Both are in the world's
units: mass times units a second squared, so a body of mass `m` hanging still
pulls `m * 981`. A new joint is not judged for its first half second, while
it settles into its load. A broken joint is gone - the `joint` component with
it - and the entity's script hears `on_joint_break`.

```lua
Ragdoll.set(knight, "active", true)       -- go limp (false: stand as bones)
Physics.apply_impulse(spine, 300, 120)    -- a limp bone takes pushes and
local vx, vy = Physics.get_velocity(spine) -- answers like any body

function on_joint_break(entity)           -- on the entity that owns the joint
end
```

A kick in the same frame as going limp reaches the bone once its body is
built, as does a `set_velocity` on a prefab spawned that frame. An impulse
is mass times speed, and a bone at the default `density` weighs about a
thousand - give a ragdoll that has to be knocked about a small `density`.
Test Lab scene 183; the showcase's Storming the Gate.

---

## Wheel, target, motor and friction joints

Four more `joint_type`s (Oct 2026; Unity's `WheelJoint2D`, `TargetJoint2D`,
`RelativeJoint2D` and `FrictionJoint2D`). Their springs are a `frequency`
in Hz and a `damping_ratio` (0 bounces for ever, 1 settles without
overshooting), and do not depend on the masses they carry.

```json
"joint": { "target_entity": "Chassis", "joint_type": { "Wheel": {
    "anchor_b": [-85, -45], "axis": [0, 1], "frequency": 2.2, "damping_ratio": 0.45,
    "limits": [-30, 30], "motor_speed": 0, "motor_max_torque": 2500000 } } }
"joint": { "target_entity": "Cursor", "joint_type": { "Target": {
    "anchor": [0, 0], "frequency": 4, "damping_ratio": 0.7, "max_force": 80000 } } }
"joint": { "target_entity": "Hook", "joint_type": { "Motor": {
    "linear_offset": [0, -56], "angular_offset": 0, "max_force": 50000, "max_torque": 3000000 } } }
"joint": { "target_entity": "Ground", "joint_type": { "Friction": { "max_force": 400, "max_torque": 9000 } } }
```

- **Wheel** - this entity is the wheel, the target the chassis. The wheel
  slides along `axis` (in the chassis's frame, up by default) against the
  spring, within `limits`, and turns freely - or as its motor drives it.
  `motor_speed` has the revolute wheel's sign, positive rolling right, and
  `Physics.set_motor_speed` drives the rotation, never the suspension. A
  revolute wheel pins the axle rigidly, so a car on it takes every bump in
  its frame.
- **Target** - pulls this entity's `anchor` toward the entity named by
  `target_entity` every step (it needs no body - a cursor sprite will do),
  or toward the world point `target` when it names none. `max_force` caps
  the pull (0: none), so a heavy thing lags and a light one snaps. The mouse
  drag: on press, `Physics.add_joint(crate, "Cursor", json)`; on release,
  `Physics.remove_joint(crate)`.
- **Motor** - holds this entity at `linear_offset` in the target's frame
  and `angular_offset` from its angle, pulled there by a spring that never
  pushes harder than `max_force` or turns harder than `max_torque` (0: no
  limit) - a load under a hook, a turret on a hull.
- **Friction** - resists sliding and turning relative to the target, by at
  most `max_force` and `max_torque`: how a top-down game, with no floor to
  rub on, slows a puck - or how a hook's swing dies away.

Every field has a default, so `{ "Wheel": {} }` is a joint. Test Lab scene
212; the showcase's Crane Yard.

---

## Audio

```lua
local handle = Audio.play_sound("sfx/jump.wav", 1.0, 1.0)
Audio.set_sound_volume(handle, 0.5)
Audio.set_sound_pitch(handle, 1.5)            -- faster and higher
Audio.stop_sound(handle)

local engine = Audio.play_sound("sfx/engine.wav", 0.5, 1.0, 0, true)  -- (clip, volume, pitch, pan, loop)

Audio.play_music("music/level1.ogg", 0.7, true)  -- (clip, volume, looping)
Audio.pause_music()
Audio.resume_music()
Audio.stop_music()

Audio.set_music_volume(0.6)
Audio.set_master_volume(1.0)

local t = Audio.get_music_position()  -- seconds into the music (0 with none)
```

`Audio.get_music_position()` is the music's own playhead - the audio clock, not
the frame clock - so a beat or a note chart timed from it stays with the
song however the frames fall. It keeps its place while the music is paused,
wraps when a looping track loops, and stays at the end of one that has
finished. It is last frame's reading: the audio device reports in buffer
steps of a few milliseconds, so a rhythm game advances its own clock by
`dt` and eases it toward this rather than reading it raw
(a rhythm package does exactly that). A package reads the same
value as `jengine_core::audio::music_position()`.

`Audio.play_sound` (and `play_sound_at`) returns an id at once, though the
sound starts when the frame's commands are applied; the host records which
sound it became, so `stop_sound`, `set_sound_volume` and `set_sound_pitch`
reach it from the same frame on, in every runtime. (Until Oct 2026 the id
reached nothing anywhere.) A fifth argument `true` loops the sound until
`stop_sound(id)` - an engine note, a fire a script owns.

### Crossfades, layers and playlists

```lua
Audio.crossfade_music("music/boss.ogg", 2.0)            -- (clip, seconds, volume?, loop?)
Audio.crossfade_music("", 3.0)                          -- fade to silence

Audio.play_music_layers({ "music/pad.ogg", "music/drums.ogg", "music/lead.ogg" }, { 1, 0, 0 }, 1.0)
Audio.set_music_layer(2, 1.0, 1.5)                      -- stem 2 (from 1) to full over 1.5 s
local v = Audio.get_music_layer(2)                      -- where it is heading

Audio.set_playlist({ "music/a.ogg", "music/b.ogg" }, { crossfade = 2.5, shuffle = false, loop = true, volume = 1 })
Audio.next_track()                                      -- crossfade into the next one now
local now = Audio.get_music_clip()                      -- "music/b.ogg"; "" with no music

Audio.set_music_pitch(0.9)                              -- every stem, slower and lower
Audio.seek_music(30)                                    -- every stem to 30 s
```

Music is one **director** (Oct 2026), the same in every runtime. A
crossfade fades the old music out while the new fades in - `play_music` is
a crossfade of 0 seconds. **Stems** start in the same frame and loop
together, so author them as one length (whole bars at one tempo) and they
stay in time; `set_music_layer` fades one in or out, which is adaptive
music - more instruments as the action rises. A **playlist** plays its
tracks once each, crossfading into the next `crossfade` seconds before one
ends, and goes round (`loop`, the default) or fades out at its end;
`shuffle` reorders each round and never opens a round with the track that
closed the last. `pause_music` holds the music and any fade in progress.
Fades run on the wall clock, so a paused game still finishes the crossfade
into its pause music. `get_music_position` follows the first stem.

**Music and the sounds a script started outlive a scene**: they belong to
the audio backend, not the world. Call `Audio.stop_all()` before
`Scene.load` when the next scene should start in silence (both example
projects' `nav` modules do), or leave the music playing on purpose.
Test Lab scene 208; the showcase's Night Drive.

### How loud a sound plays

A sound plays at **its own volume × its bus's effective volume**. Its own
volume is what you passed to `Audio.play_sound` / `Audio.play_music`, or later to
`Audio.set_sound_volume` / `Audio.set_music_volume`. The bus's effective volume is its
`Audio.set_bus_volume` value times each parent's, and becomes 0 if any bus on
the chain is muted or another bus is soloed. Script sounds play on `sfx`, music
on `music`, and an `AudioSource` on its `bus_id` (default `sfx`; an unknown name
falls back to `sfx` with a warning rather than playing silently).

```lua
Audio.play_music("music/level1.ogg", 0.3, true)
Audio.set_bus_volume("master", 0.5)   -- music now plays at 0.15, and returns
Audio.set_bus_volume("master", 1.0)   -- to 0.3: its own volume is kept
Audio.set_bus_muted("music", true)
Audio.set_music_volume(0.8)                 -- stays silent until the bus is unmuted
```

Bus changes reach sounds that are already playing, in every runtime. Before
Sep 2026 native re-applied the bare bus volume (a 0.3 track jumped to full when
a slider moved), the editor and web applied the bus only when a sound started,
and `Audio.set_music_volume` ignored the bus everywhere.

`Audio.set_music_volume` is the music's own volume, now and for the music
that follows - crossfades, stems and playlists play under it. `Audio.set_master_volume` is the backend's output
gain, applied after the whole bus tree.

A looping `AudioSource` loops on its own sound. It used to be played as music,
which stopped whatever music was already playing.

---

## Positional audio

```json
"audio_source": { "clip_id": "sounds/fountain.wav", "looping": true, "auto_play": true,
                  "spatial": true, "min_distance": 140, "max_distance": 900, "pan_width": 500 },
"audio_listener": {}
```

A sound can be heard from where it is (Oct 2026). An `audio_source` with
`spatial` plays at full volume within `min_distance` of the listener, is
silent from `max_distance`, and fades between (`(1 - t)^2` of the way across -
quickly near the source, tailing off); it is panned by how far to the left or
right it is, all the way at `pan_width`. Both follow the source and the
listener every frame. The **listener** is the entity with an
`audio_listener` - the player, usually - or else the active camera.

| Function | |
|---|---|
| `Audio.play_sound_at(clip, x, y[, volume[, pitch]])` | a one-shot heard from `(x, y)`: its volume and pan worked out from the listener when it starts (falloff 120 / 1000 / 600). Returns a sound id, as `play_sound` does |

A looping `audio_source` - spatial or not - now **stops when its entity is
despawned** (before, a torch's crackle played on after the torch was gone),
and when a scene swap hands its entity id to another source. Engine Test Lab
scene 162; the showcase's Night Market.

### Walls and rooms

```json
"audio_source": { "clip_id": "sounds/lute.wav", "looping": true, "auto_play": true, "spatial": true,
                  "occlusion": 0.45, "muffle": 900 },
"audio_listener": { "room_reverb": "sfx", "room_reach": 1100, "room_mix": 0.55 }
```

A sound behind a wall is **quieter and duller** (Oct 2026). A spatial
`audio_source` with `occlusion` loses that share of what is left of its
volume to each wall between it and the listener, and with `muffle` is
low-passed to that many hertz behind one wall, each further wall halving the
cutoff. A wall is any non-sensor collider the line between them goes in and
out of (an open shape - a segment, a polyline - counts once), the source's
and the listener's own colliders left out; so a door is a collider moved out
of the way, and a doorway the line passes through is no wall. Worked out each
frame, with nothing to call.

An `audio_listener` with `room_reverb` naming a bus **hears the room it is
in**: sixteen rays cast round it against the colliders, the share that meet a
wall within `room_reach` saying how enclosed it is (the reverb's mix,
`room_mix` closed all round and squared, so a doorway lets most of it out)
and their mean length how big (the reverb's room). So a great hall rings
long, a closet short and the open not at all, with no zone authored per room;
it combines with `audio_zone`s on that bus as zones combine. Engine Test Lab
scene 243 (by ear); the showcase's Muffled Halls.

## Freeform lights

```json
"freeform_light": { "points": [[-6, 0], [6, 0], [80, -600], [-80, -600]], "color": [1.0, 0.7, 0.3], "intensity": 1.6, "falloff": 28 }
```

Light in the shape of a polygon (Oct 2026; Unity's Freeform Light 2D): full
inside `points` (local, turned and scaled with the entity - a concave outline
is fine) and fading smoothly over `falloff` world units outside it. A window's
pool on a floor, a stage beam swung from its lamp by turning its entity, a
glowing pond. It is added into the light buffer beside the lightmaps, and
`light_at` gives the same answer. It casts no shadows. `Lighting.set_color` /
`Lighting.set_intensity` reach it like any light. Engine Test Lab scene 168;
the showcase's Stage Night.

## Display and cursor

```lua
Display.set_mode("borderless")         -- "windowed", "borderless", "fullscreen"
Display.set_size(1920, 1080)           -- the window's size, or the fullscreen resolution
Display.set_vsync(false)
Display.set_max_fps(60)                -- 0: no cap; nil or -1: the project's settings.frame_rate_cap
for _, m in ipairs(Display.get_resolutions()) do print(m.w, m.h, m.hz) end
local w, h = Display.get_size()        -- what the window has now
if not Display.supports("vsync") then --[[ hide the row ]] end

Cursor.set_icon("pointer")             -- default, pointer, crosshair, text, move, wait, progress,
                                       -- help, not_allowed, grab, grabbing
Cursor.set_image("sprites/quill.png", 2, 30)   -- a texture, its hot spot in its pixels
Cursor.set_image("")                   -- the system cursor again
Cursor.set_visible(false)
Cursor.set_confined(true)              -- kept inside the window
Cursor.set_locked(true)                -- hidden and held; read Input.get_mouse_delta()
```

What an options menu changes (Oct 2026). The settings are the **process's**,
not the scene's: they hold across a scene change, so an options scene can
set them for the game that follows. A setter asks; the runtime applies the
change at the start of the next frame, and the getters answer what the
window **has** - so after `Display.set_mode("fullscreen")` the readout says
fullscreen a frame later, and in a browser that refused, windowed.

- `Display.supports(feature)` - `fullscreen`, `exclusive` (a resolution of
  its own), `window_size`, `vsync`, `max_fps`, `cursor_image`,
  `cursor_confine`, `cursor_lock` - says what this runtime can do. Natively everything on a
  desktop; on Android only vsync and the cap. In a browser: fullscreen
  (borderless; "fullscreen" falls back to it) and the cursor; a page cannot
  size its window, switch vsync or keep the pointer in. The editor's Play
  takes no cap.
- A page goes fullscreen only shortly after a click or key press, and
  leaves it when the player presses Esc; either way the mode reads windowed
  again, and asking again is a change. **The pointer lock is the same**: a
  page grants it only after a click and lets it go on Esc, and the lock is
  then off again - lock on a click (PLAY), not at load. Natively it is
  winit's `Locked` grab, or `Confined` on Windows, which has none; the
  motion is raw either way, so `Input.get_mouse_delta` counts at the edge.
- `Display.get_resolutions()` lists each size once, at its highest rate,
  largest first (in a browser, the screen's size alone).
- A cursor picture must be a texture the runtime can load; it is applied once
  it has arrived. Store the choices with `Save.set` and apply them in
  `init` - the runtime does not remember them for you.

Engine Test Lab scene 204; the showcase's Options Menu.

## Streaming scenes

```lua
local hall = Scene.load_additive("scenes/corridor/hall_3.scene", 3840, 0)   -- a chunk handle, at once
if Scene.status(hall) == "loaded" then ... end          -- "loading", "loaded", "failed", "unloaded"
local p = Scene.progress(hall)                          -- 0..1
Scene.unload(hall)                                      -- its entities go (and anything parented to them)

Scene.preload("scenes/boss.scene")                      -- the assets, in the background
if Scene.status("scenes/boss.scene") == "loaded" then Scene.load("scenes/boss.scene") end
```

A scene can be **added** to the one running (Oct 2026): its entities come in
offset by `(x, y)` once everything they draw has loaded - native decodes the
textures off the main thread, the browsers fetch them - so a long level can
stream the next room in ahead of the player and drop the one behind, and the
frame keeps drawing while it loads. Its scripts start as any new entity's do
(`init`, then `update`). `Scene.preload(path)` loads a scene's assets
without showing it, so a later `Scene.load` is a swap rather than a hitch;
`Scene.status` and `Scene.progress` take a chunk's handle or a preload's path.

- A chunk's root entities are offset; a child keeps its place in its parent.
- Loading a whole new scene (`Scene.load`, `push`, `pop`) takes every chunk
  with the old one; preloads stay.
- A chunk unloaded while it was still loading is simply never laid in.
- The chunk scene's own top-level settings (post effects, sort layers) are
  the running scene's, not its own.

Engine Test Lab scene 206; the showcase's The Long Corridor.

## Asset packs

```lua
if Packs.status("forest") ~= "installed" then
    Packs.download("forest")                       -- a pack already here or on its way is left alone
end
local s = Packs.status("forest")                   -- "remote", "downloading", "installed", "outdated", "failed", "unknown"
local p = Packs.progress("forest")                 -- 0..1, 1 once installed
local mb = Packs.size("forest") / 1048576          -- what the download costs
if s == "failed" then print(Packs.error("forest")) end   -- asking again retries
if s == "installed" then Scene.load("scenes/forest/glade.scene") end

Packs.of("sprites/forest/tree.png")                -- "forest": the pack an asset is in ("" if in the game)
Packs.list()                                       -- every pack's name
Packs.remove("forest")                             -- delete the local copy; remote again
Packs.set_url("https://mirror.example.com/game/")  -- download from somewhere else from now on
```

Parts of a game can stay out of the build and be **downloaded when they are
needed** (Oct 2026): a later world, its music, a video. A project names its
packs in `settings.asset_packs` (Project Settings -> Asset Pipeline -> Asset
Packs), each by the asset paths it takes:

```json
"asset_packs": {
  "url": "https://cdn.example.com/my-game/",
  "packs": [{ "name": "forest", "include": ["scenes/forest/", "sprites/forest/", "music/forest.ogg"] }]
}
```

A build moves those files out of the game into one archive per pack
(`packs/forest-<hash>.jpak`) and lists them in `assets/packs.json`; upload the
`packs/` folder to `url`. With no `url`, packs are fetched from `packs/`
beside the game (or the page), which is where a Windows, Linux or web build
writes them - so a fresh build downloads from itself. Once a pack is
installed its files load exactly as the game's own do, so wait for
`installed` before loading a scene from it.

- Each download is checked against the size and SHA-256 the build recorded.
  Natively it is unpacked into a cache beside the save folder (`pack_cache`,
  or `JENGINE_PACK_DIR`); in a browser it is kept in the Cache API. Either
  way the next start of the same build finds it installed, and a new build's
  changed pack is a new file that downloads again.
- **In the editor, and a run from the project folder, every pack is local**:
  its files are on disk, so it is `installed` from the first frame,
  `Packs.download` has nothing to fetch and `Packs.of` answers `""`. Try the
  download itself in a build.
- **A pack can carry code and data**: scripts (and the modules they
  `require`), prefabs, data tables, skeletal clips and rigs. What a runtime
  loads once at startup - prefabs, tables, clips, and in a browser every
  module `require` can reach - gains the pack's when it is installed, so
  `Entity.spawn_prefab`, `Data.get` and `require` see them from the next
  frame; a script on an entity is read when the entity first appears. A
  prefab in a pack may extend one in the game. What a removed pack added
  stays known until the game restarts (its textures do not).
- **A script in a pack is as trusted as the build**: the download is checked
  against the SHA-256 in the build's own `packs.json`, so a server - or
  `Packs.set_url` - cannot hand the game other code. On iOS and Google Play,
  downloading code a script interpreter runs is generally allowed if it does
  not change what the app is; check the store's rules before a mobile game
  depends on it.
- Input maps and the entry scene always stay in the game - they are read
  before a pack could be there - and the build log says which files a pack
  claimed and left. A pack's sprites are not atlas-packed.
- A texture a scene asked for while its pack was remote is asked for again
  once the pack is installed, so it appears rather than staying blank.
- A macOS, iOS or Android build cannot download from inside its own app, so
  it needs a `url`; its `packs/` folder is written beside the app.
- In a browser the Cache API and `crypto.subtle` need a secure context
  (`https`, `localhost`); elsewhere a pack is downloaded unverified and lasts
  the session.
- **A live ops release changes the packs without a new build** (see Live ops
  below): a pack it adds is `remote`; a pack it changes that is installed is
  **`outdated`** - the old copy still loads, and `Packs.download` fetches the
  new one (a failed update leaves it `outdated`, still loading). A release's
  archives are fetched from the service, the build's from its own `url`.

Engine Test Lab scene 225.

## Live ops

```lua
local coins = LiveOps.get("coins_per_win", 10)    -- the service's value for this player, else the game's default, else 10
local layout = LiveOps.get("shop_layout")          -- any JSON: a number, a string, a table
if LiveOps.variant("shop_test") == "list" then end -- the A/B variant this install is in ("" if none)
if LiveOps.event_active("double_coins") then
    local left = LiveOps.event_ends("double_coins")    -- seconds until it ends
    local name = LiveOps.event_name("double_coins")    -- what the service calls it
end
local soon = LiveOps.event_starts("winter")        -- seconds until it starts (0 once it has)
for _, id in ipairs(LiveOps.events()) do end       -- every event on now
LiveOps.status()     -- "off", "defaults", "cached" or "live": where the values came from
LiveOps.error()      -- why the last answer was refused or failed, or ""
LiveOps.fetch()      -- ask again now (it asks by itself: at start, every few minutes, when an event starts or ends)
LiveOps.fetching()   -- whether an answer is on its way
LiveOps.set_player("p_42")   -- who is signed in, so rules naming players reach them (asks again)
LiveOps.server_time()        -- the service's clock in Unix seconds - for timers a changed device clock cannot cheat
LiveOps.keys()               -- every key there is a value for
LiveOps.revision()           -- the snapshot's revision; LiveOps.release() the content release it names
LiveOps.updated()            -- goes up whenever the values may have changed: compare it to redraw a shop
LiveOps.set_attribute("store", "steam")       -- something of this game rules can target ("" takes it away; asks again)
LiveOps.expose("shop_test")                   -- this install saw its variant: counted in the experiment's results
LiveOps.expose("shop_test", "purchase", 4.99) -- it did something worth counting there, the value summed
```

**Live ops** (Oct 2026) is what a shipped game is told without a new build:
**remote config** worked out for this player, the **A/B experiment** variants
it is in, the **live events** on now, and the **content release** - which
asset packs are current. A project points at a live ops service in
`settings.liveops` (Project Settings -> Live Ops); the service is
`jengine-server --liveops <dir>` (or `Server::liveops` in a game's own
server), changed on its dashboard at `/admin/liveops`.

```json
"liveops": {
  "url": "https://ops.example.com",
  "public_key": "jlo-public-...",
  "channel": "live",
  "editor_channel": "dev",
  "defaults": { "coins_per_win": 10, "shop_layout": "grid" },
  "region": "",
  "attributes": { "store": "steam" }
}
```

- **The defaults ship with the game**: every value is there from the first
  frame - before the service answers, offline, with live ops off - and a
  script reads it the same way whatever answered.
- **An answer is believed only if it is signed** by the key the game was
  built with (`public_key`; the dashboard and `/liveops/v1/key` show it). A
  content release can carry scripts, so neither the address nor the
  transport vouches for one. One older than the answer the game has, or made
  for another install or channel, is refused too - `LiveOps.error()` says why.
- **The last answer is kept** (natively beside the saves, in a browser in
  `localStorage`) and believed again at the next start - checked again - so
  `status` is `cached` until the service answers.
- **Events switch on and off on time** from their windows, by the service's
  clock, even between asks; the game asks again when one starts or ends,
  which is when the values it carries change.
- **On the service, a value is decided in this order, the later winning**:
  the parameter's value, its first matching rule (platform, app version -
  `settings.app.version` - player or install, a percentage rollout, a time
  window), the experiment variant, then the live events on now. An install's
  rollout bucket and variant come from a hash of its id, so they stay put.
- **What the game says of itself**: every ask carries the player's language
  (`Locale.current()` - a change asks again), the region (the system locale's,
  `FI` of `fi-FI`, unless `region` names one) and the game's attributes
  (`attributes` in the settings, then `LiveOps.set_attribute` over them, all
  sent as text). A rule's `languages` (`ja` matches `ja-JP`), `regions` and
  `attributes` match against them.
- **Experiments are counted**: `LiveOps.expose(experiment)` says this install
  saw its variant, `LiveOps.expose(experiment, event, value)` that it did
  something worth counting (`value` summed). The service counts each for the
  variant **it** gives this install, read on its dashboard (`results`). It is
  sent by the same per-frame sync as the asks - natively through the
  runtime's HTTP client, in a browser by `fetch` - and answers `false` (and
  sends nothing) when live ops are off or no experiment is named. An
  experiment the service is not running refuses it (logged).
- **Content**: `jengine liveops publish <build> --server <url> --channel <c>`
  uploads a build's packs and publishes them as the channel's next release;
  games lay it over their own packs (see Asset packs). A rollback publishes
  an older release again, as a new one.
- **The editor's Play** asks for `editor_channel` (`dev`) as platform
  `editor`, and does not apply a content release - its packs are the
  project's own files.

Engine Test Lab scene 226.

## Rumble and vibration

| Function | |
|---|---|
| `Input.rumble(pad, strong, weak, seconds)` | shake gamepad `pad` (0 is `Pad0`): the strong (low, heavy) and weak (high, buzzy) motors, each 0..1, for `seconds` (at most 5). `0` strength or seconds stops it; a new rumble on a pad replaces the last |
| `Input.vibrate(seconds[, strength])` | buzz the device itself - a phone - at `strength` 0..1 (default 1) |

Oct 2026 (Godot's `start_joy_vibration` / `vibrate_handheld`). A rumble plays
through gilrs' force feedback natively, the Gamepad API's `dual-rumble` in
the browsers (Chrome and Edge; Firefox has no actuator) and the controller's
own vibrator on Android, where one motor stands for both. `vibrate` is
`navigator.vibrate` in a phone's browser (Android; iOS has none) and the
system vibrator in an Android build, which the builder gives the `VIBRATE`
permission; a desktop has nothing to buzz and ignores it. Both are dropped
quietly where the hardware cannot do them. A steady rumble is asked for again
every frame for a little longer, so it stops by itself a moment after the
script stops asking. Engine Test Lab scene 167; the showcase's The Forge.

## Platform (web portals)

| Function | |
|---|---|
| `Platform.name()` | the portal the web build was made for: `"jcabinet"`, `"poki"`, `"crazygames"`, `"youtube"`, or `"none"` |
| `Platform.game_ready()` | the game can be played now - only when the project's `web_game_ready` is `"script"`; otherwise the first frame said it |
| `Platform.gameplay_start()` / `Platform.gameplay_stop()` | play started (a level, a resume) / stopped (menu, pause, level end, game over) |
| `Platform.happy_time()` | something great just happened |
| `Platform.submit_score(value[, board])` | an integer score: YouTube's best score, or a jCabinet leaderboard named `board` |
| `Platform.show_ad(kind[, reward])` | `"midgame"` or `"rewarded"` (`reward`: YouTube's id for this kind of reward); answered by the `platform_ad_finished` event, `{ kind, shown, rewarded }` |

Events, heard with `Event.on(name, entity)`: `platform_ad_finished`, and
`platform_pause` / `platform_resume` when the portal pauses the game (YouTube
Playables) - the frame the pause arrives on still runs, so a game can save.
While an ad plays or the portal holds the game, nothing runs and nothing
sounds; scripts need not pause or mute anything.

Oct 2026. The calls reach a portal only in a web build made for one (Project
Settings -> Web, the Build panel's Portal, `jengine web-build --platform`).
Natively, in the editor and in a plain web build they do nothing and `show_ad`
answers at once, not shown - the same flow, testable anywhere. All four
runtimes install them. See [`docs/web-platforms.md`](web-platforms.md) for what
each portal hears. Engine Test Lab scene 295.

## Audio effects

```json
"audio_zone": { "size": [420, 260], "bus": "sfx", "reverb": 0.75, "room": 0.95, "fade": 90 }
"audio_zone": { "size": [1500, 400], "bus": "master", "lowpass": 420, "fade": 25 }
```

Every mixer bus has a **low-pass filter** and a **reverb** (Oct 2026;
Godot's bus effects, Unity's mixer effects and Reverb Zones). The buses are
real tracks in both backends - kira sub-tracks natively, a Web Audio filter /
convolver chain in the browsers - nested as the buses are, so an effect on
`master` reaches every sound, and an effect on `sfx` leaves the music alone.

**A project can author its buses** (Oct 2026): `settings.audio_mixer` in
`project.jengine` - the editor's Mixer panel - lists the buses a game starts
with, each with a `parent`, `volume`, `muted` and `effects` (the same names
as `set_bus_effect`), parents first. Every runtime applies it before any
script runs, so `Audio.create_bus` and `Audio.set_bus_effect` are for what
changes during play, not for setting up the mix.

```json
"audio_mixer": { "buses": [
  { "name": "music", "volume": 0.6 },
  { "name": "ambience", "parent": "sfx", "volume": 0.5, "effects": { "reverb": 0.4, "room": 0.8 } }
] }
```

| Function | |
|---|---|
| `Audio.set_bus_effect(bus, effect, value)` | `"lowpass"` (cutoff in hertz; `0` is off - a muffle at a few hundred), `"reverb"` (how much is mixed in, 0..1) or `"room"` (0..1, a closet to a cathedral); `"echo"` (how much echo is heard, 0..1), `"echo_time"` (seconds between echoes, up to 2) and `"echo_feedback"` (how much of each comes back, up to 0.95); `"distortion"` (0..1, a crackle that keeps full scale); `"compress"` (0..1: a threshold falling to -30 dB, a ratio rising to 8:1, half made up); `"eq_low"` / `"eq_mid"` / `"eq_high"` (decibels, -24..24: a shelf at 250 Hz, a bell at 1 kHz, a shelf at 4 kHz) |
| `Audio.set_variation(clip, pitch, volume)` | every one-shot of `clip` plays up to `pitch` semitones (0..12) and up to `volume` (a fraction, 0..1) either way of what was asked - footsteps that never sound the same twice. Both 0 removes it. Wherever the clip is played: a script, an `audio_source`, an animation frame. Music and looping sounds are left alone |

The effects run in the order EQ, distortion, low-pass, compressor, echo,
reverb (Oct 2026 for the first four). A radio is a bus with its lows and
highs cut, some distortion and compression:

```lua
Audio.set_bus_effect("music", "eq_low", -20)
Audio.set_bus_effect("music", "eq_high", -16)
Audio.set_bus_effect("music", "distortion", 0.45)
Audio.set_bus_effect("music", "compress", 0.75)
Audio.set_variation("sounds/step.wav", 3, 0.35)
```

An **`audio_zone`** is a box (`size`, turned and scaled with its entity) that
gives `bus` its `lowpass`, `reverb`, `room` and `echo` (with `echo_time` and
`echo_feedback`) while the listener - the
`audio_listener`, or the camera - is inside, coming in over `fade` world
units inside its edge (the cutoff slides on a log scale, as a muffle is
heard). A zone's effects and a script's combine: the lower cutoff, the wetter
reverb and echo, the stronger distortion and compression, the EQs added.
Engine Test Lab scenes 165 and 198; the showcase's Sea Cove and Echo Canyon.

## Spectrum and synthesis

```lua
local bands = Audio.get_spectrum(16)   -- 16 numbers 0..1, low to high
local loud = Audio.get_level()         -- 0..1, the mix's loudness now
Audio.synth({ wave = "saw", note = 60, duration = 0.25, attack = 0.005, decay = 0.1,
              sustain = 0.5, release = 0.3, volume = 0.4, bus = "sfx" })
```

What the speakers are playing, as numbers, and notes made from numbers
(Oct 2026; Godot's spectrum analyser and audio generator, Unity's
`GetSpectrumData`):

- **`Audio.get_spectrum(n)`** - `n` bands (16 by default, at most 256) of
  the whole mix, music and sounds together, spaced evenly on a log scale
  from 40 Hz to 16 kHz, each the loudest frequency in it as -70..0 dB mapped
  to 0..1. They rise at once and fall back by at most five a second, so a
  beat shows and nothing flickers. A pulse on the beat: watch the lowest
  few bands jump above their own recent average.
- **`Audio.get_level()`** - the mix's loudness, the RMS of its last 1,024
  samples.
- **`Audio.synth(opts)`** - plays a note, rendered from numbers: a `wave`
  (`sine`, `square`, `saw`, `triangle`, `noise`) at `freq` Hz or MIDI
  `note` (69 is A4), sliding to `slide` Hz if given, under an envelope that
  rises over `attack`, falls to `sustain` over `decay`, holds until
  `duration` and fades over `release` (seconds), at `volume`, on `bus`
  (`sfx` by default). Each distinct note becomes a clip the first time it
  plays and is reused after. Ten seconds at most.

Every runtime feeds one ring of samples - natively a tap on the master
track, in the browsers an `AnalyserNode` - and one analysis
(`audio::analysis`) reads it, so a band means the same everywhere. Test
Lab scene 215; the showcase's Equaliser.

## Screenshots

```lua
local id = Screenshot.take({ texture = "photos/1", file = "overlook_01", scale = 0.25 })
-- ...a frame or two later:
if Screenshot.ready(id) then UI.set_visible(gallery_photo, true) end
```

`Screenshot.take` (Oct 2026; Godot's `get_viewport().get_texture().get_image()`,
Unity's `ScreenCapture`) copies the **next frame as it is presented** -
world, post effects and UI, exactly what the player sees - and returns an
id:

- `texture` - written into that texture id, so any sprite or `ui_image`
  naming it shows the photo (a gallery, a save slot's thumbnail). Name it in
  the scene beforehand: it shows white until the photo arrives.
- `file` - natively a PNG under `<JENGINE_SAVE_DIR or saves>/screenshots/`
  (`.png` added; no `..` or drive letters); in a browser, offered as a
  download.
- `scale` - shrink it, 0.05..1 (a box filter): 0.25 for a thumbnail.

`Screenshot.ready(id)` turns true once it is written - the GPU copy is read
back a frame or two later. To keep a viewfinder, a HUD or the gallery out of
the photo, hide them in the frame you call `take`. Test Lab scene 216; the
showcase's Photo Mode.

## Behaviour trees

A `behavior_tree` (Oct 2026; `ecs::behavior_tree`) decides, every frame,
what an entity should be doing now - patrol, chase, search, flee - where a
behaviour graph (below) answers events. The tree is authored in the scene:

```json
"behavior_tree": { "root": { "type": "selector", "reactive": true, "children": [
  { "type": "sequence", "children": [
    { "type": "near", "target": "Thief", "distance": 180 },
    { "type": "move_to", "label": "chase!", "target": "Thief", "speed": 150, "arrive": 46 },
    { "type": "call", "call": "catch" } ] },
  { "type": "sequence", "children": [
    { "type": "check", "key": "noise", "op": "set" },
    { "type": "move_to", "target": "@noise", "speed": 120 },
    { "type": "wait", "label": "search", "seconds": 2 },
    { "type": "set", "key": "noise", "value": false } ] },
  { "type": "sequence", "children": [
    { "type": "move_to", "label": "patrol", "target": [-480, 40] },
    { "type": "wait", "seconds": 1, "jitter": 1 },
    { "type": "move_to", "label": "patrol", "target": [120, 120] } ] }
] } }
```

Every node answers success, failure or running; a running leaf carries on
next frame. Any node takes a `label` (what the readout shows).

| Node | Does |
|---|---|
| `sequence` | Each child in turn while they succeed; fails with the first that fails. |
| `selector` | Each child in turn while they fail; succeeds with the first that succeeds. |
| `parallel` | Every child at once; succeeds when `succeed` of them have (0, all). |
| `random` | One child, picked at random each time it starts. |
| `invert` / `succeed` | Swaps the child's answer / always succeeds. |
| `repeat` | The child again, at most once a frame: `times` (0, forever), or `until_failure`. |
| `cooldown` | Fails without running the child for `seconds` after it last finished. |
| `timeout` | Aborts the child and fails past `seconds`. |
| `check` | A blackboard `key` against `value` with `op`: `==` `!=` `<` `<=` `>` `>=` `set` `unset`. |
| `near` | The target within `distance`; `store` keeps its position. |
| `chance` | Succeeds with probability `p`. |
| `wait` | `seconds` (plus up to `jitter`). |
| `move_to` | To within `arrive` of the target at `speed` (`away`: until that far from it). |
| `set` | A blackboard `key` to `value` (`add` to add). |
| `write` | Any component field (`component`, dotted `field`) on itself or `target`. |
| `event` | Calls `on_<event>` on the entity's script. |
| `call` | Calls `on_<call>` and runs until the script answers (below). |

A **target** is an entity's name, the nearest with a tag (`"tag:guard"`), a
blackboard key (`"@noise"`: a point, a name or an entity id) or a point
`[x, y]`. `move_to` steers the entity's `nav_agent` when it has one - so it
paths round walls - else moves it straight, by its `velocity` when it has one
(so a `face_velocity` turns it). Between two `nav_agent`s, avoidance keeps
them their radii apart: `arrive` must be more than both together, or the
chase never ends.

A **`reactive`** sequence or selector looks again from its first child every
frame, so a higher-priority branch cuts in: the guard above drops its patrol
mid-step when the thief comes near. The branch it leaves is **aborted** - a
`move_to` stops its agent, a `call`'s key reads `"aborted"`.

The **blackboard** is the entity's `Blackboard` custom component - read and
written with `Entity.get_custom` / `set_custom` like any other - and the tree
keeps `_running` there: the labels of the leaves running now.

```lua
-- the tree's `call` leaf: do the work, then say how it went
function on_catch(entity)
    Transform.set_position(thief, home_x, home_y)
    Entity.set_custom(entity, "Blackboard", "catch", "success")   -- or "failure"
end

Entity.set_custom(guard, "Blackboard", "noise", { x, y })   -- send it to look
BehaviorTree.set(guard, "enabled", false)                     -- pause it
BehaviorTree.set(guard, "restart", true)                      -- from the root again
BehaviorTree.set(guard, "root", { type = "wait", seconds = 5 })   -- a new tree
```

`BehaviorTree.set(e, field, value)` takes `root`, `enabled`, `loop` (a tree
that does not loop stops when its root finishes and calls `on_tree_end`) and
`restart`. It runs inside the motion step in every runtime. Test Lab scene
219; the showcase's Night Watch.

## Navigation

```json
"nav_region": { "outline": [[-600, -340], [600, -340], [600, 340], [-600, 340]], "agent_radius": 13 },
"nav_agent": { "speed": 80, "radius": 12, "arrive_distance": 4, "avoidance": true }
```

Paths off the grid (Oct 2026; Godot's `NavigationRegion2D` /
`NavigationAgent2D`). A `nav_region` is the walkable area inside its
`outline` (local points, turned and moved with its entity) less every
non-sensor `collider` in it - not an agent's, and not a dynamic body's
unless it is a `nav_obstacle` - in `obstacle_mask` (against the collider's
`collision_membership`), and less the solid layers of any colliding
tilemap. Each obstacle is grown by `agent_radius` and the outline shrunk by
it, so a path keeps an agent's whole body clear; the result is baked into a
visibility graph over the grown corners - exact shortest paths - and
rebaked when a collider in it moves. A concave collider counts as its hull.
F4 draws the grown shapes and each walking agent's path.

**Tilemap walls** (Oct 2026) are baked as the physics builds them - whole
cells merged into runs, shaped tiles as their outlines - from every layer a
`tilemap_collider` names `solid` or that is flagged `collision`; a
`set_tile` that digs a wall out or builds one rebakes the region, and every
agent replans. (Before, a tilemap's walls were air to a region.)

**Moving obstacles** (Oct 2026; Unity's `NavMeshObstacle` with carving):

```json
"nav_obstacle": { "snap": 16 }
```

A dynamic body with a `nav_obstacle` is baked in like a static collider,
keyed by its position snapped to `snap` units (and its turn to a twentieth
of a half turn), so its region rebakes when it has really moved - not every
frame it jitters - and agents whose path went through where it now stands
go round. Mind where they rest: a crate whose grown outline meets a wall's
seals the gap between them, and an agent left in a sealed pocket has
nowhere to go.

A `nav_agent` given a target walks there at `speed` through the region it
stands in: it sidesteps an agent standing, crossing or coming the other way
(both to their right, so two meeting head on pass), queues behind one going
its way, is pushed apart from any it overlaps, replans when pushed off its
line, and - sent with a crowd to one place - stops at the crowd's edge. It
moves its `transform` (turned by a `face_velocity`, if it has one), so give
it no physics body that would fight it. An unreachable target stops it.

| Function | |
|---|---|
| `Nav.set_target(e, x, y)` | send the agent walking to `(x, y)` - snapped to the nearest place it can stand, so a click on a fountain walks to its rim |
| `Nav.stop(e)` | stop it where it is |
| `Nav.is_moving(e)` | whether it still has somewhere to walk to - `false` once it arrives, is stopped or finds no way. A target set this frame is seen next frame |
| `Nav.set(e, field, value)` | any `nav_agent` field by name (`speed`, `radius`, `avoidance`, ...), or the region's (`agent_radius`, `obstacle_mask`, `outline`) |
| `Nav.find_path(x0, y0, x1, y1)` | the shortest walkable way, `{ {x, y}, ... }` with both ends (each snapped), through the region `(x0, y0)` is in; `nil` when there is none |
| `Nav.closest_point(x, y)` | the nearest point an agent can stand on: `x, y`, or `nil` with no region |

Engine Test Lab scene 163; the showcase's Garden Party.

## Localization

```json
// assets/locale/ui.data - a data table: a row per key, a column per language
{ "schema": "Strings", "version": 1, "rows": [
  { "id": "_name",       "en": "English",          "pl": "Polski" },
  { "id": "_decimal",    "en": ".",                "pl": "," },
  { "id": "title",       "en": "The Polyglot Inn", "pl": "Wielojęzyczna Gospoda" },
  { "id": "rooms#one",   "en": "{n} room free",    "pl": "{n} wolny pokój" },
  { "id": "rooms#few",   "en": "",                 "pl": "{n} wolne pokoje" },
  { "id": "rooms#many",  "en": "",                 "pl": "{n} wolnych pokoi" },
  { "id": "rooms#other", "en": "{n} rooms free",   "pl": "{n} wolnego pokoju" }
]}
// on an entity
"text": { "text": "The Polyglot Inn", "text_key": "title" }
```

Text in the player's language (Oct 2026; Godot's `TranslationServer`, Unity's
string tables). **A string table is a data table whose id starts with
`locale/`** - `assets/locale/*.data` with its `.dataschema`, a field per
language code - so it loads wherever data tables load and is edited in the
editor's table editor. `_name` is a language's own name and `_decimal` its
decimal separator. A `text` or `ui_text` with a `text_key` is written in the
current language and again whenever it changes.

A lookup tries the current language, its base (`pt` for `pt-BR`), English,
then the first code alphabetically; a key no table has comes back as itself, so it
shows on screen. With an `n` (or `count`) argument, the plural form the
language's rule picks is tried first - `key#one`, `key#few`, `key#many`,
`key#other` (CLDR's rules for the common languages; English's for the rest).
`{name}` is replaced by an argument, `{price:2}` with two decimals, numbers
with the language's `_decimal`; `{{` and `}}` are braces.

The language follows the system's (the browser's, or Windows') until a
script chooses one, and stays chosen across scene loads.

| Function | |
|---|---|
| `Locale.get(key[, args])` | `key` in the current language, e.g. `Locale.get("rooms", { n = 5 })` |
| `Locale.set(code)` | choose the language (`""` follows the system again) |
| `Locale.current()` | the language in use, as a code |
| `Locale.list()` | the language codes the tables carry, alphabetically (a menu wanting its own order keeps its own list) |
| `Locale.name(code)` | that language's `_name` (for a language menu), or its code |

A `.gui` document's text has no key: set it with `GUI.set(doc, node, "text",
Locale.get(...))`. The built-in font covers Latin scripts; a game in Greek,
Cyrillic or CJK ships its own font. Engine Test Lab scene 164; the
showcase's The Polyglot Inn.

## Physics

```lua
Physics.apply_force(entity, 0, -200)        -- pushes for this frame; call every frame it should act
Physics.apply_impulse(entity, 10, 0)        -- instantaneous velocity change
Physics.set_velocity(entity, 0, 5)
Physics.set_angular_velocity(entity, 0.5)

local vx, vy = Physics.get_velocity(entity)
local omega  = Physics.get_angular_velocity(entity)

-- Contact iteration (1-based index, Lua-style)
for i = 1, Physics.contact_count() do
    local a, b, started = Physics.get_contact(i)
    -- started == true on contact begin, false on end
end
```

### Raycast

```lua
local hit = Physics.raycast(ox, oy, dx, dy, max_dist)
if hit ~= nil then
    -- hit.entity_id            which entity was hit
    -- hit.x, hit.y             where, in world units
    -- hit.normal_x, .normal_y  the surface normal there
    -- hit.distance             how far along the ray, in world units
end
```

`nil` (native) / `null` (browser) means nothing was hit within `max_dist`.

The direction does **not** have to be normalised - it is normalised for you,
so `max_dist` and `hit.distance` are in world units whatever length you
pass. This matters because rapier measures time of impact in multiples of
the direction's length, so a version that skipped it would report a hit at
400 units as a distance of 40.

Sensors are hit, deliberately: a script asking what is along a line wants
to know about a trigger volume, and rapier's own default filter includes
them.

**An entity a script spawned is named by its handle** (Oct 2026) - in a hit's
`entity_id`, in `Physics.get_contacts()` rows, and for `Physics.get_velocity`
and `get_angular_velocity` - so `hit.entity_id == bomb` works for a `bomb`
from `Entity.spawn_prefab`. They used to report the real id, which a script
never sees for a spawned entity. In the editor and web builds a ray also
sees tilemap colliders and carved land now, as a native one always did.

Works in all four runtimes since Sep 2026. It was the last function that
did not - unbound on native, and bound-but-inert in the editor and on web,
where it read a host hook nobody installed and returned `null` in silence.
No `if raycast == nil` guard is needed any more.

### World and body settings

```lua
Physics.set_gravity(gx, gy)                 -- both components required
Physics.set_body_type(entity, "static")     -- "dynamic" | "static" | "kinematic"
```

`Physics.set_body_type` matches its argument **case-insensitively**, so `"Static"` and
`"STATIC"` mean the same as `"static"`. An unrecognised name logs a warning and
falls back to `"dynamic"`. The three runtimes used to parse this string
separately and disagreed about capitalisation — a script verified in the editor
with `"Static"` shipped with a wall that fell over — so they share one parser
now (BUGS #41).

`set_body_type` changes the entity's `RigidBody2D` as well as its body, in every
runtime (`ecs::systems::set_entity_body_type`). The engine makes each body match
its component every frame, so an inspector edit reaches the simulation - and
until Sep 2026 that put a script's change straight back: a kinematic ball
switched to `"dynamic"` hung where it was. Test Lab scene 138.

`Physics.set_gravity` wants **both** components. Native raises a script error if you
omit one; the browser runtimes cannot, so they treat a missing component as
`0.0`. Do not rely on that: `Physics.set_gravity(0)` used to mean two different things
in the editor and in a shipped build (BUGS #46).

Physics commands address bodies by entity, including entities a script created
with `Entity.spawn` / `Entity.spawn_prefab` — see BUGS #43 and #45 for the period when
the editor got that wrong.

### Things that break (no script needed)

A `breakable` body comes apart when it is hit hard enough (Sep 2026). What is
measured is the **jolt**: how much the body's velocity changed across a new
contact, within a short window - not the contact impulse, which for a block in
a tower is dominated by the weight resting on it.

```json
{
  "breakable": {
    "break_speed": 320.0,
    "cols": 2, "rows": 1,
    "generations": 0,
    "scatter": 25.0,
    "piece_lifetime": 0.0,
    "debris": "Dust", "debris_count": 14
  }
}
```

- It breaks into `cols` x `rows` pieces, each cut from the body's own sprite
  (its `uv_rect`, so a normal map follows), with a copy of its material, rigid
  body and occluder, sized to the piece. Each piece carries on with the
  velocity its part had, plus the spin and `scatter` (units/s outward).
- A `grid` (the default `pattern`) breaks only a `Box` collider. A piece is
  breakable again only while `generations` is above zero (one less each
  time), and despawns after `piece_lifetime` seconds when that is set.
- `"pattern": "voronoi"` (Oct 2026) breaks any convex collider - a box, a
  triangle, a `ConvexPolygon` - into `pieces` shards: the Voronoi cells of
  seeds crowded round the point it was struck (`focus`, 0 even to 1 all round
  the impact), so glass crazes small where the stone hit and splits in long
  shards away from it. Each shard is a convex-polygon body drawn as a
  textured `mesh` cut from the same picture (a sprite, or a `mesh` the body
  already was), with the material and occluder, and remembers where it sat
  on the picture so it can break again. The same blow in the same place
  always breaks the same way, in every runtime.
- A piece is always **loose** (dynamic), whatever held the whole. A body
  that cannot move - a pane in its frame, `Static` or `Kinematic` - never
  jolts, so it breaks on what struck it instead: the striker's change in
  velocity, its impulse over its mass, against the same `break_speed`. Give
  the pane collision bits that leave out its own kind, or shards falling
  past a lower pane break it too.
- `debris` names a particle emitter by entity name; it bursts `debris_count`
  particles where the body broke.
- Every runtime breaks things right after its physics step
  (`ecs::fracture::break_on_impact`); a fast piece wants `ccd` like any
  fast body. Test Lab scenes 138 and 254 (Voronoi), the showcase's Collapse
  and The Glass House.

### Trigger areas (no script needed)

`TriggerArea2D` is a lightweight, non-physical overlap test that runs
each frame after the motion systems and before `update_lifetimes`. It
is **not** part of the physics solver — it does not produce contacts,
forces, or collision response.

```json
{
  "trigger_area": {
    "shape": { "kind": "aabb", "value": { "half_extents": [16, 16] } },
    "match_tags": ["pickup"]
  }
}
```

- `shape` is one of `{ "kind": "aabb", "value": { "half_extents": [hx, hy] } }`
  or `{ "kind": "circle", "value": { "radius": r } }`.
- `match_tags` is an optional whitelist of `Tags` to test against;
  empty matches every entity with a `Transform`.
- The system writes the runtime-only `TriggerOverlaps { entered,
  staying, exited }` component on the trigger entity, populated with
  raw entity bits. Scripts and other systems read it to drive their
  own logic (event dispatch, sound effects, score, etc.).

### Arrival (SeekTarget.slowing_radius)

`SeekTarget` gained a `slowing_radius` field (default `0.0`). When
non-zero, the seeker's max speed is linearly scaled toward zero as it
approaches the target inside the radius, producing a smooth arrival
without overshoot:

```json
{
  "seek_target": {
    "target": { "kind": "Named", "value": "Player" },
    "accel": 800.0,
    "max_speed": 220.0,
    "slowing_radius": 80.0
  }
}
```

### Native behavior components — Step 6 reopen (broader vocabulary)

These run as pure native systems each frame — no script required.
All are serializable scene components and ship with built-in
defaults. See `crates/jengine-core/tests/step6_native_behaviors.rs`
for executable specs.

#### `wrap_bounds` — wrap-around screen edges

Wraps `Transform.position` around a rectangular box. Min-inclusive,
max-exclusive.

```json
{ "wrap_bounds": { "min": [-320, -240], "max": [320, 240] } }
```

#### `clamp_bounds` — Pin position inside a rectangle

Clamps `Transform.position`. If `zero_velocity_on_hit` is `true`
(the default) and the entity has a `Velocity2D`, the matching axis
is zeroed on contact so a player pinned against a wall doesn't
accumulate undelivered velocity.

```json
{
  "clamp_bounds": {
    "min": [-100, -100],
    "max": [100, 100],
    "zero_velocity_on_hit": true
  }
}
```

#### `despawn_out_of_bounds` — Auto-despawn off-screen entities

Despawns the entity when `Transform.position` leaves the box plus
`margin` of slack. Cheap fallback for projectiles and particles
that escape culling.

```json
{
  "despawn_out_of_bounds": {
    "min": [-1024, -1024],
    "max": [1024, 1024],
    "margin": 32.0
  }
}
```

#### `face_velocity` — Rotate to match velocity direction

Sets `Transform.rotation = atan2(vy, vx) + offset_radians` whenever
the entity's speed is at least `min_speed`. Below `min_speed`,
rotation is left alone so a stopped entity doesn't snap to angle 0.

```json
{ "face_velocity": { "offset_radians": 0.0, "min_speed": 0.001 } }
```

#### `angular_velocity` — Constant spin

Adds `omega * dt` (radians per second) to `Transform.rotation`
each frame.

```json
{ "angular_velocity": { "omega": 3.14159 } }
```

#### `flicker` — Timed sprite alpha flash

Toggles `SpriteComponent.color[3]` between `0.0` and the captured
original alpha at `frequency` toggles per second, for `duration`
seconds. Self-priming: drop it on any sprite with no setup. The
component auto-removes on expiry and the original alpha is
restored.

```json
{ "flicker": { "frequency": 10.0, "duration": 0.5 } }
```

#### `shake` — Decaying positional jitter

Applies a transient additive jitter offset on top of
`Transform.position`. `intensity` decays linearly to zero over
`duration` seconds; `frequency` controls the sample rate of the
jitter (samples are linearly interpolated). Self-priming:
captures the entity's current position as the origin on the
first tick. The component auto-removes on expiry and the
original position is restored.

```json
{ "shake": { "intensity": 4.0, "frequency": 30.0, "duration": 0.3 } }
```

> **Deferred:** boid separation/alignment behaviours are still
> blocked on the spatial-index work — see
> `docs/archive/scripting-redesign-plan.md` Step 6 and
> `docs/random-ideas.md`.

### Character controllers

A `character_controller` moves a **kinematic** body the way a platformer
hero or a top-down character moves: by a velocity asked for each frame,
stopped by walls and sliding along them. It climbs slopes up to
`max_slope_deg`, steps up ledges up to `step_height`, and stays on the
ground going down slopes (`snap_to_ground`). It is carried by the kinematic
platform it stands on, and stands on one-way platforms (a collider with
`"one_way": true`). It applies no gravity or acceleration of its own. Those
are the game's feel, so the script adds them to the velocity it asks for.

```json
"rigid_body": { "body_type": "Kinematic", "fixed_rotation": true },
"collider": { "shape": { "Capsule": { "half_height": 8, "radius": 12 } } },
"character_controller": { "max_slope_deg": 50, "step_height": 12, "snap_to_ground": 8, "push_mass": 0 }
```

```lua
local c = Physics.get_character(hero)          -- what the last physics step did, or nil
if c.grounded then vy = 0 else vy = vy - 1400 * dt end
if jump_pressed and c.grounded then vy = 520 end
Physics.move_character(hero, vx, vy)           -- the velocity to move by next step
Physics.move_character(hero, vx, vy, true)     -- ...falling through one-way platforms
```

`Physics.get_character` returns these fields:

- `grounded`, the ground's normal (`ground_nx`, `ground_ny`) and
  `ground_entity`
- `wall`: -1 for a wall on the left, 1 for one on the right, 0 for none
- `ceiling`, which is true on the frame it bumps its head
- `sliding`, which is true on a slope too steep to stand on
- `vx`, `vy`: how it actually moved
- `touching`: every entity it touches, sensors included, for pickups and
  hazards

The controller moves by the frame's `dt`, before the fixed physics step, so
it moves smoothly on any refresh rate. A `push_mass` above 0 pushes dynamic
bodies it walks into. A platformer package builds the platformer
feel on top: acceleration, coyote time, jump buffering, variable jump
height, and wall jumps.

---

## Shape queries

```lua
-- Everything a blast of radius 120 reaches - asked of the colliders, so a
-- long beam whose middle is out of reach is caught by its end.
for _, id in ipairs(Physics.overlap_circle(x, y, 120)) do
    Physics.apply_impulse(id, ...)
end
local hits = Physics.overlap_box(x, y, 80, 30, angle)   -- a sword's swing
local under = Physics.overlap_point(mx, my)             -- what the mouse is over
-- A cannonball of radius 15 fired along (dx, dy): where it would first strike.
local hit = Physics.circle_cast(ox, oy, dx, dy, 15, 2000)
if hit then mark(hit.x, hit.y) end
```

Questions asked of the **colliders** (Oct 2026), in every runtime:

| Function | |
|---|---|
| `Physics.overlap_circle(x, y, radius)` | every entity a collider of which overlaps the circle - touching counts - each once, in order of id |
| `Physics.overlap_box(x, y, width, height[, rotation])` | the same for a box about `(x, y)`, turned by `rotation` radians |
| `Physics.overlap_point(x, y)` | every entity a collider of which contains the point |
| `Physics.circle_cast(ox, oy, dx, dy, radius, max_dist)` | a circle swept along a line: `raycast`'s table (`entity_id`, `x`, `y`, `normal_x`, `normal_y`, `distance`) for the first thing it touches, or `nil`. `distance` is how far its centre travelled; `x, y` is where it touched |

Sensors are included, as in `raycast`. A spawned entity is named by the
handle `spawn_prefab` returned. Natively they ask the live physics world;
the browser runtimes ask the same snapshot `raycast` reads, with the same
arithmetic (`physics::query`). Unlike `Entity.find_in_radius`, which reads
transforms, these see a collider's whole shape. Engine Test Lab scene 161;
the showcase's Siege Blast.

## Tilemap

```lua
local id = Tilemap.get_tile(map_entity, "ground", 5, 3)
Tilemap.set_tile(map_entity, "ground", 5, 3, 12)
Tilemap.set_tile(map_entity, "ground", 5, 3, nil)  -- clear
Tilemap.set_tile(map_entity, "ground", 5, 3, 0)    -- also clear
Tilemap.set_tile(map_entity, "ground", 5, 3, -1)   -- also clear

local tile = Tilemap.get_tile_at_world(map_entity, "ground", 100, 200)

local tx, ty = Tilemap.world_to_tile(map_entity, 100, 200)
local wx, wy = Tilemap.tile_to_world(map_entity, 5, 3)
```

A tilemap is placed by its **world** position: a map with a `parent` is
drawn, collides and is picked by `world_to_tile` / `tile_to_world` where its
parent carries it (Oct 2026; every reader used its local offset before). Its
scale is not applied - a tile is `tile_size` big, and a map with a scale says
so once in the log - so change `tile_size`, or zoom the camera.

### Tilemap collision

A tilemap collides when it carries a `tilemap_collider` naming its layers,
or has a layer flagged `collision` (which is solid, and not drawn):

```json
"tilemap_collider": { "solid": ["ground"], "one_way": ["ledges"] }
```

**`nil`, `0` and any negative all mean "nothing here"** (since 2026-09-20).
A gid of `0` can never name a tile - every tileset's `first_gid` is 1 or
more - so it is the "empty" of every tilemap format there is, and it is
what a generator filling an array starts with. `Tilemap.get_tile` answers `0` for
an empty cell in bounds and `-1` for a cell outside the layer or a missing
layer, in every runtime (verified Sep 2026 against the native binding and
the browser preamble; this paragraph used to claim `nil`).

Until that date the three spellings did different things in different
runtimes: `nil` was a hard error on native (`bad argument #5`, thrown
every frame it was called), while the browsers turned the `-1` native
accepted into a gid of four billion - a tile with no art, which draws as
nothing and **collides as rock**. Do not add a fourth caller that converts
the value itself; `tilemap::script_tile_id` is the one place that rule
lives.

The physics step builds one static body for the tilemap, with its tiles
merged into as few boxes as it can - so nothing catches on the seams between
tiles - and rebuilds it when the tiles change. So
`Tilemap.set_tile(map, "ground", x, y, nil)` really does knock a hole in the floor.
A one-way layer blocks a `character_controller` only from above.

**Editing tiles is cheap, and dig away** (since 2026-09-21). The colliders
are grouped per 32x32 chunk and only the chunks you touched are rebuilt, so
a `Tilemap.set_tile` costs the same on a million-tile world as on a small one. Two
consequences worth knowing:

- **`Tilemap.set_tile` with the value a cell already holds costs nothing at all**,
  so a brush that fires every frame while the button is down needs no
  "did it change?" bookkeeping of its own.
- **Editing many tiles in one frame is one rebuild per chunk**, not one per
  tile, so carving a tunnel in a single frame is cheaper than carving it
  over several. What is *not* free is the bridge: one `Tilemap.set_tile` per cell is
  one deferred command per cell, so filling a whole map from a script is
  bounded by the command queue rather than by physics.

Measured on a 1000 x 1000 map: an untouched frame went from 966 us to
884 ns, and a one-tile edit from a full 11.6 ms rebuild to 1.1 us. Scene 94
in the Engine Test Lab is where you can watch it.

### Tile shapes and tilemap shadows

A tile is solid over its whole cell unless its tileset gives it a **shape**
(Oct 2026; Godot's TileSet physics and occlusion layers, Tiled's tile
collision editor, which imports as these):

```json
"tilesets": [{ "texture_id": "sprites/rock.png", "tile_width": 32, "tile_height": 32, "...": "...",
  "shapes": [
    { "tile": 4, "collision": [[0, 32], [32, 32], [32, 0]] },
    { "tile": 8, "collision": [[0, 0], [32, 0], [32, 10], [0, 10]] },
    { "tile": 6, "collision": [] },
    { "tile": 9, "occluder": [] }
  ] }]
```

Points are in the tile's **pixels**, `(0, 0)` its top-left and y down, as
the art is drawn; `tile` is local to the tileset. `collision` is a convex
outline - a slope, a plank, a ledge - or empty for a tile that is walked
through (grass in a solid layer). `occluder` is what casts a shadow and
defaults to the collision outline; empty casts nothing (glass, a crystal
vein). A hexagonal map's tiles are whole or nothing. The tilemap editor's
palette has the common shapes as presets (**Shape**, under the selected
tile).

Tiles cast shadows when a `tilemap_occluder` names their layers:

```json
"tilemap_occluder": { "layers": ["rock"] }
```

The cells are merged into a few long outlines first - a wall of a hundred
tiles is the four edges round it, not four hundred - and rebuilt only when
the tiles or the shapes change, so `Tilemap.set_tile` reaches the shadows as
it does the colliders. The rest is a `light_occluder`'s (`shadow_tint`,
`shadow_opacity`, `self_shadow`, `shadows_casters`, `shadow_length`), for the
whole map. With `self_shadow` off (the default) rock a lamp can see is lit
and what is beyond it is in shadow; on, the rock itself is dark. Engine Test
Lab scene 177; the showcase's Lamplit Mine.

### Autotiling

A layer can be painted in **terrains** and draw its own edges and corners
(Oct 2026; Godot's terrains, Unity's rule tiles). Put an `autotile`
component on the tilemap's entity naming the layer and each terrain's first
gid, lowest priority first:

```json
"autotile": { "layers": [{ "layer": "ground", "terrains": [1, 17, 33] }] }
```

That layer's cells then hold **terrain numbers** - `1` the first terrain,
`2` the next, `0` none - so painting is just
`Tilemap.set_tile(map, "ground", x, y, 2)`, and the coast, beach or cliff
around the cell is redrawn the frame it lands. Nothing is stored: the
renderer draws a **dual grid**, a tile on every corner shared by four cells,
chosen by which of the four are that terrain or higher. So each terrain
needs sixteen consecutive tiles from its first gid, tile `m` covering the
corners in mask `m` - bit 1 the top-left, 2 top-right, 4 bottom-left, 8
bottom-right (tile 0 is never drawn) - and a higher terrain draws over a
lower one, so any two terrains meet with no tiles made for the pair. Keep a
layer's terrains in one tileset; they stack in tileset order.

A `tilemap_collider` on the layer collides with every painted cell, as it
would with plain tiles. Engine Test Lab scene 151; the showcase's Mapmaker.

### Isometric maps and sorted layers

A tilemap can be laid out in **diamonds** (Oct 2026), and a layer can be
**sorted with the sprites**:

```json
"tilemap": {
  "tile_size": [96, 48], "orientation": "isometric",
  "layers": [
    { "name": "ground", ... },
    { "name": "houses", ..., "y_sort": true },
    { "name": "hill_top", ..., "y_sort": true, "offset": [0, 32] }
  ], ...
}
```

- **`orientation: "isometric"`**: `tile_size` is the diamond. The entity's
  transform is the **top vertex of cell (0, 0)**; a column runs down to the
  right, a row down to the left. `Tilemap.world_to_tile` /
  `tile_to_world` / `get_tile_at_world` follow it in every runtime, so
  picking the diamond under the mouse is one call. A tile whose image is
  taller than the diamond - a block, a house, a tree - stands on its cell,
  bottom-aligned, at its image's proportions. Autotiling works the same way
  on the diamond grid, and a `tilemap_collider` collides in diamonds.
  Tiled's `"orientation": "isometric"` imports as one.
- **`y_sort: true`** on a layer draws its tiles in the **sprite pass**,
  each sorted with the `y_sort` sprites of the tilemap's `z_order` by its
  cell - an isometric tile by its cell's centre, an orthogonal one by its
  cell's bottom edge. That is what lets a character walk behind a house
  tile and in front of it. A plain layer is drawn in the tilemap pass,
  under every sprite, as before.
- **`offset`** moves where a layer is **drawn** (world units, y up) - a
  hill's second storey is a layer lifted by a block's height. The cells,
  picking, the colliders and the sort stay on the grid; a lifted layer
  sorts a hair in front of what is on the same cell below it.

Engine Test Lab scene 152; the showcase's Hillside Hamlet.

### Tile data, random tiles and tiles that spawn

```json
"tilesets": [ { "texture_id": "sprites/isle.png", "tile_width": 64, "tile_height": 64, "columns": 8, "tile_count": 16,
  "first_gid": 1, "image_width": 512, "image_height": 128,
  "data":   { "7": { "kind": "lava", "burns": true, "damage": 3 } },
  "random": [ { "tile": 3, "variants": [[3, 6], [4, 1], [5, 2]] } ],
  "spawns": [ { "tile": 10, "prefab": "Chest" }, { "tile": 11, "prefab": "Palm", "keep": true } ] } ]
```

A tileset can say more than what a tile looks like (Oct 2026; Godot's
custom data layers, alternative tiles and scene tiles):

- **`data`** - any JSON for a local tile. Tiled's tile properties import as
  it. `Tilemap.get_tile_data(e, layer, x, y, field)` reads one field of the
  cell's tile, or the whole table with no `field`; `nil` for an empty cell, a
  tile with no data or no such field.
- **`random`** - a tile drawn as one of its `[tile, weight]` variants, picked
  by a hash of the cell's place: the same cell always draws the same one,
  and a meadow never repeats. The cell keeps the tile it names, so data,
  colliders and paths read that tile. A variant can itself animate.
- **`spawns`** - a tile that becomes an instance of a prefab at its cell's
  centre when the scene loads, the cell emptied unless `keep`. A tile painted
  later with `Tilemap.set_tile` stays a tile, and a v2 scene spawns nothing.

```lua
local x, y = Tilemap.world_to_tile(isle, px, py)
if Tilemap.get_tile_data(isle, "ground", x, y, "burns") then hurt(player) end
```

Engine Test Lab scene 200; the showcase's Treasure Isle.

### Animated tiles

```json
"tilesets": [{ "texture_id": "sprites/cave.png", ...,
  "animations": [
    { "tile": 7, "frames": [ { "tile": 8, "duration": 0.07 }, { "tile": 9, "duration": 0.07 } ] },
    { "tile": 27, "scatter": true, "frames": [ ... ] }
  ] }]
```

A tileset can animate its tiles (Oct 2026): wherever a cell names `tile`
(local to the tileset, from 0), it is drawn as its `frames` in turn, each
for `duration` seconds, then round again - water, torches, a waterfall.
Every cell shows the same frame (Tiled's way) unless the animation has
`scatter`, which starts each cell at its own point in the loop, so a sea of
one water tile does not flip in lockstep. The cells keep the tile they name:
`Tilemap.get_tile`, `set_tile`, the colliders and pathfinding never see a
frame, so a script reads and writes the animated tile as one tile.

The clock is the map's own (`anim_time`, never saved), advanced in Play by
every runtime's shared motion step; while editing, tiles show their base
tile. Autotile layers animate too, and Tiled's tile animations import (their
millisecond durations as seconds). Engine Test Lab scene 160; the
showcase's Waterfall Grotto.

### Hexagonal maps

```json
"tilemap": {
  "tile_size": [80, 92], "orientation": "hexagonal", "hex_side": 46,
  "stagger_axis": "y", "stagger_index": "odd", ...
}
```

A tilemap can be laid out in **hexagons** (Oct 2026), as Tiled has them:
`tile_size` is the hexagon's box and `hex_side` its flat sides - the
upright ones of a pointy-topped hexagon, the level ones of a flat-topped
one (half the height, or width, is a regular hexagon). With `stagger_axis`
`"y"` (the default) the hexagons point up and every other row is pushed
half a hexagon right; with `"x"` they are flat-topped and every other
column is pushed half a hexagon down. `stagger_index` says which: the
`"odd"` ones (the default) or the `"even"`. The entity's transform is the
top-left of cell (0, 0)'s box, as on a square map.

Picking (`Tilemap.world_to_tile`, `get_tile_at_world`), `tile_to_world`, a
`tilemap_collider` (a hexagon per solid cell) and `Tilemap.find_path` all
follow the hexagons - a path steps to any of the six neighbours, and the
`mode` argument is ignored. A tile whose image is taller than the hexagon
stands on its cell, bottom-aligned, and a `y_sort` layer sorts by the
cell's centre. Autotiling is a square-corner scheme and does not apply: a
hexagonal map draws an autotile layer's cells as plain tiles. Tiled's
`"orientation": "hexagonal"` (with `hexsidelength`, `staggeraxis`,
`staggerindex`) imports as one; its staggered *isometric* maps still import
as square.

| Function | |
|---|---|
| `Tilemap.neighbors(entity_id, x, y)` | the cells next to `(x, y)` that are on the map, as `{ {x, y}, ... }` - six on a hexagonal map, four on a square or isometric one |
| `Tilemap.distance(entity_id, ax, ay, bx, by)` | steps from one cell to the other through those neighbours, ignoring what is on the cells (hex steps, or Manhattan) |

```lua
-- Every hex a piece can reach with 3 steps of move, forest costing 2.
for _, n in ipairs(Tilemap.neighbors(map, c, r)) do
    local nc, nr = n[1], n[2]
    -- ...
end
if Tilemap.distance(map, archer_c, archer_r, foe_c, foe_r) <= 3 then shoot() end
```

Engine Test Lab scene 157; the showcase's Hex Strategy Board.

---

## Spline shapes

```json
"spline_shape": {
  "points": [[-300, -60], [-150, -20], [0, 0], [150, -20], [300, -60]],
  "fill_bottom": -400, "fill_texture": "sprites/soil.png", "fill_tile": 256,
  "edge_texture": "sprites/grass.png", "edge_width": 64, "edge_tile": 256, "edge_offset": 0.45,
  "collider": true, "friction": 0.9
}
```

Terrain drawn from a curve (Oct 2026; Unity's SpriteShape). The points become
a smooth curve (`smooth`, Catmull-Rom, `detail` segments between two points)
or straight lines; an open line is filled down to the local y `fill_bottom`,
a `closed` loop fills its inside. The fill is tiled with `fill_texture` every
`fill_tile` world units (empty is white, so `fill_color` alone; a
`fill_color` with no alpha draws no fill); the **edge** is a strip
`edge_width` thick along the curve, `edge_texture` repeating every `edge_tile`
along it with its top row outermost, `edge_offset` of it outside the fill -
the outside is worked out from how the shape winds, so a hill drawn either
way or a loop either way round has its grass on top. Both draw in the sprite
pass at `z_order`, lit like sprites, the edge over the fill.

With `collider` on (the default) the shape is solid - a convex triangle per
triangle of the fill - and raycasts hit it in every runtime. There are no
script setters: the shape is authored (the editor's inspector edits the
points as a list). Engine Test Lab scene 155; the showcase's Rolling Hills.

## Area effects

```json
"area_effect": { "size": [2000, 600], "buoyancy": 2.4, "drag": 2.0, "angular_drag": 3.0 }
```

A box that pushes the dynamic bodies in it (Oct 2026; Unity's effectors,
Godot's area gravity), `size` wide and tall about its entity and turned with
it. Every fixed physics step each collider inside is given, in proportion to
how much of its outline is in the box and to its mass:

- `buoyancy` - upward, in multiples of gravity at full depth: 1 holds a body
  still under water, 2 floats it half under. The box's **top is the
  surface**, so moving the entity moves the tide. It pushes at the middle of
  the part under water, so a hull that heels turns itself level (and a
  square crate floats on a corner, as a real one does).
- `force` - a steady acceleration, `[x, y]` units/s² (wind, a current, an
  updraft). **Particles feel it too**, so smoke bends the way the crates go.
- `attract` - towards the box's middle (negative pushes away), strongest
  there and gone at its corners.
- `drag`, `angular_drag` - velocity and spin lost per second inside.
- `enabled` - off, it does nothing.

Kinematic and static bodies are not pushed, nor are sensors. Nothing is
wired per host: the boxes are read when the physics bodies are synced.

| Function | |
|---|---|
| `Area.set(entity_id, field, value)` | any field by name - `"enabled"`, `"force"` (`{ x, y }`), `"buoyancy"`. A name the box has not got is logged |

```lua
-- A breeze that swings from side to side.
Area.set(wind, "force", { math.sin(t * 0.2) * 140, 0 })
```

Engine Test Lab scene 156; the showcase's Harbour at Night.

## Carvable sprites

```json
"carvable": { "collider": true, "collider_step": 3, "scorch": 12, "friction": 0.8 }
```

```lua
Sprite.carve(island, x, y, radius)   -- a hole in world units
```

A sprite with a `carvable` component (Oct 2026) draws a **copy** of its
texture that `Sprite.carve` cuts holes in - the authored texture, and every
other sprite drawing it, is untouched, and a scene loaded again is whole
again. A hole is cleared from the copy's alpha and written to the GPU a
rectangle at a time, the frame it lands; `scorch` darkens a rim that wide
round it (`scorch_color`). Texels at or below `alpha_cutoff` are empty.

With `collider` on (the default) the sprite gets a static physics body made
of what is solid: a small convex piece along the outline every
`collider_step` texels and boxes inside, rebuilt when a hole is cut - so a
crate standing on carved-away ground falls, and a body pushed into the land
is pushed back out of the top. `friction` is the land's. The hole is placed
by the sprite's world transform, so a turned or scaled sprite carves where
it is drawn. Raycasts hit carved land in every runtime. Engine Test Lab
scene 153; the showcase's Crater Isle.

## Paintable sprites

```json
"paintable": { "blend": "over", "alpha_cutoff": 0.5 }
```

```lua
-- a stamp in world units: where, how big, its colour, its shape, its turn
Sprite.paint(floor, x, y, 30, 0.10, 0.003, 0.003, 0.9, "splat", angle)
Sprite.paint(floor, x, y, 120, 0.03, 0.02, 0.02, 0.5, "streak", angle)
Sprite.paint(floor, x, y, 12, 0.05, 0.05, 0.05, 0.5, "disc")
```

A sprite with a `paintable` component (Oct 2026) draws a **copy** of its
texture that `Sprite.paint` stains - blood on a floor, scorch marks, oil,
footprints. A stain is painted once, kept for good, and costs nothing on the
frames after it: a stamp writes only the rectangle it touched, to the copy
and to the GPU. The authored texture, and every other sprite drawing it, is
untouched, and a scene loaded again is clean again.

The shapes are drawn rather than loaded: a soft `disc` (`size` across), a
`splat` (a ragged blob `size` across that throws droplets along `rotation` -
blood from a hit, paint from a throw; the default) and a `streak` (a
tapered smear `size` long along `rotation` - a drag mark, a burn line).
Each splat's shape is a hash of where it landed, so it is the same in every
runtime. The colour is linear, like a sprite's, and defaults to white; its
alpha is the stamp's strength. `blend` says how a stamp meets the floor:
`over` covers it by that alpha, `multiply` darkens it by the colour (a
scorch, a soak), `erase` takes alpha away (a glow fading, a stain washed
off). An eleventh argument names the blend for one stamp, over the
component's: `Sprite.paint(e, x, y, 40, 0, 0, 0, 1, "disc", 0, "erase")`.
Texels at or below `alpha_cutoff` are holes and stay holes.

**A paintable can be a canvas**: a transparent texture with `alpha_cutoff`
0 is a layer you paint onto - `over` composites alpha, so an empty texel
takes the stamp's colour and gains its opacity. And an emitter on the same
entity whose `emit_shape` is `sprite` starts its particles from what is
painted **now** - the shape is read from the painted copy and read again
whenever it changes, and an empty canvas emits nothing. The showcase's
Wildfire paints each burning cell into three canvases - the glowing front
(its flames), embers, smoke - and erases them as the cells burn out; Engine
Test Lab scene 231.

**Wet paint** (Oct 2026): the `gloss` blend writes no colour - it raises
the sprite's normal-map alpha, its specular mask, so a stroke shines under
a light where the surface around it is matt (the material needs a
`normal_map_id` and some `specular`). With `dry_seconds` on the paintable
the shine fades back to the authored mask, to a third in that many seconds,
on the wall clock. And the `spray` shape is a disc of speckle, dense at its
middle and scattered at its rim - an aerosol; a stroke of them fills in.

```lua
Sprite.paint(wall, x, y, 30, 1.0, 0.3, 0.6, 0.95, "disc", 0)            -- the paint
Sprite.paint(wall, x, y, 40, 1.0, 0.3, 0.6, 0.30, "spray", 0)           -- its overspray
Sprite.paint(wall, x, y, 33, 1, 1, 1, 1, "disc", 0, "gloss")            -- and it is wet
```

The showcase's Graffiti Alley sprays every stroke this way; Engine Test Lab
scene 232.
The stamp is placed by the sprite's world transform. Engine Test Lab scene
228; the showcase's Firefight.

```lua
-- a dent in the sprite's normal map: depth is how steep its rim is
Sprite.press(snow, x, y, 14, 1.5, "streak", heading)   -- a boot print
Sprite.press(snow, x, y, 8, 1.6, "disc")               -- a paw
Sprite.press(sand, x, y, 40, -1.0, "disc")             -- negative: a mound
```

`Sprite.press` (Oct 2026) takes the same shapes and stamps them into the
sprite's **normal map** instead of its colour: a footprint in snow, a tyre
rut in mud, a dent in sheet metal - relief the lights see, so a low light
catches each print's rim on the side it faces. The sprite's material must
name a `normal_map_id` (one without has nothing to dent and ignores
presses); the press works on a copy of that map, keeps its alpha (the
specular mask), and rewrites only the texels round the stamp. `depth` is the
rise of the rim over about a texel - `1` is a rim near 45 degrees, left out
it is `1`; a negative depth raises a ridge. A press **sets** a depth rather
than digging, so a print over a print is no deeper. Paint and press
together for a print with a shadow in it: the showcase's Snowfield presses
each print and paints a faint blue multiply into it. Engine Test Lab scene
229.

---

## Liquids

```json
"fluid": { "radius": 16, "viscosity": 5, "pour_rate": 0, "pour_angle": 0, "pour_speed": 70,
           "cooling": 0.04, "freeze_below": 0.17, "size": 36, "texture_id": "sprites/fx/soft_dot.png",
           "gradient": [[0, 0.13, 0.12, 0.12, 1], [1, 7.0, 6.3, 4.5, 1]],
           "lights": { "max": 10, "radius": 260, "intensity": 2.2, "color": [1, 0.5, 0.15] } }
```

```lua
Fluid.set(melt, "pour_rate", 110)   -- pour (0 stops it)
Fluid.set(melt, "clear", true)      -- empty it
```

A `fluid` (Oct 2026) pours particles that behave as one liquid - molten
metal, water, honey. They fall, push each other apart where they crowd and
draw together where they thin (double density relaxation), even out their
speeds by `viscosity`, and slide off and pool against every non-sensor
`collider` - the outlines particles meet - so a liquid fills a mould, runs
down a ramp and levels in a tank. It pours `pour_rate` particles a second
from `pour_at` in the entity's own frame, along `pour_angle` turned with the
entity, at `pour_speed`, up to `max_particles`. Each particle cools by
`cooling` a second, takes its colour from `gradient` (`[t, r, g, b, a]`, 0
cold to 1 hot), and below `freeze_below` sets solid where it lies - the
liquid piles against it. `lights` hangs a few shadowless lights on the
hottest moving particles. `Fluid.set` writes any field by name and keeps the
liquid; `clear` empties it.

It draws as a soft dot a particle in the sprite pass. Put it under a
`canvas_group` with a **`threshold`** (0..1, and `threshold_soft`) and the
group's picture is cut where its alpha crosses that and drawn in its own
colour: the dots merge into one hard-edged surface. Engine Test Lab scene
233; the showcase's The Foundry.

---

## Mirrors

```json
"mirror": { "width": 560, "depth": 300, "resolution": 0.75, "tint": [0.6, 0.66, 0.8, 1], "z_order": -45 }
```

```lua
Mirror.set(m, "enabled", false)   -- turn it off (and stop paying for it)
```

A `mirror` (Oct 2026) is a line along its entity's local x, `width` long,
facing its local +y. It shows what is `depth` in front of the line,
reflected, over the region as deep **behind** it - so in a top-down room
with a mirror wall the room seems to go on through the wall, its far side
shown in reverse; turn the entity to aim it. Each frame a view of its own
draws the region in front into a texture `resolution` texels a world unit
(1 matches a camera at zoom 1), lit and shadowed like the rest of the
world, and the sprite pass draws it behind the line at `z_order`,
multiplied by `tint` - glass a little dark and blue is what makes each
reflection dimmer than the one it reflects. Reflections are drawn in other
mirrors' views too, so **a mirror sees mirrors**: give two mirrors that face
each other a depth that reaches past the other and the corridor between
them repeats, a frame a step. A mirror costs a world pass over its own
small region, and none draws whose reflection is nowhere near the screen.
A reflection has no post chain of its own - the screen's runs over it.
`Mirror.set` writes any field by name. Engine Test Lab scene 236; the
showcase's Mirror Maze.

---

## Water surface

```json
"water_surface": { "width": 2000, "depth": 420, "spacing": 8, "tension": 0.018, "damping": 0.02, "spread": 0.25,
                   "splash": 0.014, "splash_speed": 150, "chop": 22, "chop_strength": 1.6,
                   "swell_height": 16, "swell_length": 520, "swell_speed": 70,
                   "buoyancy": 2.4, "drag": 2.6, "angular_drag": 3.0,
                   "color_top": [0.06, 0.15, 0.21, 0.86], "color_deep": [0.0, 0.0, 0.01, 0.97],
                   "foam_color": [0.5, 0.68, 0.82, 0.85], "foam_width": 4, "z_order": 20 }
```

```lua
Water.splash(sea, x, -18)        -- push the surface down at world x
Water.set(sea, "chop", 30)       -- any field: a storm getting up
```

A `water_surface` (Oct 2026) is a body of water seen side on, `width` wide
and `depth` deep, its surface at the entity's position at rest. The surface
is a row of springs `spacing` apart - each pulled back to rest (`tension`),
slowed (`damping`) and pulling its neighbours (`spread`), every number per
step of a fixed 60 Hz - so a push runs out along it as two waves and dies
away, the same at any frame rate. Every solid collider that crosses the
surface faster than `splash_speed` pushes it by its speed past that times
`splash` (read from its `velocity`, or from how far it moved when it has
none), `chop` pokes it at random that many times a second, and a `swell` -
a sine `swell_height` high, `swell_length` long, moving at `swell_speed` -
rolls under it all.

What is in it **floats on that surface**: with `buoyancy` (in multiples of
gravity), `drag` and `angular_drag` the water is also an area effect whose
top is the surface under each body, so a boat climbs a crest, drops into a
trough and heels on the slope. Keep the drag high (the default is 2.5): a
thin hull on strong buoyancy and little drag rings like a spring until it
leaps out. Bodies slower than `splash_speed` do not push the surface, so
what bobs does not shake the water it rides. It draws as one mesh,
`color_top` at the surface fading to `color_deep` at the bottom, with a line
of `foam` along the top that thickens where the surface moves fastest.
Engine Test Lab scene 237; the showcase's Harbour Storm.

---

## Sprite fills

```json
"sprite": { "texture_id": "sprites/fx/soft_dot.png", "size": [240, 70],
            "fill": { "mode": "radial", "amount": 0.0, "start": 1.5708, "clockwise": true } }
"sprite": { "texture_id": "__white", "size": [612, 12], "anchor": [0, 0.5],
            "fill": { "mode": "bar", "amount": 1.0, "from": "left" } }
```

```lua
Sprite.set_fill(warning, t / 1.3)   -- how much is drawn, 0..1
```

A sprite's `fill` (Oct 2026) can draw only part of it - Unity's
`Image.fillAmount`, Godot's `TextureProgressBar`. A **`bar`** draws the first
`amount` of the sprite from one side (`from`: `left`, `right`, `bottom`,
`top`), its texture cropped with it: a health bar, a charge, a fuse. A
**`radial`** draws the first `amount` of a turn about the sprite's middle,
from `start` (radians: 0 along +x, the default `PI / 2` straight up),
clockwise unless `clockwise` is false - its texture cut with it, so a disc
texture gives a pie and a square one a square that fills round: a cooldown,
or the warning a boss's slam fills before it lands. `Sprite.set_fill(e,
amount)` moves either (it warns on a sprite with no such fill). A bar is one
cropped rectangle, so every pass that draws sprites - normals, shadows,
light layers - draws it cropped; a radial fill is a fan of triangles the
sprite pass draws, and passes that only draw rectangles draw it whole.
Engine Test Lab scene 238; the showcase's Arena Boss.

---

## Post volumes

```json
"post_volume": { "size": [980, 1000], "blend": 300, "weight": 1.0, "priority": 0, "exposure": 1.2,
                 "effects": [
                   { "name": "color_grade", "params": { "saturation": { "Float": 0.6 },
                                                        "tint_color": { "Vec4": [0.55, 0.7, 1.0, 0.2] } } },
                   { "name": "vignette", "enabled": true, "params": { "intensity": { "Float": 0.55 } } } ] }
```

A `post_volume` (Oct 2026; Unity's Volume, Godot's `WorldEnvironment` per
area) is a box, `size` about its entity, whose post-effect values the main
camera takes on as it comes in: whole inside, fading to nothing over `blend`
world units outside. Each of its `effects` names an effect of the post chain
and the `params` it wants; `enabled` switches it on (or off) inside;
`exposure` sets the HDR exposure. Volumes stack by `priority`, lowest first,
each blending over what the scene and the lower ones made, and `weight`
scales one. Numbers and vectors blend; an int switches half way.

The scene's own post settings are never changed: each frame the screen's
chain is drawn from a **blended copy**, so leaving every volume gives back
exactly the scene's values, and `PostProcess.set_*` from a script still sets
the scene's. An effect a volume names that the scene does not list is added
at the volume's values; one the scene lists **disabled** is switched on and
blended from the values the scene gave it - so author a room's effect in
the scene, disabled, for it to fade in rather than appear. A volume in a
streamed-in room scene (`Scene.load_additive`) works like any other, which
is how each room of a house has its own look with no script. Views and
mirrors draw with the scene's settings. Engine Test Lab scene 239; the
showcase's Haunted Manor.

---

## Falling sand

```json
"pixel_sim": { "cells": [480, 270], "steps_per_second": 60, "seed_texture": "maps/cave_seed.png",
               "sources": [ { "at": [-500, 420], "material": "sand", "rate": 150, "radius": 10, "active": true } ],
               "collider": true, "collider_interval": 0.3, "collider_step": 2, "glow": 2.2 }
```

```lua
PixelSim.paint(entity, x, y, radius, "water")   -- a disc of a material at a world point
```

A `pixel_sim` (Oct 2026; Noita's world, the Powder Toy) is a grid of
`cells`, each a material, stepped by rules `steps_per_second` times a second
whatever the frame rate: **sand** falls and piles, **water** flows and
levels, **oil** floats on water and burns, **fire** rises, spreads to what
burns (oil quickly, wood slowly) and burns down to **smoke**, **smoke** and
**steam** drift up and thin, **lava** runs slowly, lights what burns and
turns water to steam and itself to **stone**; **stone** and **wood** stay
put. The materials are `empty`, `stone`, `sand`, `water`, `oil`, `fire`,
`smoke`, `lava`, `steam` and `wood`.

The entity's **sprite** draws it - its size and place are what the cells
cover, its texture is replaced by the simulation's own, sampled nearest
unless `texture_filter` names `__pixel_sim/` - and fire and lava are drawn
again additively at `glow` (0 for none), which bloom picks up. With
`collider`, stone, wood and sand at rest are **solid to bodies**: the
entity's `carvable` (added if missing) holds the solid cells and the physics
builds its collider from them, rebuilt at most every `collider_interval`
seconds - so a crate sits on a dune and falls when it is washed away.
`collider_step` is the cell block the outline is traced at.

`seed_texture` starts it from a picture: each opaque pixel becomes the
material whose key colour is nearest (sRGB stone `#808080`, sand
`#e6c86e`, water `#286ee6`, oil `#462814`, fire `#ff8c00`, smoke
`#3c3c3c`, lava `#ff3c00`, steam `#dce6f0`, wood `#825028`), scaled to
`cells`. Keep it out of `sprites/`, which the build packs and compresses.
`sources` pour a material at a point in the sprite's local units, `rate`
cells a second over `radius`. `PixelSim.paint` takes a **world** point and
radius; a paint queued in `init`, before the first step, is kept. The cells
are runtime state, so a saved scene starts again from its seed. Engine Test
Lab scene 240; the showcase's Falling Sands.

---

## Smoke

```json
"smoke": { "cells": [200, 110], "steps_per_second": 60, "iterations": 24,
           "buoyancy": 300, "weight": 1, "cooling": 0.5, "fade": 0.06, "damping": 0.1,
           "vorticity": 0.8, "opacity": 1.6, "walls": [true, true, true, true], "obstacles": true,
           "color": [0.5, 0.5, 0.5, 1],
           "sources": [ { "at": [-290, -300], "radius": 30, "density": 34, "temperature": 10,
                          "velocity": [0, 160], "push": 6, "color": [0.46, 0.43, 0.41, 1], "active": true } ] }
```

```lua
Smoke.puff(entity, x, y, radius, amount, vx, vy)   -- smoke and a push at a world point
Smoke.set(entity, "fade", 0.2)                     -- any field by name
Smoke.set(entity, "sources.2.active", true)        -- or a source's (counted from 1)
```

A `smoke` component (Oct 2026; Jos Stam's *Stable Fluids*) is a grid of air
over its entity's **sprite**, whose quad is the box the air fills: `cells`
across and down, stepped `steps_per_second` times a second whatever the
frame rate. Each step its `sources` add smoke (`density` a second, in their
`color`, linear), heat (`temperature`) and a push toward their `velocity`
(`push` is the share of the gap closed a second - a fan is a source with a
push and no smoke); heat lifts the air (`buoyancy`) and smoke weighs it down
(`weight`); `vorticity` keeps the small whirls a coarse grid loses; the air
carries itself and everything in it along; and a pressure solve
(`iterations` sweeps) makes it flow without piling up. Heat, smoke and
motion fade by `cooling`, `fade` and `damping` (shares a second). `walls`
says which edges - left, right, top, bottom - hold the air in; an open edge
lets it out.

With `obstacles`, every non-sensor collider over the box is **solid** to it,
and one that moves - by its body, its velocity or a script, turning
included - drags the air in its cells along, so a swinging weight pushes the
smoke aside and leaves a wake. The smoke is drawn through a texture of its
own (sampled smoothly unless `texture_filter` names `__smoke/`), its alpha
`1 - e^(-smoke * opacity)`, and it is **lit** like any sprite unless the
entity is `unlit`, so lamps and a window beam light it and shadows cut
through it. `Smoke.puff` takes a world point and radius. The air is runtime
state: a saved scene starts clear. Engine Test Lab scene 241; the
showcase's Smoke Chamber.

---

## Rewind

```json
"rewind":     { "mode": "record", "speed": 1.6 }
"rewindable": { "seconds": 12 }
```

```lua
Rewind.set(clock, "mode", "rewind")   -- "record" | "rewind" | "hold"
Rewind.set(clock, "speed", 2.0)
```

A `rewindable` entity (Oct 2026; Braid, Prince of Persia's sands) keeps the
last `seconds` of its history - its transform, its `velocity` (on a dynamic
body, the body's), its sprite's colour and `uv_rect`, its sprite animator's
frame, and its particle emitter's rate and whether it is on - one sample a
frame while its world's **`rewind` clock** records. One clock a world (the
first found). Set to `rewind`, the clock's cursor walks back through the
recording at `speed` and every rewindable is put where it was then, blended
between the samples either side; at the start of the recording it stops.
`hold` keeps the world still at the cursor. Set back to `record`, the world
goes on **from the cursor**: what came after is forgotten, a body gets the
velocity it had then, an animator plays on from its frame and an emitter is
as it was.

While not recording, bodies are put back by their transforms (the physics
teleports them) with no velocity, so gravity does not pull them out of the
past; animators are paused and emitters make no new particles. A
rewindable that did not exist yet at the cursor - spawned after the
recording began - is hidden until the world catches up with it. A script's
own variables are its own to rewind: keep its clock in step by counting
down at the clock's speed while rewinding, as the showcase does. Engine Test
Lab scene 242; the showcase's Rewind Ruins.

---

## Fog

```json
"fog": { "color": [0.55, 0.62, 0.75], "density": 0.72, "scatter": 34, "noise_scale": 260, "noise_amount": 0.75,
         "wind": [26, 2], "ground": -270, "height": 210, "size": [0, 0] }
```

```lua
Fog.set(mist, "density", 0.35)   -- any field by name; the drift goes on
```

A `fog` (Oct 2026) is air that catches light - a lamp's halo spreading past
its own reach, the shadows of what stands in the light cut through the mist
as shafts, and everything farther off going grey. One a world (the first
found), applied where each pixel already has its light (the lighting
composite): its **density** there - `density` at its thickest, billowing in
noise `noise_scale` world units across by `noise_amount`, drifting with
`wind`, thinning to a third every `height` world units above `ground` (0:
even), inside a box of `size` about its entity fading at the edges, or
everywhere with `[0, 0]` - veils what is behind it, and the light the air
catches is added in its `color`: the light gathered over `scatter` pixels
round the point. Being the scene's own light, the fog is lit exactly where
the scene is - occluders, cookies and light layers included - so it needs a
lit scene. `light_at` leaves it out. Unlit things are drawn over it,
unveiled. Engine Test Lab scene 244; the showcase's Moor at Midnight.

---

## Wave function collapse

```json
"wfc": { "layer": "ground", "cells_per_second": 150, "seed": 118, "outside": "x",
         "tiles": [ { "gid": 1, "edges": ["x", "x", "x", "x"], "weight": 0.15 },
                    { "gid": 7, "edges": ["x", "o", "o", "x"], "weight": 0.9 } ] }
```

```lua
Wfc.generate(map, x, y, w, h)   -- a region of the layer, in cells
function on_wfc_done(entity) end   -- on the map's own script, a region finished
```

A `wfc` generator (Oct 2026; Maxim Gumin's WaveFunctionCollapse, the simple
tiled model) on a tilemap entity fills regions of its `layer` with its
`tiles` - each a gid and the label of each edge, **up, right, down, left** -
so that every two tiles side by side carry the same label on the edges that
touch. `Wfc.generate` queues a region: every cell starts able to be any tile,
the tiles **already in the layer** just outside the region narrow the cells
beside them - so a new stretch joins the old one seamlessly, and a level can
go on for ever - and the layer's edge narrows its cells to tiles whose edge
there is `outside` (empty: any). Then the cell with fewest tiles left is
settled, a tile picked by `weight`, and what that rules out is carried
outward; `cells_per_second` of them a second (0: the whole region at once),
each written into the layer as it settles, so a map can be watched being
decided. A cell left with no tile begins the region again from another seed,
a few times before it is given up with a log line. At most 64 tiles. The
choices follow from `seed`, the region and how many regions came before, so
a scene makes the same maps every run, and asking for a region again makes
another. Engine Test Lab scene 245; the showcase's The Endless Dungeon.

---

## Vision

```json
"vision": { "radius": 330, "fov": 1.1, "direction": 0, "draw": true, "z_order": 6,
            "color": [1.0, 0.68, 0.21, 0.22], "edge_color": [1.0, 0.68, 0.21, 0.0],
            "targets": "thief", "rays": 56 }
```

```lua
local n = Entity.get_custom(guard, "Vision", "seen")   -- how many it sees
Vision.set(guard, "color", { 1.0, 0.15, 0.1, 0.3 })    -- any field by name
function on_seen(entity) end   -- the first target came into view
function on_lost(entity) end   -- the last one left it
```

A `vision` (Oct 2026; the visibility polygon of every stealth game) is what
an entity can see: `radius` far, across `fov` radians about its facing - the
entity's turn plus `direction`, a full turn or more seeing all round - and
nothing behind a wall. A wall is any non-sensor collider (its own and the
targets' left out). Each frame rays go at every corner in reach, just either
side of it, and evenly across the cone (`rays`), and where each first meets a
wall makes the polygon. With `draw` it is written into the entity's own
`mesh` as a fan, `color` at the eye fading to `edge_color` at the reach, so it
sorts with the sprites (give the entity `unlit` and an additive material for
a lit cone). Whatever carries the **`targets` tag** and stands inside it is
seen: the count is the entity's custom `Vision.seen`, and its script is told
`on_seen` when the first is seen and `on_lost` when the last is lost. Engine
Test Lab scene 246; the showcase's Eyes in the Dark.

---

## Day cycle

```json
"day_cycle": { "hour": 6.0, "hours_per_second": 0.4, "sun": "Sun", "sunrise": 6.0, "sunset": 18.0,
               "sky": true, "sky_z": -1000,
               "keys": [ { "hour": 12.0, "sky_top": [0.1, 0.3, 0.85], "sky_horizon": [0.55, 0.75, 1.0],
                           "ambient": [1, 1, 1], "ambient_intensity": 0.6,
                           "sun_color": [1.0, 0.97, 0.9], "sun_intensity": 1.0 } ],
               "bodies": [ { "name": "SunDisc", "rise": 6.0, "set": 18.0, "height": 0.85 } ],
               "night": [ "Stars" ] }
```

```lua
local hour = Entity.get_custom(day, "DayCycle", "hour")  -- 0 to 24
DayCycle.set(day, "hour", 18.0)              -- jump the clock (no dawn or dusk is told)
DayCycle.set(day, "hours_per_second", 0.0)   -- stop it; any field by name
function on_dawn(entity) end   -- the hour passed sunrise
function on_dusk(entity) end   -- the hour passed sunset
```

A `day_cycle` (Oct 2026) keeps the time of day and writes what it changes.
Its `keys` say, at given hours, the sky at the top and at the horizon, the
ambient light and the sun's colour and strength; between two keys every
number is eased, and the last wraps to the first across midnight. Each frame
it writes the world's `ambient_light`; the `directional_light` on the entity
named `sun` - rising in the east (+x) at `sunrise`, overhead half way, setting
in the west at `sunset`, weak while low and off below the horizon, so shadows
swing under it; with `sky`, a gradient behind everything as the entity's own
`mesh` at `sky_z`, kept over the main camera's view; the entities in `bodies`
(a sun disc, a moon) on arcs across the view between their `rise` and `set`
(`height` a share of the view's half height); and the sprites of those in
`night` faded in by how dark the ambient is. One cycle a world - the first
found. Engine Test Lab scene 247; the showcase's A Day on the Island.

---

## Floating origin

```json
"floating_origin": { "threshold": 4000, "enabled": true }
```

```lua
local x = Transform.get_position(car)
local true_x = x + (Entity.get_custom(car, "FloatingOrigin", "x") or 0)  -- how far it has really come
FloatingOrigin.set(car, "enabled", false)      -- or "threshold"
function on_origin_shift(entity) end           -- the world was just moved back
```

A 32-bit float has about seven digits, so a thing a million units out is
placed to a tenth of a unit and shakes. A `floating_origin` (Oct 2026) keeps
its entity near the origin instead: when it strays past `threshold` along an
axis, the **whole world is moved back** along that axis by where it is - every
root entity, the points trails, cloth and ropes keep, fluid particles, a
rewindable's history, the particles in flight, the camera, the physics bodies
(moved, keeping their speed and contacts) and the motion blur's history - so
nothing on screen jumps. Each axis on its own: a car on a side-scroller's
road is moved back in x only, so the ground stays at the height it was
authored at. The total moved is kept as the custom `FloatingOrigin.x` / `y`;
a script that remembers world positions of its own should work in **true**
coordinates (a position plus that total), or take the shift off them in
`on_origin_shift`. Keep `threshold` above the view's width, so a shift reads
as a camera cut. One per world - the first found. Engine Test Lab scene 248;
the showcase's The Endless Road.

---

## Time scale

```json
"time_scale": { "scale": 1.0, "ignore_world": true }
```

```lua
Time.set_scale(0.125)                        -- the whole world at an eighth (applied at the frame's end)
local s = Time.get_scale()                   -- what the last frame ran at
local real = s > 0 and dt / s or 0           -- the real time behind a scaled dt
TimeScale.set(hero, "ignore_world", true)    -- or "scale"; adds the component
Audio.follow_time(true)                      -- music and new one-shots bend with the world
```

`Time.set_scale` multiplies the `dt` every host hands the whole world -
behaviours, velocity, physics, particles, animation and the `dt` scripts are
given. A `time_scale` (Oct 2026) gives one entity a pace of its own: `scale`
multiplies its time, and `ignore_world` runs it on real time, so a hero in
bullet time moves at full speed through a crawling world. It reaches the
entity's velocity, sprite animation, skeleton (on the root), particles and
trail; a script moving it divides its `dt` by `Time.get_scale()` itself, and a
physics body cannot keep its own pace (rapier steps the world at once). A
world paused with `Time.set_scale(0)` stops even an entity that ignores it.
`Audio.follow_time(true)` multiplies the music's pitch and every one-shot
started after it by the world's scale (never below a twentieth) - off by
default, because `Time.set_scale(0)` is how a game pauses and a pause menu
wants its music. Engine Test Lab scene 249; the showcase's Bullet Time.

---

## Cast shadow

```json
"cast_shadow": { "view": "side", "opacity": 0.8, "fade": 0.6, "lights": 2,
                 "max_length": 4.0, "lean": 0.35, "light": "" }
```

```lua
CastShadow.set(e, "opacity", 0.5)   -- any field by name
CastShadow.set(e, "view", "top")
```

The lighting here is flat land: an occluder's shadow runs to the edge of
its light, which is right seen from above and wrong seen from the side,
where a person's shadow lies on the ground at their feet. A `cast_shadow`
(Oct 2026) draws that one: its sprite's own silhouette, darkened (`color`,
`opacity`) and sheared from its foot along the light, fading to `fade` at
its tip, `z_offset` under the sprite. The light is the scene's - the
brightest `directional_light` and the nearest `point_light`s that reach it,
the strongest `lights` of them (so two lamps give two crossed shadows), or
only the one named `light`. In a **side** view a sun's own direction says
how high it stands: falling steeply it casts a footprint (`min_length` of
the height), low it casts a long shadow (up to `max_length` heights), and
below the horizon none; a lamp's shadow points away from it, by how high
it hangs above the foot. The shadow lies back into the ground by `lean`
per unit of length - the slant the floor is seen at. In a **top** view the
shadow lies along the light on the ground, as long as `height` over the
light's `height` (the tangent of its elevation). The fixed drop shadow of
`shadow_caster` is the other kind. Engine Test Lab scene 250; the
showcase's Sundial Plaza.

---

## Auto normal

```json
"auto_normal": { "bevel": 12.0, "strength": 2.0, "shape": "round", "cutoff": 0.5 }
```

```lua
AutoNormal.set(e, "strength", 0.0)   -- any field by name; 0 lays it flat
AutoNormal.set(e, "shape", "pillow") -- "round", "linear" or "pillow"
```

A normal map is something an artist paints, and flat cut-out art has none,
so lights pass over it as if it were a decal. An `auto_normal` (Oct 2026)
works one out of the sprite's own silhouette: each texel's distance in from
the nearest transparent one (alpha under `cutoff`), over `bevel` texels,
becomes a height - a quarter-round (`round`), a straight chamfer
(`linear`) or a soft S (`pillow`) - and its slope, steepened by
`strength`, the normal. The edge of the texture counts as outside, and a
hole is an edge too. The map is made once where the pixels are and again
only when the texture or a field changes; it is named in the sprite's
material (a `lit_sprite` one is added if the sprite has none), so it goes
through the same deferred normal pass as a painted map - `height` on the
lights and `specular` on the material mean what they always do. A material
that already names a painted normal map keeps it. Engine Test Lab scene
251; the showcase's The Paper Theatre.

---

## Tile light

```json
"tile_light": { "solid_layers": ["rock"], "decay": 0.8, "solid_decay": 0.45, "intensity": 2.0,
                "max_rate": 30,
                "emitters": [{ "tile": 8, "color": [1.0, 0.36, 0.08], "intensity": 1.0 }] },
"tile_light_source": { "color": [1.0, 0.62, 0.3], "intensity": 0.85, "on": true }
```

```lua
TileLight.set(cave, "intensity", 0.0)    -- any tile_light field by name
TileLight.set(torch, "on", false)        -- or any tile_light_source field
```

A cave of tiles lit by a hundred torches cannot afford a hundred
shadow-casting point lights. A `tile_light` on a tilemap (Oct 2026) floods
light through its cells instead, the way Terraria's and Minecraft's does:
each cell takes the brightest light of any source, per colour channel,
falling by `decay` for every open cell it crosses (diagonals by
`decay^sqrt 2`, never through a gap two rock corners close) and by
`solid_decay` for every cell of rock it goes into - and light in the rock
never comes out again, so a wall's face glows and fades inward and a
one-cell wall still blocks. Because it is the brightest and not the sum, a
thousand sources cost one pass over the map. Sources are entities with a
`tile_light_source` standing in a cell of the map (a torch, a miner's
lantern) and the map's own tiles listed in `emitters` by global id (lava,
mushrooms, crystals). `solid_layers` names the layers that are rock; empty,
the layers flagged `collision` (which, as in Tiled, are not drawn - name a
drawn layer here instead). The light is kept as one texel a cell,
filtered smoothly, rebuilt only when the map, a field or a source changes
(at most `max_rate` times a second), and drawn into the light buffer as a
lightmap over the map, `intensity` times; `light_at_textured` reads it
too. Orthogonal maps only. Engine Test Lab scene 252; the showcase's
Torchlit Depths.

---

## Trajectory

```json
"trajectory": { "velocity": [900, 40], "active": true, "seconds": 3.0, "every": 3,
                "line": "Aim", "hit_line": "Ghost" }
```

```lua
Trajectory.set(cue, "velocity", { vx, vy })   -- any field by name
Physics.set_velocity(cue, vx, vy)              -- the shot: it goes down the line
local hit = Entity.get_custom(cue, "Trajectory", "hit")   -- "Ball3", or ""
```

A `trajectory` on a dynamic body (Oct 2026) shows where the body goes if
it is set moving at `velocity` now - a cue ball's aim line, a grenade's
arc, a pinball off the bumpers. The physics steps a **copy** of the whole
world `seconds` ahead (at most 20) by the same pipeline, settings and
contacts the real step uses, so the answer is not a ray's guess: a shot
then struck with `Physics.set_velocity` at that velocity goes exactly down
the line, cushions, pegs and all. The body's places every `every` steps
are written into the `line` of the entity named `line` (in that entity's
own space), the first other dynamic body it touches and that body's path
from then into `hit_line`'s, and the custom `Trajectory` fields `hit` (the
struck entity's name, `""` for none), `hit_x` / `hit_y` (where the body
is when it touches), `end_x` / `end_y` and `steps` are rewritten every
frame for a script. `active = false` clears the lines. It runs at the end
of the physics frame, so it sees this frame's world; area effects (water,
wind) are not in the copy. Engine Test Lab scene 253; the showcase's
Billiard Hall.

---

## Save states

```lua
State.save("quick")            -- the whole world, at the end of this frame's physics
State.load("quick")            -- put back exactly as it was
if State.has("quick") then ... end   -- true straight after a save is asked for
State.forget("quick")
```

A save state (Oct 2026) is the whole running world written down under a
name: every entity and component (custom components too), and every
body's pose, velocity and spin - an emulator's quicksave, a puzzle's undo,
a checkpoint, a replay's start. A load puts it back **under the same entity
handles**, so the handles a script holds still work; whatever was spawned
since is gone and whatever was destroyed since is back. Both are answered
at the end of the physics frame, which holds the world and the physics
together, so a load lands before the next scripts run and the world plays
on from it as it did the first time. What is **not** rewound: a script's
own variables (keep what must be in a custom component, which is - the
showcase keeps its run's clock that way), runtime state a component never
saves (a trail's points, a particle's age), music, and whatever lives
outside the world. States live in memory for the session; a scene change
does not clear them. Engine Test Lab scene 255; the showcase's Quicksave
Workshop.

---

## Text along a path

```json
"text": { "text": "WONDERLAND", "align": "center" },
"text_path": { "path": "Arch", "offset": 240.0, "side": 0.0, "scroll": 0.0, "spacing": 6.0, "repeat": false }
```

```lua
TextPath.set(marquee, "scroll", 60)   -- any field by name
TextPath.set(label, "offset", 120)
```

A `text_path` (Oct 2026) lays an entity's world `text` along a curve: the
`vector_path` of the entity named `path` (bezier and all, flattened as a
`path_follow` flattens it), or its `line` - in world space, so a path that
moves, swings or is bent by a script every frame carries its letters. Each
letter is laid out as text always is, then its place along the baseline
becomes a distance along the curve (`offset` is where the text starts, so
with `"align": "center"` it is where the middle sits) and it stands on the
curve turned to its tangent, `side` off it - to the **left** of the path's
direction, which is up for a path drawn left to right and **outward for one
drawn clockwise**, so text round a ring reads upright on top when the ring
runs clockwise. `scroll` moves `offset` on by itself (a marquee); on a closed
path, or with `repeat`, the text wraps round, so keep it shorter than the
path. `spacing` adds room between letters. Drawn as one mesh of the font's
atlas - a distance-field font stays sharp at every turn - in the sprite pass
at the text's `z_order`; outlines, shadows and rich text are not bent.
Engine Test Lab scene 256; the showcase's Carnival Banners.

---

## Beat clock

```json
"beat_clock": { "bpm": 124.0, "offset": 0.0, "beats_per_bar": 4, "follow_music": true, "running": true }
```

```lua
function on_beat(entity) ... end   -- on every beat
function on_bar(entity) ... end    -- on the first beat of every bar (after on_beat)

local phase = Entity.get_custom(e, "BeatClock", "phase")   -- 0 on the beat, toward 1
local pulse = (1 - phase) ^ 2                               -- a kick's shape
BeatClock.set(e, "bpm", 140)                                -- any field by name
```

A `beat_clock` (Oct 2026) turns the music's tempo into `on_beat` and
`on_bar` calls on its entity's own script. It is timed by the **music's own
playhead** (`get_music_position()`), which every host publishes a frame late
and in buffer steps, so the clock runs on `dt` and is eased toward each fresh
reading, snapped to it when it is more than 60 ms off - a clock summed from
`dt` alone slides off the song within a minute. `offset` is where the first
downbeat falls in the track, in seconds. A looping track's playhead jumps
back to its start and the beat with it, so a loop made of whole bars
restarts on a downbeat. With no music playing, or `follow_music` false, it is
a metronome on `dt`; `running` false holds it. Between the calls, the custom
`BeatClock` fields say where it is: `beat` (of the song, from 0), `bar`,
`beat_in_bar`, `phase` (0 to 1 through the beat), `count` (beats fired) and
`time` - ease anything that pulses from `phase`, so a pulse starts on the
beat however the frames fall. Setting `bpm` or `offset` keeps the clock's
time. Engine Test Lab scene 259; the showcase's Beat Hall.

---

## Things that part as you pass

```json
"displacement_field": { "size": [1500, 820], "cell": 12, "recover": 0.9 }
"displacer":          { "radius": 80, "strength": 1.0 }
"field_bend":         { "max_angle": 0.95, "squash": 0.25, "stiffness": 55, "damping": 5.5, "wind": 0.07, "wind_speed": 1.4 }
```

Grass that parts round whoever walks through it (Oct 2026). A
`displacement_field` is a grid of pushes laid over an area (`size`, centred
on its entity, `cell` world units a cell). A `displacer` - a walker, a cart,
the mouse - presses the cells within `radius` of it outward from its centre,
hardest close in; each cell keeps the strongest push it has had and lets it
go to half over `recover` seconds, so a path stays parted for a moment
behind whoever made it. A `field_bend` sprite reads the push where it stands
and leans away from it about its anchor (anchor a blade at its foot), up to
`max_angle` radians, a little shorter under it (`squash`), on a damped spring
(`stiffness`, `damping`) - so it sways back past upright and settles - and
with `wind` it sways on its own, out of step with its neighbours. A
displacer or bender with a `field` name keeps to that field. It all runs in
the motion step and writes the transforms, so it behaves the same in every
runtime; a script that also turns a bender is overwritten. Engine Test Lab
scene 261; the showcase's Meadow Trails.

---

## Smooth terrain

```json
"smooth_terrain": { "cols": 80, "rows": 45, "cell": 18, "iso": 0.5, "seed": 135, "noise_scale": 330, "fill": 0.58,
                    "border": true, "fill_texture": "sprites/rock.png", "tile": 10,
                    "edge_texture": "sprites/moss.png", "edge_width": 14, "collider": true, "friction": 0.6 }
```

```lua
Terrain.edit(e, x, y, radius, amount)   -- dig (amount > 0) or build (< 0) round a world point
```

Ground drawn and collided as the contour of a density grid (Oct 2026;
marching squares), so its walls are slopes rather than stairs. The grid has a
value at every corner of `cols` x `rows` cells of `cell` world units, centred
on the entity; a corner at or above `iso` is solid, and the contour between
corners is interpolated, so it is smooth. Without an authored `density` (a
list of `(cols + 1) * (rows + 1)` values, row by row from the bottom) it is
generated from fractal noise - `seed`, `noise_scale` (how wide its features
are), `fill` (more is more solid) - with a solid `border`. `Terrain.edit`
moves the corners within `radius` toward empty or solid by `amount`, fully at
the middle; call it each frame with `amount * dt` for a steady dig. It is
drawn as a `fill_texture` tiled every `tile` cells and an `edge_texture`
strip `edge_width` deep inside the contour (its top row on the surface),
sorted by `z_order` with the sprites, and with `collider` it is a fixed body
- boxes for full cells, a convex piece per cut cell - whose colliders are
swapped when it changes, so what rested on dug ground falls. The density is
saved with the scene. Engine Test Lab scene 262; the showcase's Smooth Caves.

---

## Fog of war

```json
"fog_of_war": { "size": [1440, 810], "cell": 12, "unseen_color": [0.0, 0.0, 0.01, 0.96],
                "explored_alpha": 0.6, "reveal_speed": 4, "max_rate": 30 },
"sprite": { "texture_id": "", "size": [1440, 810], "z_order": 20 }

"sight": { "radius": 150, "soft": 50 }          -- on a scout, a tower: what it reveals
"fog_hidden": { "threshold": 0.35 }             -- on an enemy: drawn only while seen
```

```lua
Entity.get_custom(enemy, "Fog", "visible")        -- true while a sight is on it
Entity.get_custom(fog, "FogOfWar", "explored")    -- the share of the map ever seen, 0 to 1
```

A map dark but for what is seen (Oct 2026). A `fog_of_war` is a grid of
`cell`-sized cells over `size` world units, centred on its entity; every
`sight` reveals the cells within its `radius` (fading over its last `soft`
units), each cell's visible share easing toward it at `reveal_speed` a
second and back as the sight leaves - and what was once seen stays
**explored**. The entity's own `sprite` shows it: give it one over the area,
above the map, and the engine points it at a texture of one texel a cell
(`__fog/...`, smoothed), dark `unseen_color` where nothing has been and
dimmed by `explored_alpha` where something was but is not now, remade at
most `max_rate` times a second. A `fog_hidden` entity is **drawn by no
pass** while its cell is seen no more than `threshold` - it still moves,
collides and runs its script. Nothing needs a script; the custom fields
above are written each frame for one that wants them. Engine Test Lab scene
263; the showcase's Fog of War.

---

## Dialogue

```json
"dialogue": { "table": "dialogue/baker", "start": "start", "autostart": false, "vars": "Village",
              "text": "Line", "speaker": "Speaker", "portrait": "Portrait",
              "choices": ["Choice1", "Choice2", "Choice3"], "number_choices": true, "reveal_speed": 40 }
```

```lua
Dialogue.start(e, node)     -- play from `node` (the dialogue's `start` when nil)
Dialogue.advance(e)         -- finish the line still typing, else go on to the next
Dialogue.choose(e, n)       -- take choice n (from 1) of the line on show
Dialogue.stop(e)            -- end it now
Entity.get_custom(e, "Dialogue", "playing")   -- also node, speaker, line, choices (how many), typing
Entity.get_custom(v, "DialogueVars", "gold")  -- the variables, on the `vars` entity
```

A conversation is a **data table** under `dialogue/` (Oct 2026), so it is
delivered and edited wherever data tables are. A row is a node:

| field | |
|---|---|
| `speaker`, `portrait`, `text` | a line - `text` is rich text, `{var}` filled in from the variables |
| `next` | the node after it; none ends the conversation |
| `choices` | `[{ "text", "next", "if", "set" }]` - only those whose `if` holds are shown |
| `if`, `else` | a node whose condition fails is passed over to `else` |
| `set` | `{ "var": value }` on entering; `"+1"` / `"-3"` add to a number |
| `event` | calls `on_<event>(entity)` on the runner's script |

A row with no `text` is a step of logic: its `set` and `event` happen and it
goes on to `next` at once. A condition is terms joined by `and` / `or`
(`and` binding tighter), each `var`, `!var` or `var <op> value` with `>=`,
`<=`, `>`, `<`, `==`, `!=`.

The `dialogue` component plays it into the scene's own entities, named: the
line into `text` (a `text` or `ui_text`, its typewriter restarted at
`reveal_speed` - make it `rich` for the markup), the speaker's name into
`speaker`, the portrait into `portrait` (a `sprite` or `ui_image`, hidden
when a line has none) and each choice into one of `choices` (blank when
there are fewer). Variables live in the `DialogueVars` custom component of
the entity named by `vars` (the runner itself when empty), so several
runners can share them. `on_dialogue_line(entity)` and
`on_dialogue_end(entity)` are called on the runner's script. Engine Test Lab
scene 264; the showcase's Village Chatter.

---

## UI drag and drop

```json
"ui_drop_slot": { "accepts": ["weapon"], "accept_color": [0.6, 1, 0.6, 1], "refuse_color": [1, 0.45, 0.4, 1] }
"ui_draggable": { "tag": "potion", "stack": "potion_red", "count": 3, "max_stack": 5, "slot": "A2",
                  "ghost_alpha": 0.85, "held_alpha": 0.35 }
```

```lua
UI.drop(item, "WeaponSlot")                 -- put it there, by the rules a drop follows
Entity.get_custom(item, "Drag", "slot")     -- also count, held
function on_ui_drop(entity) end             -- on the item's own script, after it lands
```

Entity-UI items picked up and put down in slots (Oct 2026). A `ui_drop_slot`
is a place, named by its entity; it takes an item whose `tag` is in its
`accepts` (any, when that is empty). A `ui_draggable` is an item - its
`ui_image` the picture, its `ui_text` (if it has one) the count, shown above
one - kept centred in the slot its `slot` names.

With the pointer: press on an item and a **ghost** of it follows the pointer
while the item dims where it was; the slot under the pointer is tinted
`accept_color` or `refuse_color`; let go over a slot that takes it and it
moves there. A slot holding an item **stacks** the two when they share a
non-empty `stack` key (up to `max_stack`; an item emptied that way is hidden),
else **swaps** them when the item's old slot takes the other, else refuses.
Let go anywhere else, or press the right button, B or Backspace, and it goes
home. With a controller or the keyboard the same goes through focus: confirm
(A, Enter, Space) on a focused slot picks its item up, the ghost sits over
whichever slot has focus, and confirm again puts it down - so make the slots
`focusable`. Engine Test Lab scene 265; the showcase's Loot Satchel.

---

## Sprite stacking

```json
"sprite_stack": { "texture_id": "sprites/house.png", "slices": 21, "size": [96, 96], "spacing": 3,
                  "fill": 3, "color": [1, 1, 1, 1], "z_order": 0, "y_sort": true, "spin": 0 }
```

Pseudo-3D from slices of a model (Oct 2026). The texture is a strip of
`slices` equal pictures, left to right, the model cut horizontally from its
bottom (the first) to its top. Each slice is drawn `size` world units across,
turned by the entity's rotation, and raised `spacing` world units **up the
screen** from the one below - so a model turns in place like a toy on a
turntable, and when the camera turns (`Transform.set_rotation(camera, a)`)
the slices still rise up the screen: the town keeps standing up while it
swings round under the view. Make `spacing` one texel in world units (`size`
over the slice's pixels) and `fill` a few, which draws copies of each slice
up to the next so nothing shows between them. `spin` turns it on its own,
radians a second.

Stacks sort among themselves by where their foot is **on the screen**
(`y_sort`), which is what a turning camera needs - a sprite's `y_sort` sorts
by world y. So in a scene whose camera turns, make everything that stands up
a stack, and the ground a lower `z_order`. Engine Test Lab scene 266; the
showcase's Toy Town.

---

## Mode 7

```json
"mode7": { "texture_id": "sprites/track.png", "ground_size": [2048, 2048], "x": 0, "y": 0, "angle": 0,
           "height": 34, "fov": 1.25, "horizon": 0.55, "far": 1800, "size": [1280, 720],
           "near_color": [1, 1, 1, 1], "far_color": [0.7, 0.78, 1, 1],
           "follow": "Player", "follow_distance": 115, "z_order": -50 }
"mode7_sprite": { "plane": "", "x": 120, "y": -40, "angle": 1.57, "frames": 8 }
```

```lua
Mode7.place(e, x, y, angle)        -- stand a sprite (or a plane's camera) at a ground point
Mode7.set(e, field, value)         -- any field of the entity's mode7_sprite, else its mode7
```

A ground plane seen in perspective, and sprites standing on it scaled by
their distance - the SNES racing view (Oct 2026). A `mode7` is a screen
rectangle (`size`, centred on its entity) showing the ground texture
(`texture_id`, covering `ground_size` ground units about the ground origin,
+y up the image) from a camera at ground point (`x`, `y`) facing `angle`
(radians, 0 along +x), `height` above it with a horizontal field of view
`fov`; the horizon is `horizon` of the way up the rectangle and the ground is
drawn out to `far`, coloured from `near_color` to `far_color`. Samplers
clamp, so give the ground texture a border of grass (or sea) and it reads as
endless. With `follow` naming an entity that has a `mode7_sprite`, the
camera sits `follow_distance` behind it, facing its way. Draw the sky as an
ordinary sprite behind the plane, reaching a little below the horizon.

A `mode7_sprite` stands on the plane named by `plane` (the first, when
empty): its `transform` is written each frame to where its ground point falls
on the screen and its scale to how big a ground unit is there, so give its
`sprite` its size in ground units, `anchor` it at its feet and `y_sort` it -
nearer is lower on the screen, so it draws in front. With `frames` its
texture is a strip of views of it, frame 0 seen from behind and going round
anticlockwise, picked by its `angle` against the camera's. Behind the camera
or past `far` it is hidden. Engine Test Lab scene 267; the showcase's Kart
Circuit.

---

## Palettes

```json
"post_process": { "effects": [{ "name": "palette", "enabled": true, "params": {
    "preset": { "Float": 1 }, "dither": { "Float": 0.6 }, "cell": { "Float": 2 },
    "cycle_start": { "Float": 12 }, "cycle_end": { "Float": 16 }, "cycle_speed": { "Float": 6 },
    "count": { "Float": 0 }, "c0": { "Vec4": [0.1, 0.06, 0.16, 1] } } }] }
```

```lua
PostProcess.set_float("palette", "preset", 0)          -- the Game Boy
PostProcess.set_float("palette", "count", 4)           -- use c0..c3 instead
PostProcess.set_vec4("palette", "c0", r, g, b, 1)      -- a colour of its own, sRGB 0..1
```

The frame quantised to a handful of colours (Oct 2026). The `palette` effect
runs last in the output pass - after the tonemap, the grade and the grain,
before nothing - so what reaches the display is exactly the palette's
colours; the UI is drawn after it and stays as it is. Each pixel is taken to
sRGB, nudged by a 4x4 ordered (Bayer) threshold in cells of `cell` screen
pixels scaled by `dither` (0 is none), and replaced by the nearest colour.
`preset` picks a built-in palette - 0 Game Boy, 1 PICO-8, 2 CGA, 3 Sweetie
16, 4 EGA - unless `count` is above 0, which uses the colours `c0` ..
`c<count-1>` (sRGB, at most sixteen). With `cycle_end` above `cycle_start`
the colours at those indices rotate `cycle_speed` steps a second, the old way
water and fire moved: paint the things that should move in colours only they
use. Engine Test Lab scene 268; the showcase's Pocket Palette.

---

## Paint and halftone

```json
{ "name": "paint", "enabled": true, "params": { "radius": { "Float": 6 }, "strength": { "Float": 1 } } }
{ "name": "halftone", "enabled": true, "params": { "mode": { "Float": 1 }, "cell": { "Float": 7 }, "strength": { "Float": 0.85 },
  "ink": { "Vec4": [0.02, 0.02, 0.04, 1] }, "paper": { "Vec4": [1, 0.97, 0.9, 1] } } }
```

Two looks for the whole frame (Oct 2026), both in the post chain's output
pass, on the tonemapped colour - the frame the display shows - and under
the UI, which stays sharp. **`paint`** is a Kuwahara filter: each pixel
takes the mean colour of the calmest of the four squares of `radius` texels
meeting at it, so flat areas become brush strokes and edges stay sharp
(`strength` mixes it with the frame). **`halftone`** prints the frame as
dots on a turned grid of `cell` pixels, each dot sized by the colour at its
cell's centre: `mode` 0 is one screen of `ink` on `paper` at `angle`
degrees (newsprint), 1 is cyan, magenta, yellow and black screens at 15, 75,
0 and 45 degrees with black in `ink` (a comic book). Dark dots merge into
each other, as printed ones do. Halftone reads the frame before `paint`,
so with both on the dots follow the unpainted picture.

```lua
PostProcess.set_enabled("halftone", true)
PostProcess.set_float("halftone", "mode", 0)
PostProcess.set_vec4("halftone", "paper", 0.8, 0.74, 0.6, 1)
```

Engine Test Lab scene 272; the showcase's Oil and Ink.

---

## Line boil

```json
{ "name": "boil", "enabled": true, "params": { "amount": { "Float": 1.6 }, "scale": { "Float": 22 }, "rate": { "Float": 8 } } }
```

Lines that wobble as if drawn again a few times a second (Oct 2026), the
look of a hand-drawn cartoon. A post effect in the output pass: the whole
frame is read through a smooth noise offset of up to `amount` pixels, its
pattern `scale` pixels across, drawn afresh `rate` times a second - so it
steps from drawing to drawing rather than swimming. Every sprite, line and
world text edge boils; flat areas barely change; the UI, drawn after the
post chain, stays still. A `seed` parameter fixes the pattern (a test, or a
script that wants to choose when it changes).

```lua
PostProcess.set_float("boil", "rate", 14)
```

Engine Test Lab scene 278; the showcase's Sketchbook.

---

## Lens rain

```json
{ "name": "lens_rain", "enabled": true, "params": { "amount": { "Float": 0.6 }, "size": { "Float": 11 }, "speed": { "Float": 1 },
  "mist": { "Float": 0.35 }, "refraction": { "Float": 1.4 } } }
```

Rain on the glass the scene is seen through (Oct 2026): beads that form
and dry, and drops that run down in wiggling columns leaving a trail of
beads, each bending the picture behind it, with a glint on the side facing
up and left. A post effect in the output pass, worked out from the clock
with nothing stored, so it costs the same however long it has rained.
`amount` is how wet (0 is off), `size` a drop's radius in pixels, `speed`
how fast they run, `refraction` how strongly they bend (0 leaves the
picture alone). `mist` steams the glass - a soft blur, lifted a little -
except where drops sit or a running drop has just wiped it. A fixed `time`
parameter stops the clock (a test, a script's freeze frame). The UI, after
the post chain, stays dry.

```lua
PostProcess.set_float("lens_rain", "amount", 1.0)   -- a downpour
```

Engine Test Lab scene 280; the showcase's Rainy Window.

---

## Speech bubbles

```json
"speech_bubble": { "max_width": 250, "font_size": 18, "padding": 13, "offset": [0, 62], "duration": 3.6,
                   "texture": "sprites/bubble.png", "border": 18, "tail_texture": "sprites/tail.png", "tail": [18, 16],
                   "color": [1, 1, 1, 1], "text_color": [0.1, 0.09, 0.14, 1], "reveal_speed": 45, "z_order": 60 }
```

What a character says, in a bubble over its head (Oct 2026). Put a
`speech_bubble` on the speaker and call `Bubble.say(e, text, seconds)`; the
engine wraps the words to `max_width` UI pixels, sizes the bubble to them
(plus `padding`), and points its tail at `offset` world units from the
speaker - from the bubble's bottom edge, `tail` (`[width, least length]`)
pixels at least. The bubble is entity UI, kept on screen, and **kept clear
of the others**: the lowest speaker's bubble is placed first and each above
is pushed up out of the way, so a crowd talking at once stacks rather than
overlaps, and every tail still reaches its own speaker. `texture` is
nine-sliced with `border` pixels kept at its edges (empty is a plain
rectangle), `tail_texture` has its point at the bottom (empty is a stem),
both tinted `color`. `reveal_speed` types the words out, and saying a line
again types it again. `seconds` 0 keeps the bubble until the words change;
left out, the bubble's own `duration`. A speaker with no `speech_bubble`
gets one with the default look. Empty words say nothing.

```lua
Bubble.say(baker, "Fresh bread, warm from the oven!", 3)
Bubble.say(guard, "Halt!")                     -- for the bubble's own duration
Bubble.set(baker, "reveal_speed", 0)           -- any field by name
```

Engine Test Lab scene 281; the showcase's Talk of the Town.

---

## Sprite outlines

```json
"sprite_outline": { "enabled": true, "color": [1.0, 0.8, 0.25, 1.0], "width": 2, "glow": 6, "pulse": 0, "pulse_amount": 0.5 }
```

A glowing ring round a sprite's own shape (Oct 2026) - the unit you have
selected, the door you can open, the card under the pointer. The ring
follows the picture's opaque pixels, not its rectangle: solid out to
`width` world units, fading to nothing over `glow` past that, in `color`,
and round any holes in the shape too. `pulse` makes it breathe that many
times a second, fading by `pulse_amount` at the trough. It is drawn just
behind the sprite and unlit, as a glow is, and follows a carved or painted
sprite's current shape; a sprite showing one cell of a sheet is ringed by
that cell alone, never by its neighbours. `enabled` false draws nothing and
costs nothing - author it off and turn it on to select. Plain sprites only:
a jelly, a tilted sprite, or a nine-sliced or tiled fill draws no outline.

```lua
Outline.set(unit, "enabled", true)
Outline.set(unit, "color", { 1.0, 0.25, 0.2, 1.0 })   -- an enemy under the pointer
Outline.set(unit, "pulse", 1.5)
```

Engine Test Lab scene 282; the showcase's Unit Select.

---

## Lightning

```json
"lightning": { "start": [0, 0], "end": [0, -300], "spread": 400, "interval": [1.5, 4.0],
               "jag": 0.22, "detail": 6, "branches": 4, "branch_length": 0.35, "width": 3, "glow": 6,
               "color": [1, 1, 1, 1], "glow_color": [0.55, 0.65, 1.0, 0.6], "duration": 0.45, "restrikes": 2,
               "light_color": [0.75, 0.82, 1.0], "light_intensity": 1.5, "light_radius": 600, "flash": 0.5, "z_order": 20 }
```

Branching lightning, a new bolt every strike (Oct 2026) - a storm, chain
lightning, a tesla coil. A bolt leaves from the entity (plus `start`) and
lands where `Lightning.strike(e, x, y)` says, or, while `interval` is not
`[0, 0]`, by itself every so many seconds within `spread` of `end` along x.
Its shape is grown from a seed - the line halved `detail` times, each
middle pushed sideways by up to `jag` of its segment, with `branches` forks
reaching `branch_length` of it - so no two strikes match, and every runtime
draws the same one. It lasts `duration` seconds, flickering back
`restrikes` times as it fades, drawn as a soft core (`width`, `color`) in a
glow (`glow` times as wide, `glow_color`), additive and unlit. While it
shows it lights the world: shadowless point lights along it
(`light_intensity`, `light_radius`, `light_color`) and `flash` added to the
ambient light, all scaled by how bright it is - so in a dark lit scene the
sea and the cloud appear for an instant. Each strike calls the entity's own
script's `on_strike(entity)`, and its custom `Lightning` fields say how many
there have been and where the last landed (`strikes`, `x`, `y`).

```lua
function on_strike(cloud)
    thunder_at = t + 0.5    -- the thunder, late by the distance
end
Lightning.strike(cloud, x, -40)                  -- now, landing there
Lightning.set(cloud, "interval", { 0, 0 })        -- only when told
```

Engine Test Lab scene 283; the showcase's Thunderhead.

---

## Flow along a path

```json
"flow_path": { "path": "Race", "width": 96, "speed": 110, "bank_speed": 0.3, "grip": 2.5, "tag": "floats",
               "texture": "sprites/water.png", "tile": 160, "color": [1, 1, 1, 1], "soft": 0.14, "z_order": -55 }
```

Water running along a drawn curve (Oct 2026) - a millrace, a river, a
lava channel, a moving walkway. `path` names an entity carrying a
`vector_path` (which can draw nothing itself); the water is a ribbon
`width` wide along it, its `texture` repeated every `tile` world units down
the curve and **running downstream** at `speed`, round every bend, its edges
fading over `soft` of the half-width. Author the texture with its pattern
running down it (v) and seamless top to bottom. Anything with a `velocity`
inside the channel - only things tagged `tag`, when it is set - is eased
towards the water's velocity by `grip` a second: along the curve, `speed`
mid-stream and `bank_speed` of it at the banks. A dynamic body is carried
too, through its velocity. Several flows on one path layer: a wide still
one (`speed` 0, `grip` 0, no texture) is a bank, a narrower faster one with
a soft texture is foam. Give floating things `face_velocity` and they turn
the way they drift. Water turns into a bend half its width before it, but a
square corner still throws a float over the bank - draw bends as curves.

```lua
Flow.set(water, "speed", 200)       -- open the sluice
Flow.set(water, "speed", -120)      -- run it backwards
```

Engine Test Lab scene 284; the showcase's Millrace.

---

## Snow cover

```json
"snow_cover": { "depth": 6, "max_depth": 13, "rate": 0.6, "color": [0.93, 0.95, 1.0, 1.0],
                "slope_limit": 1.5, "bumps": 1.2, "cutoff": 0.5, "step": 0.5 }
```

Snow that settles on a sprite and melts again (Oct 2026) - roofs, sills,
fence rails, the tops of boughs. The engine finds every edge of the
sprite's own picture that faces up (a texel whose alpha passes `cutoff`
with clear sky above it) and lays snow `depth` texels deep on it: less
where the surface is steep (none at `slope_limit` texels of rise a texel
across), thinner off the end of a ledge, its top lumpy by `bumps` and
shaded a little at its foot. Walls hold none. `rate` adds to the depth each
second up to `max_depth`; a negative rate melts it. It is drawn over the
sprite, lit as the sprite is, and follows the picture it is given - a
sheet's cell, a carved or painted sprite. The snow lies in the picture's
**clear texels above** each surface, so leave headroom over the art's top
edges. The picture is remade only when the depth has moved by `step`
texels.

```lua
Snow.set(roof, "rate", -3)      -- a thaw
Snow.set(roof, "depth", 0)      -- or gone at once
```

Engine Test Lab scene 285; the showcase's First Snow.

---

## Navigation links

```json
"nav_link": { "from": [0, 0], "to": [60, 0], "kind": "jump", "duration": 0.5, "one_way": false, "cost": 1, "enabled": true }
```

A way across that is not walkable ground (Oct 2026) - a jump over an
alley, a ladder, a plank, a zipline, a door that is sometimes open. It runs
from its entity (plus `from`) to the world point `to`, and may join two
points of one `nav_region` or two regions - which is how an agent gets from
one region to another at all. Agents plan across links as they plan round
obstacles: a route may walk to a link, cross it and walk on, and of several
ways the cheapest wins - a link counts its length times `cost`, so a
zipline worth taking even where walking is possible has a `cost` under 1,
and a risky jump one over it. `one_way` links are only crossed from `from`
to `to`; `enabled` false takes a link out of every plan (`NavLink.set(e,
"enabled", false)` - a drawbridge raised). An agent crosses in a straight
line over `duration` seconds (0: at its walking speed), never touching the
ground between; while it does, its custom `NavCrossing` fields say what it
is crossing (`kind`) and how far (`t`, 0 to 1) - `kind` is empty
otherwise - and its own script's `on_nav_link(entity)` is called as it sets
off. With no way across, an agent walks as far as its own region lets it,
as it always has. `Nav.is_moving` is true while crossing.

```lua
local kind = Entity.get_custom(runner, "NavCrossing", "kind")
if kind == "jump" then
    local up = math.sin(math.pi * Entity.get_custom(runner, "NavCrossing", "t"))
    Transform.set_scale(runner, 1 + 0.4 * up, 1 + 0.4 * up)
end
```

Engine Test Lab scene 286; the showcase's Rooftop Runners.

---

## Path morphing

```json
"path_morph": { "target": "", "duration": 1.1, "points": 160, "ease": "back", "progress": 1 }
```

One drawn shape turning into another (Oct 2026) - a logo becoming a bird,
a blob becoming a star. On an entity with a `vector_path`,
`Morph.to(e, name, seconds)` eases its path into the shape of the
`vector_path` on the entity called `name`, over `seconds` (left out, the
morph's `duration`), its fill and stroke colours too. Both shapes are cut
into `points` points evenly spaced along their outline, and the target's
are turned round (and reversed if it runs the other way) to start where it
best lines up with the shape now, so a morph never twists or turns inside
out. Shapes are read in each entity's own frame: the target may be drawn
anywhere, at any size - a row of small ones works as a palette of shapes to
become. `ease` is `in_out`, `linear`, `out` or `back` (overshoots and
settles). The entity's own script's `on_morph_end(entity)` is called as a
morph finishes. A morphed path is written as straight segments, enough
`points` of them to read as curves.

```lua
Morph.to(logo, "Bird", 1.2)
Morph.set(logo, "ease", "back")
```

Engine Test Lab scene 287; the showcase's Shape Shifter.

---

## Toasts

```json
"toast_stack": { "corner": "top_right", "margin": [24, 24], "size": [380, 88], "gap": 10, "max_shown": 3,
                 "seconds": 4.5, "slide": 0.4, "texture": "sprites/card.png", "border": 18,
                 "color": [1, 1, 1, 1], "title_color": [1, 0.85, 0.45, 1], "text_color": [0.95, 0.92, 0.85, 1],
                 "title_size": 21, "text_size": 16, "icon_size": 56, "padding": 16, "z_order": 220,
                 "icons": ["sprites/icon_cup.png"] }
```

Short notices that slide in from a corner of the screen (Oct 2026) - an
achievement, an item found, a friend online. A `toast_stack` on any entity
owns one `corner` (`top_right`, `top_left`, `bottom_right`, `bottom_left`);
`Toast.show(stack, title, text, icon, seconds)` queues a toast (`seconds` 0
or left out: the stack's own). At most `max_shown` are on screen, the
newest nearest the corner and the older ones easing along to make room,
the rest waiting their turn; each slides in from beyond the screen's edge
over `slide` seconds, stays its seconds and slides out the way it came. A
toast is a card `size` UI pixels - nine-sliced from `texture` with
`border` pixels kept, or a plain `color` card without one - with its icon
on the left and its title over its text, which wraps. **An icon is named
only when a toast is shown, too late to load a file then**, so list every
icon a stack may show in its `icons`. Timed on the wall clock, so a
paused game still shows its toasts. The stack's own script's
`on_toast(entity)` is called as each appears.

```lua
Toast.show(achievements, "Unlocked: Champion", "Won the summer tournament.", "sprites/icon_cup.png")
Toast.set(achievements, "corner", "bottom_right")
```

Engine Test Lab scene 288; the showcase's Trophy Room.

---

## Input replays

```lua
if Replay.live_key("R") then Replay.record(); Scene.load(SCENE) end   -- record a run from a fresh scene
if Replay.live_key("P") then Replay.play(); Scene.load(SCENE) end     -- play it back exactly
local status = Replay.status()   -- idle, armed, recording, playing or finished
```

A run recorded and played back exactly as it happened (Oct 2026) - a
pinball game, a speedrun, a bug a tester can hand over. What makes a run
is its starting world, its input and its frames' time, so a replay keeps
the last two and starts again from the first: `Replay.record()` and
`Replay.play()` each **arm** the replay, and the **next scene load**
starts it, so recording and playback both begin on a freshly built world -
load the scene yourself after arming. Each frame's input (keys, mouse,
wheel, typed text, pads, fingers, gestures, action timing) and frame time
are kept while recording and handed back while playing, in place of the
live ones; the physics steps on the frame times, so it goes the same way.
`Replay.stop()` ends either (a recording is kept; stopping playback hands
the game back to the player where it is), `Replay.clear()` forgets the
recording, a scene load ends one under way. `Replay.frames()` is how many
frames were kept, `Replay.progress()` how far playback has got (0 to 1).

**While a replay plays, a script reads the replayed input** - including the
key it would read to stop the replay. `Replay.live_key(name)` answers
whether the *player* pressed a key this frame, replay or not: read the keys
that steer the replay through it, and they never become part of what is
replayed.

What a replay cannot reach: anything timed on the wall clock rather than
the frame (toasts, speech bubbles, wet paint), and `math.random` unless the
script seeds it. The recording lives in memory for the session.

Engine Test Lab scene 289; the showcase's Instant Replay.

---

## Scene transitions

```lua
Scene.transition("scenes/act_2.scene", "iris", 1.1)
Scene.transition("scenes/act_1.scene", "dissolve", 1.2, "masks/folds.png")
```

Load a scene with the outgoing frame kept and wiped away over the new one
(Oct 2026). `style` is `fade`, `iris` (a circle opening from the middle),
`diamond` (diamonds opening left to right), `slide` (the old frame slides
off to the left), `wipe` (a soft edge sweeping left to right) or `dissolve`
(the old frame burns away where `texture` is darkest first - its red,
stretched over the screen - or through noise with no texture); `seconds`
is how long it takes (0.6 by default). The frame kept is exactly what was on
screen, UI included. Otherwise it is `Scene.load`: the scene loads by the
same path in every runtime.

A dissolve's texture must already be loaded when the transition starts -
name it from something in the outgoing scene (an invisible sprite does) -
and keep it **out of `sprites/`**, which a build packs into atlases. A
platform whose screen cannot be copied from changes the scene with no
transition. Engine Test Lab scene 273; the showcase's Curtain Call.

---

## Slicing

```json
"sliceable": { "inside_texture": "sprites/melon_inside.png", "separation": 140, "min_area": 120, "generations": 1, "piece_lifetime": 3 }
```

A body a blade can cut in two (Oct 2026). `Physics.slice(x0, y0, x1, y1)`
cuts every `sliceable` the segment crosses along its line: the body's
convex outline (a box, triangle, convex polygon or circle) becomes two
halves, each a dynamic body of its own with that half for a collider, drawn
as a textured mesh cut from the same picture - or from `inside_texture`,
laid over it the same way, to show what is inside. The halves keep the
body's velocity plus `separation` (units / s) away from the cut, its
material and occluder, and are named `<name>/a` and `<name>/b`. A half
smaller than `min_area` is not cut off (a graze at the edge cuts nothing);
`generations` is how many more times a half may be cut; `piece_lifetime`
removes the halves after that many seconds.

```lua
-- each frame the blade moved: cut along the stretch it moved
Physics.slice(last_x, last_y, x, y)
```

Engine Test Lab scene 274; the showcase's Fruit Slicer.

---

## Portals

```json
"portal": { "pair": "Orange", "width": 150, "depth": 150, "z_order": 11, "carry": true }
```

A linked pair of lines, each showing the view out of the other and
carrying what goes in out of the other (Oct 2026). A portal lies along its
entity's local x, `width` long, and faces its local +y, like a `mirror`;
`pair` names the partner entity. What is `depth` in front of the partner is
drawn over the region as deep **behind** this portal, turned half round -
the partner's left on this portal's right, as walking through would have
it - at `z_order`, tinted by `tint`. Put the window over something that
reads as a hole (a gap in a floor, a dark backing).

With `carry` (the default) anything that moves - a physics body that is not
static, or a sprite with a `velocity` - whose middle crosses the line from
the front, within the width, comes out just in front of the partner at the
same place turned, its velocity and rotation turned by the angle between the
two. A body needs a gap in whatever collider lies behind the portal, or it
lands on the floor before its middle reaches the line.

```lua
Portal.set(orange, "enabled", false)   -- close it
Portal.set(blue, "carry", false)       -- a window only
```

Engine Test Lab scene 275; the showcase's Portal Lab.

---

## Jelly

```json
"jelly": { "cols": 6, "rows": 6, "stiffness": 90, "damping": 4, "coupling": 400, "response": 0.55, "max_stretch": 0.35 }
```

A sprite drawn as a grid of points on springs (Oct 2026) - a block that
squashes when it lands, wobbles as it flies and quivers when poked. The
sprite's quad is cut into `cols` x `rows` cells; each point is pulled back
to its place by `stiffness`, slowed by `damping`, and pulled toward its
neighbours by `coupling`, so a push at one corner runs across the whole.
A change in the entity's velocity in a frame - a landing, a bounce, a jump
- is felt by every point, the more the further it lies from the side the
push came from (`response` scales it): the side that hit stays put and the
far side carries on, and the sides bulge. No point moves more than
`max_stretch` of the sprite's size. The collider does not deform; the
sprite keeps its material and colour.

```lua
Jelly.poke(block, x, y, 500, 60)   -- push the points near (x, y) in
Jelly.set(block, "stiffness", 200) -- firmer
```

Engine Test Lab scene 276; the showcase's Jelly Bounce.

---

## Off-screen markers

```json
"offscreen_marker": { "target": "Beacon1", "margin": 56, "arrow_texture": "sprites/arrow.png", "icon_texture": "sprites/flame.png",
                      "icon_offset": [0, 90], "label": "Beacon1Label", "label_format": "{d} m", "units_per_meter": 40 }
```

A marker kept at the edge of the screen pointing at something out of sight,
that turns into an icon over it once it is in view (Oct 2026). Put it on an
entity of its own with a sprite. While its `target` (an entity name) is out
of the view less `margin`, the marker sits where the line from the view's
middle to the target leaves that inner rectangle, turned to point at it (its
picture pointing along +x), showing `arrow_texture`; in view, it sits at the
target plus `icon_offset`, upright, showing `icon_texture`. `label` names a
text entity kept just inside the arrow saying how far it is - `{d}` in
`label_format`, in metres of `units_per_meter`. Whether the target is in
view is the custom `OffscreenMarker.visible`. The view is the screen
camera's as last drawn, so an edge marker is one frame behind a moving
camera.

```lua
Marker.set(marker, "target", "NextBeacon")
Marker.set(marker, "enabled", false)      -- leave it where it is (and hide it)
if Entity.get_custom(marker, "OffscreenMarker", "visible") then ... end
```

Engine Test Lab scene 277; the showcase's Beacon Run.

---

## Radial menus

```json
"ui_radial_menu": { "sections": ["Fire", "Ice", "Wind", "Earth"], "radius": 128, "dead_zone": 40, "start": 90,
                    "hover_scale": 1.3, "hover_tint": [1, 1, 1, 1], "idle_tint": [0.62, 0.62, 0.7, 0.85] }
```

A wheel of sections chosen by direction from its middle (Oct 2026) - a
spell wheel, a weapon wheel, an emote ring. Put it on an entity-UI element;
its middle is the wheel's. `sections` names other entity-UI elements, laid
out round the wheel `radius` UI pixels out, the first at `start` degrees (90
is the top) and the rest clockwise. The section the pointer - or a pad's
left stick - points at is hovered: grown by `hover_scale` and tinted
`hover_tint`, the others `idle_tint`; nearer the middle than `dead_zone`
hovers nothing. A click, A, Enter or Space picks it: the wheel's custom
`RadialMenu` fields are written (`hovered` and `picked` as indices from 0,
or -1; `picks`, a count; `label`, the picked section's name) and its **own**
script's `on_radial_pick(entity)` is called. `open` false hides the wheel
and its sections. `aim` (degrees) points the wheel for the player - a
tutorial, or an AI choosing - until set to nil.

```lua
function on_radial_pick(wheel)
    cast(Entity.get_custom(wheel, "RadialMenu", "label"))
end
Radial.set(wheel, "open", false)
Radial.set(wheel, "aim", 0)     -- point at the section on the right
```

Engine Test Lab scene 279; the showcase's Spell Wheel.

---

## Silhouettes

```json
"silhouette": { "color": [0.45, 0.95, 1.0, 1.0], "outline": 3, "fill": 0.2 }
"silhouette_occluder": { "alpha_cutoff": 0.5 }
```

A thing hidden behind a wall drawn over the wall as a bright outline (Oct
2026) - the thief you can still follow through the manor. Put a
`silhouette` on the sprite that may be hidden and a `silhouette_occluder` on
the sprites that may hide it (walls, roofs, tree crowns). Wherever an
occluder sorts above the entity (a higher `z_order`) and their quads
overlap, the entity's sprite is drawn again over that occluder - one z
above it, unlit - in `color`, outlined `outline` texels wide and filled at
`fill`, and clipped to the occluder's own pixels whose alpha passes
`alpha_cutoff`. So the outline shows exactly where the wall covers the
entity, through a window it does not, and in the open there is none.
Engine Test Lab scene 269; the showcase's Behind the Walls.

---

## Sprite tilt

```json
"sprite_tilt": { "yaw": 0.3, "pitch": -0.1, "distance": 650, "back_texture": "sprites/card_back.png", "shine": 0.2, "speed": 10 }
```

A sprite turned about its own axes, in perspective (Oct 2026) - a playing
card leaning toward the pointer, a coin spinning on its edge, a card turned
over. `yaw` turns it about its up axis (positive swings its right side
away), `pitch` about its across axis (positive tips its top away), both in
radians about the sprite's middle, and `distance` is how far the eye is in
front of it, in the sprite's own units - smaller is stronger perspective.
The picture is foreshortened, not bent: every corner carries its
perspective weight, as a `mesh` does. Turned past a quarter either way it
shows its back: `back_texture` read the right way round, or with none the
front seen through from behind. `back_color` tints the back; `shine`
brightens the corners turned toward you and darkens those turned away.

`target` (`[yaw, pitch]`) eases the turn there at `speed` (a rate: 0
snaps), so a script says where a card should lean and gets the motion for
nothing:

```lua
SpriteTilt.set(card, "target", { 0.4, -0.2 })   -- lean toward the pointer
SpriteTilt.set(card, "target", { math.pi, 0 })  -- turn it over
SpriteTilt.set(coin, "yaw", t * 2.4)            -- or drive it directly
```

`SpriteTilt.set(e, field, value)` writes any field by name. A tilted sprite
keeps its material, colour and sorting; its `fill` is not applied. Engine
Test Lab scene 270; the showcase's Card Table.

---

## Afterimages

```json
"afterimage": { "interval": 0.03, "lifetime": 0.35, "color": [0.5, 0.4, 1.0, 0.8], "end_color": [0.2, 0.2, 1.0, 0.0], "min_speed": 700 }
```

Fading copies of a sprite left where it has been (Oct 2026) - a ninja's
dash, a speedster's blur, a ghost's trailing poses. Every `interval`
seconds while `emitting` the sprite leaves an echo of itself **as it was
drawn then** - its texture, its `uv_rect` (so an animation's pose is kept),
its turn, scale and flip - which fades from `color` to `end_color` over
`lifetime`, at most `max_echoes` at once. `min_speed` (world units a
second) leaves echoes only while it moves at least that fast, which is how
a dash trails and a walk does not. `flat` draws each echo as the sprite's
shape in `color` rather than the tinted picture; `blend_mode` `Additive`
makes the trail glow. Echoes draw just under the sprite at its z, age on
the entity's own clock (`time_scale`), and go with the entity.

```lua
Afterimage.set(ninja, "emitting", false)   -- stop; the echoes left fade out
```

Engine Test Lab scene 271; the showcase's Dash Echoes.

---

## Particles

```lua
Particles.emit_burst(emitter_entity, 50)
Particles.set_active(emitter_entity, true)
Particles.set_rate(emitter_entity, 100)  -- particles per second
Particles.set(emitter_entity, "angle", { -1.5, -1.3 })  -- any field by name

local total = Particles.get_count()
```

**Particles can meet the world** (Sep 2026). An emitter with `collision`
hits every non-sensor `collider` in the scene - the same shapes physics
uses, placed by their entity's transform - whether or not physics runs:

```json
"collision": { "response": "kill", "bounce": 0.4, "friction": 0.2, "mask": 65535 }
```

`response` is `"kill"` (the particle dies where it hits - rain) or
`"bounce"` (it comes off keeping `bounce` of its speed into the surface and
losing `friction` of its speed along it - sparks, hail). `mask` is the
collision layers it hits, against each collider's `collision_membership`.
The test is a particle's step against the colliders' outlines, so a
particle already inside a closed shape (a box, a circle, a polygon) leaves
it freely, and an open one (a segment, polyline or heightfield) is hit from
either side. Particles never enter the physics world, so a storm costs what
its particles do.

**Sub-emitters** burst another emitter where a particle dies or hits:

```json
"sub_emitters": [{ "on": "collision", "emitter": "Splash", "count": 3, "chance": 0.5 }]
```

`emitter` is the entity **name** of an ordinary emitter - usually one with
`emission_rate: 0` that does nothing else - and the burst is its particles:
its texture, speeds, angles and shape, about the point where it happened.
`on` is `"death"` (every death, a lifetime ending or a kill on collision) or
`"collision"` (every hit, bounce or kill); `chance` thins them out. The
showcase's Storm splashes its rain this way; Test Lab scene 134.

**`unlit` on an emitter's entity** (Oct 2026) draws its particles at full
brightness in a lit scene - snow on a moonlit night, embers, sparks - the
way it does for a sprite: they glow whatever the light, and a lit emitter's
particles in front of an unlit glow hide it. It used to do nothing, and only
additive emitters looked unlit. Test Lab scene 230.

**Trails** draw a ribbon behind each particle through where it has been
(Oct 2026; Godot's particle trails, Unity's Trails module):

```json
"trail": { "length": 10, "interval": 0.03, "width": 0.8, "taper": true, "fade": true }
```

A point is recorded every `interval` seconds and the last `length` (2 to 16)
are kept, so the trail follows the path the particle really took - gravity,
wind, turbulence and bounces included - and ends where it is. `width` is a
share of the particle's size at the head; `taper` narrows it and `fade`
thins it to nothing at the tail. The ribbon uses the emitter's texture
across its width (a soft dot gives soft edges) and is drawn under the
particles. `Particles.set(e, "trail", { ... })` sets one, `nil` removes it,
`"trail.length"` changes a field. GPU particles keep no history and have
none. Test Lab scene 180; the showcase's Firefly Glade.

A particle can play a **flipbook** - frames of a sprite sheet (Oct 2026;
Unity's Texture Sheet Animation, Godot's particle animation):

```json
"flipbook": { "columns": 8, "rows": 4, "mode": "life" }
```

The emitter's texture is cut into `columns` x `rows` frames, read left to
right and top to bottom; `frames` uses only the first so many (0 is all of
them). `mode` picks the frame each particle shows:

- `life` (the default) - frame 0 at birth to the last at death, `cycles`
  times over the life (1 by default): an explosion that blooms and fades,
  smoke that billows and thins.
- `fps` - `fps` frames a second, looping: a flame, a twinkle.
- `random` - one frame drawn at birth and held: a sheet of leaves or debris.

`random_start` starts each particle on a frame of its own (drawn at birth)
and counts on from there, so a cloud of them never flickers in step.
`Particles.set(e, "flipbook", { ... })` sets one, `nil` removes it, and
`"flipbook.fps"` changes a field. GPU particles have no flipbook. Test Lab
scene 210; the showcase's Fireworks Night.

Particles can **give off light** (Oct 2026): `light` on the emitter makes
each particle a point light, where it is drawn and in its colour - sparks
that light the floor they fall past, embers that light the rock as they
rise, fireflies.

```json
"light": { "every": 2, "max": 40, "radius": 120, "intensity": 1.4 }
```

- `every` - one particle in this many carries one, chosen by its own seed,
  so a particle has a light (or not) all its life. 1 by default.
- `max` - at most this many an emitter, the oldest first (64).
- `radius`, `intensity`, `falloff` - as a point light's (80, 1, 2).
- `use_particle_color` - the particle's colour (on), or `color`.
- `fade` - dimmed by the particle's alpha (on), so a fading spark's light
  fades with it; `scale_with_size` - its radius follows the particle's size
  over its size at birth (on), so a shrinking ember's light shrinks.

They cast no shadows and light every layer. A scene lit only by particles
still runs its light pass, but the rest of it is as dark as its ambient
light: give the floor something to reflect - a light colour, a normal map.
`Particles.set(e, "light.intensity", 0)` dims them all without touching the
particles; `nil` for `"light"` removes them. GPU particles carry none. Test
Lab scene 220; the showcase's Ember Cavern.

An emitter emits from its **world** position: a child of a moving thing
emits from where it is drawn (the runtimes used its local position until
Sep 2026).

An emitter starts its particles on an `emit_shape` about its position
(since Sep 2026 - a single point before that, so rain across a sky took a
row of emitters):

```json
"particle_emitter": { "emit_shape": { "kind": "line", "length": 1200 }, ... }
```

`point` (the default), `circle` (`radius`, and `edge: true` for only the
rim), `rect` (`size: [w, h]`) or `line` (`length`, horizontal). Offsets are
uniform over the shape.

Two shapes are **the entity's own** (Oct 2026): `path` starts particles
evenly along the `vector_path` on the emitter's entity - a fuse, a neon
sign, a ring of sparks - and `sprite` over its sprite's opaque pixels (alpha
above `cutoff`, 0.5 by default), or with `edge: true` only its outline - a
silhouette burning, a ghost misting, a dissolve. Both follow the entity's
turn and scale. To emit from a sprite you also want to see, put the emitter
on a child with an invisible copy of the sprite (alpha 0): an entity has one
emitter.

```json
"particle_emitter": { "emit_shape": { "kind": "sprite", "edge": true }, ... }
```

The pixels are read from the texture once (in the step every host runs with
its assets) and again only when the sprite's texture, region, size or anchor
change; until they are read, the emitter emits from its own point. Test Lab
scene 181; the showcase's Phoenix.

Since Sep 2026 an emitter also says how its particles blend and change over
their life:

```json
"particle_emitter": {
  "blend_mode": "Additive",
  "color_curve": [[0, 1, 0.8, 0.4, 0], [0.2, 1, 0.8, 0.4, 1], [1, 0.8, 0.1, 0, 0]],
  "size_curve":  [[0, 0.5], [0.3, 1.0], [1, 0.2]],
  "stretch": 0.04
}
```

- `blend_mode` - `Additive` for fire, sparks and magic: overlapping particles
  add up to a bright core. Particles were always alpha-blended before.
- `color_curve` - `[t, r, g, b, a]` keys over the life (`t` 0..1), replacing
  `color_start` -> `color_end`. The way to fade a particle in as well as out.
  Colours may exceed 1.0 in an HDR scene.
- `size_curve` - `[t, multiplier]` keys over the start size, replacing
  `size_start` -> `size_end`.
- `stretch` - seconds of flight drawn as length: the particle is turned along
  its velocity and lengthened by the distance it covers in that time. Sparks,
  rain and tracers want 0.02 - 0.08.

A sprite blends by its material: `"material": { "material_id":
"default_sprite", "blend_mode": "Additive" }`, or `"material_id":
"additive"`. That field existed long before any renderer read it - until Sep
2026 it drew alpha-blended in every runtime.

---

## UI Elements

Lightweight HUD primitives. For richer widgets see [GUI Documents](#gui-documents).

```lua
local label = UI.create_text("Score: 0", "top_left", 10, 10)
local panel = UI.create_panel("center", 0, 0, 200, 80)

UI.set_text(label, "Score: 42")
UI.set_visible(panel, false)
UI.set_position(label, "top_right", -10, 10)
```

Both take optional trailing arguments: `UI.create_text(text, anchor, ox, oy [, r, g, b, a, font_size, align])` (white, 16, `"left"`) and `UI.create_panel(anchor, ox, oy, w, h [, r, g, b, a])` (translucent black, alpha 0.5). Native required the panel's colour until Sep 2026, so the five-argument call threw there.

`anchor` is one of the [UI Anchors](#ui-anchors). `ox` / `oy` are pixel offsets from that anchor.

### Widgets: reading, writing and clicks

Entity-UI widgets - `ui_button`, `ui_slider`, `ui_toggle`,
`ui_progress_bar`, `ui_text_input` - are authored on scene or prefab
entities beside a `ui_element`. The engine drives them from the pointer in
every runtime; a script reads and writes them by property:

```lua
local price = UI.get_prop(slider, "value")      -- where the player left it
UI.set_prop(bar, "value", 0.4)                  -- clamped to the bar's min..max
UI.set_prop(buy, "disabled", money < cost)      -- dims it and stops clicks
local name = UI.get_prop(field, "text")         -- what was typed
UI.set_prop(icon, "region", "ice")              -- a `ui_image` names an atlas region
UI.set_prop(window, "size", { 470, 180 })       -- any element, in UI pixels: a window that fits its contents
```

A `ui_image` takes `"region"` like a sprite since 2026-09-24: author
`{ "texture_id": "atlases/shop.png", "region": "lemon" }` and the engine
fills in the rect, as it does for sprites. Before that an icon cut from a
sheet needed a hand-computed `uv_rect`, and could not be swapped from script.

A `ui_image` can be a **nine-slice** (Oct 2026): its corners keep their
size and only the edges and the middle stretch to the element, so one small
framed panel from a kit draws any window. Borders are in **texture pixels**,
drawn `scale` UI pixels per texture pixel - a 14-pixel pixel-art corner at
`scale: 2` is 28 UI pixels and stays crisp. jSuite writes these borders into
its kits (`nineSlice` on a kit item).

```json
"ui_image": { "texture_id": "ui/panel.png",
              "nine_slice": { "left": 14, "right": 15, "top": 14, "bottom": 15, "scale": 2 } }
```

**A button click is an event**, `ui_click`, with the button's entity id.
It fires on **release**, over the button the press began on - dragging off
before letting go backs out, as it does for `.gui` buttons:

```lua
function init(entity)
    buy = Entity.find_by_name("BuyLemons")
    Event.on("ui_click", entity)
end

function on_ui_click(entity, data)
    if data.button == buy then ... end
end
```

Do not poll `UI.get_prop(b, "pressed")` for this. It is true on **every
frame the button is held**, so a purchase written that way repeats sixty
times a second, and a script ticking slower than the frame can miss a quick
click. `pressed` and `hovered` are for drawing. Before 2026-09-24 the
click list was collected by every runtime and delivered by none, so polling
was the only way.

### Text fields

A `ui_text_input` (and a `.gui` `TextInput`) edits like a text field
everywhere (Oct 2026): a caret placed by a click, shift+arrows, a drag, a
double click (a word) or a triple click (everything) to select, ctrl+arrows
by word, Home / End, Backspace / Delete (ctrl: a word), ctrl+A / C / X / V
with the system clipboard, and an IME's composition shown underlined at the
caret. A long line scrolls to keep the caret in sight.

```json
"ui_text_input": { "placeholder": "your name", "font_size": 22, "max_length": 16, "password": false }
```

```lua
UI.set_prop(field, "focused", true)        -- the caret at the end; any other field lets go
UI.set_prop(field, "selection", { 0, 5 })  -- characters; or "caret", a number
if UI.get_prop(field, "submitted") then    -- Enter, this frame
    local name = UI.get_prop(field, "text")
end
local at = UI.get_prop(field, "caret")
local picked = UI.get_prop(field, "selected")
```

- **A focused field owns the keyboard**: every key pressed or repeated that
  frame is consumed (all but Tab and the F-keys), so a game's own bindings -
  H for the hub, Escape for back - do not fire while someone writes. Escape
  lets the field go; the next Escape is the game's.
- `max_length` counts characters, not bytes; `password` shows `*`s.
- A `.gui` `TextInput` takes the same properties through `GUI.set` / `GUI.get`
  (`max_length`, `password`, `caret`, `selection`, `submitted`, `selected`)
  and is focused by a click as well as by `GUI.focus`.
- In a browser the typing goes through a hidden textarea at the caret, which
  is what brings IME composition, paste and a phone's on-screen keyboard to
  a canvas. Copying needs the page served over https.

Engine Test Lab scene 205; the showcase's Guest Book.

---

## GUI Documents

`.gui` JSON assets attached via `GuiComponent`. Reference nodes by their `id` field.

```lua
GUI.show("ui/hud.gui")
GUI.hide("ui/pause_menu.gui")

GUI.set("ui/hud.gui", "score_label", "text", "42")
local current = GUI.get("ui/hud.gui", "score_label", "text")

-- Click handler. The 4-arg form routes the click to the named function;
-- multiple buttons can register distinct callbacks on the same entity.
function on_resume_click(entity) GUI.hide("ui/pause_menu.gui") end
function on_quit_click(entity)   Scene.load("scenes/title.scene") end
GUI.on_click(entity, "ui/pause_menu.gui", "resume_btn", "on_resume_click")
GUI.on_click(entity, "ui/pause_menu.gui", "quit_btn",   "on_quit_click")

-- Focus a text input (pass "" to clear focus) - a click on it does too. A
-- focused TextInput is a text field (see "Text fields" above): a caret,
-- selection, the clipboard, Enter, Escape. While focused it CONSUMES the
-- keys typed into it, so an action bound to one does not fire while typing.
GUI.focus("ui/hud.gui", "name_input")

-- Read back what the player typed:
local name = GUI.get("ui/hud.gui", "name_input", "text")
```

### Styling and animating a menu

A node's style has four fields for making a menu look like one (Sep 2026):

| Field | |
|---|---|
| `corner_radius` | rounded corners, px - the background, the border, a button, a progress bar and the shadow follow them |
| `shadow` | `{ "color": [r,g,b,a], "offset": [x, y], "blur": px }` - a soft drop shadow under the node (y down) |
| `opacity` | 0..1, **inherited**: a card at 0.5 draws everything on it at half |
| `scale` | about the node's centre, **inherited**, and applied by layout - a click hits what is drawn |

A `Button` also takes `alignment` (`"Left"`, `"Center"`, `"Right"`) for its
label and `hover_scale`; its `hover_color` and `pressed_color` are drawn
(they were authored and never drawn before Sep 2026), eased in with the
growth over a tenth of a second. `navigate_ui` tracks the pointer in every
runtime; nothing in a script is needed.

All of them are settable, so a script animates a screen change with two
numbers on one card:

```lua
GUI.set("ui/menu.gui", "card", "opacity", k)       -- fade the whole card
GUI.set("ui/menu.gui", "card", "scale", 1.05 - 0.05 * k)
GUI.set("ui/hud.gui", "toast", "offset_y", -40 * (1 - k))
GUI.set("ui/inv.gui", "slot_3", "border_color", { 1, 0.35, 0.05, 1 })   -- a table is fine
GUI.set("ui/hud.gui", "panel", "shadow", { color = { 0, 0, 0, 0.5 }, offset = { 0, 6 }, blur = 12 })
```

`GUI.set` takes a **table** since Sep 2026 - a list becomes a JSON array,
keys a JSON object - which is the same value the JSON string form
(`"[1, 0.35, 0.05, 1]"`) spells. Natively a table used to be a hard error,
and in the browsers it was stringified into `"1,0.35,0.05,1"` and dropped.
Showcase scene 22, Test Lab scene 136.

### Menus from a controller or the keyboard

A menu works without a pointer, and nothing in the script changes for it.
Nodes marked `"focusable": true` (the same flag Tab uses) are walked by the
**arrow keys, a D-pad or the left stick** - to the nearest focusable node in
the direction pressed, stopping at the edges, holding a direction repeats -
and **Enter, Space or A press** the focused one. The press arrives as the
same `on_gui_click` a click sends, disabled buttons refuse it as they refuse
a click, and a row moved to inside a scrolling list is scrolled into view.
Tab still walks the document's own order. Engine-side:
`ecs::systems::navigate_ui`, every runtime, test-lab scene 107.

Three rules keep it out of a game's way:

- **Only a direction or Tab creates focus.** Enter, Space and A may be the
  game's own keys, so on a menu with nothing focused they do nothing.
- **Only navigation's focus is pressed.** A click that focuses a button (as
  clicking a focusable node does) does not let a later Space press it again;
  a direction makes the focus navigation's, and a pointer press hands it
  back.
- **A document whose script runs its own cursor opts out** with
  `"navigable": false` at the top of the `.gui` - otherwise a second cursor
  moves beside the script's and a press can land on a different row. The
  arcade's documents do, because `lib/menu.lua` is that cursor.

Mark only menus `focusable`: a HUD button marked so would take the D-pad
the player is moving with while it is on screen.

Entity UI does the same for elements with `"focusable": true` on their
`ui_element`: a focused button presses as a `ui_click`, a toggle flips, and
a focused slider takes left and right as its value (a tenth of the range a
press). A focused element is drawn with a ring. A focused `.gui` **slider**
takes left and right the same way (Oct 2026; up and down still leave it), and
a rounded node's focus ring follows its corners. A `.gui` toggle does not flip
itself: its `on_gui_click` handler sets `checked`.

Text fields have a caret, selection and the clipboard - see "Text fields"
above.

### Screen readers

The UI is read by the platform's **screen reader** (Oct 2026;
`ui::accessibility`): Narrator, NVDA or JAWS on Windows, VoiceOver on a Mac,
Orca on Linux - through AccessKit natively, and in a browser through a
hidden ARIA copy of the menu in the page (`#jengine-accessibility`) with a
polite live region. Every entity-UI widget and every `.gui` text, button,
text field, toggle, slider and progress bar is a node with a name, a role, a
place and its state ("Subtitles, checkbox, checked"; "Music volume, slider,
70%"), read top to bottom, left to right, and focus is where navigation's
focus is. A reader's "move to" and "press" go through the same navigation a
D-pad does, so a press arrives as the `on_gui_click` / `ui_click` a click
sends, and a reader's "increase" moves a slider a tenth of its range. Nothing
to set up: the tree is gathered inside `navigate_ui`, which every runtime
runs. Android and iOS have no adapter yet.

A widget is named by its own words - a button by its label (an entity-UI
button by its child's `ui_text`), a field by its placeholder. Where those are
missing or wrong, say so:

```json
"accessible": { "label": "Options", "description": "Sound, controls and display" }
```

on an entity-UI element (a button that is only an icon), or the same object
as `style.accessible` on a `.gui` node - a toggle or slider is otherwise
named by its node id. `"hidden": true` leaves decoration out, and a caption
drawn beside a widget that already carries the name should be hidden, or it
is read twice. Empty text is skipped.

```lua
Accessibility.announce("Saved")          -- the reader says it, unprompted
local s = Accessibility.speech()         -- "Music volume, slider, 70%"
```

`Accessibility.announce(text)` speaks through the live region - a pickup, a
saved game, a menu that opened; the same words twice are said twice.
`Accessibility.speech()` is what a reader says for the focused node (empty
with nothing focused): a caption for players who cannot hear the reader, or
the text a game speaks itself where there is no screen reader. Test Lab
scene 218; the showcase's Readable Menus.

`GUI.set` accepts a stringified value; the engine parses it according to the property type (color, number, text, ...).

Settable properties: `text`, `placeholder`, `value`, `checked`, `visible`,
`disabled`, `focusable`, `tab_order`,
`constraint_left` / `constraint_right` / `constraint_top` / `constraint_bottom`
(absolute edge-pinning — this is how a node is made to follow the cursor), and
`texture_id`, `uv_rect`, `tint` (alias `color`), `bg_color` (on a Button
its face, `normal_color`; on a TextInput its field - since Oct 2026; a
Button never drew its style's `bg_color`), `fg_color` (a
progress bar's fill, since Oct 2026),
`normal_color` / `hover_color` / `pressed_color`, `overflow`,
`scroll_x` / `scroll_y`, and `width` / `height` / `font_size` (the GUI system
has no scale of its own, so fitting a fixed-design panel to the surface is done
by setting these from script). A bare number for `width` / `height` is pixels -
`GUI.set(gui, "hp_bar", "width", 120)` - and `{ "type": "Percent", "value": 50 }`
sizes against the parent. (Before Sep 2026 only the `{ type, value }` form
parsed, and a number was dropped silently.)

**On a `TextInput`, `text` is what the player typed** — the same field
`GUI.get` reads — so `GUI.set(gui, node, "text", "")` clears the box and
`GUI.set(..., "text", "wolf")` fills it. Until Sep 2026 it wrote the
`placeholder` instead, which looks identical on an empty field and is why
nobody noticed (BUGS #138). The grey prompt now has its own property.

**`disabled` is a button's refusal**, and since Sep 2026 it is real: the
button draws at half brightness and every runtime's hit-test skips it, so
`GUI.on_click` does not fire. It is what a shop row wants for something the
player cannot afford — hiding the row instead says "this does not exist",
which is a different sentence. Before that the flag serialized, loaded, and
did nothing at all.

All runtimes share one implementation — `ui::gui_document::apply_gui_node_prop`.
Add a property there, never in a per-runtime copy.

### Scrolling containers

A node whose style sets `"overflow": "Scroll"` clips its children to its content
box and scrolls them. **The engine handles this itself** — the mouse wheel over
the container scrolls it, the offset is clamped to the content, and a scrollbar
is drawn — so a scrolling list needs no script at all. `"overflow": "Hidden"`
clips without scrolling; `"Visible"` (the default) is the old behaviour, where
oversized children simply draw over their surroundings.

Scripts can drive the offset when they want to:

```lua
-- Read where the wheel left it (a string natively, a number in the editor —
-- tonumber handles both).
local y = tonumber(GUI.get("ui/hub.gui", "scene_list", "scroll_y")) or 0

GUI.set("ui/hub.gui", "scene_list", "scroll_y", y + 54)  -- down one row
GUI.set("ui/hub.gui", "scene_list", "scroll_y", 0)       -- top
GUI.set("ui/hub.gui", "scene_list", "scroll_y", 99999)   -- bottom
```

The layout engine clamps `scroll_y` to the scrollable range, so writing a large
number is the portable way to say "scroll to the end" without knowing the
content height. Clipping also applies to hit-testing: a row scrolled out of view
is not clickable in any runtime.

### The engine already scrolls it for you

A script does not have to do any of this. Every runtime handles, with no script
involved:

* the **mouse wheel** over the container;
* a **pointer drag** — press inside it and the content follows, flick and
  release and it coasts to a stop;
* **elastic ends** — pulling past either end resists, and springs back on
  release. The stretch cannot exceed 55% of the container, however hard you
  pull.

On the web, touch events are mapped onto the mouse fields, so the drag *is* the
touch gesture.

**Buttons fire on release, and a drag cancels the release.** This is what makes
a list of buttons scrollable: press-to-fire would activate whichever row your
finger landed on before it moved. Tapping still works exactly as before; a
gesture that travels more than a few pixels becomes a scroll and never becomes
a click.

**The stretch is not `scroll_y`.** It lives in a separate, never-serialized
field so `scroll_y` can stay clamped — which is what makes `99999` mean "the
end". So a script reading `scroll_y` while the content is visibly stretched
sees `0` or the maximum, and that is right: the honest position is the bound,
and the stretch is a moment in a gesture.

> Until Sep 2026 this was wheel-only, and only in the browser runtimes: native
> called neither `apply_gui_wheel_scroll` nor anything else, so a scrolling list
> in a native build could be moved by a script and nothing else.

**Size the container to the window, in the document.** A scroll container clips
its children to its own box, but *nothing clips that box to the screen* — so a
container taller than the window pushes its lower rows, and the bottom half of
its scrollbar, below the visible area, where scrolling cannot reach them. The
list looks like it simply stops early.

Give the container a `Percent` height so the layout engine derives it from the
same screen height it is already laying the document out with:

```json
{ "id": "list_frame", "style": { "height": { "type": "Percent", "value": 0.55 } } }
```

Prefer this over computing pixels in a script and pushing them in with
`GUI.set(..., "height", ...)`: that works, but it puts the window geometry in
two places that have to agree, and the script's copy is one frame behind on
every resize. The Engine Test Lab hub
(`projects/engine-test-lab/assets/ui/hub.gui`) sizes itself this way.

`SizeValue::Percent` is **per hundred**: `100.0` is full width, `50.0` is half.
So is `ConstraintValue::Percent`, and so is the `%` field in the GUI editor's
inspector — one convention throughout.

It was not always. The runtime read this one as a 0–1 fraction while everything
else used per-hundred, so a document authored in the editor at `50%` laid out
fifty times too large in play mode, and one hand-written with `1.0` for "full"
laid out at 1% in the editor canvas. Fixed Sep 2026 by moving the runtime to
per-hundred — the convention the authoring tool already used — and migrating
the nine `.gui` files written the other way. `tests/gui_documents_lay_out_sanely.rs`
keeps them honest: it lays out every `.gui` in the repo and rejects a node that
ends up several times the size of the screen or entirely off it, which is what
a percentage in the wrong convention looks like. (BUGS.md #10.)

Working example: `projects/engine-test-lab` scene 33 (Scroll Views) and the
hub menu itself.

---

## Scene Management

```lua
Scene.load("scenes/level2.scene")
Scene.push("scenes/pause_menu.scene")  -- pushes current scene on stack
Scene.pop()                            -- restore previous scene

local name = Scene.get_name()

-- P7.8: Dynamic script loading
Script.load(entity_id, "assets/scripts/enemy-ai.lua")     -- attach Lua script
Script.load(entity_id, "assets/scripts/patrol.js")        -- attach JavaScript script
```

**`Script.load(entity_id, path)`** — Dynamically attach a script to an entity at runtime.

| Parameter | Type | Description |
|-----------|------|-------------|
| `entity_id` | number | The numeric entity ID to attach the script to |
| `path` | string | Relative path to script file (e.g., `"assets/scripts/enemy.lua"`). Extension determines language: `.lua` → Lua/Luau, `.js` → JavaScript |

**Return**: `void`

**Behavior**:
- Script is queued for loading and will be initialized on the **next frame** (after current frame's scripts finish).
- The script's `init(entity)` is called when attached, then `update(entity, dt)` runs normally each frame.
- If the path does not exist or has an unknown extension, an error is logged and the command is skipped.
- Can be called from any script (`init`, `update`, timers, events).

**Example** (Lua):
```lua
function init(entity)
    -- Spawn bare enemy entity (no script yet)
    local enemy_id = Entity.spawn("Enemy")
    -- Attach AI script at runtime
    Script.load(enemy_id, "assets/scripts/enemy-chase.lua")
end
```

---

## Save Slots

Versioned key/value storage scoped to named **slots**. Writes are buffered in
memory; `Save.commit` is what persists them.

| Function | Returns | Notes |
|---|---|---|
| `Save.set(slot, key, value)` | — | Buffered. `value` may be a nested table / array. |
| `Save.get(slot, key)` | value or `nil` | `nil` for both "no such slot" and "no such key". |
| `Save.commit(slot)` | — | Writes the slot to storage. Nothing persists without it. |
| `Save.clear(slot)` | — | Drops every key; the next commit deletes the file / entry. |

**There is no `save_load`.** Reading is the load: a slot is pulled out of
storage the first time it is touched, so `Save.get` in `init()` on a fresh
launch returns what the previous run committed.

```lua
function init(entity)
    -- Reads whatever the last run committed; nil on a first-ever launch.
    local coins = Save.get("profile", "coins") or 0
    Entity.set_custom(entity, "Wallet", "coins", coins)
end

function on_run_end(entity)
    Save.set("profile", "coins", Entity.get_custom(entity, "Wallet", "coins"))
    Save.set("profile", "unlocks", { "dash", "double_jump" })
    Save.commit("profile")   -- nothing is written until this line
end
```

**Storage.** Native writes `${JENGINE_SAVE_DIR:-./saves}/<slot>.json`; browser
runtimes (editor Play mode and web builds) use `localStorage` under
`jengine_save:<slot>`. The format is `{ "version": 1, "data": { … } }`, and a
slot written by an unknown future version is ignored rather than guessed at.

Slot names are sanitised to `[A-Za-z0-9-_.]` before they reach the filesystem.

**The editor writes the same storage a web build reads**, so a save made while
testing in the editor is the save the deployed game loads. Clear it with
`Save.clear` (or the browser's site-data controls) if a stale test save is
confusing a run.

**Availability.** All four work on native, in the editor viewport, and in web
builds as of Sep 2026. Earlier builds registered them on native only, where
they were `nil` in the editor — see
[`docs/engine-audit-2026-09.md`](engine-audit-2026-09.md).

**Not provided:** there is no way to list slots or enumerate the keys in one
from a script, and no explicit "does this save exist" — test a known key
against `nil`.

---

## GPU particles

A `gpu_particles` emitter draws hundreds of thousands of particles in one
call (Sep 2026). It is **stateless**: each particle is a closed form of its
index and the time - its lifetime and phase from a hash of its index, its
direction, speed and spawn point reseeded every time it is reborn, its path
the integral of launch velocity, `gravity` and `drag`, then pulled in
(`attract`), turned about the emitter (`swirl`) and wobbled (`noise`) - so the
vertex shader computes every one and the CPU touches none.

```json
"gpu_particles": { "count": 140000, "lifetime": [2.5, 4.5], "speed": [60, 240],
                   "drag": 0.8, "swirl": 1.1, "arms": 4, "radius": 40,
                   "size": [2.2, 0.8], "stretch": 1.5, "noise": 18,
                   "color_start": [1, 0.6, 1, 0], "color_mid": [1, 0.6, 1, 0.3], "color_end": [0.5, 0.8, 1, 0],
                   "blend_mode": "Additive", "z_order": -36 }
```

- It sorts by `z_order` with sprites, blends by `blend_mode`, is lit unless
  `unlit`, and lives in its entity's space: the transform places, turns and
  scales the cloud (a non-uniform scale squashes it onto a floor).
- `arms` launches along that many directions instead of across `angle`; with
  `swirl` and a narrow `speed` range, spiral arms.
- `Particles.set(e, field, value)` reaches it like a CPU emitter -
  `"count"`, `"playing"` (which stops its clock), anything.
- **It meets the world** (Oct 2026), still without state. `attractors` - up
  to four, each at `x`, `y` or at an entity it `follow`s by name - draw a
  particle in as it ages (the share of the way in is `1 - 1 / (1 + strength
  * age)`) and turn it round by `swirl` radians a second, both falling off
  past `radius` (0 reaches everywhere). With `"collide": true`, a particle
  found inside one of the eight nearest non-sensor colliders is put out on
  its nearest side - drawn every frame, a stream sliding round a rock and
  packing against it - and `pile` sets each such particle down up to that
  far past the surface (its own share), a soft heap rather than a hard line.
  A box or a circle collider is met as itself, anything else as its
  bounding box.

```json
"gpu_particles": { "count": 200000, "angle": [-0.2, 0.2], "speed": [260, 430], "collide": true, "pile": 16,
                   "attractors": [ { "follow": "Twister", "strength": 0.9, "swirl": 4.5, "radius": 190 } ] }
```

- What stateless rules out: a closed-form particle cannot bounce, be pushed
  by a hit, or trail behind a moving emitter. Engine Test Lab scenes 144 and
  201; the showcase's Spell Storm and Sandstorm.

### GPU particles that keep state

```json
"gpu_particles": { "count": 9000, "speed": [140, 420], "angle": [1.15, 2.0], "gravity": [0, -520],
                   "collide": true, "simulate": "state", "bounce": 0.45, "friction": 0.15, "inherit": 0.8 }
```

With `"simulate": "state"` (Oct 2026) every particle is a position, a
velocity, an age and a life in a GPU buffer, and a compute pass steps them
once a frame. So a particle is **born where the emitter is** - a moving
emitter leaves a wake behind it instead of carrying its cloud along -
**bounces** off the colliders it meets (`bounce`, the share of its speed
into the surface it keeps, default 0.3; `friction`, the share along it it
loses, default 0.1) and settles on them, is **pushed** by a collider that
moves (its `Velocity2D` - a kinematic body given a velocity, say), and is
thrown on by the emitter's own speed (`inherit`, the share of it a particle is
born with). Everything else reads as before: `count`, `lifetime`, `speed`,
`angle`, `arms`, `radius`, `edge`, `gravity`, `drag`, the look; `noise` is a
turbulent field anchored to the world, turning at `noise_speed`, and an
attractor is a real force - `strength` a spring toward it (per second
squared), `swirl` a push round it, both falling off past `radius`. A stepped
cloud starts empty and fills over one lifetime.

A device with no compute - the WebGL2 fallback - cannot step one, and draws
the emitter in closed form instead: the same cloud, without the bouncing,
pushing and wake. `Particles.set(e, "simulate", "closed")` switches one
back (and `"state"` starts it again, empty). Engine Test Lab scene 260; the
showcase's Ember Storm.

## Swapping sprites by name

```json
"sprite_library": { "categories": {
  "hat":  { "summer": { "texture_id": "sprites/dolls/hat_summer.png" },
            "winter": { "texture_id": "sprites/dolls/hat_winter.png", "size": [140, 90] } },
  "boot": { "summer": { "texture_id": "sprites/dolls/boots.atlas", "region": "sandal" },
            "winter": { "texture_id": "sprites/dolls/boots.atlas", "region": "snow" } } } }
"sprite_resolver": { "category": "hat", "label": "summer" }
```

Parts by name (Oct 2026; Unity's Sprite Library and Sprite Resolver). A
`sprite_library` holds what a figure can wear, by **category** and
**label**; each entry is a texture, an optional atlas `region`, and an
optional `size` and `anchor` (the sprite keeps its own when absent). A
`sprite_resolver` on a sprite names its category and the label it shows, and
finds its library on itself or up its parents - so one library on a rig's
root dresses every part of the rig, and the parts keep their bones, their
animation and their place.

| Function | |
|---|---|
| `Sprite.set_label(e, label, category)` | On a part: show `label` (moving it to `category` too, if given). On a library's entity: every part under it whose category has `label` shows it - a whole outfit in one call - or only `category`'s parts, if given |

A part is rewritten only when its library, category or label changes, so an
animation or a script that sets the texture itself is not undone each frame.
Every texture a library names is loaded with the scene. Engine Test Lab scene
199; the showcase's Paper Dolls.

## Colliders in the shape of a sprite

```json
"collider": { "shape": { "Box": { "width": 96, "height": 96 } }, "friction": 0.7 },
"sprite_collider": { "alpha_cutoff": 0.5, "alpha_tolerance": 1.5 }
```

A `sprite_collider` (Oct 2026) traces the outline of its sprite's opaque
pixels - alpha above `alpha_cutoff`, simplified to within `alpha_tolerance`
texels, scaled by the entity's transform - and writes it into the entity's
`collider` as a `ConvexDecomposition`, so the physics, raycasts, shape
queries, particle collisions, navigation and the F4 outlines all use it. The
shape authored on the `collider` is a placeholder: the body is not built
until the outline is traced, and is rebuilt when the sprite's texture,
region, size, anchor or scale change. A `collider` is added if there is
none. Every island of the sprite is a loop of the shape (a
`ConvexDecomposition`'s `loops` says where each starts); holes are filled.
It works on prefabs spawned at run time too. Test Lab scene 191, showcase
scene 72.

## Conveyors

```json
"collider": { "shape": { "Box": { "width": 800, "height": 16 } }, "friction": 0.9 },
"conveyor": { "speed": 150 }
```

```lua
Physics.set_conveyor_speed(belt, -150)   -- the other way
```

A `conveyor` (Oct 2026; Unity's SurfaceEffector2D) makes a collider's
surface slide at `speed` world units a second along the entity's own +x -
turn the entity to aim it. What rests on it is carried by **friction**, as on
a real belt: the collider never moves, so nothing about the body changes,
and a crate on a steep belt still slides back if friction cannot hold it.
Rapier is told the contact's tangent velocity; the hook is asked only about
pairs with a conveyor in them. Test Lab scene 193, showcase scene 74.

## Rich text

A world `text`, an entity-UI `ui_text` or a `.gui` Text node with
`"rich": true` reads its string as markup (Oct 2026) - Godot's BBCode
spelling:

```json
"text": { "text": "Once, in the [color=#3a78c8]Blue Hollow[/color], lived [b]Ember[/b].[pause=0.6] She kept a [icon=sprites/key.png] key.",
          "rich": true, "reveal_speed": 26 }
```

| Tag | Does |
|---|---|
| `[color=#e05040]..[/color]` | colours the run - sRGB hex (`#rgb` too), or `red`, `orange`, `gold`, `green`, `teal`, `blue`, `purple`, `pink`, `white`, `grey`, `black` |
| `[b]..[/b]` | bold |
| `[wave]..[/wave]` | letters bob in a travelling wave |
| `[shake]..[/shake]` | letters jitter |
| `[rainbow]..[/rainbow]` | letters cycle through the hues, as bright or as deep as the text's own colour |
| `[pause=0.5]` | the typewriter waits half a second here |
| `[icon=sprites/key.png]` | a square image a line tall, in the line |

- `[[` is a literal `[`; an unknown tag is kept as text. Without `rich` the
  string is drawn as it is, brackets and all.
- **`reveal_speed`** (characters a second) is a typewriter: it starts when the
  text changes - `Text.set` with a new string starts it over - waits at each
  `[pause]`, and fades each character in. 0 shows it all at once. Setting the
  same string again does not restart it. It types **plain** text too (since
  Oct 2026 - before, only `rich` text was drawn typing, while a dialogue
  counted a plain line as still typing).
- Layout, wrapping and alignment are the plain text's. One size per text.
- **In the UI** (Oct 2026): a `ui_text` takes `rich` and `reveal_speed` as
  world text does (`UI.set_text` with a new string restarts its typewriter);
  a `.gui` Text node takes `rich`, read **after** its `{{variables}}` are
  filled in - so `"[color=gold]{{gold}}[/color] coins"` colours the value.
  A `.gui` node has no typewriter. Icons are preloaded from both.

## Outlines and shadows

A world `text` or an entity-UI `ui_text` can carry an outline and a shadow
(Oct 2026):

```json
"text": { "text": "SALE!", "font_scale": 3.4, "color": [1, 0.86, 0.1, 1],
          "outline": { "width": 4, "color": [0, 0, 0, 1] },
          "shadow": { "offset": [6, -6], "color": [0.35, 0.02, 0.02, 1], "softness": 0 } }
```

- **`outline`**: `width` in world units (UI pixels for `ui_text`), `color`
  (default black). Drawn under the text, round every glyph.
- **`shadow`**: `offset` (y up, default `[2, -2]`), `color` (default black at
  0.6), `softness` (0 is a hard copy). An `offset` of `[0, 0]` with some
  softness is a glow - a neon tube's light on the wall behind it, with the
  text drawn brighter than white in an HDR scene.
- Both follow the text: rich text's wave and shake, the typewriter, turning.
  A rich text's outline is one colour whatever the markup's colours.
- On a **distance-field** font - the built-in one, or any `.font` made with
  `jengine font-sdf` - an outline is the glyph with its edge moved out, round
  at the corners, a quad a glyph. The field only encodes so far past the ink:
  two layout pixels of the built-in font, times `font_scale` (about 7 units at
  scale 3.5), and a wider outline is cut to that. On a **bitmap** font the
  outline is eight copies round the text and the shadow cannot be softened.
- A **lit** text's outline is lit too: a light outline over a light surface
  takes the same colour as the surface under the same light, so outline lit
  text in a contrasting dark.

Engine Test Lab scene 154; the showcase's Neon Signs.

## Sharp text

The built-in font is a **signed distance field** (Sep 2026): world text,
entity UI and `.gui` text are sharp at any `font_scale`, zoom or turn, where
they used to be a 16 px bitmap stretched. Sizes are unchanged - `font_scale`
1 and a UI `font_size` of 16 are the native size, as before.

**A game's own font can be one too** (Oct 2026). Turn a bitmap `.font` into a
distance field with the engine's CLI:

```
jengine font-sdf assets/fonts/ui.font            # in place
jengine font-sdf assets/fonts/ui.font assets/fonts/ui-sdf.font --scale 2
```

It rewrites the `.font` and its sibling `.png`, and moves no text: advances,
the line height and where each glyph's ink sits are kept (each glyph's rect
grows by `sdf_spread` on every side, its offsets moved out to match). The
field is baked finer than the source by default - enough for a 48 px field,
as the built-in face is - so a small font keeps its counters; `--scale` picks
the factor. A field is only as good as its source: convert a font rasterized
large (the arcade's 64 px `tabletop.font`), not a 12 px pixel face, which is
better left a bitmap and drawn nearest. A `.font` with `"sdf": true` is drawn
as a distance field in every runtime with nothing else to set.

**A font can be a TTF or OTF file** (Oct 2026): a `font_id` naming a `.ttf`
or `.otf` draws in it - Cyrillic, Greek, Japanese, Chinese, Korean, whatever
the file covers - on world text, entity UI and `.gui` widgets alike.

```json
"text": { "text": "こんにちは世界", "font_id": "fonts/noto/NotoSansJP-Regular-subset.ttf", "font_scale": 2.0 }
```

There is no atlas to bake. ASCII is baked when the font loads, and every
other glyph the first time a text asks for it, into a distance field the
built-in face's size. So `Text.set` to new characters just works; a glyph's
first frame may show the font's `?`. A character the file has not got draws
its `?`. Ship a font **subset** to the characters a game uses (fontTools'
`pyftsubset`): a full CJK face is 10 to 17 MB, and the subsets the test lab
and showcase use (`assets/fonts/noto/`, SIL OFL) are about 15 to 40 KB each.
Test Lab scene 186; the showcase's Signs of the Silk Road.

**Shaped and right-to-left text** (Oct 2026). A line with Arabic, Hebrew,
an Indic script, Thai, Lao or Myanmar in it is **shaped** - letters joined
and dotted, conjuncts formed, vowel signs moved before their consonant,
marks placed on their bases, by `rustybuzz` (HarfBuzz's rules) - and set in
**visual order** by the Unicode bidirectional algorithm: Arabic and Hebrew
right to left, numbers and Latin inside them left to right, in a line that
reads the way its first strong letter does.

```json
"text": { "text": "السعر ٣٥ درهم", "font_id": "fonts/noto/NotoSansArabic-Regular-subset.ttf", "align": "center" }
```

Nothing to set: it is the font file and the letters that decide, on world
text, entity UI and `.gui` alike, in every runtime. Keep a font's **layout
tables** when subsetting (`pyftsubset --layout-features='*'`) - without
GSUB and GPOS there is nothing to shape with. Three limits: a shaped line
wraps only at a space, rich-text styles on it land on glyphs rather than
letters, and a text field's caret in it is approximate. Lines without such
letters cost nothing more. Test Lab scene 217; the showcase's Bazaar Signs.

## Sprite masks

A `sprite_mask` is a shape that clips other drawing (Sep 2026) - a portal
showing another place, a window onto a layer behind, a frame a picture stays
inside. Anything whose `masked` names the mask's entity is clipped to the
shape's inside, or with `outside` to everything but it.

```json
"sprite_mask": { "shape": { "Ellipse": { "size": [200, 280] } }, "feather": 16 }
"masked":      { "by": "Portal", "outside": false }
```

- The shape is a `Rect` (`size`), an `Ellipse` (`size`) or a `Polygon`
  (`points`, up to 16, either winding), in its entity's own space - so it
  moves, turns and scales with its transform, and a script moves a portal by
  moving its entity. `feather` is a soft edge in the shape's units.
- Sprites, meshes, vector paths, world text and a particle emitter's
  particles can be masked; the default shader and every graph material
  honour it. One shape can be used both ways: a wall `outside` an arch has a
  hole in it, and a landscape `inside` the same arch shows through.
- **A mask can be a texture's alpha** (Oct 2026; Unity's `SpriteMask`), so
  any painted silhouette clips - a torn edge, a keyhole, a splash of ink:

  ```json
  "sprite_mask": { "shape": { "Texture": { "texture_id": "masks/tear.png",
                                           "size": [240, 336] } } }
  ```

  The texture is read over a `size` quad about the entity (the whole texture,
  or `uv_rect: [u0, v0, u1, v1]` of it), soft where its alpha is soft;
  `alpha_cutoff` above 0 keeps what is over it with a hard edge. It turns and
  scales with its entity like a shape, and is preloaded with the scene. It is
  read with linear filtering whatever the project's texture filter.
- A masked sprite's normal map is not clipped, so in a lit scene its
  lighting response reaches past the mask.

## Cloth and rope

A `cloth` is a grid of points and a `rope` a chain of them, moved every frame
by the engine (Sep 2026) and written into the entity's `mesh` - added if it has
none - so they draw in the sprite pass like anything else: sorted, lit,
blended. Author the `mesh` for its texture, colour and `z_order`, and leave its
vertices empty.

```json
"mesh":  { "texture_id": "sprites/flag.png", "z_order": 3, "vertices": [], "indices": [] },
"cloth": { "size": [190, 88], "cols": 20, "rows": 10, "anchor": [0, 1], "pin": "Left",
           "gravity": [0, -450], "wind": [900, 60], "gust": 0.35,
           "flutter": 300, "wave": 4000, "wave_length": 150, "wave_speed": 1.3,
           "bend": 0.25, "iterations": 8, "shade": 0.5 }
```

- `pin` is `Top` (a banner), `Left` / `Right` (a flag on a pole), `TopCorners`
  (a sheet on a line) or `None`. The pinned points follow the entity, so a
  flag on a swinging pole swings with it; `anchor` says which point of the
  rectangle sits on the entity's origin, as a sprite's does.
- The wind gusts (`gust`, 0..1), `flutter` adds curl-noise eddies and `wave`
  a travelling ripple across the cloth, growing from the pinned edge to the
  free one. A 2D sheet the wind has pulled taut can only ripple sideways, so a
  flag without `wave` flies as a flat board.
- `bend` resists folding: a 2D sheet has nowhere to go out of its plane, so
  without it slack cloth crumples onto itself. `shade` darkens folds.

```json
"rope": { "length": 860, "segments": 63, "width": 5, "start_offset": [0, 60],
          "end": "PostR", "end_offset": [0, 60],
          "pins": [ { "at": 0.5, "entity": "Plank10", "offset": [0, 58] } ] }
```

- A rope runs from its own entity (at `start_offset`) to the entity named
  `end` (at `end_offset`), or hangs free when `end` is empty. `pins` hold it
  part way along, by fraction of its length, to named entities.
- It is **visual**: nothing here pushes a body. A rope that must hold weight
  is a rapier joint (`Rope`, or planks on revolute joints for a bridge), and
  a `rope` pinned along it is how it is drawn - the showcase's Cloth & Rope
  draws its bridge's rails and deck ropes that way.
- `collide: true` stops points at non-sensor colliders (the CPU outlines
  particles use); a point that starts inside a collider stays there.

```lua
Cloth.set(flag, "wind", { 2200, 140 })   -- a gale
Cloth.set(flag, "wave", 9000)
Cloth.set(rope, "length", 300)           -- works on a rope too
```

Every runtime steps them in `ecs::cloth::tick_cloth`, right after the
hierarchy resolve. Test Lab scene 139, `cloth_and_rope`.

## Lighting & Post-Processing

```lua
-- Per-light controls (entity must have a Light2D component)
Lighting.set_color(light_entity, 1.0, 0.8, 0.6)
Lighting.set_intensity(light_entity, 1.5)
Lighting.set_height(sun_entity, 0.3)   -- a low sun: long shadows
Lighting.set_radius(light_entity, 200)

-- Global ambient
Lighting.set_ambient(0.1, 0.1, 0.15, 0.3)

-- Scene-wide: toon lighting (bands, softness, dither cell) and ambient
-- occlusion (radius, strength). Each writes the scene's component or adds
-- one; 0 bands / 0 strength removes it. nil arguments are the defaults.
Lighting.set_toon(3, 0.08)          -- three cel bands
Lighting.set_toon(4, 1.0, 2)        -- dithered: every band edge a 4x4 ordered pattern, 2-unit cells
Lighting.set_toon(0)                -- off
Lighting.set_occlusion(48, 0.6)
Lighting.set_occlusion(nil, 0)      -- off
Lighting.set_global_illumination(1.5)     -- light bounces (scale 4 by default)
Lighting.set_global_illumination(2, 2)    -- stronger, and finer: a texel every 2 pixels
Lighting.set_global_illumination(0)       -- off

-- Post-process effects (toggle + parameters)
PostProcess.set_enabled("bloom", true)
PostProcess.set_float("bloom", "threshold", 0.8)
PostProcess.set_vec4("vignette", "color", 0, 0, 0, 1)

-- HDR and the output pass (see below)
PostProcess.set_enabled("hdr", true)
PostProcess.set_float("tonemap", "exposure", 1.4)
PostProcess.set_float("tonemap", "mode", 2)   -- 0 none, 1 reinhard, 2 aces, 3 agx
```

### Global illumination

```json
"global_illumination": { "strength": 1.5, "scale": 4, "cascades": 0 }
```

Light that bounces (Oct 2026): the lit outlines of the occluders shine
again, and every pixel gathers what reaches it round the occluders in
between, coloured by what it came off - sun through red glass onto a stone
floor warms the wall above it, and a shadow takes a little of the room's
colour instead of only the ambient. It is **radiance cascades** on the GPU,
added to the light buffer before the composite, so the bounce is multiplied
by the surface it lands on like any light.

- One per scene, like ambient occlusion. `strength` scales what is added;
  `scale` is how many frame pixels make one GI texel each way (the bounce
  is soft, so 4 is plenty; 2 is four times the work); `cascades` 0 takes as
  many as cross the frame.
- What bounces is the occluders' outlines (a `light_occluder`'s edges), lit
  by the direct light - so a floor, a wall or a pillar has to be an occluder
  to shine again. Lightmaps, freeform lights and emission are not bounced.
- It needs a float frame (the world drawn in `Rgba16Float`): native and
  WebGPU have one; a WebGL2 page without float targets leaves it out and
  logs once. `light_at` does not include it.

Engine Test Lab scene 202; the showcase's Stained Light.

### Light layers

A light lights only the sprites on its layers (Oct 2026; Godot's
`light_mask`, Unity's target sorting layers): a point, spot or directional
light's `light_mask` and a sprite's `light_layers` are bitmasks, and the
light lights the sprite when they share a bit. Every light lights every
layer and every sprite is on layer 1 unless they say otherwise, so nothing
changes until something does.

```json
"light_layers": { "layers": 2 },
"point_light": { "...": "...", "light_mask": 2 }
```

```lua
Lighting.set_mask(torch, 1 + 2)   -- layers 1 and 2 (add the bits up)
```

Eight layers, the low eight bits. It is how a character gets a key light
that does not spill onto the backdrop, or lightning that flashes the sky and
leaves the people black against it. Only sprites choose: tilemaps, text,
meshes and particles are on every layer, and so is the ambient light,
lightmaps and freeform lights. The layers ride beside the normals in the
normal pass, which runs whenever a light leaves a layer out. `light_at`
asks for every layer; `light_at_on(p, lights, layers)` asks for a sprite's.
Engine Test Lab scene 179; the showcase's Moonlit Duel.

### Subtractive lights

```json
"point_light": { "color": [1.0, 0.57, 0.21], "intensity": 1.8, "radius": 300, "subtract": true }
```

A point or spot light with `subtract` takes its light away instead of
adding it (Oct 2026; Godot's Light2D `BLEND_MODE_SUB`): darkness cast by a
cursed lantern, a pool of shadow with no occluder. It is drawn after the
lights that add, and the frame never goes below black - so at its heart a
light of 1 takes away at most what was there. It subtracts its **colour**:
taking white from a warm candle's light leaves the blue at zero first and a
brownish fringe round the pool, so give darkness the colour of the light it
eats. Shadows, cookies and layers work as for any light; its highlight
subtracts too. `light_at` subtracts the same way and stops at black.
`Lighting.set_intensity` fades it like any light. Test Lab scene 213; the
showcase's Ghost Lantern.

### Light cookies

```json
"point_light": { "radius": 440, "cookie": "sprites/window_light.png", "cookie_scale": 1.0, ... }
```

A point or spot light can shine **through a texture** (Oct 2026) - a
cookie, as Unity and Godot call it: a window's panes on the floor, leaf
dapple, a stained-glass pattern, a fan's turning blades. The picture spans
`radius` times `cookie_scale` each way from the light, with the light at its
centre and the picture's top towards +y; it **turns with the entity** (a
spot light's with its aim, entity turn plus `direction`), so turning the
light turns the pattern. It multiplies what the light gives: white passes,
black stops, a colour tints, alpha fades - and outside the picture the light
gives nothing, so a cookie is also the light's shape. Draw cookies with soft
edges: a hard-edged mask shows its texels as a staircase along any slanted
edge, however it is filtered.

There is no script setter; a cookie is authored, and a script turns or
moves the light. `light_at` cannot see a cookie (it has no pictures, as with
lightmaps); `light_at_textured` can. Until the picture has loaded the light
is not drawn, rather than drawn uncut. Engine Test Lab scene 159; the
showcase's Moonlit Study.

### Shadows in the shape of the sprite

```json
"light_occluder": { "from_alpha": true, "alpha_cutoff": 0.5, "alpha_tolerance": 1.5 }
```

A `light_occluder` casts a box (`size`, or the sprite's with
`from_sprite`) or the `polygon` it is given - or, with **`from_alpha`**
(Oct 2026), the **outline of its sprite's opaque pixels**: a tree's crown
with the gaps between its clumps, each picket of a fence, a scarecrow's
cross. The outline is traced from the texture (its `uv_rect` region of it)
at `alpha_cutoff`, simplified to within `alpha_tolerance` texels, and kept
until the sprite's texture, region, size or anchor change; each separate
island of the sprite is a loop of its own, and holes are left out. Until
the texture's pixels arrive it is the sprite's box.

Every corner is an edge each nearby light spends from its pool of 448
(see the occluder budget), so a loop keeps at most 48 corners - a larger
tolerance is fewer, and cheaper. A translucent part of the sprite (a rail
drawn at half alpha under a cutoff of 0.5) casts nothing. Engine Test Lab
scene 158; the showcase's Lantern Orchard.

### HDR

A scene opts into HDR in its post block:

```json
"post_process": {
  "hdr": true, "tonemap": "aces", "exposure": 1.0,
  "effects": [ { "name": "bloom", "params": { "threshold": { "Float": 1.0 } } } ]
}
```

The world is always drawn into a float target where the GPU has one
(`Rgba16Float`; every desktop adapter and WebGPU do, some WebGL2 and older
GLES devices do not). What `hdr` changes is whether values past 1.0 are
**kept**:

- **Light adds up.** A light at intensity 3 lights a floor to three times its
  albedo. Without `hdr` the composite stops light at 1.0, as the 8-bit light
  buffer always did - which is what every scene written before Sep 2026 was
  tuned against, and why it is opt-in.
- **Colours may exceed white.** A sprite coloured `(4, 2, 0.5)` - the vertex
  colour is a float - is a glowing ember, and bloom finds it that way.
- **The output pass tonemaps.** `aces` (the default for an HDR scene; the film
  curve Unity and Godot ship, which rolls bright colours toward white), `agx`
  (holds hue in bright colours), `reinhard`, or `none` (clamp). `exposure`
  multiplies the scene first. An LDR scene is never tonemapped.

With `hdr`, set bloom's `threshold` to 1.0 or more: then only what is
brighter than white glows - the fire, not the wall it lights.

**Bloom** is a mip chain since Sep 2026 (a 13-tap downsample through up to
seven half-resolution levels and a tent upsample back, as in Unity and
Godot), so a glow can reach a fair fraction of the screen. Its parameters:
`threshold`, `knee` (the soft knee, as a fraction of the threshold),
`intensity`, and `scatter` (0..1, how much each wider level adds - higher is
a wider, softer glow). `blur_size` belonged to the 9-tap bloom it replaced
and is ignored.

**UI is drawn after the post chain** since Sep 2026, onto the display, so
bloom, vignette and chromatic aberration no longer touch entity UI, `.gui`
documents, touch controls or the debug overlays. World text is part of the
world and still goes through the chain - `unlit` text too.

### Pixelate and CRT

Two effects for pixel art (Sep 2026), authored like any other and toggled
with `PostProcess.set_enabled`:

- **`pixelate`** point-samples the frame in cells of `height / lines` screen
  pixels (`lines`, default 180: 4-pixel cells at 720p, 6 at 1080p). Author the
  art at one texel per cell - a 180-line game in a 720-unit camera draws each
  texel 4 units wide - and sample it nearest (`settings.texture_filter`), and a
  rotated or scaled sprite lands on the same grid as everything else rather
  than breaking into finer-than-art pixels. The grid is anchored to the world,
  so a panning camera scrolls smoothly and nothing shimmers; a moving object
  steps a cell at a time, as it would in a low-resolution game. It renders at
  full resolution and point-samples, which looks the same as drawing small and
  scaling up but saves nothing. Put it last, or before `crt`: bloom after it is
  smooth, bloom before it is blocky.
- **`crt`** bends the picture like a tube's glass (`curvature`), darkens the
  gap between scanlines (`lines`, `scanline`) and lays an RGB aperture mask
  over it (`mask`). Give it the same `lines` as `pixelate` so each art row is
  one scanline.

UI is drawn after both, so text stays sharp over a pixelated world - use
entity UI or a `.gui` for captions, not world text.

### Colour look-up tables and film grain

A **colour look-up table** grades the whole frame (Oct 2026; Unity's Color
Lookup): paste the neutral strip on a screenshot, grade the screenshot in an
image editor, cut the strip out again - that strip is the grade. A strip of
`n` slices is `n * n` by `n` pixels (16 or 32 slices); red runs right in a
slice, green down, blue slice by slice. `renderer::lut::identity(n)` makes
the neutral one, and the Test Lab ships `assets/luts/neutral.png` (32).

```json
"color_lut": { "texture_id": "luts/dusk.png", "strength": 1.0 }
```

```lua
PostProcess.set_lut("luts/winter.png", 0.5)   -- another grade, half way
PostProcess.set_lut("", 0)                    -- none
```

It is applied in the output pass, after the tonemap - on the colours the
strip was painted against, so it means the same in an HDR scene - and the UI
is drawn after it, ungraded. **Keep strips out of `assets/sprites/`**: the
asset pipeline packs that directory into atlases. And a strip must be
loaded: name it in the scene (a `color_lut`, or a small sprite showing it),
since nothing draws a LUT to ask for its texture.

**Film grain** is an effect like the others, drawn in the same place:

```json
"post_process": { "effects": [{ "name": "film_grain", "params": { "intensity": { "Float": 0.08 }, "size": { "Float": 1.5 } } }] }
```

`intensity` is how far a pixel moves (in display terms, 0.05 - 0.15 for a
film look), `size` the grain in pixels; a new pattern each frame, gentler in
the highlights. `PostProcess.set_enabled("film_grain", on)`. Test Lab scene
182; the showcase's Turning Seasons.

### Anti-aliasing

```json
"post_process": { "effects": [{ "name": "fxaa", "params": { "span": { "Float": 8.0 } } }] }
```

**`fxaa`** (Oct 2026) smooths the stairs on a turned edge - a rotated
sprite, a thin line, a mesh, world text. It runs in the output pass on the
tonemapped colour, finds an edge where a pixel's diagonal neighbours
disagree and blurs along it, at most `span` texels; straight runs are left
as they are, and the UI, drawn after the chain, stays sharp. It is skipped
for a pixel-perfect camera, whose stairs are the art.
`PostProcess.set_enabled("fxaa", on)`. Test Lab scene 192; the showcase's
Spirograph.

### Motion blur

```json
"post_process": { "effects": [{ "name": "motion_blur", "params": {
    "shutter": { "Float": 1.0 }, "samples": { "Float": 12.0 }, "max_length": { "Float": 0.08 } } }] }
```

**`motion_blur`** (Oct 2026) smears each pixel along its motion across the
screen since the last frame. A velocity pass draws every covering sprite's
own motion - its movement and the camera's together - over the camera's
motion for everything else (tilemaps, particles, text), so a car the camera
follows has none and stays sharp while the street it drives down streaks,
and a car overtaking it streaks along its own path. A tap counts only where
what is there moves far enough to reach the pixel, so a sharp thing does not
bleed into the blur beside it. `shutter` is how much of a sixtieth of a
second's motion is smeared (the same length at any frame rate), `samples`
the taps, `max_length` the longest smear in screen UV. Only the main camera
draws it, and only sprites have motion of their own in it: world text,
tilemaps and particles move as the world under the camera does - a label
riding with the camera smears, so put one in the UI. `PostProcess.set_enabled("motion_blur", on)`. Test Lab scene 234;
the showcase's Night Pursuit.

### Lens flare

```json
"post_process": { "effects": [{ "name": "lens_flare", "params": {
    "threshold": { "Float": 3.0 }, "intensity": { "Float": 0.5 }, "ghosts": { "Float": 6.0 },
    "spacing": { "Float": 0.32 }, "halo": { "Float": 0.42 }, "halo_width": { "Float": 0.05 },
    "dispersion": { "Float": 0.008 }, "streak": { "Float": 0.6 }, "streak_length": { "Float": 0.3 },
    "tint": { "Vec4": [1.0, 0.85, 0.7, 1.0] } } }] }
```

**`lens_flare`** (Oct 2026) adds the reflections a bright light leaves in a
camera's lens. It is screen space: what is brighter than `threshold` is
taken down to a quarter of the frame and blurred, and each pixel looks back
through the centre of the frame at that soft copy - so anything that bright
(a sun, a flash, a lamp in an HDR scene) throws a chain of soft, coloured
`ghosts` (0 - 8, `spacing` apart) to the opposite side of the frame, a
`halo` (its distance in screen heights, `halo_width` its softness; 0 for
none) and, with `streak`, a horizontal anamorphic streak `streak_length`
screen widths each way. `dispersion` reads red and blue a little apart for
the fringes; `tint` colours the lot; a light near the frame's edge flares
fainter. Nothing has to be marked as a light - set the threshold above
everything that should not flare (in an HDR scene, above 1). The threshold
is tested on the **quarter-size average**, so a light a few pixels across
must be brighter than one that fills a patch: give a sun a small white-hot
core. It runs where it sits in the chain: first, before bloom, so bloom
softens the flare too.
`PostProcess.set_enabled("lens_flare", on)`, `PostProcess.set_float` for
any number. Test Lab scene 235; the showcase's Orbital Station.

### Depth of field

```json
"post_process": { "effects": [{ "name": "depth_of_field", "params": {
    "focus": { "Float": 0.0 }, "range": { "Float": 2.0 }, "strength": { "Float": 0.55 },
    "max_radius": { "Float": 16.0 } } }] }
```

```lua
PostProcess.set_float("depth_of_field", "focus", z)   -- rack focus to a layer
```

**`depth_of_field`** (Oct 2026) blurs each layer by how far it is from the
one in focus. A layer's depth is its **z** - the sprite's `z_order`, plus its
sort layer's base - so a scene drawn in layers (far hills, trees, a subject,
foliage by the lens) needs nothing more: a pixel more than `range` from
`focus` in z is blurred by `strength` pixels per unit of z past it, up to
`max_radius` (32 at most). Pull `focus` from one z to another and the focus
racks. It reads each pixel's z from the velocity pass (the one `motion_blur`
uses), which draws **covering sprites** only: world text, particles,
meshes, lines and tilemaps take the depth of the sprite under them - so a
label over a soft layer is soft with it - and over nothing they stay sharp.
A soft sprite edge takes the depth of what is under it. Two rules make it a
lens rather than a blur: a sharp layer does not leak into the soft ones
round it, and a soft layer behind never bleeds over a sharp one in front
(a soft one in front does spread over what it covers, as near ferns do over
a deer). Put it first in the chain, before bloom. Main camera only.
Test Lab scene 258; the showcase's Rack Focus.

### Light shafts

**`light_shafts`** (Sep 2026) streams light from a source in beams - god
rays through leaves, a window's light across a dusty hall. Every pixel
marches to the source and gathers what it crosses that is brighter than
`threshold`, each step weighted by `decay` times the last, so the gaps in
whatever stands **in front of** the light on screen become beams and the
shapes cut them. It is screen-space: a thing only blocks the light where it
covers the bright source in the frame, not where it stands between them in
the world.

| Param | Default | |
|---|---|---|
| `source_x`, `source_y` | 0, 1000 | the source, a **world** point - it may be off screen |
| `length` | 1.0 | reach, in screen heights: at this distance from the source the beams are at half strength |
| `intensity` | 0.8 | how strongly the gathered light is added |
| `decay` | 0.97 | each step's weight after the last (0.9 - 0.99) |
| `threshold` | 0.6 | only what is brighter casts - set it above the sky and below the sun |
| `color` | (1, 0.95, 0.85, 1) | the beams' tint |

Put it first in the chain, before bloom. A moving light is followed with
`PostProcess.set_float("light_shafts", "source_x", x)` (and `source_y`) each
frame. Showcase scene 19, Test Lab scene 133.

### Tilt-shift and blur

**`tilt_shift`** (Sep 2026) is sharp in a horizontal band and blurs away
from it - the photographer's trick that makes a town look like a model of
one. **`blur`** is the same disc everywhere: a world softened behind a menu.
There was no blur of any kind in the post stack before.

| `tilt_shift` param | Default | |
|---|---|---|
| `focus_y` | 0 | the band's centre, a **world** height - it stays on its street as the camera moves |
| `band` | 60 | half-height of the sharp band, world units |
| `falloff` | 220 | how far past the band the blur takes to reach full, world units |
| `max_blur` | 10 | the radius far from the band, pixels (up to 48) |
| `min_blur` | 0 | the radius inside the band |

`blur` takes `radius` (pixels). Both are a 32-tap golden-angle disc, which
reads as a lens rather than a box. Put them before the grade and the
vignette; a strong `saturation` and `contrast` in the grade finish the
miniature look. Showcase scene 23, Test Lab scene 137.

**`blur` can keep a rectangle sharp** (Oct 2026): give it a focus rectangle
and everything outside it goes out of focus - the room the player is in,
the world beyond its walls soft.

| `blur` param | Default | |
|---|---|---|
| `radius` | 6 | the radius outside the rectangle, pixels (up to 48) |
| `focus_x`, `focus_y` | 0 | the rectangle's centre, **world** units |
| `focus_w`, `focus_h` | 0 | its size, world units - 0 for no rectangle (blur everywhere) |
| `falloff` | 40 | how far outside it the blur takes to reach `radius`, world units |
| `focus_blur` | 0 | the radius inside it |

```lua
-- the walker has gone in: ease the outside out of focus
PostProcess.set_enabled("blur", true)
PostProcess.set_float("blur", "radius", 7 * inside)
PostProcess.set_float("blur", "focus_x", x)   -- a rectangle that moves
```

The rectangle is placed through the camera's view, so it stays on the house
as the camera follows someone round it (an unturned camera). Test Lab scene
227; the showcase's The Lit House.

---

## Camera

`Camera.set_zoom(zoom)` sets the **active camera**'s zoom: `1.0` is the framing
the scene was authored at and higher values are closer in. The engine could
always zoom - `update_camera` copies the `Camera2DComponent.zoom` field into
the renderer every frame - but until Sep 2026 nothing could ask it to, so a
scene wanting to zoom had to move every entity instead.

```lua
local zoom = 1.0
if Input.is_key_just_pressed("E") then zoom = math.min(zoom * 1.1, 4.0) end
if Input.is_key_just_pressed("Q") then zoom = math.max(zoom / 1.1, 0.25) end
Camera.set_zoom(zoom)
```

The value is **clamped to `0.01 .. 100`** rather than ignored: a zero or
negative zoom collapses the view matrix and blanks the screen, which reads as a
crash rather than as a bad argument.

`Camera.get_zoom()` reads it back. That is worth having now that the engine
can choose the zoom itself: `CameraFollow.frame_tag` frames a whole set of
entities and zooms to fit, so a game can no longer assume it knows the value
it last set. A setter without a getter is only an asymmetry until something
else starts writing the number.

It needs an entity carrying an active `Camera2DComponent`. A scene with no
camera entity renders through the runtime's own default camera, which this
cannot reach; that case logs a warning rather than doing nothing quietly.

**Where the camera is** (Oct 2026): `Camera.get_position()` returns `x, y`,
the centre the camera is drawing - following, framing and all. The camera
entity's own transform is not that: `camera_follow` and `frame_tag` move the
renderer's camera and never write back, so `Transform.get_position(camera)`
answers wherever the scene authored it. Parallax layers and a dead-zone
overlay are placed from this.

**A dead zone and look-ahead** (Oct 2026; Godot's drag margins, Cinemachine's
dead zone and lookahead) on `camera_follow`:

```json
"camera_follow": { "smoothing": 0.12, "dead_zone": [360, 240], "look_ahead": 0.55, "look_ahead_max": 240 }
```

- `dead_zone` - `[width, height]` in world units round the middle of the
  view. The camera holds while the target moves inside it and is dragged by
  its edge past it, so a jump does not bob the view. `[0, 0]` follows every
  step, as before.
- `look_ahead` - seconds: the camera aims that far ahead of the target by its
  velocity (up to `look_ahead_max` world units), easing round by
  `look_ahead_smoothing` when the target turns. 0 is off.

Both apply when following one entity, not while framing a set.
`Camera.set_follow(entity, field, value)` changes any `camera_follow` field
by name at runtime - `Camera.set_follow(hero, "dead_zone", { 360, 240 })`.
Engine Test Lab scene 166; the showcase's Autumn Run.

**A pixel-perfect camera** (Sep 2026): `Camera.set_pixel_size(size)`, or
`"pixel_size"` on the `camera` component, draws the world at one pixel per
`size` world units and scales it up - pixel art at its own resolution. 0 (the
default) draws at the screen's.

```json
"camera": { "zoom": 1.0, "is_active": true, "viewport_width": 1280, "viewport_height": 720, "pixel_size": 4 }
```

- Everything in the world is drawn small - sprites, tilemaps, lights, the post
  chain - into a target of one texel per art pixel, so a turned sprite is made
  of whole art pixels and the frame costs what a 320x180 one does. UI is drawn
  after, at the screen's resolution, and stays sharp.
- The target sits on the art's grid (its edge a whole number of pixels from
  the world origin), so art placed on that grid lands texel for texel.
- A pan stays smooth: the camera snaps to the grid and the output pass slides
  the picture by what the snap took off, from a texel of margin.
- The world the screen shows is unchanged, so mouse picking and
  `get_mouse_world_position` need nothing.
- `pixelate` is the other way to the look: drawn at full resolution, then
  point-sampled. It costs a full frame and a pan steps a pixel at a time.
- **Whole pixels** (Oct 2026): by default the authored area fills the
  screen exactly, so on a window that is not a multiple of the art an art
  pixel is, say, 4.27 screen pixels - four and five by turns, and a moving
  sprite ripples. `"integer_scale": true` on the camera makes every art
  pixel the largest whole number of screen pixels that keeps the authored
  area in view, and shows a little more world to fill the screen (Godot's
  integer scaling with an expanded aspect - no black bars). The camera's
  view is that larger area everywhere, so the mouse still picks what is
  under it. Authored on the component; there is no script setter.

**More than one camera** (Sep 2026): an active camera with a `target` draws
into a texture of that name instead of onto the screen - a **view**. Any
sprite, mesh or UI image naming the texture shows what it sees.

```json
"camera": { "zoom": 2.0, "is_active": true, "viewport_width": 1280, "viewport_height": 720,
            "target": "views/bridge", "target_size": [320, 180] }
"sprite": { "texture_id": "views/bridge", "size": [108, 62] }
```

- Each view is drawn every frame, before the main camera, by the same world
  pass - lights, shadows, Scene Color, the post chain (with the scene's
  settings) - into its own buffers. So a monitor shows another room lit as it
  is lit, a minimap is a camera far above, and split screen is the main camera
  plus a view shown by a UI image over half the screen.
- The main camera is the active one with **no** target (the highest
  `priority` of them); `Camera.set_zoom` and `Camera.set_pixel_size` act on it. Move a view's camera like any entity -
  `Transform.set_position`, or parent it to what it follows.
- `target_size` is the texture's size in pixels (0 takes the screen's), and
  the authored viewport is fitted to its shape as the screen's is.
- A camera can watch a sprite showing its own texture: it sees the picture
  from a frame before, so a camera on its own monitor draws a tunnel.
- Every view costs a whole world pass. Keep them small, or few.

**Split screen is one field** (Oct 2026; Unity's viewport rect): a camera's
`screen_rect` - `[x, y, w, h]`, fractions of the screen from its top-left, y
down - draws it onto that part of the screen. The engine makes it a view with
a target of its own (`__screen/<id>`) sized to its part, and shows it with an
image under every other UI; a smaller part draws over a larger one, so an
inset sits on the view it is set into. Its authored area is scaled by its
share, so half the screen shows half the world at the full screen's scale.

```json
"camera": { "zoom": 1.0, "is_active": true, "screen_rect": [0.0, 0.0, 0.497, 1.0] }
```

```lua
Camera.set_screen_rect(cam, 0, 0.5, 1, 0.5)  -- the bottom half
Camera.set_screen_rect(cam, 0, 0, 0, 0)      -- a plain camera again
```

The main camera still draws behind the parts and shows wherever they leave
the screen bare; aim it at nothing for a plain divider. Under the
`ScaleAndCenter` UI mode the parts divide the authored area, not the window.
Mouse-to-world reads the main camera. Test Lab scene 184; the showcase's Two
Views of the Railway.

**Cameras hand over and stay in shapes** (Oct 2026; Cinemachine's
priorities, blends and confiner). Of the active cameras with no `target`, the
one with the highest `priority` draws the screen - the first of them on a tie,
so a scene of one camera, or of several all at 0, is unchanged. When another
takes over, its `blend` eases the view across over that many seconds -
position, zoom (by ratio) and turn - while the new camera follows its target
as if no blend were running; 0 cuts. A camera's `confine` is a polygon in
world units that its **whole view** stays inside, not just its centre: the
view slides along the walls and round an inside corner, so a room of any
shape never shows past its walls. A view larger than the shape keeps only its
centre inside.

```json
"camera": { "zoom": 1.2, "is_active": true, "priority": 0, "blend": 0.9,
            "confine": [[0, 0], [1800, 0], [1800, 1200], [0, 1200]] }
```

```lua
Camera.set(vault_cam, "priority", 10)   -- hand it the screen
Camera.set(cam, "confine", {})          -- let it look past the walls
Camera.set(cam, "blend", 0)             -- cut from now on
```

`Camera.set(e, field, value)` writes any camera field by name
(`screen_rect` has its own setter). `Camera.set_zoom`, `Camera.set_pixel_size`,
the listener for positional sound and mouse-to-world all act on the main
camera. `CameraFollow.bounds` still clamps the centre to a rectangle;
`confine` is the shape. Test Lab scene 197; the showcase's The Heist.

**A camera draws only its layers** (Sep 2026): an entity's `render_layers` is
a bitmask (an entity without one is on layer 1, bit 0), and a camera's
`cull_mask` says which layers it draws - every one by default. A camera draws
an entity when they share a bit, which is how a minimap shows markers the
street views do not, or leaves the roofs off to show the roads.

```json
"camera":        { "zoom": 0.72, "is_active": true, "target": "views/map", "cull_mask": 3 }
"render_layers": { "layers": 2 }
```

Sprites, meshes, paths, world text, trails, skinned meshes, tilemaps, shadows
and both kinds of particles are culled; lights are not (a light in a hidden
layer still lights what the camera does draw). The editor's own camera draws
every layer while editing.

Screen-shake on the **active camera**. `amplitude` is the peak offset in world
units, `duration` is the total length in seconds (the amplitude fades linearly
to zero across it), and the optional `frequency` is the oscillation rate in Hz
— pass `0` or omit it for the default 20 Hz.

```lua
Camera.shake(6, 0.18)        -- short punchy kick
Camera.shake(14, 0.35, 12)   -- bigger, slower rumble
```

Starting a shake while one is running **replaces** it rather than queueing, so
rapid hits stay punchy instead of accumulating into a long wobble. Stack the
effect by scaling `amplitude` at the call site.

### Turning the camera

There is **no `set_camera_rotation`**: the view is turned by the camera
entity's own `Transform.rotation`, so `Transform.set_rotation(camera, radians)` — the
same call that turns any other entity — is the whole API. Find the camera by
name as you would any entity.

```lua
local camera = Entity.find_by_name("Camera")
Transform.set_position(camera, car_x, car_y)
Transform.set_rotation(camera, heading - math.pi / 2)  -- forward points up the screen
```

Turning the camera anticlockwise swings the world clockwise on screen, which
is the useful sign: a camera set to a car's heading holds the car's nose
still and turns the track around it. Before Sep 2026 `Camera2D` had a position
and a zoom but no rotation, and a game wanting this had to rotate every entity
in the world instead.

Three things follow the rotation so you do not have to correct for it:

- **`Input.get_mouse_world_position()`** turns the screen offset by the camera's
  rotation in every runtime, so the world point under the cursor is right at
  any angle.
- **Culling** allows for the wider box a turned camera sees (at 45° the
  corners swing out by √2), so tiles no longer pop in at the edges.
- **UI does not turn.** Both UI systems are screen space, so a `.gui`
  document and `UIElement` widgets stay upright while the world spins — which
  is what you want for a readout, and means a HUD needs no compensation.

World text turns too: `Transform.set_rotation` on a text entity turns the whole
block about its own position, so a label under a turned camera can be turned
back to stay readable - which is what scene 82's amber label does. Before
Sep 2026 the renderer drew glyphs axis-aligned and ignored the rotation.

Engine Test Lab scene 82 drives all of it.

---

## Collisions: how hard, and where

`Physics.get_contacts()` returns every contact this frame, and since Sep 2026 each
row says how hard it was:

```lua
for _, c in ipairs(Physics.get_contacts()) do
    local a, b, started, impulse, x, y = c[1], c[2], c[3], c[4], c[5], c[6]
    if started and impulse > 400 then
        break_thing(a, b, x, y)
    end
end
```

The first three fields are what the row always was, so a script unpacking
three values is unaffected. `on_collision_enter`'s payload carries the same
three (`ev.impulse`, `ev.x`, `ev.y`).

Two things to know:

- **A contact is reported on its second step.** Rapier's solver writes the
  impulse during the step that resolves the pair, so reading it on the step
  the pair appears gives zero for every impact in the world. One frame of
  delay buys a number that means something.
- **Put a threshold under any damage.** A resting stack is a column of live
  contacts pushing against each other; a game that damaged on all of them
  would grind its own level to gravel while nobody touched it. Engine Test
  Lab scene 84 shows both - three landings, and a stack that never stops
  touching.

---

## Animation frame events

A clip's frames can carry triggers, authored in the editor's animation
editor or in the `.anim` file: `PlaySound` plays a clip, and `EmitEvent`
calls the entity's script.

```lua
script = { events = { "on_animation_event" } }

function on_animation_event(entity, ev)
    -- ev.name is the event the frame named; ev.data is whatever it carried.
    if ev.name == "hitbox_on" then
        live = true
        damage = ev.data.damage
    elseif ev.name == "hitbox_off" then
        live = false
    end
end
```

This is how frame data is written: the hitbox is live on the frames the clip
says it is, rather than on a timer kept beside the animation that drifts out
of step the moment the clip's timing changes.

A frame's event is also an **ordinary engine event**, so a script on another
entity can hear it - which is what most games need, because the animation is
on the sprite and the logic is on one controller:

```lua
Event.on("footstep", entity)          -- in init

function on_footstep(entity, data)    -- the handler, by convention
    dust(data.step)
end
```

`Event.send(name, data)` delivers its payload to listeners the same way.
Before Sep 2026 the native dispatch dropped it and every listener saw `nil`.

Both halves were dead until Sep 2026 - every runtime computed what a frame
fired and threw it away, so a sound on a frame was silent and
`on_animation_event` was a handler name nothing called. Engine Test Lab
scene 83 drives it.

---

## Debug Drawing & Overlay

One-frame primitives queued onto the debug renderer. Repeat the call every frame to keep them visible.

```lua
Debug.draw_line(0, 0, 100, 100, 1, 0, 0)           -- red line
Debug.draw_rect(50, 50, 80, 40, 0, 1, 0)           -- green outline
Debug.draw_circle(0, 0, 25, 1, 1, 0)               -- yellow circle

Debug.set_overlay(true)   -- show FPS / entity count overlay
```

---

## Utility

```lua
log("Player spawned at " .. tostring(x))
```

`log` is the only synchronous side effect in the API — everything else is queued.

`Time.get_delta` is **not** a bridge function — `dt` is passed into `update(entity, dt)` directly. Use that parameter.

### `Time.get_unix` — the wall clock

```lua
local now = Time.get_unix()          -- seconds since 1970, as a float
Save.set("slot", "left_at", now)
```

The engine's other clocks measure *this run*: `dt`, `Debug.get_frame_time` and
`now_ms` all count from an arbitrary point in the process and say nothing
about dates. `Time.get_unix` is the system clock, so it is the only one
whose readings mean anything either side of a restart — "how long was I
away?", a daily reward, an offline payout. An idle game is built on it.

It is read **once per frame** into the frame's snapshot, so every script in
a frame agrees about the time, and it is subject to the usual caveats about
system clocks: it can jump, and it can go backwards. Clamp the difference
rather than trusting it (`(now - then).max(0)`), and cap what a long absence
pays out.

Native reads `SystemTime`; the browser runtimes read `Date.now() / 1000`.

---

## Noise

Smooth random fields for islands, clouds, wind, wobble and wear (Oct 2026;
`jengine_core::noise`). Every function is a pure function of its arguments
and a `seed` - any number, 0 when left out - so the same call gives the same
value in every runtime: native and the browsers run the same Rust.

```lua
local h = Noise.perlin(x * 0.05, y * 0.05, seed)        -- about -1..1
local w = Noise.perlin3(x * 0.05, y * 0.05, t * 0.3)     -- z as time: changes in place
local s = Noise.simplex(x * 0.05, y * 0.05, seed)        -- about -1..1, no grid grain
local f1, f2, region = Noise.cellular(x * 0.1, y * 0.1, seed)
local cloud = Noise.fractal(x, y, { kind = "simplex", octaves = 5, frequency = 0.01, seed = 3 })
local map = Noise.grid(0, 0, 64, 40, 1, { mode = "ridged", frequency = 0.08 })  -- 64 * 40 values
```

- **Perlin** and **simplex** are about -1..1 and smooth everywhere; Perlin is
  zero on every whole-number point, so scale the input (features about
  `1 / frequency` units across).
- **Cellular** (Worley) puts one point in every unit cell: `f1` and `f2` are
  the distances to the nearest two, in cells, and `region` is 0..1, the same
  over the nearest point's whole cell. `f1` makes cobbles and island
  centres, `f2 - f1` (zero on the borders) cracks and veins, `region` a
  colour per patch.
- **Fractal** sums `octaves` (1-12, default 5) of a `kind` (`"perlin"`,
  `"perlin3"` reading `z`, `"simplex"` - the default - or `"cellular"`),
  each `lacunarity` (2) times the frequency and `gain` (0.5) times the
  amplitude of the last. `mode` is `"fbm"` (-1..1, hills and clouds),
  `"ridged"` (0..1, crests - mountain ranges) or `"turbulence"` (0..1,
  billows - smoke, marble). `frequency` (1) scales the input first.
- **Grid** is a fractal sampled `w` x `h` times, `step` apart from `x0, y0`,
  returned row by row (`values[r * w + c + 1]`) - a whole map in one call
  rather than one per cell. Past 1,048,576 values it returns an empty list
  and logs.

Packages call the same functions from Rust (`jengine_core::noise::simplex`,
`Fractal { .. }.sample`). Test Lab scene 211; the showcase's Procedural Isles.

---

## Key Names

Valid string values for `Input.is_key_pressed` / `Input.is_key_just_pressed` (case-sensitive).

| Category | Keys |
|----------|------|
| Letters | `A`–`Z` |
| Digits | `0`–`9` |
| Function | `F1`–`F12` |
| Arrows | `Left` `Right` `Up` `Down` |
| Modifiers | `ShiftLeft` `ShiftRight` `ControlLeft` `ControlRight` `AltLeft` `AltRight` |
| Special | `Space` `Enter` `Escape` `Backspace` `Tab` `Insert` `Delete` `Home` `End` `PageUp` `PageDown` |
| Punctuation | `Minus` `Equal` `BracketLeft` `BracketRight` `Backslash` `Semicolon` `Quote` `Backquote` `Comma` `Period` `Slash` |
| Lock | `CapsLock` `NumLock` `ScrollLock` `PrintScreen` `Pause` |
| Numpad | `Numpad0`–`Numpad9` `NumpadAdd` `NumpadSubtract` `NumpadMultiply` `NumpadDivide` `NumpadDecimal` `NumpadEnter` |

The browser's `KeyboardEvent.code` spelling of a letter, digit or arrow is
accepted as an alias - `"ArrowUp"`, `"KeyW"`, `"Digit1"` - both here and in
`project.jengine` input-action bindings. Prefer the canonical names above: they
are what the engine reports and what the editor's binding dropdown offers.
(Until Sep 2026 the aliases were refused, and three projects' hand-written
`"ArrowUp"` bindings silently bound nothing - test-lab BUGS #98.)

Source: `crates/jengine-core/src/input/mod.rs` (`KeyCode::from_name`, `ALL_KEYS`).

---

## UI Anchors

Valid `anchor` values for `UI.create_text` / `UI.create_panel` / `UI.set_position`:

`top_left` `top_center` `top_right` `center_left` `center` `center_right` `bottom_left` `bottom_center` `bottom_right`

Source: `UIAnchor::from_name` in `crates/jengine-core/src/ui/anchor.rs`.

---

## Batch Scripts (Step 4+)

A **batch script** processes many entities in one VM call instead of one call per entity. It runs against entities carrying a `BatchScriptComponent` whose `path` matches. The script declares its column contract in a `system` header and provides a single `update(ctx, q)` function that operates on typed-array views.

### System header

```js
const system = {
  query: {
    read:  ["Velocity2D"],         // components the script reads (snapshot)
    write: ["Position2D"],         // components the script writes (mutated)
    read_optional: ["Acceleration2D"], // OPTIONAL — Slice 5.2; see below
    state: {                       // OPTIONAL — Slice 5.1
      acc: { type: "f32", default: 0.0 }
    }
  },
  tick_rate: 60,                   // OPTIONAL — frames between calls (1 = every frame)
};
```

- `read` / `write` — components the script reads (snapshot) and writes (mutated in place). As of Slice 5.3, both are arbitrary-length lists drawn from the **batch vocabulary**: `Position2D`, `Velocity2D`, `Acceleration2D`, `Drag2D`, `Force2D`, `Wind2D`, `Impulse2D`, `Torque2D`. The combined column count (`read.len() + write.len() + read_optional.len()`) must not exceed `MAX_BATCH_COLUMNS = 12`; over-cap specs are rejected at load with `BatchError::TooManyColumns`. The same component name may not appear in more than one of read/write/read_optional (`BatchError::DuplicateComponent`). Empty `read` and `write` (legacy walking-skeleton scripts) still work and back-fill to `read=["Velocity2D"], write=["Position2D"]`.
- `read_optional` (Slice 5.2, JS **and** Lua) — components the script *may* use when present. The dispatcher partitions the batch by archetype so a single VM call sees a uniform population: every entity in the call either has every listed optional component, or none do. Inside `update`, probe `q.read_optional.<Component>` — it is the column object (`{ x, y, ... }` of `Float32Array` views in JS, or per-mode column tables in Lua) when the component is present, and `null` (JS) / `nil` (Lua) when absent. Walking-skeleton scope: only `Acceleration2D` is currently accepted; unknown component names fail at load.
- `state` (Slice 5.1 / 5.1b, JS **and** Lua) — typed per-entity local storage. Each field becomes a column of values (one per entity in the batch) exposed to the script as `q.state.<name>`. The container type depends on the language and (for Lua) the host-configured buffer mode — `Float32Array` for JS, Lua table / `F32Buffer` userdata / Luau `buffer` for Lua. Field schema:
  - `type`: `"f32"` is the only supported type this slice. Other types log a warning and are skipped.
  - `default`: number used for any entity whose stored state slot is missing or has been resized (fresh spawn, schema change, scene saved before 5.1).
  - Field order on disk: **alphabetical by name**. Renaming a field changes its slot and resets affected entities to `default`. Adding a field is safe (it gets the default for all existing entities). Reordering the source-text declaration has no effect.
- `tick_rate` — optional integer. If set, the script only runs every Nth frame; in skipped frames `q.count` is 0.

#### Lua state example

Lua scripts use the same `system.query.state` schema. The container type for `q.state.<name>` follows the runtime's [`BatchLuaBufferMode`] (Table / UserData / Buffer); the example below uses Table mode (1-indexed), which matches `q.write.Position2D.x` / `q.read.Velocity2D.x` exposure in the same mode:

```lua
system = {
  query = {
    read  = { "Velocity2D" },
    write = { "Position2D" },
    state = { acc = { type = "f32", default = 0.0 } },
  },
}

function update(ctx, q)
  local px, py = q.write.Position2D.x, q.write.Position2D.y
  local vx, vy = q.read.Velocity2D.x, q.read.Velocity2D.y
  local acc = q.state.acc
  for i = 1, q.count do          -- Lua tables are 1-indexed
    px[i] = px[i] + vx[i] * ctx.dt
    py[i] = py[i] + vy[i] * ctx.dt
    acc[i] = acc[i] + ctx.dt
  end
end
```

In `Buffer` mode, `q.state.<name>` is a Luau `buffer`; access it with `buffer.readf32(q.state.acc, (i-1)*4)` / `buffer.writef32` using byte offsets, exactly like `q.write.Position2D.x` in that mode.

#### Optional reads (Slice 5.2)

`read_optional` lets one script integrate Velocity2D **and** an optional Acceleration2D in the same loop without paying for the column when no entity in the batch has it. Because the dispatcher partitions by archetype, the inner loop sees a single mode per call — branch once on the column object's truthiness, then tight-loop:

```js
function update(ctx, q) {
  const px = q.write.Position2D.x, py = q.write.Position2D.y;
  const vx = q.read.Velocity2D.x,  vy = q.read.Velocity2D.y;
  const dt = ctx.dt, n = q.count;
  const a  = q.read_optional.Acceleration2D;       // object or null
  if (a) {
    const ax = a.x, ay = a.y;
    for (let i = 0; i < n; i++) {
      px[i] += (vx[i] + ax[i] * dt) * dt;
      py[i] += (vy[i] + ay[i] * dt) * dt;
    }
  } else {
    for (let i = 0; i < n; i++) {
      px[i] += vx[i] * dt;
      py[i] += vy[i] * dt;
    }
  }
}
```

Lua mirror (Table mode shown; `q.read_optional.Acceleration2D` is a column table or `nil`):

```lua
function update(ctx, q)
  local a = q.read_optional.Acceleration2D
  if a then
    local ax, ay = a.x, a.y
    for i = 1, q.count do
      q.write.Position2D.x[i] = q.write.Position2D.x[i] + (q.read.Velocity2D.x[i] + ax[i] * ctx.dt) * ctx.dt
      q.write.Position2D.y[i] = q.write.Position2D.y[i] + (q.read.Velocity2D.y[i] + ay[i] * ctx.dt) * ctx.dt
    end
  else
    for i = 1, q.count do
      q.write.Position2D.x[i] = q.write.Position2D.x[i] + q.read.Velocity2D.x[i] * ctx.dt
      q.write.Position2D.y[i] = q.write.Position2D.y[i] + q.read.Velocity2D.y[i] * ctx.dt
    end
  end
end
```

In Lua, `q.read_optional` is always a table — only the per-component fields are nil when absent. In JS, `q.read_optional.<Component>` is `null` when absent (the field is always defined). Only components listed in `read_optional` are exposed; unlisted names are not present even when entities carry them.

#### Multiple read/write columns (Slice 5.3)

`read` and `write` accept any combination of vocabulary components up to the
12-column cap. A 5-read / 3-write integrator (full physics-ish step) looks like:

```js
const system = {
  query: {
    read:  ["Acceleration2D", "Drag2D", "Wind2D", "Force2D", "Impulse2D"],
    write: ["Velocity2D",     "Position2D", "Torque2D"],
  },
};

function update(ctx, q) {
  const ax = q.read.Acceleration2D.x, ay = q.read.Acceleration2D.y;
  const dx = q.read.Drag2D.x,         dy = q.read.Drag2D.y;
  const wx = q.read.Wind2D.x,         wy = q.read.Wind2D.y;
  const fx = q.read.Force2D.x,        fy = q.read.Force2D.y;
  const ix = q.read.Impulse2D.x,      iy = q.read.Impulse2D.y;
  const vx = q.write.Velocity2D.x,    vy = q.write.Velocity2D.y;
  const px = q.write.Position2D.x,    py = q.write.Position2D.y;
  const tx = q.write.Torque2D.x,      ty = q.write.Torque2D.y;
  const dt = ctx.dt, n = q.count;
  for (let i = 0; i < n; i++) {
    const vxn = vx[i] * (1 - dx[i]) + (ax[i] + wx[i] + fx[i] + ix[i]) * dt;
    const vyn = vy[i] * (1 - dy[i]) + (ay[i] + wy[i] + fy[i] + iy[i]) * dt;
    vx[i] = vxn;            vy[i] = vyn;
    px[i] += vxn * dt;      py[i] += vyn * dt;
    tx[i] += (ix[i] - fx[i]) * dt;
    ty[i] += (iy[i] - fy[i]) * dt;
  }
}
```

Important details:

- **`write` columns are pre-filled with the current ECS snapshot** before the
  call. Reading `vx[i]` in the loop above gives the previous frame's value;
  the write is only committed when `update` returns.
- **Counter math** scales with column count. Per frame:
  `bytes_in  = (n_read + n_write + n_optional_present) * 2 * n_entities * 4`
  `bytes_out = n_write * 2 * n_entities * 4`.
- **Cap (`MAX_BATCH_COLUMNS = 12`)** applies to the *sum* of `read + write + read_optional`. Hitting the cap is a load-time error; split a system in two if you need more columns.
- **Lua buffer modes** all support multi-column. Table / UserData mode use the
  same `column.x[i]` / `column.y[i]` pattern as the example above (1-indexed);
  `Buffer` mode uses `buffer.readf32(column.x, (i-1)*4)` / `buffer.writef32`.

#### Lua handle reuse across frames (Slice 5.4a)

Starting in Slice 5.4a the Lua batch runtime caches the per-script `q` / `ctx`
wrapper tables and the per-mode column storage handles across frames instead of
rebuilding them on every call:

- **Table mode** caches the outer `q.read.<C>` / `q.write.<C>` / `q.state.<n>` /
  `q.read_optional.<C>` wrapper tables and the inner `.x` / `.y` column tables.
  The inner sequences are still **rebuilt per call** via `create_sequence_from`
  — that turned out to be ~2× faster than `raw_set`-ing into a reused table in
  mlua/Luau (per-call FFI overhead dominates the loop, and `create_sequence_from`
  takes a single Rust→Lua hop). Effect at 10k / 5r3w: `cp_in` drops from
  4321 µs → 2348 µs (-46%).
- **UserData mode** caches the `F32Buffer` userdata and grows the underlying
  `Vec<f32>` in place when the batch size increases. No allocation in steady
  state. Effect at 10k / 5r3w: `cp_in` drops from 222 µs → 51 µs (-77%).
- **Buffer mode** caches the Luau `buffer` and reuses it as long as it's large
  enough; grows in place when batch size increases. Effect at 10k / 5r3w:
  `cp_in` drops from 125 µs → 54 µs (-57%).

The cache only ever grows (never shrinks) so a one-frame spike to N=10k keeps
N=10k worth of column storage alive. Scripts cannot observe the cache — handles
are reset whenever the script is reloaded.

For A/B testing the optimization, set `JENGINE_BATCH_LUA_NO_CACHE=1` before
launching to force a full handle rebuild every frame (matches the pre-5.4a code
path).

#### Writeback elision (Slice 5.4b)

Slice 5.4b makes batch script writebacks **dirty-aware** for both Lua (all
three buffer modes) and JS (the slice-5.3 generic `run_batch` path): if the
script left a write column bit-identical to its pre-script value, the runtime
skips the `copy_from_slice` back into the ECS column. Granularity:

- **Lua Table mode** is **per-element** — only the slots whose value
  actually changed are stored back. Free upgrade because the copy-out loop
  already does one `raw_get` per slot.
- **Lua UserData / Buffer + JS** is **per-column** (whole axis) — a single
  slice `==` compare gates the bulk `copy_from_slice`.

Scripts cannot observe the elision. Comparison is by **bit pattern**, so
`-0.0` and `+0.0` are treated as distinct (any ECS observer that cares about
the difference is preserved), and `NaN` columns are always reported dirty
(sound but conservative — `NaN == NaN` is false).

The `batch_dirty_bytes` profiling counter now reports the **post-skip**
writeback (vs `batch_copy_out_bytes`, which keeps its meaning of the
would-be writeback budget). Their ratio reveals the elision rate; a
script whose `update` does nothing produces `batch_dirty_bytes == 0`.

For A/B testing, set `JENGINE_BATCH_DIRTY_DISABLE=1` before launching to
bypass the compare and always copy (matches the pre-5.4b code path). On
typical x86 hardware the bandwidth wins are inside noise — the value of
this slice is the counter visibility, which downstream consumers (ECS
change-detection, replication, render dirty culling) can leverage.

### `update(ctx, q)`

```js
function update(ctx, q) {
  const dt  = ctx.dt;                  // f32, frame delta seconds
  const n   = q.count;                 // integer, number of entities in this batch
  const vx  = q.read.Velocity2D.x;     // Float32Array, length n
  const vy  = q.read.Velocity2D.y;
  const px  = q.write.Position2D.x;    // Float32Array, length n (mutate in place)
  const py  = q.write.Position2D.y;
  const acc = q.state.acc;             // Float32Array, length n (Slice 5.1 JS / 5.1b Lua, mutate in place)
  for (let i = 0; i < n; i++) {
    px[i] += vx[i] * dt;
    py[i] += vy[i] * dt;
    acc[i] += dt;
  }
}
```

Notes:

- `i` is **chunk-local**, not entity-id. When the worker pool is enabled (`--batch-workers N`), each worker sees a contiguous chunk and `i` runs `0..chunk_size`. Scripts that need a stable per-entity identity must read it from a component, not from `i`.
- Mutations to `q.write.*` and `q.state.*` are copied back to ECS components after `update` returns. Reads from `q.read.*` are snapshot-only — writing to them has no effect.
- Per-entity `BatchScriptComponent.state: Vec<f32>` stores the field values in alphabetical schema order. Scenes serialize state via serde; the field is `#[serde(default, skip_serializing_if = "Vec::is_empty")]` so scripts with no state add nothing to the JSON.

---

## Behavior Graph (Step 7)

A **behavior graph** is a declarative, zero-VM-overhead alternative to scripts
for entities whose logic can be expressed as a graph of predefined ops. The
graph compiles to a flat `Vec<OpInstruction>` interpreter — no FFI crossings,
no allocations on the hot path.

Use a behavior graph when:
- The behaviour is data-driven and designer-editable (move with input, bounce,
  despawn after N seconds, play sound on collision)
- You want it to run on **every matching entity** in the scene at native speed
- The entity also carries a script (`BehaviorGraphComponent` and
  `ScriptComponent` can coexist; scripts win write conflicts)

Use a script when:
- Behaviour requires complex control flow, custom data structures, or dynamic
  state that doesn't fit in a typed component column
- Logic is unique to one entity

### Scene component

```json
{
  "behavior_graph": {
    "graph_path": "behaviors/enemy_basic.behavior",
    "overrides": {
      "move_speed_node": { "speed": 180.0 }
    }
  }
}
```

`overrides` is a map of node id → parameter overrides. When absent (the common case) the field is omitted from the JSON.

### `.behavior` asset format

```json
{
  "name": "Moth",
  "version": 1,
  "nodes": [
    { "id": "tick",   "op": "OnTick" },
    { "id": "prey",   "op": "GetNearestByTag", "params": { "tag": "player" } },
    { "id": "there",  "op": "GetEntityPosition", "in": { "entity": { "from": "prey" } } },
    { "id": "go",     "op": "MoveToward",
      "in": { "target_pos": { "from": "there" }, "speed": { "value": 90.0 } } }
  ],
  "edges": [
    { "from": "tick", "to": "go" }
  ]
}
```

Files live in `assets/behaviors/` and use the `.behavior` extension.

Two kinds of wiring, and they are easy to confuse:

- **`edges` carry control.** An edge says "when this runs, run that next". It
  goes from a trigger (or a gate, or an `If`'s `then` / `else` - name the port
  with `"out"`) to an op that *does* something. Reads and pure maths are never
  on an edge.
- **`in` carries data.** Each input pin is either a literal
  `{ "value": ... }` or a reference to another node's output,
  `{ "from": "node", "out": "pin" }`. Left off, `out` means the node's
  **primary** output - the first one listed in the reference below, and for
  most ops the only one. Two triggers have a second: `OnCollision.enter`
  (did the contact start or end) and `OnMouseClick.button`. An `out` naming nothing is a compile error. A
  reference may add `"scale"` and `"offset"`, applied numerically.
- **Reads and pure maths run when something that runs needs them** - they
  are never on an edge. `GetNearestByTag` above runs once per tick because
  `go` does. They can take a trigger's output too: `MathFloat` of
  `OnEvent.value` is fine. (Until Sep 2026 neither was true - `out` was
  ignored, and a trigger's output could feed an action but not arithmetic.)

`params` are values baked in at authoring time - a tag, an event name, a
prefab id. The generated reference below says which each op takes.

### Graphs and scripts share one event bus

Since Sep 2026, both ways, in every runtime:

- **A script's `send_event(name, sender, data)` reaches every graph's
  `OnEvent` with that name.** `OnEvent` has one payload pin and it is a
  float, so `data` becomes a number: a number is itself, a numeric string is
  that number, a table with a numeric `value` is that, and anything else is
  `0`. A targeted `send_event_to` is aimed at one script and does not reach
  graphs. An animation frame's `EmitEvent` does, since it travels the same
  bus.
- **A graph's `EmitEvent` reaches scripts' `on_<name>(entity, data)`** as
  well as other graphs, with `data = { value, x, y }` - its `value` pin if
  wired, and **where the emitting entity was** when the op ran. A frame late,
  as trigger and contact callbacks are: graphs run after scripts.

Before that the two buses never met, so a graph could not tell the game
anything and a script could not tell a graph anything.

**`OnDespawn` runs after the entity has gone.** Its position, rotation and
scale read as they were last seen - so a death burst or a split lands where
the thing died - but anything else on the entity (a custom field, a
velocity) reads zero. Put what a death handler needs into the graph as a
literal, or emit it earlier.

**`SetCustomField` creates the component entry** if the entity has none yet,
as a script's `set_custom` does. It used to write only into an entry that
already existed, which silently discarded a graph's state unless the prefab
happened to pre-author it.

### Op reference

<!-- BEGIN GENERATED BEHAVIOR OPS -->

68 ops, generated from `behavior::catalog::op_catalog` - the same catalog the editor's node palette reads.

**Triggers** - roots: each starts a program when its event happens

| Op | Inputs | Outputs | Params | Does |
|---|---|---|---|---|
| `OnTick` | - | - | - | Fires once per frame for every entity carrying the graph. Read the frame's dt with GetDeltaTime. |
| `OnSpawn` | - | - | - | Fires the first frame the entity exists. |
| `OnDespawn` | - | - | - | Fires the frame the entity is despawned. Position, rotation and scale read as they were last seen; anything else on the entity is already gone. |
| `OnCollision` | - | `other`: Entity, `enter`: Bool | - | Fires on collision enter (`enter = true`) or exit (`enter = false`). |
| `OnTrigger` | - | `other`: Entity | - | Fires when another entity enters this entity's TriggerArea. |
| `OnTimer` | - | `tick_index`: Float | `interval_seconds`: Float = 1 | Fires every `interval_seconds`, persistently per-instance. |
| `OnKey` | - | - | `key`: String (required), `edge` (pressed / released / held) = "pressed" | Fires when a key transitions (pressed/released) or every frame while held. Only one OnKey per graph. |
| `OnEvent` | - | `value`: Float | `name`: String (required) | Fires when a named event arrives - from another graph's EmitEvent, or a script's send_event (a number, a numeric string, or a table's `value`; else 0). |
| `OnMouseClick` | - | `pos`: Vec2, `button`: Float | `button` (left / right / any) = "any" | Fires when the chosen mouse button transitions to pressed. Outputs click position and button index (0=left, 1=right). |

**Reads** - component values into pins; no writes

| Op | Inputs | Outputs | Params | Does |
|---|---|---|---|---|
| `GetInputAxis` | - | `value`: Vec2 | - | Reads the current 2D input axis (e.g. WASD / left stick). |
| `GetPosition` | - | `value`: Vec2 | - | Reads the entity's Transform.position. |
| `GetVelocity` | - | `value`: Vec2 | - | Reads the entity's Velocity2D. |
| `GetNearestByTag` | - | `value`: Entity | `tag`: String (required), `exclude_self`: Bool = false | Finds the nearest entity carrying `tag` and outputs its EntityRef (empty if none). Set `exclude_self` to skip this entity (separation/flocking). |
| `GetEntityPosition` | `entity`: Entity | `value`: Vec2 | - | Reads the Transform.position of the referenced entity. An empty or invalid ref resolves to this entity's own position (so movement toward it is a no-op). |
| `GetCustomField` | - | `value`: Float | `component`: String (required), `field`: String (required) | Reads a numeric field from a custom component on this entity (0.0 if missing). |
| `GetRotation` | - | `value`: Float | - | Reads the entity's Transform.rotation (radians). |
| `GetScale` | - | `value`: Vec2 | - | Reads the entity's Transform.scale. |
| `GetDeltaTime` | - | `value`: Float | - | Reads the current frame's `dt` (seconds). |

**Pure** - arithmetic and logic on pins

| Op | Inputs | Outputs | Params | Does |
|---|---|---|---|---|
| `Compare` | `a`: Float, `b`: Float | `value`: Bool | `op` (Eq / Ne / Lt / Le / Gt / Ge) (required) | Compares two scalars and produces a Bool. |
| `Not` | `in`: Bool | `value`: Bool | - | Inverts a Bool. |
| `BoolOp` | `a`: Bool, `b`: Bool | `value`: Bool | `op` (And / Or / Xor) (required) | Binary boolean op (And/Or/Xor) on two Bools. |
| `SelectFloat` | `cond`: Bool, `a`: Float, `b`: Float | `value`: Float | - | Ternary float select: `cond ? a : b`. |
| `SelectVec2` | `cond`: Bool, `a`: Vec2, `b`: Vec2 | `value`: Vec2 | - | Ternary Vec2 select: `cond ? a : b`. |
| `MakeVec2` | `x`: Float, `y`: Float | `value`: Vec2 | - | Pack two Floats into a Vec2. |
| `GetVec2X` | `in`: Vec2 | `value`: Float | - | Extract Vec2.x as a Float. |
| `GetVec2Y` | `in`: Vec2 | `value`: Float | - | Extract Vec2.y as a Float. |
| `MathFloat` | `a`: Float, `b`: Float | `value`: Float | `op` (Add / Sub / Mul / Div / Min / Max / Mod / Pow) (required) | Binary float arithmetic. Div/Mod by zero outputs 0. |
| `MathVec2` | `a`: Vec2, `b`: Vec2 | `value`: Vec2 | `op` (Add / Sub / Mul / Div / Min / Max) (required) | Componentwise binary Vec2 arithmetic. |
| `ScaleVec2` | `v`: Vec2, `s`: Float | `value`: Vec2 | - | Multiply a Vec2 by a scalar. |
| `Length` | `in`: Vec2 | `value`: Float | - | Length of a Vec2. |
| `Normalize` | `in`: Vec2 | `value`: Vec2 | - | Unit-length copy of a Vec2 (zero-length → ZERO). |
| `Distance` | `a`: Vec2, `b`: Vec2 | `value`: Float | - | Euclidean distance between two Vec2s. |
| `Dot` | `a`: Vec2, `b`: Vec2 | `value`: Float | - | Dot product of two Vec2s. |
| `Lerp` | `a`: Float, `b`: Float, `t`: Float | `value`: Float | - | Linear interpolation a + (b - a) * t. t is not clamped. |
| `Clamp` | `in`: Float, `min`: Float, `max`: Float | `value`: Float | - | Clamp `in` into `[min, max]`. |
| `MathUnary` | `in`: Float | `value`: Float | `op` (Abs / Sign / Neg / Sqrt / Sin / Cos / Tan / Floor / Ceil / Round) (required) | Unary float math. Sqrt of negatives outputs 0. |
| `Atan2` | `y`: Float, `x`: Float | `value`: Float | - | atan2(y, x) in radians. |

**Gates** - control flow, some with per-instance state

| Op | Inputs | Outputs | Params | Does |
|---|---|---|---|---|
| `If` | `cond`: Bool | - (branches: then / else) | - | Routes execution down `then` or `else` based on `cond`. |
| `Cooldown` | `seconds`: Float | - | - | Suppresses downstream execution until `seconds` elapse since last fire. |
| `Once` | - | - | - | Fires downstream the first time only; latches per-instance. |
| `Delay` | `seconds`: Float | - | - | Suppresses downstream until `seconds` have elapsed since first visit, then fires every tick. |
| `ForCount` | `count`: Float | - | - | Fires downstream up to `count` times per instance, then latches off. |

**Direct writes** - applied immediately, in the tick

| Op | Inputs | Outputs | Params | Does |
|---|---|---|---|---|
| `SetVelocity` | `vel`: Vec2 | - | - | Overwrites Velocity2D. |
| `AddVelocity` | `delta`: Vec2 | - | - | Adds to Velocity2D. |
| `Translate` | `delta`: Vec2 | - | - | Adds to Transform.position. |
| `ApplyForce` | `force`: Vec2 | - | - | Adds to Force2D. |
| `LookAt` | `target_pos`: Vec2 | - | - | Rotates Transform to face `target_pos` (Vec2-only in v1). |
| `MoveToward` | `target_pos`: Vec2, `speed`: Float | - | - | Moves Transform.position toward `target_pos` at `speed` units/sec (Vec2-only in v1). |
| `SetPosition` | `pos`: Vec2 | - | - | Overwrites Transform.position with `pos`. |
| `SetRotation` | `rot`: Float | - | - | Overwrites Transform.rotation (radians) with `rot`. |
| `SetScale` | `scale`: Vec2 | - | - | Overwrites Transform.scale with `scale`. |
| `SetSpriteColor` | `r`: Float, `g`: Float, `b`: Float, `a`: Float | - | - | Overwrites SpriteComponent.color (RGBA, each clamped to [0,1]). `a` defaults to 1. |
| `SetSpriteAlpha` | `alpha`: Float | - | - | Overwrites only the alpha channel of SpriteComponent.color (clamped to [0,1]). |
| `AddTag` | - | - | `tag`: String (required) | Inserts `tag` into the entity's Tags (creates Tags if absent). |
| `RemoveTag` | - | - | `tag`: String (required) | Removes `tag` from the entity's Tags. No-op if absent. |
| `SetCustomField` | `value`: Float | - | `component`: String (required), `field`: String (required) | Sets `component.field = value` on the entity's CustomComponents, creating the component entry if needed (as set_custom does). |

**Setup components** - idempotent - wire them from a one-shot trigger, not `OnTick`

| Op | Inputs | Outputs | Params | Does |
|---|---|---|---|---|
| `BounceOnBounds` | - | - | `bounds`: Aabb | Idempotently adds/updates a BounceArea component. Wire from a one-shot trigger, not OnTick. |
| `DespawnAfter` | `seconds`: Float | - | - | Adds a Lifetime component that despawns the entity after `seconds`. Does not reset if already present. |

**Structural commands** - deferred to the barrier after the tick, like a script's

| Op | Inputs | Outputs | Params | Does |
|---|---|---|---|---|
| `SpawnEntity` | `at`: Vec2, `rotation`: Float, `scale`: Vec2, `with_velocity`: Vec2 | - | `prefab_id`: String (required) | Queues a deferred spawn of `prefab_id` at `at`. Optional rotation (radians), scale (Vec2, defaults to (1,1)), and initial velocity. |
| `Despawn` | - | - | - | Queues a deferred despawn of the current entity. |
| `DespawnByTag` | - | - | `tag`: String (required) | Queues despawn of every entity whose Tags component contains `tag`. Resolved at the command barrier. |
| `ChangeScene` | - | - | `scene_id`: String (required) | Surfaces a scene-change request to the host (runtime wires the swap). |

**Side effects** - deferred to the barrier

| Op | Inputs | Outputs | Params | Does |
|---|---|---|---|---|
| `Sequence` | - | - | - | No-op passthrough; useful as an editor-side label/grouping anchor. |
| `PlaySound` | `volume`: Float = 1 | - | `sound_id`: String (required) | Queues a deferred sound playback. |
| `PlayAnimation` | - | - | `anim_id`: String (required) | Queues a request to start `anim_id` on the self entity (host wires to animator). |
| `StopAnimation` | - | - | - | Queues a request to halt the animation playing on the self entity. |
| `EmitEvent` | `value`: Float | - | `name`: String (required) | Sends a named event with an optional `value`: to every graph's OnEvent, and to scripts' `on_<name>(entity, data)` as `{ value, x, y }` - where this entity was. |
| `Log` | `value`: Float | - | `label`: String (required) | Prints `[Behavior] label` (or `label: value` if `value` is connected) to the engine log. |

<!-- END GENERATED BEHAVIOR OPS -->

### Telemetry

Frame counters exposed by `ScriptSystem::take_frame_metrics()`:

| Counter | Meaning |
|---------|---------|
| `behavior_commands_emitted` | structural commands queued this frame |
| `behavior_overbudget_ms` | cumulative ms over soft budget |
| `behavior_critical_overbudget_ms` | cumulative ms over hard budget |

Full spec: `docs/step7-behavior-graph-spec.md`.

---

## Browser Adapter Parity

The browser JavaScript adapter (`adapters/js/browser.rs`, used by `jengine-web`, `jengine-editor-viewport`, `jengine-benchmark-web`) defines every global as a JS function that proxies through `window.__jengine_play`; the Fengari Lua runtime binds the names in `BRIDGE_FUNCTIONS` (`editor/src/lib/lua-runtime.ts`) to them and gathers them into the same namespaces native builds. Name-level agreement with native is guarded by `script_global_parity` and `host_command_parity`; behavioural differences are registered in `docs/parity-gaps.md`.

**P7.7 wedges 1–2.5 status:** All native bridge names are now **available as flat globals** on the browser adapter. Wedge 1 added `find_by_name`, `find_by_tag`, `on_event`, `set_post_process_float`, `set_post_process_vec4`. Wedge 2 added `get_name`, `get_mouse_position`, `is_mouse_left_just_pressed`, `set_ambient_light`, `set_light_color`, `set_light_intensity`, `set_light_radius`, `contact_count`, `get_contact`. Wedge 2.5 added `get_material` and `gui_focus`. Scripts using the canonical bridge names now work portably across both targets with no remaining gaps.

**Browser-only surface (do not rely on for portable scripts):**

Convenience helpers the browser preamble defines that Luau does not, because
the language already has them:

| Browser name | Use instead under Luau | Notes |
|---|---|---|
| `clamp(v, lo, hi)` | `math.min(math.max(v, lo), hi)` | |
| `distance(x1, y1, x2, y2)` | `math.sqrt(dx*dx + dy*dy)` | |
| `angle_between(x1, y1, x2, y2)` | `math.atan2(dy, dx)` | Luau has `math.atan2`; Fengari does not — see the dialect note in CLAUDE.md |
| `get_delta_time()` | the `dt` parameter to `update` | |

Aliases — both spellings work everywhere, the left is legacy:

| Legacy name | Preferred (Lua) | Flat name (JS) |
|---|---|---|
| `get_entity_by_name(name)` | `Entity.find_by_name(name)` | `find_by_name` |
| `get_entities_by_tag(tag)` | `Entity.find_by_tag(tag)` | `find_by_tag` |
| `register_event_listener(name, e)` | `Event.on(name, e)` | `on_event` |
| `set_post_process_param_float` / `_vec4` | `PostProcess.set_float` / `set_vec4` | `set_post_process_float` / `_vec4` |

`Physics.get_contacts()` (all rows at once) and `Physics.contact_count()` + `Physics.get_contact(i)` (one at a time) both exist everywhere.

**Genuinely missing from Luau (native):**

| Function | Notes |
|---|---|
| `raycast(ox, oy, dx, dy, max_dist)` | **Works everywhere since Sep 2026.** It was the last genuinely-missing function: unbound on native, and bound-but-inert in the editor and on web, where it read a `state.physics_raycast` hook no host installed and returned `null`. Both halves are closed and no guard is needed. |

> This table used to list ~30 functions as "not yet on bridge", including the
> profiling getters, the joint API, `set_gravity`, `set_body_type`, `lerp` and
> the tween API. **That was stale** — every one of them is bound in
> `adapters/luau.rs`, verified by compiling a Luau script and checking which
> globals are non-nil, not by reading the source. If you are about to write a
> `if fn == nil` guard because of something you read here, check first.

**Native-only surface (missing from browser adapter):**

None. Every bridge function listed in the [API Index](#api-index) is now reachable from the browser adapter.

**Browser runtime caveats** (function exists but is a no-op / partial today): `gui_show`, `gui_hide`, `gui_set`, and `gui_focus` are accepted on the browser adapter and queue commands, but the WASM runtime does not render or apply them — commands are silently dropped. `gui_on_click` **does** fire on web (P7.7 wedge 14): the WASM runtime loads `.gui` documents referenced by `GuiComponent::gui_id` on scene boot, hit-tests mouse-down events against `compute_layout`, and invokes the registered callback. Wedge 14.1 closes the scene-swap gap: new `.gui` references introduced by post-boot scene loads are now async-fetched alongside scripts and textures. The UI helpers (`create_ui_text`, `create_ui_panel`, `set_ui_*`) remain queued-but-dropped. `get_material` returns the material id stored on `MaterialComponent` but does not yet reflect runtime overrides. These will be progressively enabled as the WASM runtime grows the matching subsystems.

When writing scripts intended to run in both targets today, restrict yourself to the intersection of both surfaces (or use a small adapter shim in your script).

---

## Determinism (Batch Scripts)

> Scope: **batch scripts** dispatched through `BatchScriptPool` (the data-oriented API used by `BatchScriptComponent`). Per-entity script invocations through the bridge runner are **not** covered by this section.

Slice 5.5c shadows `Math.random()` (JS) and `math.random()` (Lua) with a deterministic xorshift64* PRNG, seeded per-batch from the tuple:

```text
seed = mix_seed(scene_seed, frame, system_id, chunk_id)
```

- `scene_seed` — set once via `BatchScriptPool::set_scene_seed(u64)`. Default `0`.
- `frame` — bumped by `BatchScriptPool::advance_frame()` (called from `run_batch_scripts` once per frame).
- `system_id` — `DefaultHasher::hash(script_path) as u32` so two different scripts in the same frame get independent streams.
- `chunk_id` — the worker-pool chunk index (0 for single-chunk and Direct mode).

`mix_seed` chains three `splitmix64` finalizers. The resulting `u64` reseeds a per-worker `DetRng` immediately before the script's `update` runs.

### Reproducibility guarantees

| Property | Guaranteed | Notes |
|---|---|---|
| Same `scene_seed`, same N workers, same workload, two runs | **Bit-identical** writes | CI tested in `tests/script_determinism.rs` (`*_is_reproducible_across_runs`). |
| Chunk-0 prefix across N ∈ {1, 2, 4, 8} | **Bit-identical** for the leading `N_ENTITIES / max_workers` entities | Chunk-0 always seeds with `chunk_id=0`. CI tested (`*_chunk0_prefix_matches_across_worker_counts`). |
| Full-column equality across different N | **Not** guaranteed | Entities in chunk 1+ draw from a different stream than the same entities under N=1. |
| `system.random.seed` / `os.time()` | Not seeded | Scripts that bypass `Math.random` / `math.random` are not reproducible. |

For full N-independence, seed your own per-entity RNG inside the script using a stable identifier (e.g., entity index passed via a read column).

---

## Maintaining This Document

This document is the contract between script authors and the engine, and
it is upstream of more than itself: the documentation site regenerates
`api-reference/scripting-api.mdx` from it, `public/ai/llm-context.md` is
this document with a preamble, and `public/ai/api-spec.json` — what AI tools
are told the engine can do — takes its function list from the API Index
below and each function's description from its `reg!`. A function missing
here is missing from all of them.

When you add or change a script-facing function:

1. **Wire every runtime.** `ecs/systems.rs` apply arm + `BridgeCommand` +
   `ScriptCommand` + `bridge/translator.rs` + `adapters/luau.rs` +
   `adapters/js/browser.rs` + `BRIDGE_FUNCTIONS` in
   `editor/src/lib/lua-runtime.ts` + the viewport and web
   `drain_play_commands_into` + its namespace row in
   `scripting/namespaces.rs` + a `reg!` in `package/core_script_api.rs`.
   Missing the `BRIDGE_FUNCTIONS` entry is the classic failure: the
   function works on native and is `nil` in the editor, which is where you
   develop. **The `reg!` is not optional either**: its description and
   argument types are the editor's tooltip, the index's signature (without
   one the index prints `(…)`), and the description AI agents read in
   `api-spec.json`. Until Oct 2026 sixty bound functions had none, so the
   spec described each as `<Namespace> API function.`.
2. **Regenerate the index** — it is generated, not hand-written:

   ```powershell
   cargo run -p jengine-core --example script_api_index
   ```

   It reads the `ScriptApiRegistry` (every `reg!` in `core_script_api.rs`,
   installed dynamically on the VM), the `globals.set(…)` calls in
   `adapters/luau.rs`, and `BRIDGE_FUNCTIONS` for the Runtimes column.
   The older advice here was to grep only `globals.set(…)`, which misses
   every registry function — that is how the index fell 50 behind.

3. **Write the prose section** for the function. The index gives the
   signature; the section explains the behaviour, and nothing generates
   that.
4. **Push it downstream:** `npm run sync` in `website/` regenerates the
   site pages and the AI spec.

### Gates

```powershell
cargo test -p jengine-core --test script_api_index_is_current  # index matches the code
cargo test -p jengine-core --test script_global_parity          # every runtime has it
cargo test -p jengine-core --test preamble_command_tags         # command tag is unique
cargo test --workspace
```

The first two are ratchets: `script_global_parity` lists the known
native-only functions and fails on a new one *and* on a listed one that has
since been fixed, so the list cannot rot.
