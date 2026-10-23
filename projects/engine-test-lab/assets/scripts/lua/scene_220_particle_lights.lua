-- Scene 220: particles that give off light (`light` on each emitter).
-- L turns the light down to nothing and back through Particles.set.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/220-particle-lights.scene"
local INTENSITY = { Sparks = 1.4, Fireflies = 1.1, Embers = 1.3 }
local e = {}
local on = true

function init(entity)
    for name, _ in pairs(INTENSITY) do e[name] = Entity.find_by_name(name) end
    e.Line0 = Entity.find_by_name("Line0")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("L") then
        on = not on
        for name, v in pairs(INTENSITY) do
            Particles.set(e[name], "light.intensity", on and v or 0)
        end
    end
    UI.set_text(e.Line0, on and "particle lights on" or "particle lights off")
end
