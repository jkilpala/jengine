-- Scene 213: a canvas group drawn through a material (a glow round the
-- whole group) and a subtractive light (`subtract` on a point light) gliding
-- past a lamp.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/213-group-materials-and-dark-lights.scene"
local dark, line
local on = true
local moving = true
local t = 0

function init(entity)
    dark = Entity.find_by_name("Dark")
    line = Entity.find_by_name("Line0")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local key = Input.is_key_just_pressed
    if key("1") then
        on = not on
        Lighting.set_intensity(dark, on and 0.9 or 0)
    end
    if key("2") then moving = not moving end
    if moving then t = t + dt end
    Transform.set_position(dark, 300 + math.sin(t * 0.8) * 240, -60 + math.sin(t * 1.3) * 40)
    UI.set_text(line, "dark light " .. (on and "on" or "off") .. (moving and ", moving" or ", held"))
end
