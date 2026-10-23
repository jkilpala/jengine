-- 123 Sundial Plaza: the `day_cycle` on this entity moves the sun; every
-- sprite with a `cast_shadow` lays its silhouette on the cobbles along the
-- light. This script walks the townsfolk across the square, lights the lamp
-- posts at dusk (so the walkers' shadows fall two ways at night) and jumps
-- the clock on 1-4.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/123-sundial-plaza.scene"
local LEFT, RIGHT = -620, 620

local day, status
local lamps = {}
local walkers = {}
local captions = true
local hurry = false
local lit, glow = false, 0
local t = 0

function init(entity)
    day = entity
    status = Entity.find_by_name("Status")
    for i = 0, 1 do lamps[#lamps + 1] = Entity.find_by_name("Lamp" .. i) end
    local speeds = { 70, -55, 90 }
    for i = 0, 2 do
        local e = Entity.find_by_name("Walker" .. i)
        local x, y = Transform.get_position(e)
        walkers[#walkers + 1] = { e = e, x = x, y = y, v = speeds[i + 1], phase = i * 1.7 }
    end
end

function on_dusk(entity) lit = true end
function on_dawn(entity) lit = false end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("Space") then
        hurry = not hurry
        DayCycle.set(day, "hours_per_second", hurry and 1.2 or 0.35)
    end
    for k, h in ipairs({ 8, 12, 15.5, 22 }) do
        if Input.is_key_just_pressed(tostring(k)) then
            DayCycle.set(day, "hour", h)
            lit = h > 18 or h < 6
        end
    end
    t = t + dt
    for _, w in ipairs(walkers) do
        w.x = w.x + w.v * dt
        if w.x > RIGHT then w.x, w.v = RIGHT, -math.abs(w.v) end
        if w.x < LEFT then w.x, w.v = LEFT, math.abs(w.v) end
        Transform.set_position(w.e, w.x, w.y + math.abs(math.sin(t * 6 + w.phase)) * 3)
        Transform.set_scale(w.e, w.v > 0 and 1 or -1, 1)
    end
    glow = glow + ((lit and 1 or 0) - glow) * math.min(1, dt * 1.5)
    for _, l in ipairs(lamps) do
        Lighting.set_intensity(l, glow * 1.3)
        Sprite.set_color(l, 3.0, 2.4, 1.5, glow * 0.9)
    end
    local hour = Entity.get_custom(day, "DayCycle", "hour") or 0
    local hh = math.floor(hour)
    local mm = math.floor((hour - hh) * 60)
    UI.set_text(status, string.format("%02d:%02d", hh, mm))
end
