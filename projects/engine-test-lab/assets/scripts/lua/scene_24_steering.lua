-- Scene 24: Steering
-- FollowTarget snaps Transform toward a moving target.
-- SeekTarget accelerates Velocity toward it, with arrival deceleration.
-- The target itself wanders via Oscillator.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/24-steering.scene"

function init(entity)
    log("[24-steering] ready - 4 followers + 3 seekers chase the yellow target")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
