-- Scene 121: ambient occlusion from occluders, on and off every two
-- seconds through Lighting.set_occlusion.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/121-ambient-occlusion.scene"

-- The scene's ambient_occlusion.
local RADIUS, STRENGTH = 40, 0.8
local state
local on = true
local t = 0.0

local function show()
    if state then Text.set(state, on and "occlusion ON" or "occlusion OFF") end
end

function init(entity)
    state = Entity.find_by_name("State")
    show()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if t >= 2.0 then
        t = 0.0
        on = not on
        -- The scene's ambient_occlusion radius; strength 0 removes it.
        Lighting.set_occlusion(RADIUS, on and STRENGTH or 0)
        show()
    end
end
