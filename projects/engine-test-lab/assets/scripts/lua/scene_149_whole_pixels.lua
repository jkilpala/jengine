-- Scene 149: the scaling is the engine's (`integer_scale` on a pixel-perfect
-- camera); this drifts one square slowly across the checks.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/149-whole-pixel-scaling.scene"
local drifter
local t = 0

function init(entity)
    drifter = Entity.find_by_name("Drifter")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if drifter then
        Transform.set_position(drifter, -450 + (t * 18) % 900, -150)
    end
end
