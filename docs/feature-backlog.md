# jEngine Feature Backlog

Living document of features not yet implemented. Ordered roughly by impact /
effort. Items here are *plans*, not commitments — move a section to
[shipped.md](shipped.md) once it ships, and leave it here while any part of it
is still open.

Current gaps found by audit, ranked and tracked, live in
[engine-audit-2026-09.md](engine-audit-2026-09.md).

Phases 1–10 (atlas, materials, post-FX, lighting, shader graph, physics, GUI,
audio, particles, node-based shader editor) are complete. Test-lab scenes
01–28 are authored and wired into `nav.lua` / `hub.scene`. See
the generated component reference in [`website/`](../website/src/content/docs/api-reference/components.mdx) for the
shipped feature reference.

---

## 1. Cheap wins (≈ hours each)

### 1.3 Camera shake / screen flash helpers
- Script API: `camera_shake(amplitude, duration, freq?)`,
  `screen_flash(color, duration)`.
- Implementation: `CameraShake` resource decays per frame and adds offset to
  active camera before view matrix build. `screen_flash` pushes a fullscreen
  quad pass in the post chain that fades to clear.
- **Status (camera_shake): done.** `Camera2D` now carries inline shake state
  (`shake_offset`, decay, frequency); `BridgeCommand::CameraShake` is wired
  through Luau, the rquickjs JS host, and the browser-JS shim (`cmsh` tag);
  native runtime + editor Game-tab viewport both drain a pending-shakes queue
  before forming the view matrix each frame. Unit tests in
  `crates/jengine-core/src/renderer/camera.rs` cover the lifecycle. Registered
  in `core_script_api.rs` and documented in `docs/scripting-api.md`.
- **Status (screen_flash): done.** Implemented by extending the existing fade
  overlay pipeline. `TransitionMode::Flash { initial_alpha }` rides the same
  premultiplied-alpha fullscreen-triangle pass as `fade_in`/`fade_out`;
  `BridgeCommand::ScreenFlash` flows through Luau, the rquickjs JS host, and
  the browser-JS shim (`scrf` tag); native runtime renders fully via
  `TransitionState::new_flash`. Editor Game-tab viewport logs the call as a
  preview stub (parity with `fade_in`/`fade_out` — the editor preview's
  WebGL2 pipeline doesn't run the overlay compose yet; tracked as a future
  wedge). Registered in `core_script_api.rs`, documented in
  `docs/scripting-api.md`, and unit-tested in
  `crates/jengine-runtime/src/fade_overlay.rs` (start/mid/end alpha + clamp).

### 1.5 Asset hot-reload during Play
- File-watch (notify crate) for `.scene`, `.lua`, `.js`, `.material`,
  `.shader`, `.atlas`, `.tilemap`, textures.
- On change while playing: rebuild the affected resource and patch the
  current world (or scripts: re-`load()` keeping live entities).
- Toggle in editor settings.

**Status (2026 native wedge — shipped):** opt-in via `JENGINE_HOT_RELOAD=1`
env var on the native `jengine-runtime`. Wired through the existing
`jengine_core::asset::watcher::AssetWatcher` (notify v8, 100 ms debounce).
Per-frame `drain_hot_reload` in `crates/jengine-runtime/src/wgpu_scene_runner.rs`
classifies each changed path with `crates/jengine-runtime/src/hot_reload.rs`
(`HotReloadKind::{Scene, LuaScript, JsScript, Texture, Unsupported, Irrelevant}`)
and routes to:
- `.scene` matching the active scene → existing `reload_scene` path.
- `.lua` / `.luau` / `.js` → new `ScriptSystem::reload_script_path(id)` which
  calls `ScriptBridgeRunner::unload_path(id)` on both Lua and JS bridges;
  affected entities re-attach + re-`init` next frame from disk.
- `.png` / `.jpg` / `.jpeg` → new `AssetManager::reload_texture_bytes(id, abs)`
  which drops the raw bytes cache + uploaded wgpu texture and re-decodes.
