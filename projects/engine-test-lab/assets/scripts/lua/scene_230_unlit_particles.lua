-- Scene 230: `unlit` on a particle emitter is honoured in a lit frame.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/230-unlit-particles.scene"
local lamp, on = nil, false

function init(entity)
    lamp = Entity.find_by_name("Lamp")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("L") then
        on = not on
        Lighting.set_intensity(lamp, on and 1.6 or 0)
    end
end
