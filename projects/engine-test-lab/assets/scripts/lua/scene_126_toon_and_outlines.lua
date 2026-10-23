-- Scene 126: toon lighting on and off every two seconds (Lighting.set_toon),
-- and a star drawn through the ink outline graph, whose line is sampled a
-- few texels out.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/126-toon-and-outlines.scene"

local label
local on = true
local t = 0.0

local function show()
    if label then UI.set_text(label, on and "toon lighting ON" or "toon lighting OFF") end
end

function init(entity)
    label = Entity.find_by_name("State")
    show()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if t >= 2.0 then
        t = 0.0
        on = not on
        -- The scene's toon_lighting; 0 bands removes it.
        Lighting.set_toon(on and 3 or 0, 0.05)
        show()
    end
end
