-- Scene 180: particle trails (`trail` on an emitter). The trails are the
-- emitters' own; T takes them away and puts them back with Particles.set.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/180-particle-trails.scene"
local TRAILS = {
    Fountain = { length = 10, interval = 0.03, width = 0.9, taper = true, fade = true },
    Swirl = { length = 16, interval = 0.04, width = 0.7, taper = true, fade = true },
    Bouncers = { length = 16, interval = 0.05, width = 0.5, taper = true, fade = true },
}
local emitters = {}
local on = true
local status

function init(entity)
    status = Entity.find_by_name("Status")
    for name, _ in pairs(TRAILS) do emitters[name] = Entity.find_by_name(name) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("T") then
        on = not on
        for name, e in pairs(emitters) do
            if e then Particles.set(e, "trail", on and TRAILS[name] or nil) end
        end
    end
    if status then UI.set_text(status, on and "trails on" or "trails off") end
end
