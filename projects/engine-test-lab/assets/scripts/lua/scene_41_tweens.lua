-- Scene 41 - Tweens & Easing
--
-- Every moving thing here is a Tween component authored in the .scene file -
-- this script does not animate anything. That is the point: a Tween is
-- declarative (target, from, to, duration, ease, on_complete) and the engine's
-- tween system ticks it.
--
--   target      PositionX / PositionY / Rotation / ScaleX / ScaleY / Opacity
--   ease        Linear / EaseInQuad / EaseOutQuad / EaseInOutQuad
--   on_complete Stop (clamp at `to`) / Loop (restart) / PingPong (swap + restart)
--
-- The four top rows share a duration and differ only in `ease`, so they
-- separate and re-converge - the clearest way to see what a curve does.

script = { tick_rate = 10 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/41-tweens.scene"

local readout

function init(entity)
    readout = Entity.find_by_name("Readout")
    if readout then
        Text.set(readout, "7 tweens, 0 lines of animation script - all authored in the scene")
    end
    log("[scene_41] ready - compare the four easing curves on the top rows")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
