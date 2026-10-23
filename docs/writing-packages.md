# Writing a jEngine package

A package is a Rust crate that implements `JEnginePackage`. It can add ECS
components, systems that run inside the engine's frame, script API metadata,
and editor extensions.

This document is the contract. If something here is wrong, fix the doc — the
package system spent a long time *looking* like it worked while
`register_systems` was called by nobody (see `projects/engine-test-lab/BUGS.md`
#13), and the cure for that is a description that matches the code.

## The shape of a package

```rust
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::script_api::ScriptApiRegistry;
use jengine_core::package::system_schedule::{SystemPhase, SystemSchedule};
use jengine_core::package::{JEnginePackage, PackageMetadata};

pub struct MyPackage;

impl JEnginePackage for MyPackage {
    fn metadata(&self) -> PackageMetadata {
        PackageMetadata {
            name: "my-package".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "What it does".into(),
            author: String::new(),
            engine_version: Some("0.1.0".into()),
            dependencies: vec![],
        }
    }

    fn register_components(&self, registry: &mut ComponentRegistry) {
        registry.register::<MyComponent>("my_component", "My Component", "Custom");
    }

    fn register_systems(&self, schedule: &mut SystemSchedule) {
        schedule.add_system("my_system", SystemPhase::PreUpdate, 0, |ctx| {
            let (world, dt, resources) = ctx.split();
            for (_, my) in world.query_mut::<&mut MyComponent>() {
                my.timer += dt;
            }
            let _ = resources;
        });
    }

    fn register_script_api(&self, _api: &mut ScriptApiRegistry) {}
}
```

**For a game's own package, start in the editor**: File > *New Rust
Package* (also in the Packages dialog and the command palette) scaffolds
`templates/package/` into `<project>/packages/<name>/`, lists it in the
project's `project.jengine`, and offers the viewport rebuild that links it.
The name is checked as it is typed by the builder's own rule
(`packages::validate_package_name`: a crate name, and not one whose script
namespace the engine already has - `sprite` would be `Sprite`). The CLI does
the same with `--project`:

```
jengine package new tide-rules --project projects/my-game   # in a project, listed
jengine package new my-package                  # a full Rust package in ./packages
jengine package new my-sounds --type asset      # assets only, no crate
jengine package new my-lib --type script        # Lua/JS only, no crate
```

A package made in a project is **standalone** - its `Cargo.toml` carries an
empty `[workspace]` table - because a project inside this repository would
otherwise be claimed by the engine's workspace, which does not list it.

The other package commands: `jengine package list`, `jengine package add <name>
--path <path>` (or `--git <url> [--tag <tag>]`), `jengine package remove <name>`,
and `jengine package resolve`, which fetches every declared package to a local
path and emits the Cargo dependencies.

The Rust scaffold is a working package with one of each thing a package
adds - a component, a system (a free function, so a test can call it on a
world of its own), a script command, and state **published** into the
entity's custom component for scripts to read back, since a command returns
nothing - plus the rule itself as a plain function and three tests. Its path to `jengine-core` worked out from where
it lands (inside a project that is four levels up, not two; the fixed
`../../crates/jengine-core` the template used to carry broke every package
scaffolded in a project), and names its type the way the builder links it
(`cool-fx` → `cool_fx::CoolFxPackage`, also written as `entry_point` in its
`package.toml`). Until Oct 2026 it named the type `CoolFx`, which the
generated runtime crate then failed to find, and its template taught the
native-only `register_script_api`. A package in this repository must also be
added to the root `Cargo.toml` workspace `members`, or `cargo test --workspace`
never builds it.

## Documenting it

The package's page on the documentation site is **generated** from the
package (`website/scripts/extract-packages.ts`, run by `npm run sync`): the
components, the script commands under their namespace, the dependencies, and
the projects and scenes that use it are read from the source, and the prose
is the package's `README.md` - or, with no README, the `//!` crate doc at the
top of `src/lib.rs`. So write the README the way the generated page cannot:
what the package is for, what each component means, what each command does,
what scripts read back, and what the tests prove. Name commands the way
scripts call them (`Platformer.input(e, ...)`), not by their flat names.

The sync **fails** on a package with no documentation, or with the scaffold's
`TODO`s still in it. The repository's Claude Code hooks
(`.claude/hooks/package-docs.mjs`) regenerate a package's page after an edit
to it and refuse to end a turn while a package is undocumented.

## Reading the project's data tables

A package can read the project's `.data` tables - the ones scripts read
with `Data.get` and designers edit in the table editor - through
`jengine_core::data::global()`, which returns the loaded `DataRegistry`
once a runtime has published it:

```rust
let Some(reg) = jengine_core::data::global() else { return };  // not yet
for id in reg.row_ids("data/cards") {
    let row = reg.row("data/cards", id).unwrap();   // fields as JSON
}
```

The native runner and the web runtime publish the tables when they load
them at startup; the editor viewport publishes them when the editor seeds
them for Play, which can be a frame or two after the scene starts - so read
lazily, and wait rather than fail when `global()` is `None`.
A deckbuilder can build its whole game from a handful of tables this way -
cards, enemies, relics, encounters - with no content in its Rust at all.

## Timing to the music, and to the moment of an input

Two clocks a package cannot get from its `dt`:

- `jengine_core::audio::music_position()` is the music's playhead in
  seconds, or `None` with no music. Every runtime publishes it after its
  audio update (scripts read the same value as `Audio.get_music_position()`). It
  is coarse - the audio device reports in buffer steps of a few
  milliseconds, and it is a frame old - so run your own clock on `dt` and
  ease it toward each fresh reading, as a rhythm game's song clock does.
- `jengine_core::profiling::now_ms()` is a monotonic wall clock on every
  platform (`Instant` panics on wasm32). Stamp a script command with it
  when the moment it arrived matters: the native runner runs packages
  after scripts and the editor viewport runs them before, so "the next
  step" is a frame later in one than the other. A rhythm game stamps each
  press this way and subtracts the difference at its next step.

## A clip format of your own

A package can play a file type the audio backends cannot decode by
rendering it to PCM when it loads. Implement `register_audio`:

```rust
fn register_audio(&self, audio: &mut AudioRegistrar) {
    audio.clip_transcoder("song", transcode_song); // fn(&[u8]) -> Result<Transcoded, String>
}
```

Both `load_clip`s - kira natively, Web Audio in the editor and web builds -
ask `jengine_core::audio::transcode` before decoding, so a clip whose id
ends in `.song` reaches your function in every runtime, and from then on it
is an ordinary clip to the music director, buses and scripts. Return stereo
`left` / `right`, the `rate`, and a `loop_start` frame if a looping play
should go back somewhere other than the top. The registry is the process's
(the backends do not know the package host). Native `audio_refs` preloading
calls `transcode::prepare`, so the render happens while the scene loads
rather than on the frame it first plays. `jengine-tracker` is the worked
example; if your format is one of the engine's own, add it to `KNOWN` in
`audio/transcode.rs` so a project without the package is told which to add.

## Moving things through a level

A package cannot reach the physics world - the runtimes own it - but it can
steer what the physics step moves. The engine's `CharacterController2D` is
the seam: a package writes the velocity it wants onto the component in
`ScriptExec`, and the physics step moves the body, slides it along walls
and slopes, and writes back `grounded`, the ground's normal, `wall`,
`ceiling` and everything it touched. The package reads those on the next
frame.

The shape that works is a pure function from (input, what the controller
reported) to (velocity, events) - a platformer hero written that way has the
whole feel of a platformer is tested without a world, and only the
thin outer layer needs physics to test.

## Owning a physics world

When steering through a component is not enough - a force at a point on
every fixed step, or which point of a body a contact touched - a package can
own a **rapier world of its own**: depend on `rapier2d` (the workspace
version), keep the sets in the package's state, step them at a fixed rate
from `dt`, and write the poses back to plain sprites. Nothing in it is the
runtime's, so no host is wired and it is the same in every runtime.
A car-combat game is the case it was built for: cars as bodies pushed by four
tyre forces every 1/120 s, and damage read from each contact's point in the
car's own frame - neither of which the runtime's `PhysicsWorld` would hand a
package.

The trade: those bodies are invisible to everything else - the runtime's
colliders, `Physics.raycast`, triggers, the F4 outlines - so keep to it only
when the package owns everything that has to collide. And note what bit
that package first: **`RigidBody::reset_forces` does not reset torques.**
A force added at a point is also a torque; reset only the forces each step
and the torque piles up until every body spins. Call `reset_torques` too.

## Writing into a `Tilemap`: where cell (0, 0) is

**A tilemap's transform is the top-left corner of cell (0, 0), with rows
running down.** `Tilemap::tile_to_world` is where that rule lives, and
`sync_tilemap_colliders` builds its boxes with the identical arithmetic, so
the drawn tiles and the collidable ones always agree with each other. What
they will not agree with is a package that has its own idea.

This is worth stating because a *centred* grid is the natural thing to write
and the natural thing for a scene to author - the camera, the backdrop and the
HUD are all placed about the middle of the map. So a package that publishes
world coordinates about the centre must move the transform to the corner
itself:

```rust
// The scene authors the centre; the engine wants the corner.
tr.position.x = centre.0 - width as f32 * tile * 0.5;
tr.position.y = centre.1 + height as f32 * tile * 0.5;
```

Read the authored position **once**, before anything derives from it - after
the first write the transform *is* the corner, and reading it again as the
centre walks the map half its own size every time the level is rebuilt.

The first two packages that wrote a tilemap both got this wrong, and it does
not look like a coordinate bug from the outside. In a bomb-arena game the arena
drew in one quadrant while the players walked around another, which was
reported as "the players are not spawned at the grid". In a dig-and-build game
the hero was put down 53 tiles above the top of the terrain, which was
reported as "the player just drops" - a physics complaint about a map that was
never under them. If your package writes a tilemap, give it a test that the
positions it publishes land on the tiles the engine draws.

**Ask the map, do not do the arithmetic.** Since Oct 2026 a tilemap can be
isometric (`orientation`), and then "column times tile width" is wrong.
`Tilemap::grid_to_local` / `local_to_grid` are the grid's shape, and
`tile_to_world` / `world_to_tile` / `cell_quad` are built on them; a package
that goes through them works on either kind of map.

**Build `Tilemap` and `TilemapLayer` with `..Default::default()`.** Both
have a `Default` (a layer's is visible), and both grew fields in Oct 2026
(`orientation`; `offset` and `y_sort`) - every literal that spelled out all
of them stopped compiling, five packages among them. Name the fields you
mean and default the rest, and the next field costs you nothing.

## Changing a texture at runtime

Change the pixels in place and say which rectangle changed:

```rust
let tex = assets.texture_bytes_mut("terrain#mine").unwrap();
tex.rgba[i * 4 + 3] = 0;
assets.mark_texture_dirty("terrain#mine", x, y, w, h);
```

The next draw writes that rectangle to the GPU. No host needs telling,
because `get_wgpu_texture` answers `None` for a dirty texture and every host
uploads on `None`. A texture the project filters linearly has mipmaps, and those are patched
too: only the texels your rectangle reaches in each level are recomputed
and written, so painting into a large texture every frame (a racing game's skid
marks) costs what the paint covers, not what the texture holds. Keep the
rectangle tight - a whole-texture rectangle every frame is a whole rebuild.

Write into a **copy** if other sprites share the texture -
`ecs::carve` names its copies `"<texture>#carved:<entity bits>"` - and give a
scene's reload a way back to the original.

## Pooling sprites: a re-used slot must drop its `uv_rect`

A package that owns a pool of `SpriteComponent`s and re-points them every
frame - a common idiom for road strips, crowds of foes and map tiles -
has to invalidate the atlas lookup when a slot's `region` changes:

```rust
if s.region.as_deref() != want {
    s.region = want.map(str::to_string);
    s.uv_rect = None;
    s.region_resolved = false;
}
```

`resolve_sprite_regions` resolves a region **once** and then skips that sprite
for good, which is right for a scene entity and wrong for a pool. Without the
invalidation a slot keeps the first rect it ever resolved, so pooled sprites
draw as each other; and a slot that has never resolved anything draws the
region-less **whole atlas** for as long as it has no rect. Both were reported
together, as "the textures are flickering and maybe showing the whole atlas".

The pass runs at `PreRender`, after `ScriptExec`, so invalidating in your
system is resolved before anything is drawn - there is no wrong frame.

## Texture and material ids are interned

`SpriteComponent::texture_id`, `Mesh2D::texture_id` and
`MaterialComponent::material_id` are `jengine_core::asset::AssetId`s
(`TextureId` / `MaterialId`), not `String`s (Oct 2026). Equal ids share one
string, so the renderer sorts and batches by pointer rather than by `memcmp`.
Build one from any string with `.into()`, read it as a `&str` through
`as_str()` or deref, and compare it to a `&str` or `String` directly:

```rust
let sprite = SpriteComponent { texture_id: "sprites/hero.png".into(), ..Default::default() };
if sprite.texture_id == sheet { /* ... */ }
slot.texture_id = sheet.as_str().into();
```

It is `Copy`, so assigning one costs nothing. Interning one locks a table
the first time it meets a string, so make the id once and copy it rather than
building it from a string every frame in a hot loop.

## Composing packages: depend on the crate, and list both

Most packages depend on `jengine-core` and nothing else. A scene can use two
packages at once, but those are independent packages on one entity list, not
one package using another's types. A package that does depend on another
comes with a rule worth stating before the precedent spreads:

> **A crate dependency compiles a package in. It does not register it.**

`jengine-builder` writes one `host.add_package(Box::new(..))` per package
named in the project's top-level `"packages"` array. It does **not** walk
transitive dependencies. So a package reached only because another package
depends on its crate is compiled, linkable and completely unregistered: its
components never reach the `ComponentRegistry` and its script commands are
never installed. The scene still loads, the component key lands in
`CustomComponents`, and the first sign is `attempt to call a nil value` when a
player presses a button.

Say `my-dungeon` depends on `my-crawler` for its corridors and `my-battle`
for its fights. Then a project using `my-dungeon` lists **all three** of
`my-dungeon`, `my-crawler` and `my-battle`. `cargo test -p jengine-core
--test packages_a_project_lists_cover_their_dependencies` fails a project
that lists one and not the others, and names which.

Note what the failure looks like if the guard is not there, because it does
not point at the dependency: `my-dungeon` calls the crawler's code directly,
so the dungeon game keeps working. What breaks is a **different game** in the
same project, built on `my-crawler` alone, whose component is suddenly
unregistered. A
missing entry for a package you were not thinking about breaks a game you were
not looking at.

`PackageMetadata::dependencies` does not change this - it is declarative and
the builder does not read it. It is what the guard above reads, though, so it
has to be true: `every_cargo_dependency_on_a_package_is_declared` fails a
package that links a `jengine-*` crate without naming it. The other direction
is deliberately free, because the field means "needs that package present",
which is a superset of "links it" - a package that relies on the runtime's
physics declares `jengine-physics` and depends on no such crate.

### What a crate dependency is actually for

Reaching another package's **types** without a round trip through the ECS or
JSON. A package's modules are `pub`, though its types are generally not
re-exported at the crate root, so the path is
`my_crawler::game::CrawlGame` rather than `my_crawler::CrawlGame`.

### Two things composing found, both in the package being composed

Both are the same shape, and it is the shape to expect: **a package written as
a whole game keeps a piece of itself on its component**, where only its own
runtime can reach it. Composing pulls that piece out, and until it is pulled
out the second game gets a quietly incomplete version of it.

* **The picture.** A first-person crawler package built its view inside
  `CrawlGame::view_json` - the walls from `view::build`, then the monsters
  folded in and the whole thing sorted back to front. A second game could only
  reach `view::build`, which is the walls, so the composing game's first
  corridor had no monsters in it: invisible right up to the step that opened a fight, which
  reads as a monster teleporting into your face. It is `Crawl::picture` now and
  both games call it.
* **The turn loop.** A battle package advanced a foe's turn through
  `take_ai_turn`, driven by its own component's `BattleAi` request. A game
  driving a `Battle` directly gets no turns taken for it, and there is nothing
  in the type to suggest it should - so a fight opens, hands the first turn to
  a rat, and waits forever. That is a hung game rather than a lost one, and no
  test caught it because every test poked the fighters instead of playing. If
  you drive another package's model, **play it to the end in a test** with only
  the verbs your scripts have.

`Battle::check_over` became public for the same reason: it is called by the
turn loop, which is every way a fighter falls *inside* that loop and not the
only way one can fall. A composing package that folds hp in from elsewhere has
to be able to ask the model to re-decide.

The alternative - two packages sharing state through the `ResourceMap` - has
**never been done**: every resource read in this repo is a package reading one
it inserted itself, or the frame the host lends (`InputState`, `FrameCamera`,
see "What every host lends"). The resource
contract was written for runtime-versus-package arbitration, not for peers. If
you reach for it, you are first.

## Phases

A runtime owns a `PackageHost` and calls `run_phase` at each boundary of its
frame. Your system runs at the phase you register it with, and within a phase
in ascending `priority` order (lower runs first; negative means "before the
default").

| Phase | When it runs | Typical use |
|---|---|---|
| `PreUpdate` | before scripts | input processing, resetting per-frame state |
| `ScriptExec` | after the script system | reacting to what scripts queued |
| `Physics` | after the runtime's own physics step | reading this frame's simulation results |
| `PostPhysics` | after behavior/trigger flush | contact reactions |
| `Animation` | after skeletal/IK/hierarchy resolve | anything reading final transforms |
| `PreRender` | immediately before the draw | camera-relative work — parallax registers here |
| `Render` | **not run** — see below | — |
| `PostRender` | **not run** — see below | — |

### `Render` and `PostRender` are deliberately not run

A system registered there would execute and be unable to draw anything: the
renderer, sprite batch and GPU state are owned by the runtime and are not
shared through the `ResourceMap`. Running those phases would invite draw code
that silently does nothing, which is the exact failure this system already had
once. **A package cannot render yet.** Handing out renderer access safely is
its own design problem.

## Reaching engine state: resources

A system gets `&mut hecs::World`, `dt`, and a `ResourceMap`. The map holds
whatever the runtime chose to share, keyed by type:

```rust
let (world, dt, resources) = ctx.split();
if let Some(physics) = resources.get_mut::<PhysicsWorld>() { /* ... */ }
```

**Always handle the resource being absent, and do nothing when it is.** This is
not defensive padding — it is how the runtime decides who owns a subsystem. The
built-in `jengine-physics` package registers a `Physics`-phase system, but the
native runtime and the editor viewport both step rapier on their own hardcoded
path and deliberately do **not** share `PhysicsWorld`. The package's system
therefore runs and does nothing, instead of double-stepping the simulation. A
system that unwraps a resource would panic in every shipping runtime.

A system can also publish results back the same way — `resources.insert(value)`
— which is how contact events get out of the physics package.

### What every host lends, every phase

Since Sep 2026 both hosts that run packages (the native runtime and the
editor viewport) call `PackageHost::run_phase_with`, which puts two things in
the map for the length of each phase:

| resource | what it is | rule |
|---|---|---|
| `jengine_core::input::InputState` | the host's input for this frame - keys, actions, mouse, gamepads, typed text | **read it, never `remove` it.** It is moved in and back out (it owns maps that would allocate to clone); a system that takes it loses the frame's input for everything after, and the host logs an error. |
| `jengine_core::package::host::FrameCamera` | the `Camera2D` the frame is drawn through: position, zoom, viewport, rotation | `camera.0.screen_to_world(mouse, screen)` with the input's `mouse_position` and `screen_size` is the world under the pointer. |
| `jengine_core::asset::AssetManager` | the host's assets: atlases and regions, fonts, materials, GUI documents, skeletal clips | moved in and out like the input - **read it, never `remove` it**; a package that needs a region rect, a font's metrics or a material's definition reads it here rather than through a sprite's resolved `uv_rect`. |

Before this a package got a world and a `dt` and nothing else, so it could not
read a key: a platformer package had to be fed the player's input by a script
command every frame (`Platformer.input`), which is exactly the second-per-frame-caller
shape the profiling notes warn about. The test-lab's `testlab-spin` (scene 60)
is the worked example - hold Space and its bars spin faster - and
`packages_get_the_frame` in core is the guard that both hosts go through the
lending form.

Script commands are **not** in the map (a `FrameScriptCommands` resource used
to exist, read by `jengine-physics` and inserted by no host - it is gone); a
package's own script commands reach it through `register_script_commands`
handlers in every runtime (see below).

## Components in the editor

Register a component that has a `Default` with `register_defaulted` rather
than `register`, and the editor can add it and edit it with no more work:
its inspector is derived from that default (`editor_registry::derived_component_info`)
- numbers, flags, strings, vectors, colours by name, asset paths by name,
and an enum's options from serde. Without a `Default` only a component that
deserializes from `{}` derives one. Hand-written metadata
(`register_editor`, or an `editor.json`) still wins, for ranges and
descriptions the derivation cannot know.

## Components: your scene must be v2

`register_components` puts your component in the same `ComponentRegistry` the
scene loader consults. That only helps for **v2 scenes**:

- **v1** scenes deserialize into `ComponentBundle`, a fixed struct in
  `jengine-core` with no field for anyone else's component. Your key is
  unknown, gets collected into `CustomComponents`, and is effectively dropped.
- **v2** scenes (`"version": 2`) dispatch every key through the registry.

So a `.scene` that uses a package component must declare `"version": 2`. See
`projects/engine-test-lab/assets/scenes/59-packages.scene`.

A v2 entity names what it hangs from the way a v1 one does - `"parent":
"Body"` beside `"name"` and `"components"` - so a v2 scene can hold a rig.
Until Sep 2026 it could not, and a save dropped every `Parent` (BUGS #151).
A save also writes `"parent_index"` wherever the parent's name is shared,
since a name alone would hang every foot from the first `Leg`.

## Script commands work everywhere

`register_script_api` handlers are installed **only** by `ScriptSystem`
(native Luau / QuickJS). The editor viewport and web runtime execute Lua
through Fengari against a hardcoded `BRIDGE_FUNCTIONS` allowlist that knows
nothing about packages, so a global registered that way is `nil` in the
runtime you develop in.

Use `register_script_commands` instead:

```rust
fn register_script_commands(&self, commands: &mut PackageCommandRegistry) {
    commands.register("my-package", "do_thing", &["e", "amount"], |host, args| {
        let amount = args.number("amount").unwrap_or(0.0) as f32;
        if let Some(e) = args.number("e").filter(|v| *v >= 0.0)
            .and_then(|v| host.resolve_entity(v as u32))
            && let Ok(mut t) = host.world().get::<&mut Transform>(e)
        {
            t.position.x += amount;
        }
    }).expect("registering do_thing");
}
```

One handler, dispatched by every runtime through
`package::script_commands::handle_package_command`. Because it takes
`CommandArgs`, it is testable natively with `JsonArgs` — unlike the browser
hosts' own command arms, which only run in a browser.

**A command cannot return a value.** Browser hosts drain the command queue
*after* the script has finished running, so there is nothing to return into.
That covers send / spawn / apply / set; for reads, publish state where the
script can read it (a custom component, or a namespace on `__jengine_play`).
Registration **refuses** a getter-shaped name (`get_`, `is_`, `has_`, `find_`)
rather than letting it fail silently at call time.

An argument named `e` is treated as an entity id by the shared `target`
helper, the same as core's own commands.

### Your commands' namespace

Scripts call a package's commands through a namespace, as they call core's
(`Sprite.set_color`): one per package, named from the package in
PascalCase - `my-package` is `MyPackage`, `jengine-quest` is `Quest`. If
**every** command you register starts with the same `prefix_`, the prefix is
dropped inside the namespace, so `quest_talk` is `Quest.talk`; with mixed
prefixes the names stay whole (`Shooter.ship_fire`). A remainder that is a
Lua keyword (`x_end`) keeps the whole name, since `X.end(...)` does not
parse. `scripting::namespaces::package_entries` is the rule, and every
runtime applies it to what you registered - there is nothing to declare.

So choose names with the namespace in mind: a package `my-game` registering
`mygame_start` and `mygame_move` gives scripts `MyGame.start` and
`MyGame.move`. Two things are refused, loudly in
the log rather than silently: a namespace that is already taken - by one of
core's (`Sprite`, `Physics`, ...) or by another package - leaves your
commands reachable only by their flat names. The flat names always work.

A script checks whether your package is linked by testing the namespace:
`if MyPackage == nil then return end`.

## Getting your package into a build

Packages are **Rust crates linked at compile time**. There is no dynamic
loading, and there will not be one soon — a `.dll` boundary for `&mut World`
is a much larger problem than it looks.

Declare it in `project.jengine`:

```json
{
  "packages": [
    { "name": "my-package", "source": { "type": "path", "path": "packages/my-package" } }
  ],
  "settings": { "enabled_packages": ["jengine-physics", "my-package"] }
}
```

`jengine package resolve` fetches every declared package to a local path
(cloning git sources into `.jengine/packages/`) and installs the files that
script and asset packages carry. `settings.enabled_packages` then selects which
of the **linked** packages a runtime registers; a project that omits the key
gets all the built-ins, so older projects do not silently lose physics.

### How it reaches a native build

**If a package component seems to be ignored, check that the version survived.**
The editor loads every scene into the v1 `SceneData` struct and re-serializes
it before pushing to the viewport, so anything that struct does not model is
lost in transit. `version` is modelled now (BUGS.md #29), but the *shape* of
that bug is worth remembering: the scene looks perfectly healthy - scripts run,
no warnings, no missing assets - and the package's system simply has nothing to
work on, because its components arrived as `CustomComponents` entries instead.

A native build used to be `cargo build -p jengine-runtime`, which meant the
shipped game *was* the engine's own crate — so a project package had to be
edited into the engine workspace, and building project A then project B would
clobber each other's package list.

Now `jengine-runtime` is a **library** with a thin binary beside it, and the
builder generates a standalone crate per project at
`<project>/.jengine/runtime/`:

```toml
# generated — do not edit
[package]
name = "my-game-runtime"
[workspace]                       # standalone, not a member of the engine's
[dependencies]
jengine-runtime = { path = "…/crates/jengine-runtime", default-features = false }
cool-fx         = { path = "…/packages/cool-fx" }
[features]                        # forwarded, so backend pinning still works
default = ["backend-dx12", "backend-vulkan", "backend-metal"]
backend-dx12 = ["jengine-runtime/backend-dx12"]
```

```rust
// generated — do not edit
fn main() -> anyhow::Result<()> {
    jengine_runtime::run_with(&|host| {
        host.add_package(Box::new(cool_fx::CoolFxPackage));
    })
}
```

The build compiles that crate instead of the stock runtime and ships the binary
under the usual name. Projects with **no** Rust packages generate nothing and
build the stock runtime exactly as before. Regenerating with an unchanged
package set does not rewrite the files, so Cargo does not rebuild for nothing.

The generated `add_package` line comes from your `package.toml`:

```toml
[package]
name = "cool-fx"
version = "0.1.0"
entry_point = "cool_fx::CoolFxPackage"   # optional
```

Without `entry_point` the name is guessed the way the built-ins are named —
drop a leading `jengine-`, upper-camel the rest, append `Package`, so
`cool-fx` becomes `cool_fx::CoolFxPackage`. **Declare it explicitly whenever
that guess is wrong**, which is most of the time for a package whose type is
not literally `<Name>Package`. A wrong guess shows up as a compile error in the
generated crate naming a type that does not exist.

Your type must be `pub` from the crate root, since that is how the generated
main names it.

### Where your package will and will not run

| Surface | Runs your package? |
|---|---|
| Native runtime | Yes — compiled per project |
| Web build | Yes — compiled per project |
| **Editor viewport** | Yes — after `./build-project-viewport.ps1` |

### Web builds

Since Sep 2026 a web build links a project's packages the way a native one
does: `jengine-builder` generates `<project>/.jengine/web/`, a crate that
depends on `jengine-web` and your packages, and builds it with `wasm-pack
--out-name jengine_web` so `index.html` is the same as for a stock build. A
project with no Rust packages builds the stock `jengine-web` exactly as
before. From the command line: `jengine web-build <project>` (output in
`target/web-build/<project>/`), or `jengine web-crate <project>` to generate
the crate alone and `cargo check --target wasm32-unknown-unknown` it.

One thing is different from the viewport wrapper, and it is why the generated
crate is not a plain copy of it: `jengine-web` has a `#[wasm_bindgen(start)]`
of its own and wasm-bindgen allows one per module. It sits behind
`jengine-web`'s default-on `standalone` feature; the generated crate turns
that off, registers your packages from its own start function and then calls
`jengine_web::run()`.

Your package must compile for `wasm32-unknown-unknown` - it already has to,
for the editor viewport. A package that cannot (`jengine-audio`, which is
kira) is **refused by name** when the project lists it; the build does not
quietly leave it out, because a package missing from a build is a `nil`
global at Play.

### The editor

The viewport is one prebuilt WASM module shared by every project, so your
package cannot be linked into it ahead of time. Build a viewport that carries
your project's packages:

```powershell
./build-project-viewport.ps1 -Project projects/my-game          # reuse the cache if current
./build-project-viewport.ps1 -Project projects/my-game -Force   # always compile
# or, the same things without PowerShell:
jengine viewport-use projects/my-game
jengine viewport-build projects/my-game
```

The script, the CLI command and the editor's button all call
`jengine_builder::viewport::build`, so there is one implementation and no way
for them to drift.

That generates `<project>/.jengine/viewport/` — a wrapper crate that re-exports
`jengine-editor-viewport` and registers your packages from
`#[wasm_bindgen(start)]`, which runs during module init, before the editor calls
`viewport_init` and therefore before the `PackageHost` is built — then builds it
into `<project>/.jengine/viewport/pkg/` and copies that over
`editor/src/lib/viewport-wasm/`. `wasm-bindgen` exports defined in an rlib
dependency survive into a dependent `cdylib`, so the wrapper reproduces the
viewport's entire JS API and the editor's imports do not change.

Two things to know:

- **The editor's module directory is shared, but each project keeps its
  build.** The editor imports one module from a fixed path, so it carries
  whichever project was last loaded. The build writes `.jengine-viewport.json`
  beside the module recording the project and package list, and **the editor
  checks it on project open**. A project's own build stays in
  `<project>/.jengine/viewport/pkg/` with a copy of Cargo's dep-info - every
  local source it was compiled from - so when none of those is newer than the
  build, the editor copies it in and reloads back into the project in a
  second or two, no compile. Only when the build is missing or stale does an
  amber banner name what is linked versus what this project needs, with a
  *Rebuild viewport* button that streams the compile into the Build panel and
  reloads when done. With unsaved work it asks rather than reloading.

  A project with **no** Rust packages never triggers the banner: the stock
  viewport already carries every built-in package, so there is nothing to
  rebuild, and a check that cries wolf gets ignored.
- **The wrapper crate is named `<project>-viewport`, not the viewport's name.**
  Cargo refuses two packages with the same name and version in one lockfile,
  and the wrapper depends on the viewport. `wasm-pack --out-name` pins the
  emitted filenames to the stock ones instead.

### Two things that will catch you out

- **`settings.enabled_packages` is an allow-list once it exists.** A project
  that has the key gets only the packages it names, so adding a new engine
  package does not switch it on for existing projects - you have to list it.
  (A project with no such key gets every built-in, so older projects do not
  silently lose physics.) The symptom is the familiar one: your system never
  runs and nothing says why. `package host ready: N package(s), M system(s)` in
  the log is the check.
- **Density, not entity count, is what costs.** `packages/jengine-swarm` is the
  worked example: a spatial grid made neighbour search near-linear, but in a
  fixed-size play area doubling the population doubles the *density*, so
  per-boid cost kept rising anyway (0.31 us at 1k, 2.59 us at 16k). Capping
  neighbours per entity is what actually bounded it - 41 ms/frame down to
  11 ms at 16,000. Its `tests/perf.rs` measures this without a window.

A worked example lives in the test lab:
`projects/engine-test-lab/packages/testlab-spin` is a project-local package,
and scene 60 spins bars with it. Scene 59 does the same for a built-in package —
if 59 works and 60 does not, the package system is fine and the *linking* is
what needs attention.

A package a game is actually built on needs the whole round trip: typed
components in a v2 scene, script *commands* for what a script tells it to do,
and a custom component rewritten every frame (a ball package's
`BallState.events`, say) for what a script needs to know, since a command
can't return anything. Give it a test that plays the game's real scene through
a `PackageHost` - that is the test that catches what unit tests of its parts
do not.

## Checking that it worked

`PackageHost` is introspectable, which is the answer to "is my system actually
registered?":

```rust
for (name, phase, priority) in host.systems() {
    println!("{name} @ {phase:?} ({priority})");
}
```

For a test, drive the host directly — no runtime needed:

```rust
let mut registry = ComponentRegistry::new();
jengine_core::package::core_registration::register_core_components(&mut registry);
let mut host = PackageHost::new();
host.add_package(Box::new(MyPackage));
host.initialize(&mut registry);

host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);
assert!(/* your system did its thing */);
```

`packages/jengine-parallax/tests/hosted_by_package_host.rs` is a worked example,
including loading a real v2 scene through a package-aware registry.

## Rules

- **Add every package before `initialize`.** Adding afterwards is refused and
  logged: the scene has already been loaded against a registry that lacks your
  components.
- **`initialize` once.** A second call is ignored — it would register every
  system twice and run each twice per frame.
- **Never panic in a system.** It runs inside the frame loop; a panic takes the
  game down. Missing resource, missing component, bad data — log and return.
