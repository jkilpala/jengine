-- Scene 13 - Behavior graph showcase.
--
-- All entity behaviour in this scene is driven by .behavior assets
-- (under assets/behaviors/), NOT by this script. The driver script
-- only handles tour navigation + a "ready" log so the rest of the
-- scene visibly demonstrates the BehaviorGraphSystem in isolation:
--
--   * Player        -> behaviors/player-mover.behavior
--                       (OnTick -> SetVelocity(GetInputAxis * 220)
--                                  + BounceOnBounds)
--   * Bouncer1..6   -> behaviors/bouncer.behavior
--                       (OnTick -> BounceOnBounds; initial Velocity2D
--                        is scene-authored)
--   * Fader3s/6s/9s -> behaviors/fader-{N}s.behavior
--                       (OnTick -> DespawnAfter(N) - they disappear
--                        from the scene at the marked times)
--
-- Reset: navigate away with H / Esc and back via the hub - scene
-- reload re-spawns the faders.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/13-behavior.scene"

local logged = false

function update(entity, dt)
    if not logged then
        log("[scene_13] ready - WASD/arrows move Player, watch bouncers + faders (3s/6s/9s)")
        logged = true
    end
    nav.tick(SCENE_PATH)
end
