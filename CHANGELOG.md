# Changelog

All notable changes to jEngine are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/) - which, before 1.0, means a minor
version (0.x.0) may break the script API and file formats. Breaking changes
are always listed under **Changed** or **Removed**.

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

[0.1.0]: https://github.com/jkilpala/jengine/releases/tag/v0.1.0
