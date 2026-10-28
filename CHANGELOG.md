# Changelog

All notable changes to jEngine are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Versions are `MAJOR.MINOR.PATCH`:

- **MAJOR** for a whole new generation of the engine.
- **MINOR** for new features.
- **PATCH** for fixes and improvements to what is already there.

Before 1.0, a minor version may still break the script API and file formats.
Breaking changes are always listed under **Changed** or **Removed**.

## [0.2.0] - 2026-10-28

The three themes of 0.2.0: a game ships only the parts of the engine it
uses, a game plays out the same on every machine (so multiplayer can send
inputs alone), and a 3D stage for the HD-2D look.

### Added

- **A game builds only the engine it uses.** Seventy-two optional
  subsystems are now *engine features* - fluids, smoke and falling sand,
  mirrors and portals, behaviour graphs and trees, navigation, dialogue,
  IK, skinned meshes and jiggle bones, ragdolls, the post effects with
  passes of their own, GI, asset packs, live ops, video, the screen reader,
  font files, the 3D stage and more. A build looks at the project's scenes,
  prefabs, scripts, post effects, data folders and settings and compiles in
  only what they use; the build log and report say why each one is in.
  `settings.engine_features` (`include`, `exclude`, `all`) adjusts the
  choice - `include` one that a script reaches by a name assembled at run
  time, which no scan can see. Excluding a feature the assets use fails the
  build, naming it. A script call into a feature a build left out logs once
  and does nothing. The editor's viewport keeps every feature. An empty
  project's web module is 2.41 MB gzipped (4.28 MB in 0.1.0) and its
  Windows runtime 24.1 MB (40.3 MB); the other size changes are under
  Changed.
- **The same simulation on every platform.** Physics, the engine's own
  simulation (navigation, behaviours and graphs, motion, the hierarchy, path
  following, vision, water, flow, IK, easing, noise, `light_at`, the camera)
  and the packages compute through `jengine_core::detmath` - the same
  `x.dsin()`, `y.datan2(x)`, `x.dexp()` bits on Windows, Linux, macOS and
  wasm - and rapier runs with `enhanced-determinism`, vendored
  (`vendor/rapier2d`) so that its own `f32` maths goes through `libm` too.
  Contact events, removed bodies, snapped joints and batch-script groups
  come in a fixed order. A car football match played from one seed used to
  end 2 - 0 natively and 1 - 2 in the browser; now every checkpoint
  matches on all four, which is checked on each of them before a release.
  Use `detmath` in a package's simulation code
  ([writing packages](docs/writing-packages.md)). A replay or ghost
  recorded with 0.1.0 plays back slightly differently.
- **A script's numbers are the same in every runtime.** `math.sin`, `exp`,
  `pow` and the rest, `^` and `%` compute through the engine's `libm`
  natively (a vendored Luau, built without fused multiply-add) and in the
  editor and web builds (a patched Fengari). `math.random` is one generator
  everywhere, so `math.randomseed(x)` gives the same draws natively and in
  the browser. Whole numbers in the editor and web builds no longer wrap
  past 2^31; `tostring`, `print` and `..` print Luau's text everywhere
  (`3`, not `3.0`); and `string.format("%d", x)` truncates a fractional `x`
  in every runtime instead of raising in the browsers. `math.atan(y, x)`,
  `math.atan2`, `math.pow`, `log10`, `sinh`, `cosh` and `tanh` work in
  both dialects. Test-lab scene 298.
