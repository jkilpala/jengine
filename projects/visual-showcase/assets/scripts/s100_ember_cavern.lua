-- 100 Ember Cavern: the cave is lit by its particles - every ember,
-- spark and spore carries a light (`light` on its emitter). This script
-- only bursts the bubbles (an emitter's burst fires when it goes active),
-- flickers the lava's dim glow, and turns the particle lights off and on.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/100-ember-cavern.scene"
local POPS = 4
local LIGHTS = { Embers = 1.4, Spores = 0.9 }
local POP_LIGHT = 1.5

local e = {}
local pops = {}
local captions = true
local lights_on = true
local t = 0
local rng = 100

local function rand()
    rng = (rng * 16807) % 2147483647
    return rng / 2147483647
end

function init(entity)
    for _, n in ipairs({ "Embers", "Spores", "LavaGlow0", "LavaGlow1", "LavaGlow2" }) do e[n] = Entity.find_by_name(n) end
    for i = 0, POPS - 1 do
        pops[i] = { e = Entity.find_by_name("Pop" .. i), next = 0.6 + rand() * 2.5, armed = false }
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("L") then
        lights_on = not lights_on
        for name, v in pairs(LIGHTS) do Particles.set(e[name], "light.intensity", lights_on and v or 0) end
        for i = 0, POPS - 1 do Particles.set(pops[i].e, "light.intensity", lights_on and POP_LIGHT or 0) end
    end
    -- A bubble bursts: its emitter goes inactive (which arms the burst)
    -- and active again the next frame (which fires it).
    for i = 0, POPS - 1 do
        local p = pops[i]
        if p.armed then
            Particles.set(p.e, "active", true)
            p.armed = false
        else
            p.next = p.next - dt
            if p.next <= 0 then
                Particles.set(p.e, "active", false)
                p.armed = true
                p.next = 1.2 + rand() * 3.0
            end
        end
    end
    for i = 0, 2 do
        local f = 0.42 + 0.06 * math.sin(t * 2.3 + i * 1.7) + 0.04 * math.sin(t * 5.1 + i)
        Lighting.set_intensity(e["LavaGlow" .. i], f)
    end
end
