-- Scene 133: `light_shafts` streams light from a world point. The disc
-- swings on its oscillator; the source follows it here every frame.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/133-light-shafts.scene"

local sun, status
local on = true

function init(entity)
    sun = Entity.find_by_name("Sun")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("G") then
        on = not on
        PostProcess.set_enabled("light_shafts", on)
        if status then UI.set_prop(status, "text", on and "light shafts on" or "light shafts off") end
    end
    if sun then
        local x, y = Transform.get_position(sun)
        PostProcess.set_float("light_shafts", "source_x", x)
        PostProcess.set_float("light_shafts", "source_y", y)
    end
end