- `.material` / `.shader` / `.atlas` / `.tilemap` / `.sortlayers` / audio →
  logged as TODO (separate follow-up wedge — these touch larger reload paths).

Coverage: 9 new tests (1 `asset::reload_texture_bytes_replaces_raw_cache`,
1 `runner::unload_path_drops_handle_and_per_entity_state`,
7 in `hot_reload::tests` for classification + env toggle). Editor viewport
(`jengine-editor-viewport`) and `jengine-web` are NOT wired yet — that lands
with the editor settings toggle in a follow-up.

---

## 2. Medium-effort

Everything in this section has shipped — see [shipped.md](shipped.md).

## 3. Strategic / bigger

### 3.1 User-defined Rust components with hot-reload
- Canonical design: [user-rust-components-design.md](archive/user-rust-components-design.md).
  The original "DLL hot-load on native" framing is **superseded** — the design
  doc rejects DLL hot-load and decomposes the feature into four opt-in slices.
- **Slice A — `ScriptHandler` on registry descriptors** (script-callable Rust
  handlers): **SHIPPED**.
  - **A.Lua** (prior cycle): `LuauAdapter::install_dynamic_function` at
    `crates/jengine-core/src/scripting/bridge/adapters/luau.rs` marshals
    typed args to `ScriptCallCtx` and dispatches `descriptor.handler`. Test:
    `crates/jengine-core/tests/dynamic_function_attach.rs`.
  - **A.JS-host** (this cycle): `JsAdapter::install_dynamic_function` now
    forwards to a new `HostJs::install_dynamic_function_parts` that installs
    a QuickJS global wrapping the same handler dispatch. Descriptors
    registered before the host engine is lazily constructed are stashed in
    `pending_dynamic_fns` and drained at the first `load()`. Test:
    `crates/jengine-core/tests/dynamic_function_attach_js.rs` (covers both
    pre-load and post-load install ordering).
  - **A.JS-browser** (deferred): the browser path currently logs a debug
    no-op. Unblocking it requires Phase 4.6 — routing the browser bridge
    through `BridgeContext`/`BridgeReadSnapshot` instead of the global
    `window.__jengine_play.*` state installed by `jengine-web`.
- **Slice B — `#[derive(Component)]` proc-macro** via a new `jengine-derive`
  crate (deferred). Removes boilerplate component registration.
- **Slice C — auto-generated `get_/set_<comp>_<field>` accessors** and
  matching behavior-graph nodes (deferred). Builds on Slice B.
- **Slice D — optional `#[methods]` proc-macro** for method-call syntax
  (deferred). Builds on Slice B/C.

### 3.2 Browser-based editor (Tauri-less WASM editor)
- Reuse `jengine-editor-viewport` WASM; port the Svelte panels to talk to it
  through `wasm-bindgen` instead of Tauri commands. Persistence over OPFS or
  remote storage.

