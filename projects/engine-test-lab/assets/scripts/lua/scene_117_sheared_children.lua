-- Scene 117: a turned child under a squashed parent. Both discs spin at
-- the same rate; the left one's parent is squashed to 0.4 height, so it has
-- to spin within that squash - sheared - like a plate on a table.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/117-sheared-children.scene"

local discs = {}
local t = 0.0

function init(entity)
    discs = { Entity.find_by_name("DiscFlat"), Entity.find_by_name("DiscRound") }
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    for _, d in ipairs(discs) do
        if d then Transform.set_rotation(d, t * 0.8) end
    end
end
