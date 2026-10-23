-- Scene 168: the lights are the engine's (`freeform_light`); this pulses the
-- hexagon through Lighting.set_intensity / set_color and turns the others.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/168-freeform-lights.scene"
local hex, window, l_light
local t = 0
local window_turn, l_spin = 0, false

function init(entity)
    hex = Entity.find_by_name("Hex")
    window = Entity.find_by_name("Window")
    l_light = Entity.find_by_name("LLight")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    local k = 0.5 + 0.5 * math.sin(t * 2.2)
    Lighting.set_intensity(hex, 0.3 + 1.0 * k)
    Lighting.set_color(hex, 0.4 + 0.6 * k, 1.0 - 0.5 * k, 0.6 + 0.4 * (1 - k))
    if Input.is_key_just_pressed("Space") then window_turn = window_turn + 0.4 end
    Transform.set_rotation(window, window_turn)
    if Input.is_key_just_pressed("R") then l_spin = not l_spin end
    if l_spin then Transform.set_rotation(l_light, t * 0.6) end
end