### 3.5 Spine / skeletal 2D animation
- **Slice A — Entity parenting + transform hierarchy ✅ SHIPPED.**
  Foundation: lets any entity ride along with its parent in
  world-space. Required for both Spine bones and ordinary scene
  composition (player + held weapon, vehicle + turret, planet + moon).
  - `Parent(Entity)` + `WorldTransform { position, rotation, scale }`
    components in `crates/jengine-core/src/ecs/components.rs`.
    `WorldTransform` is serde-friendly (cached); `Parent` is not
    serialized — scene format uses a stable `parent: Option<String>`
    name-ref on `EntityData` instead.
  - `resolve_world_transforms(&mut World)` in
    `crates/jengine-core/src/ecs/hierarchy.rs`. Iterative
    passes-until-stable; O(N + E) for typical depths. 2D TRS
    composition with no shear. Dangling refs / cycles fall back to
    local transform with a `log::warn!`.
  - Sprite renderers (`render_sprites_wgpu`,
    `render_sprites_wgpu_multi_materialized`,
    `render_lit_sprites_wgpu`,
    `render_sprites_with_shadows_wgpu_materialized`,
    `collect_interleaved_sort_keys`) now consume
    `Option<&WorldTransform>` and fall back to
    `WorldTransform::from(&Transform)` when no resolver has run.
    `queue_sprite` / `queue_lit_sprite` / `InterleavedItem::Sprite`
    take `&WorldTransform`.
  - All three runtimes call
    `jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world)`
    once per frame after physics/behavior systems and before the render
    passes: `jengine-runtime/src/wgpu_scene_runner.rs`,
    `jengine-web/src/lib.rs`, `jengine-editor-viewport/src/lib.rs`.
  - Scene save/load round-trip in
    `crates/jengine-core/src/scene/format.rs`: `save_scene` writes
    `entity.parent = Some(parent_name)`; `load_scene_with_mapping`
    does a second pass after spawning that resolves the name → entity
    map and inserts `Parent`. Unknown / self-parent rejected with
    warnings.
  - Script API (Lua + JS host) exposes `set_parent(child, parent)`
    (parent = `nil`/`null` detaches), `get_world_position`,
    `get_world_rotation`, `get_world_scale` via new
    `BridgeCommand::SetParent`, `ScriptCommand::SetParent` (mirrored
    applicator in `ecs/systems.rs`), and three new accessors on
    `BridgeContext`.
  - Tests: 5 hierarchy unit tests in `ecs/hierarchy.rs` (root identity,
    child translation, child rotation+scale, grandchild multi-pass,
    dangling parent fallback) + 5 integration tests in
    `crates/jengine-core/tests/hierarchy_integration.rs` (scene load
    restores Parent, resolver composes loaded chain, save round-trip,
    unknown parent ignored, self-parent rejected). All green.
  - **Out of scope for Slice A (still consume local `Transform`):**
    particles (`wgpu_systems.rs` ~853, ~1671, ~1817 + multiple
    editor-viewport call sites), lighting collection in
    `renderer/lighting.rs`, text renderer (~597), vector paths (~475),
    shadow caster geometry (~1460, ~1674, ~1807; the *placement* uses
    `Transform`, the sprite group on top now uses `WorldTransform`),
    tilemaps, the `jengine-parallax` package, and the scripting batch
    runtime's transform snapshot. Migrating each is a follow-up slice;
    none block Slice A's contract because they read positions the
    user has authored on the entity itself.

- **Slice B — Bone + SkeletonRoot components ✅ SHIPPED.**
  Skeletons ride on slice A's parent hierarchy. Bones are ordinary
  parented entities with [`Transform`] + the new `Bone` marker; the
  root carries `SkeletonRoot { name }`. Authoring lives in scene JSON
  today; a dedicated `.skeleton` resource file is deferred to the
  Spine importer (Slice D).
  - `Bone { name: String, length: f32 }` + `SkeletonRoot { name: String }`
    in `crates/jengine-core/src/ecs/components.rs`. Both serde-friendly
    with `Default` impls so they slot cleanly into `ComponentBundle`.
  - `crates/jengine-core/src/ecs/skeleton.rs`: `find_bone(world, root,
    name)`, `find_skeleton(world, name)`, and `collect_bones(world, root)`.
    Bone discovery walks the `Parent` chain (cap 1024 ancestors) so a
    given name only matches descendants of the requested skeleton —
    multiple skeletons of the same rig in one scene stay isolated.
  - Scene format (`crates/jengine-core/src/scene/format.rs`):
    `ComponentBundle.bone` + `.skeleton_root` round-trip through
    `save_scene` / `load_scene_with_mapping`. Combined with slice A's
    `parent: Option<String>` resolution this means a scene file fully
    describes a rig with no extra plumbing.
  - Script API (Lua + JS host adapters):
    `find_bone(root_id, "name") -> entity_id | nil/null` and
    `find_skeleton("name") -> entity_id | nil/null`. Returned ids
    plug straight into the existing `get_world_position` /
    `set_position` / etc. helpers. Backed by new
    `BridgeContext::find_bone` / `find_skeleton` accessors that read
    live through `world_ptr`.
  - Tests: 5 unit tests in `ecs/skeleton.rs` + 4 integration tests in
    `crates/jengine-core/tests/skeleton_integration.rs` (round-trip,
    parent-chain walk, named root lookup, full rig enumeration).