- **Gameplay in fixed steps** (`settings.simulation_hz`; Project Settings →
  Physics → Gameplay Steps, or a scene's own top-level `simulation_hz`).
  Motion, physics, scripts' `update`, timers, tweens and animation run in
  steps of exactly `1 / simulation_hz` seconds, as many as real time pays
  for, so the same inputs give the same game at any frame rate. Each press,
  finger and gesture is seen by one step, and what is drawn is interpolated
  between steps, so a 60 Hz game stays smooth on a 165 Hz screen. Off by
  default: a frame is one step of its own `dt`, as before. Test-lab scene
  299.
- **Lockstep multiplayer**: a `lockstep_session` (package `jengine-online`)
  joins a `lockstep` room on a `jengine-server` (`Server::lockstep()`; the
  stock server has one), and every machine runs each fixed step only once
  every player's input for it has arrived - so two to eight machines play
  one deterministic game sending nothing but input. Every player's input is
  on the session's `Lockstep` component; the machines compare world hashes
  and stop a match that came apart. A player who drops comes back, and a
  spectator can watch: both replay the match so far from the room's
  history, fast, then play on. A seat that has gone, or sent nothing for
  `silent_after` seconds, is played as "nothing held" instead of holding
  everyone up. Sessions can send analog strengths (`analog`), the world
  pointer (`pointer`) and a chosen list of actions (`actions`). Packages get
  a `Frame` phase, run once a display frame before the steps. Test-lab scene
  300.
- **A 3D stage** (`stage`, `stage_quad`, `stage_billboard`, `stage_group`,
  `stage_caster`, `Stage.place`, `Stage.set`) - the HD-2D look: a
  perspective camera over flat pieces placed in three dimensions. Floors,
  walls and roofs are projected into meshes in true perspective; sprites,
  emitters and lights stand in it; everything sorts by depth. A
  `stage_group` is a frame its contents stand in, moved and turned as one,
  and groups nest; `follow` resolves through groups and chains of follows.
  Billboards throw their sprite's shadow from each nearby light (`shadow`),
  and solids throw theirs (`stage_caster`), projected from the light's 3D
  position onto the floor. Two additions came with it, usable anywhere:
  `depth_haze`, a post effect fading far pixels into a colour by depth, and
  `squash` on point and spot lights, an elliptical pool for a floor seen at
  an angle. Visual Showcase 2 (`projects/visual-showcase-2`) is six HD-2D
  scenes on it.
- **The editor for macOS** (Apple Silicon): `./build-editor-release.sh`
  makes `jEngine Editor.app`, a `.dmg` and a zip of the app, signed ad hoc.
  It is not notarized, so macOS asks once before opening it (System
  Settings → Privacy & Security → Open Anyway). The editor runs at the
  display's rate there (WKWebView held it near 60 fps).
- **iOS games run on an iPhone**, not only the simulator, at the screen's
  full rate (120 fps on a ProMotion phone). A build goes in `iphone/` or
  `iphone-simulator/` under the output folder and says how to install it.
- **Logs** for web builds (Build Settings → Web, `jengine web-build
  --no-logs`): off, a game keeps its info lines - the engine's and the
  scripts' `log` / `print` - out of the browser console, and still writes
  warnings and errors. A release build with logs on asks before it starts.
- **A v1 scene can hold a package's components**, in every runtime, as a v2
  scene always could.
- `jengine-benchmark-cpu`: a headless benchmark timing the same engine code
  natively and as wasm (a frame of motion, physics and particles, and
  loading a scene), for judging what a build setting costs.

### Changed

- **Packages have versions of their own.** A package's number used to be
  the engine's, so every package moved with every release whether it had
  changed or not. Now it moves only when the package does, by what changed
  (a fix a patch, a feature a minor). In 0.2.0 `jengine-online` is 0.2.0
  (lockstep sessions), `jengine-net` and `jengine-swarm` are 0.1.1, and the
  rest stay at 0.1.0. A package's `Cargo.toml` names its version and its
  `package.toml` must agree; `engine_version` is still the oldest engine it
  works with.
- **Rust API: `scene::format::ComponentBundle` is the JSON of each
  component under its scene key** (`get::<T>("key")`, `set`, `with`),
  decoded through the component registry wherever an entity is made. It was
  a struct with a typed field per core component. Scene and prefab files
  are unchanged. Loading a v1 scene is faster (a 1.16 MB scene 72 to 48 ms)
  and so is spawning a prefab (2.2 to 1.5 us natively).
- **Smaller builds**, besides engine features: a scene's types are parsed
  through a `Value`, each component has one deserializer instead of two,
  native games no longer carry a regex engine (about 750 KB), and native
  release builds for Linux, macOS and iOS are stripped of their symbol
  tables (an empty project's Linux binary 54.1 to 37.7 MB before the other
  savings).
- **Web builds default to speed.** The editor's WASM size options
  (`opt-level = "z"` among them) are off by default: they made a game's
  code about 3.7 times slower to run (a frame 1.9 ms to 7.2) for a smaller
  download. A project saved with them on keeps them until they are
  unticked, and each checkbox says what it costs. A game's web build runs
  `wasm-opt` at `fast` (-O1) by default: the smallest download at every
  profile, built in seconds rather than minutes, and as fast as -O.
- A linear-filtered texture's **mipmaps are drawn on the GPU** when it is
  uploaded, not averaged on the CPU on the frame it is first drawn. A
  showcase scene's first frame in a browser went from 47 ms to 17 ms.
- A web game fetches its assets **at once** - a scene's textures, `.gui`
  documents, fonts, atlases, sounds, scripts and behaviour graphs, and at
  startup the scene, prefabs, script modules and data tables - where each
  file used to wait for the one before it, a round trip apiece.
- A web build ships its small assets in **one file**,
  `assets/asset_bundle.json` (prefabs, materials, shaders, `.gui`
  documents, data tables, font descriptors, behaviour graphs, `.anim`,
  atlases), read beside `project.jengine` before anything else. Scenes stay
  separate files, fetched as they are entered.
- A release web build's JSON assets are **minified**: about half the bytes.
  The project's own files are untouched.
- A scene is **parsed once** on its way in, in every runtime (it was parsed
  nine times on the web - about half the CPU time of entering a big
  scene). Core gains `load_scene_value`,
  `expand_prefab_instances_in_value`, `scene_metadata`,
  `PostProcessSettings::from_scene_value` and `scene::name_in`; the text
  versions remain as wrappers.
- A soft light tests whether a pixel is inside an occluder once instead of
  once per ray: a showcase scene under a soft sun went from 91.6 ms a frame
  to 50.0 on a laptop's high-density screen.
- A native game's window is titled with the game's name: the project's
  `settings.title`, else `settings.app.name`, else the project's name. It
  was `jEngine — wgpu scene: <scene>`.
- A package made by the scaffolds (`jengine package new`, the editor's New
  Package) names the engine that made it as its minimum `engine_version`. It
  used to name 0.1.0 whatever the engine was.
- The server console has a new look, and a machine whose clock had not been
  set yet (a Raspberry Pi just after boot) no longer leaves samples months
  off the start of its charts.

### Fixed

- Natively, a script's mouse position in the world
  (`get_mouse_world_position`) was wrong whenever the window was not the
  size the project was authored for: on a high-density Mac screen it was off
  by a factor of 2, and on a phone every tap landed too far from the centre.
- A player whose connection dropped and who came back at once could be
  refused with "already in this room" instead of getting their held seat
  back, when their room was busy (`jengine-server`).
- An iOS build for a phone was installed on the simulator by `--run`; its
  bundle identifier could hold underscores, which a provisioning profile
  refuses; and its icon was not compiled into an asset catalog.
- Natively, a scene entered by `Scene.load` or `push_scene` - every game
  launched from a hub - kept its prefab instances unexpanded, so placed
  prefabs were missing. Only start-up and streamed chunks expanded them.
- A normal-mapped mesh was lit flat (the normal pass drew sprites only), and
  depth of field took every mesh as "unknown depth" and kept it sharp (the
  velocity pass drew sprites only). Both passes draw meshes now.
- A stage piece under a parent was drawn moved by the parent's 2D transform,
  flat, on top of its projected place.
- A web game whose project has no live ops logged `settings.liveops is not
  valid: invalid type: null` at every start.
- A scene entered later in a web game fetches the fonts its new `.gui`
  documents name; they were looked for only in documents already loaded, so
  those menus drew in the built-in face.
- A release web build of a project failed inside wasm-pack (`unexpected
  argument '--no-opt'`).
- Builds and tools find their output when `CARGO_TARGET_DIR` is set.
- The editor on macOS or Linux finds `cargo`, `rustup`, `wasm-pack` and
  `wasm-opt` when started from Finder or a desktop launcher, which give an
  app the system's bare `PATH`; every build used to fail with "not found".
  On a Mac it also reinstalls its frontend's dependencies after a pull that
  changed them.
- A macOS game build's bundle identifier takes the project's id with
  underscores made hyphens: the default id (`com.jengine.my_game`) is not a
  valid `CFBundleIdentifier`.
- After a macOS or iOS build, the editor shows the build's folder in Finder
  and opens its build report. It used to `open` the `.app`, which started
  the game.
- The third-party notices left out rapier2d and Luau once both were
  vendored; vendored crates are listed again, and each patched copy (winit,
  rapier2d, luau0-src) says what was changed.
- Text files are checked out with LF line endings on every platform.

## [0.1.0] - 2026-10-23

The first public release.

### Added

- **Renderer** (wgpu): sprites with materials, blend modes, masks and
  outlines; textured meshes and vector paths sorted with sprites in one pass;
  tilemaps - square, isometric and hexagonal - with autotiling, animated and
  shaped tiles; distance-field text with rich text, outlines, shaping and
  bidirectional text from font files; 2D lighting with soft shadows, normal
  maps, specular, light layers, cookies, ambient occlusion and bounced light;
  CPU and GPU particles; HDR, bloom and a post stack (tonemapping, colour
  grading, FXAA, motion blur, depth of field, lens flares, light shafts, CRT,
  pixelation, palettes and more); shader graphs; render-to-texture cameras,
  split screen, mirrors and portals; a pixel-perfect camera.
- **Scripting**: Lua scripts that run on Luau natively and on a Lua 5.3 VM in
  the editor and the browser, through one namespaced API (`Sprite.set_color`,
  `Physics.raycast`, ...); behaviour graphs and behaviour trees; timelines,
  dialogue tables, localisation, save slots and replays.
- **Physics** (rapier): bodies, nine kinds of joint (revolute, prismatic, fixed, rope, spring,
  wheel, target, motor, friction), a character controller,
  continuous collision, ragdolls, breakable and sliceable bodies, conveyors,
  area effects and buoyancy, trajectory prediction, shape queries.
- **Animation**: sprite animation with frame events, skeletons with blend
  spaces, layers, IK and jiggle bones, skinned meshes.
- **Audio** (kira / Web Audio): mixer buses with effects, spatial sound with
  occlusion, a music director with crossfades, stems and playlists, tracker
  songs, synthesis and spectrum analysis.
- **UI**: entity UI and `.gui` documents laid out for any screen size and safe
  area, controller and keyboard navigation, text fields with IME, drag and
  drop, screen-reader support, on-screen touch controls.
- **Input**: keyboard, mouse, gamepads (with rumble), multi-touch and
  gestures, rebindable input actions with hold, tap and button prompts.
- **Packages**: Rust crates that add components, systems and script commands
  to a project, linked into its runtime, editor viewport and web build.
- **Editor** (Tauri 2 + Svelte): scene, prefab, tilemap, animation, shader
  graph and behaviour graph editors; Play mode on the same engine as a
  shipped web build; a profiler; builds for every platform; a Help menu with
  the documentation, credits and third-party licences.
- **Builds**: Windows, Web (for itch-style hosting or web portals), Android
  (APK and App Bundle) and Linux; macOS and iOS builds exist but are unverified.
- **Servers**: `jengine-server`, a game server framework - rooms, players whose
  state outlives a room, interest-managed state sync, predicted movement,
  accounts, a SQLite store, clusters, HTTP routes, an admin dashboard and
  Prometheus metrics - with `jengine-fleet` to deploy it and a web console to
  run it; `jengine-relay` for turn-based games; live ops (signed remote
  config, experiments, events and content releases) and downloadable asset
  packs.
- `project.jengine` records the engine version that saved it, so later
  releases can migrate a project.

### Known limitations

- macOS is only type-checked and iOS has never run; Linux is tested under WSL.
- Building a game compiles the engine from source, so the editor and the CLI
  need a checkout of this repository and a Rust toolchain.
- The script API, the scene format, `project.jengine` and the package API may
  change in any 0.x release.
- The server framework's known limits are listed in
  [crates/jengine-server/LIMITS.md](crates/jengine-server/LIMITS.md).

[0.2.0]: https://github.com/jkilpala/jengine/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jkilpala/jengine/releases/tag/v0.1.0
