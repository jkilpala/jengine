# jengine-audio

Sound effects and music on [kira](https://github.com/tesselode/kira), for the
native runtime.

This is a **built-in** package of the native runtime. It is **native only**:
kira does not build for `wasm32`, so a web build refuses a project that lists
it by name rather than silently leaving it out. The editor and web builds play
the same sounds through their own Web Audio backend, so leave it out of a
project's `"packages"` array and let `settings.enabled_packages` name it.

## What it registers

| Component | Is |
|---|---|
| `audio_source` | A clip on an entity: `clip_id`, `volume`, `pitch`, `looping`, `auto_play`, its mixer `bus_id`, and `spatial` (with `min_distance` / `max_distance`) to be heard from where the entity is. |

`audio_source` is also a **core** component, so it works in v1 and v2 scenes.

It re-exports core's `AudioBackend`, `KiraAudioBackend` and friends for Rust
code that wants them. Its `audio_update` system ticks a backend only if a host
shares one as a resource; the shipped runtimes drive their backend themselves.

## From scripts

Scripts use core's `Audio` namespace in every runtime: `Audio.play_sound`,
`Audio.play_sound_at`, `Audio.play_music`, `Audio.get_music_position`, the
mixer buses (`Audio.create_bus`, `Audio.set_bus_volume`,
`Audio.set_bus_effect`) and more - see the [Audio guide](/guides/audio/) and
the [Scripting API](/api-reference/scripting-api/).

The package's own `audio_*` script descriptors are **metadata only**; no
runtime installs them as globals.

## Things to know

- A clip must be preloaded: list it in the scene's top-level `"audio_refs"`
  array. `clip_id` is the path under `assets/` (`"sounds/hit.wav"`).
- Time things to the music with `Audio.get_music_position()`, not by adding
  up `dt`.