- **Slice C — Skeletal animation playback ✅ SHIPPED.**
  Bone keyframes drive local `Transform`s before the slice-A resolver
  runs, so animated bones flow through `WorldTransform` straight into
  the sprite renderers with zero extra plumbing.
  - Data types in `crates/jengine-core/src/animation/skeletal.rs`:
    `Vec2Key { time, value }`, `ScalarKey { time, value }`,
    `BoneTrack { bone_name, position, rotation, scale }`,
    `SkeletalClip { name, duration, tracks }`. Deliberately distinct
    from the existing sprite-frame `AnimationClip` so the two systems
    can coexist on the same entity (sprite UV anim + skeletal pose).
  - `SkeletalPlayer { clip, time, speed, looping, playing }` component
    lives on the same entity as `SkeletonRoot`. Serde-friendly with
    defaults (`speed=1.0`, `looping=true`, `playing=true`).
  - Pure samplers `sample_vec2` / `sample_scalar` do linear interp,
    holding endpoints outside the keyframe range and skipping empty
    tracks so a clip can override position-only without clobbering
    rotation or scale.
  - `update_skeletal_players(world, clips, dt)`:
    - Advances time per player (`rem_euclid` for looping, clamp + stop
      for one-shot).
    - Resolves each `BoneTrack::bone_name` via slice-B `find_bone`
      against the player's own root (multi-skeleton-safe).
    - Buffers writes into a Vec then applies — sidesteps the borrow
      checker without macro plumbing.
  - Hooked into all three runtimes immediately before
    `resolve_world_transforms`:
    `jengine-runtime/src/wgpu_scene_runner.rs` (native),
    `jengine-web/src/lib.rs` (deployed WASM), and
    `jengine-editor-viewport/src/lib.rs`'s `tick_play_world` — edit
    mode intentionally does **not** advance players so the inspector
    can author bind / static poses unmolested.
  - `AssetManager::insert_skeletal_clip` / `get_skeletal_clip` /
    `load_skeletal_clip(id, path)` + `skeletal_clips()` accessor
    feeding the system. Clips load from `.skelanim` JSON via serde.
  - Scene format: `ComponentBundle.skeletal_player` round-trips
    through `save_scene` / `load_scene_with_mapping`.
  - Script API (Lua + JS host): `play_animation(entity, clip)`,
    `stop_animation(entity)`, `set_animation_time(entity, t)`,
    `set_animation_speed(entity, s)`, `get_animation_time(entity)`,
    `is_animation_playing(entity)`. Backed by a single new
    `BridgeCommand::SetSkeletalPlayer` / `ScriptCommand::SetSkeletalPlayer`
    with all five mutable fields optional.
  - Tests: 8 unit (sampling boundaries, looping wrap, non-looping
    clamp + stop, paused player, missing clip) + 2 integration
    (`crates/jengine-core/tests/skeletal_animation_integration.rs`:
    scene round-trip + end-to-end load → tick → resolver pipeline).

