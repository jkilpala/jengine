# Web portals

A web build can be made for a game portal - **jCabinet** (jWorld,
FingerCorner), **Poki**, **CrazyGames** or **YouTube Playables**. The page
then loads that portal's SDK and an adapter the engine talks to, so the game
says the same few things on every portal and each one hears them its own way:

- **Loading.** The adapter feeds a portal that draws its own loading bar
  (jCabinet) from the downloads, and says loading is done when the engine has
  drawn its first frame.
- **Ready.** It tells the portal when the game can be played: on the first frame,
  or when a script says so.
- **Play.** Scripts say when play starts and stops, ask for ads, celebrate and
  send scores (`Platform.*`).
- **Pause and mute.** The portal can pause the game (YouTube does whenever the
  player looks away) or mute it (its audio setting), and every portal wants the
  game frozen and silent while an ad plays. The engine then stops updating,
  drawing, listening to input and playing sound until the portal lets go.

Nothing here changes a game that is not built for a portal: natively, in the
editor and in a plain web build the `Platform.*` calls do nothing, and an ad
request answers at once that none was shown - so a game's ad flow runs, and is
testable, everywhere.

## Choosing the portal

The portal is chosen per build, because the same game goes to several:

| Where | How |
|---|---|
| Project Settings -> Web | `settings.web_platform` in `project.jengine` - the default |
| Build panel -> Web -> Portal | this build only ("Project setting" keeps the default) |
| CLI | `jengine web-build <project> --platform poki` (`none`, `jcabinet`, `poki`, `crazygames`, `youtube`) |

`settings.web_game_ready` - Project Settings -> Web -> Game ready - is when the
portal hears the game can be played:

- `first_frame` (the default) suits a game that opens on its menu.
- `script` waits for `Platform.game_ready()`, for a game with a splash, an
  intro or its own loading first. YouTube Playables fails a game that says
  "ready" while something non-interactive is on screen.

