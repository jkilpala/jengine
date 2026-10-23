-- Scene 23: Behaviors grid
-- Showcases Oscillator / Tween / Flicker / Shake / FaceVelocity /
-- AngularVelocity — all pure-data components driven by native systems.
-- Script only handles hub/next/prev navigation.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/23-behaviors.scene"

function init(entity)
    log("[23-behaviors] ready - native behavior systems running")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