- **Slice D — Skeleton rig data + importers ✅ SHIPPED.**
  Adds the `SkeletonRig` authored format (one `BoneDef` per bone with
  bind-pose Transform + parent name), the `spawn_rig` system that
  instantiates a rig into a hecs world with all `Parent` /
  `SkeletonRoot` / `Bone` wiring done by name, and a Spine-subset JSON
  importer that produces both the rig and one `SkeletalClip` per top-
  level animation.
  - `crates/jengine-core/src/animation/rig.rs`:
    - `BoneDef { name, parent: Option<String>, position, rotation
      (rad), scale, length }` with serde defaults so JSON authoring
      stays terse.
    - `SkeletonRig { name, bones }`.
    - `spawn_rig(world, &SkeletonRig) -> HashMap<String, Entity>` —
      two-pass: spawn each bone with Transform + slice-B `Bone`, then
      wire `Parent` links and tag the first parent-less bone as the
      `SkeletonRoot`. Cycles (no parent-less bone) fall back to
      promoting the first bone so the function never panics.
    - `parse_spine_json(&str) -> Result<(SkeletonRig, Vec<SkeletalClip>)>`
      handles bones (name / parent / x / y / rotation /
      scaleX / scaleY / length) and `animations.bones`
      (`translate` / `rotate` / `scale` tracks). Spine's degrees are
      converted to radians; legacy `angle` keys are aliased to the
      modern `value`. Clip duration is the max keyframe time across
      every track. Slots / skins / IK / mesh deform are ignored —
      those belong to slices E / F.
  - `AssetManager` (slice 3.5.D additions):
    - `skeleton_rigs: HashMap<String, SkeletonRig>` storage.
    - `insert_skeleton_rig`, `get_skeleton_rig`, `has_skeleton_rig`,
      `skeleton_rig_ids`.
    - Native `load_skeleton_rig(id, path)` reads `.skel` JSON.
    - Cross-platform `import_spine_skeleton(rig_id, clip_id_prefix,
      json)` registers the rig and every contained animation in one
      shot, returning the list of registered clip ids. Native
      `load_spine_skeleton` wraps it for disk reads.
  - Script API: `spawn_rig(rig_id) -> entity_id` returns the rig's
    root entity. Backed by `BridgeCommand::SpawnRig` /
    `ScriptCommand::SpawnRig`; the applicator looks the rig up in the
    asset manager, spawns all bones, maps the root through
    `spawn_remap` so the script's returned id resolves immediately,
    and pre-populates `entity_index` with every bone so follow-up
    commands in the same drain (e.g. `set_position` on the root) work
    without an extra rebuild. Child bones are reached via the
    existing slice-B `find_bone(root, name)`. Wired on both Lua and
    JS host adapters.
  - Tests: 4 unit
    (`crates/jengine-core/src/animation/rig.rs` — hierarchy + root
    pick, parentless fallback, Spine subset parse, legacy `angle`
    alias) + 3 integration
    (`crates/jengine-core/tests/skeleton_rig_importer.rs` — asset
    manager import path, end-to-end imported rig → tick → resolver,
    direct-vs-asset-manager equivalence).

- **Deferred slices (in dependency order, picked up later):**
  - _(none — entire 3.5 skeletal animation stack is shipped.)_

