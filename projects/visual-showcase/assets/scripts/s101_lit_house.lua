-- 101 The Lit House: the walker is a behaviour tree (its rounds) and the
-- light is the engine's - the lamp's point light gets out only through the
-- gaps in the walls' occluders. This script blinks the lamp like a failing
-- bulb, and while the walker is inside it fades the roof away and blurs
-- everything outside the house (`blur` with a focus rectangle).

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/101-lit-house.scene"
local HALF_W, HALF_H = 180.000000, 130.000000
local LAMP = 1.700000
local BLUR = 7.0

local e = {}
local captions = true
local lamp_on = true
local inside = 0.0          -- 0 outside .. 1 inside, eased
local blinking = 0.0        -- seconds left in the current state
local lit = true
local rng = 101

local function rand()
    rng = (rng * 16807) % 2147483647
    return rng / 2147483647
end

function init(entity)
    for _, n in ipairs({ "Walker", "Roof", "Lamp", "Status" }) do e[n] = Entity.find_by_name(n) end
end

-- A failing bulb: long stretches lit, broken by blinks - one, or a stutter.
local function blink(dt)
    blinking = blinking - dt
    if blinking > 0 then return end
    if lit then
        lit = false
        blinking = 0.04 + rand() * 0.16
    else
        lit = true
        -- Sometimes it stutters straight back off.
        if rand() < 0.35 then blinking = 0.05 + rand() * 0.1 else blinking = 0.6 + rand() * 2.4 end
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("L") then lamp_on = not lamp_on end

    blink(dt)
    local glow = (lamp_on and lit) and LAMP or 0.0
    Lighting.set_intensity(e.Lamp, glow)
    Sprite.set_color(e.Lamp, 1, 1, 1, (lamp_on and lit) and 1 or 0.35)

    -- Inside the house: the roof fades, the outside goes out of focus.
    local x, y = Transform.get_position(e.Walker)
    local want = (math.abs(x) < HALF_W and math.abs(y) < HALF_H) and 1 or 0
    inside = inside + (want - inside) * math.min(1, dt * 4)
    Sprite.set_color(e.Roof, 1, 1, 1, 1 - inside)
    local r = BLUR * inside
    PostProcess.set_enabled("blur", r > 0.05)
    PostProcess.set_float("blur", "radius", r)
    UI.set_text(e.Status, tostring(Entity.get_custom(e.Walker, "Blackboard", "_running") or ""))
end
