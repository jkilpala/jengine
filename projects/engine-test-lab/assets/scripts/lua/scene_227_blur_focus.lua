-- Scene 227: `blur` with a focus rectangle - sharp inside a world
-- rectangle, soft outside it. The rectangle follows a wandering frame.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/227-blur-focus.scene"
local e = {}
local on = true
local t = 0

function init(entity)
    e.Box = Entity.find_by_name("Box")
    e.Line0 = Entity.find_by_name("Line0")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if Input.is_key_just_pressed("B") then
        on = not on
        PostProcess.set_enabled("blur", on)
    end
    local x, y = math.sin(t * 0.5) * 330, math.sin(t * 0.8) * 120
    Transform.set_position(e.Box, x, y)
    PostProcess.set_float("blur", "focus_x", x)
    PostProcess.set_float("blur", "focus_y", y)
    UI.set_text(e.Line0, on and "blur on, focus follows the frame" or "blur off")
end