The builder (`jengine_builder::web_platform`) puts two classic scripts in the
page's `<head>`, before the module script that starts the wasm: the portal's
SDK, then `jengine-platform.js`, assembled from `crates/jengine-builder/assets/platform/`
(`core.js` plus the portal's file).

| Portal | SDK |
|---|---|
| jCabinet | `/api/v1/sdk/jcabinet.js`, served by the portal itself |
| Poki | `https://game-cdn.poki.com/scripts/v2/poki-sdk.js` |
| CrazyGames | `https://sdk.crazygames.com/crazygames-sdk-v3.js` |
| YouTube Playables | `https://www.youtube.com/game_api/v1` |

## What each portal hears

| The engine | jCabinet | Poki | CrazyGames | YouTube |
|---|---|---|---|---|
| page loaded | `setLoadingProgress(0)` | `init()` | `init()`, `loadingStart()` | onPause / onResume / audio hooks |
| download progress | `setLoadingProgress(p)` | - | - | - |
| first frame | `gameLoaded()` | `gameLoadingFinished()` | `loadingStop()` | `firstFrameReady()` |
| game ready | - | - | - | `gameReady()` |
| `Platform.gameplay_start/stop` | - | `gameplayStart/Stop()` | `gameplayStart/Stop()` | - |
| `Platform.happy_time` | - | - | `happytime()` | - |
| `Platform.submit_score(v, board)` | `submitScore(board, v)` (needs a board) | - | - | `sendScore({ value })` |
| `Platform.show_ad("midgame")` | not shown (no midgame ads) | `commercialBreak()` | `requestAd("midgame")` | `requestInterstitialAd()` |
| `Platform.show_ad("rewarded", id)` | `showVideoAd()` | `rewardedBreak()` | `requestAd("rewarded")` | `requestRewardedAd(id)` |
| pauses the game | during its ad | during an ad | during an ad | `onPause` (and around its ads) |
| mutes the game | during its ad | during an ad | during an ad, `settings.muteAudio` | `isAudioEnabled()` false |

The adapter follows each portal's rules so a game does not have to:

- Gameplay start and stop are sent only when they change, and never during an
  ad. If the game was playing when it asked for an ad, play stops before the
  ad and starts again after it.
- CrazyGames is spoken to only where its `environment` is `crazygames` or
  `local`; anywhere else every call would throw.
- YouTube's `gameReady` waits for `firstFrameReady`, and is sent once.
- An SDK that never loaded (an ad blocker, a local test) makes every call a
  no-op, and an ad request answers "not shown".

## Scripts

```lua
-- What the build was made for: "jcabinet", "poki", "crazygames", "youtube",
-- or "none" (native, the editor, a plain web build).
if Platform.name() == "youtube" then ... end

Platform.game_ready()          -- only with web_game_ready = "script"
Platform.gameplay_start()      -- a level starts, the game resumes
Platform.gameplay_stop()       -- menu, pause, level end, game over
Platform.happy_time()          -- a boss beaten, a record broken (sparingly)
Platform.submit_score(1200, "high-scores")   -- the board is jCabinet's

-- Ads answer through an event: tell the player first for a rewarded one,
-- and give the reward only when `rewarded` is true.
Event.on("platform_ad_finished", entity)
function on_platform_ad_finished(entity, data)
    -- data = { kind = "rewarded", shown = true, rewarded = true }
    if data.rewarded then give_extra_life() end
end
Platform.show_ad("rewarded", "extra-life")   -- the id is YouTube's, one per kind of reward
Platform.show_ad("midgame")                  -- between levels; the portal decides whether one plays

-- The portal paused the game (YouTube): save now - the game stops after
-- this frame and comes back with platform_resume.
Event.on("platform_pause", entity)
function on_platform_pause(entity, data) Save.commit() end
```

While an ad plays or the portal holds the game, nothing runs: no `update`, no
timers, no physics, no sound. A script does not need to pause or mute anything
itself.

## How it fits together

- **`window.__jengine_platform`** is the adapter's face (`core.js`). The script
  preamble's `Platform.*` functions call its methods directly
  (`browser.rs`). They push no host command, so the editor viewport and the
  web runtime get them from the one preamble. Ad answers and the portal's
  pause reach scripts as ordinary `evt` commands, the queue `Event.send` uses.
- **`paused` / `muted`** on it are read by the web runtime every frame
  (`jengine-web/src/platform.rs`, `apply_host_state`). Paused stops the frame
  loop from the frame after the pause - so `platform_pause` is heard first -
  and suspends the `AudioContext`. Muted holds the master gain at 0 without
  touching the game's own master volume (`WebAudioBackend::set_host_*`).
- **`jengine-ready`**: the web runtime fires it on `window` after its first
  frame (and sets `window.__jengine_ready`). That, not `init()` resolving, is
  "loaded": after `init()` the project, scene, scripts and textures are still
  to come.
- **Natively** the `Platform.*` globals are Luau functions that answer as the
  page without an adapter does (`luau.rs`).

## Adding a portal

1. `crates/jengine-builder/assets/platform/<id>.js` defining `portal` with:
   - `name`
   - `init(host)` (may return a promise)
   - `loadingFinished`, `gameReady`, `gameplayStart`, `gameplayStop`,
     `happyTime`, `submitScore(value, board)`
   - `showAd(kind, started, reward)`: a promise of `{ shown, rewarded }`, and
     a call to `started()` when the ad really begins
   - optionally `loadingProgress(p)`

   `host.pause(on)` and `host.mute(on)` are how the portal reaches the game.
   Copy the nearest one.
2. A `WebPlatform` variant, its id, SDK URL and adapter in `web_platform.rs`.
3. The option in `BuildSettings.svelte` (`WEB_PLATFORMS`), in
   `ProjectSettings.svelte` (Web) and in `api/project.ts`.
4. This page's tables.

The adapter is plain ES5, and is tested by building a page for the portal and
playing Engine Test Lab scene 295 in it.

## Not covered yet

- **Cloud saves.** YouTube's `saveData` / `loadData` and CrazyGames' data
  module would back the save system on those portals. Saves still go to
  `localStorage`.
- **The portals' other modules:** accounts, invites and rooms, banners, game
  context, completion percentage.
- **Size and network limits.** YouTube Playables publishes bundle-size and
  network rules of its own; check them before sending a large game there.