- **Slice F shipped May 2026 — IK constraints (two-bone + look-at).**
  - `crates/jengine-core/src/ecs/components.rs`:
    `IkTwoBone { target: [f32;2], flip_bend: bool }` attached to the
    **end (effector)** bone of a 3-bone chain — the solver walks
    `Parent` twice to find `mid` and `root`, so callers never need
    to name bones manually.
    `IkLookAt { target: [f32;2], rotation_offset: f32 }` attached to
    any bone, rotates that bone's world `+X` axis (plus
    `rotation_offset`) toward the target.
  - `crates/jengine-core/src/animation/ik.rs` (new) —
    `solve_ik(&mut World)` runs both passes. Two-bone uses the
    standard planar law-of-cosines formula with `Bone.length` as the
    segment lengths; `flip_bend` selects elbow-up vs elbow-down.
    Targets out of reach are clamped to the chain's max extent so
    the end joint sits on the line from root to target. Internally
    resolves world transforms once at the start (to read joint
    positions + parent rotations) and again between the two-bone and
    look-at passes (so look-at chained to an IK-driven ancestor sees
    the updated pose).
  - Runtime hookup: all three runtimes
    (`jengine-runtime/src/wgpu_scene_runner.rs`,
    `jengine-editor-viewport/src/lib.rs` — play mode only, mirroring
    skeletal players, and `jengine-web/src/lib.rs`) call `solve_ik`
    immediately after `update_skeletal_players` and before the final
    `resolve_world_transforms` that downstream consumers
    (rendering, skinning) read from.
  - Scene format: `ComponentBundle::ik_two_bone` and
    `ComponentBundle::ik_look_at` round-trip via the standard
    `skip_serializing_if = "Option::is_none"` pattern.
  - Script API: `set_ik_target(entity_id, x, y)` on both Lua and JS
    hosts. Bridge command `BridgeCommand::SetIkTarget` → engine
    `ScriptCommand::SetIkTarget`. Applicator writes to whichever IK
    component is present on the entity (two-bone preferred); no-op if
    neither is attached.
  - Tests: 5 unit
    (`crates/jengine-core/src/animation/ik.rs` — two-bone reaches a
    reachable target, clamps to max extent when unreachable, flip_bend
    mirrors the mid-bone solution, look-at points the bone at a target,
    look-at subtracts the parent's world rotation) + 2 integration
    (`crates/jengine-core/tests/ik_integration.rs` — scene round-trip
    preserves both constraints, end-to-end two-bone solve through a
    scene-loaded chain with `Parent` name-references).

- **Slice E shipped May 2026 — Mesh-deform / skin weights.**
  - `crates/jengine-core/src/animation/skinned_mesh.rs` (new):
    `BoneInfluence { bone_index, weight, offset: [f32; 2] }`,
    `SkinnedVertex { rest_position, uv, influences }`, and
    `SkinnedMesh { texture_id, color, z_order, bones, vertices, indices }`.
    Bind pose is encoded directly in `offset` (each influence stores
    the vertex's position in that bone's local frame), so runtime
    skinning is `Σ wᵢ · (bone_world_xform · offsetᵢ)` normalized by
    total weight — no inverse-bind matrix required.
    `compute_skinned_positions(mesh, &World, skeleton_root)` resolves
    bone names via the slice-B `find_bone(root, name)` walker; falls
    back to `rest_position` when no influence resolves.
  - Renderer: `WgpuSpriteBatch::draw_triangle(p0, p1, p2, uv0, uv1,
    uv2, color)` pushes a degenerate quad
    `[p0, p1, p2, p2]` so the static `(0,1,2, 0,2,3)` index pattern
    emits one real triangle plus a zero-area triangle that produces
    no fragments. Reuses the existing sprite pipeline + index buffer
    — no new GPU resources. Costs 1 quad slot per visible triangle.
  - `crates/jengine-core/src/ecs/wgpu_systems.rs::render_skinned_meshes_wgpu`
    queries `(SkeletonRoot, SkinnedMesh)`, sorts by `(z_order,
    texture_id)`, CPU-skins via `compute_skinned_positions`, and emits
    one `draw_triangle` per index triple. Snapshots indices/uvs/color
    before computing positions to release the world borrow, and
    flushes mid-mesh when `is_full()` triggers, resuming from the
    next triangle.
  - Scene format: `ComponentBundle::skinned_mesh: Option<SkinnedMesh>`
    with `skip_serializing_if = "Option::is_none"`; save/load wired
    in `crates/jengine-core/src/scene/format.rs`.
  - Runtime hookup: native (`jengine-runtime/src/wgpu_scene_runner.rs`),
    editor viewport (`jengine-editor-viewport/src/lib.rs`), and web
    (`jengine-web/src/lib.rs`) all call `render_skinned_meshes_wgpu`
    after the text pass, before `flush_dispatch` is dropped. No-op
    when no entity has a `SkinnedMesh`.
  - Tests: 5 unit
    (`crates/jengine-core/src/animation/skinned_mesh.rs` —
    identity bind pose, bone rotation rotates vertex, weighted blend
    between two bones, missing-bone fallback, unnormalized-weights
    normalization) + 2 integration
    (`crates/jengine-core/tests/skinned_mesh_integration.rs` —
    scene round-trip preserves mesh, end-to-end skinning against a
    translated root bone).
  - No script API in this slice — `SkinnedMesh` is data-driven via
    scenes + Spine import. A `set_skinned_mesh` host call can be
    added later if user code wants to swap meshes at runtime.

### 3.7 Scripting in the deployed web runtime (`jengine-web`)
- Today `ScriptSystem` is gated `#[cfg(feature = "native")]`; `jengine-web`
  ticks scenes with behaviors + physics but skips Lua / JS. Editor viewport
  runs scripts via its own browser-host JS adapter, so this gap only bites
  shipped WASM builds.
- **Status (May 2026):** investigated, **deferred** — far bigger refactor
  than originally scoped. See plan below.

#### Why deferred
`jengine-editor-viewport/src/lib.rs` is ~5,600 lines, and the majority of
it is the browser-script host plumbing: `load_and_run_scripts`,
`sync_world_to_play_namespace` (~460 lines), `sync_play_namespace_to_world`
(~125 lines), `drain_play_commands_into` (~1,300 lines of command
handlers covering tweens, lights, GUI, dynamic load_script, debug draws,
scene transitions, etc.), `compile_browser_script`, `play_namespace()`,
the `SCRIPT_ENTITY_MAP` thread-local, GUI click registry, and friends.
All of it is `fn` (private to the viewport). `jengine-core`'s
`scripting::bridge::adapters::js` ships only the compile primitive
(`BrowserScriptFns`) plus the Luau→JS transpiler — every consumer crate
writes its own end-to-end runner.

This matches what `docs/archive/scripting-execution-plan.md` already labels
"P7.5e / P7.7 — multi-session refactor."

#### Two paths when we come back to this

1. **Minimal port (~600 lines copy-paste from viewport into jengine-web).**
   Gives shipped WASM games an `init` / `update` / `on_*` loop with
   transforms, sprite reads/writes, and collision/trigger dispatch.
   Accepts a smaller surface than the editor — no GUI clicks,
   no `load_script`, no debug-draw, no script-driven tweens, no
   light/material commands. Lowest risk; gets games playable.

2. **Lift the viewport's script host into
   `jengine-core::scripting::browser_host` and depend on it from both
   the viewport and `jengine-web`.** Correct long-term direction.
   ~1,500-2,500 line refactor that has to be done carefully because the
   viewport's editor preview depends on every command being routed
   correctly. Should be split into wedges similar to the P7.7 work
   already done (extract one command group at a time: tweens, lights,
   GUI, debug-draws, dynamic load, scene transitions). Plan it as its
   own multi-session task with checkpoints.

Recommend **Path 1 first** when we resume, so deployed games gain
scripting parity for the most common cases; then schedule Path 2 as
ongoing cleanup wedges. Carry the trigger + collision dispatch we
shipped in May 2026 over verbatim — both events already exist on the
viewport side and the patch shape is identical.

---

### 3.8 Remaining `jengine-web` parity gaps (Aug 2026)

Scripting, scene transitions, behavior graphs, audio, `.anim` `clip_source`
inlining, and GUI (show/hide, `gui_set`, `gui_on_click` + hit-testing) all
landed Aug 2026 while getting the first full game's web build running. What is
still missing, roughly in the order a game is likely to hit it:

- **`particle_source` / `.tilemap` source inlining.** Core already exposes
  the disk-side `resolve_particle_sources` / `resolve_tilemap_sources`.
  Web needs the same treatment `.anim` got: split each into
  `collect_* / inline_*` so the fetch-based host reuses the inlining rules
  (see `animation::asset` for the shape). An emitter or tilemap that points
  at a shared asset file currently loads empty.
- **Atlas region remapping.** The web builder's asset pipeline can rewrite
  scene `texture_id`s to atlas regions (`assets/atlas_manifest.json` is
  emitted), but the runtime never loads atlases, so a build with atlas
  packing enabled renders nothing for remapped sprites. Needs
  `AssetManager::load_atlas` fed from fetched `.atlas` files, mirroring
  `viewport_load_atlas`.
- **Per-material pipeline routing.** `render_sprites_with_shadows_wgpu_materialized`
  is called with `material_id: None` — custom `.material` / shader-graph
  pipelines are ignored, so materials fall back to the default sprite
  pipeline.
- **Debug draws.** `draw_debug_line` / `draw_debug_circle` (`ddl` / `ddc`)
  are the only commands that game calls that the web host still drops.
  Needs a shape-batch pass; debug-only, so lowest priority.
- **`gui_get` state mirror.** The browser wrapper reads
  `__jengine_play.gui_state`; the viewport seeds it each frame, the web
  runtime does not, so `gui_get` returns `""`. Only bites scripts that read
  GUI values back.
- ~~**`require` does not work in web builds at all.**~~ **FIXED Sep 2026**
  along with the Fengari switch below. Modules resolve through
  `window.__jengine_play.prefab_scripts`, which only the editor's Toolbar
  populates; `jengine-web` neither fills it nor calls
  `install_require_globals`, so `globalThis.require` is undefined. Every
  script with a `require(...)` compiles and then dies at init. See
  [web-lua-runtime-proposal.md](./web-lua-runtime-proposal.md).
- ~~**The Lua->JS transpiler is line-based.**~~ **Largely moot since Sep
  2026:** web builds run the editor's Fengari VM, and the transpiler is only
  the fallback when no runtime is installed. Kept below as a record of its
  limits.
- **The Lua->JS transpiler is line-based.** It matches statements by how a
  line begins and ends rather than parsing Lua, so every wrapped or
  multi-statement line is a special case. A batch of them was fixed in Aug
  2026 (see the regression tests at the bottom of `transpiler.rs`), but the
  approach has a ceiling: `repeat/until`, `goto`, varargs, and metatables
  are not handled at all. `cargo run -p jengine-core --features
  js-adapter-host --example transpile_lua -- <script.lua> | node --check -`
  is the loop for checking a project's scripts without a browser. The
  durable fix is to run Fengari in the web runtime the way the editor does -
  costed and measured in
  [web-lua-runtime-proposal.md](./web-lua-runtime-proposal.md) (67 KB
  gzipped, ~0.1 ms/frame for that game, no wasm size change).
- **Audio handle tracking.** `play_sound` returns handle `0` from the
  browser bridge, so `stop_sound` / `set_sound_volume` are inert (same
  limitation as the editor viewport).

---

## 4. Quality / DX

Everything in this section has shipped — see [shipped.md](shipped.md).

- Asset browser thumbnails for scenes, prefabs, materials, atlas regions.
- Multi-select gizmos + snap-to-grid in the viewport.
- Per-scene undo/redo stack integrated with property edits.
- Builder packaging smoke-test in CI (`cargo run -p jengine-builder` on the
  test-lab project for Windows + WASM after every workspace change).
- Inspector "copy as JSON" / "paste components" between entities.

---

## Recently fixed (May 2026)

Everything in this section has shipped — see [shipped.md](shipped.md).

- Trigger callbacks (`on_trigger_enter` / `on_trigger_exit`) and physics
  collision callbacks (`on_collision_enter` / `on_collision_exit`) now
  reach user Lua/JS scripts on both the native runtime and the editor
  viewport. Same `{ other = <entity_id> }` payload schema; 1-frame delay
  on native (physics steps after scripts), same-frame in the viewport.
- v2 scene round-trip dropped components missing from
  `register_core_components` → second Play would zero out velocity, lifetimes,
  triggers. Registry now lists all 60 built-ins.
- Revolute joint anchor sign was the wrong way around in scene 26 pendulum.
- Rope segments started outside `max_distance` → snap-back.
- Spring stiffness vs gravity made the demo look like free-fall — spring
  balls now use `gravity_scale=0` + initial velocity.
- `ensure_physics_bodies` now seeds initial `linvel` from `Velocity2D` so
  scene-authored kicks reach the physics body instead of being overwritten
  by the next physics step.
