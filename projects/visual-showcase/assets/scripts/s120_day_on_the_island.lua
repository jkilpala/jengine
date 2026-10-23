-- 120 A Day on the Island: the `day_cycle` on this entity turns the sky,
-- the light, the sun and the moon. This script lights the fisher's lamp at
-- dusk and puts it out at dawn (`on_dusk` / `on_dawn`), jumps the clock on
-- 1-4 and hurries the day on Space.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/120-a-day-on-the-island.scene"

local day, lamp, status
local captions = true
local hurry = false
local lit, glow = false, 0

function init(entity)
    day = entity
    lamp = Entity.find_by_name("Lamp")
    status = Entity.find_by_name("Status")
end

function on_dusk(entity) lit = true end
function on_dawn(entity) lit = false end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("Space") then
        hurry = not hurry
        DayCycle.set(day, "hours_per_second", hurry and 1.5 or 0.42)
    end
    for k, h in ipairs({ 8, 12, 18.6, 23 }) do
        if Input.is_key_just_pressed(tostring(k)) then
            DayCycle.set(day, "hour", h)
            lit = h > 18.5 or h < 6
        end
    end
    -- The lamp swells up and dies down rather than switching.
    glow = glow + ((lit and 1 or 0) - glow) * math.min(1, dt * 1.5)
    Lighting.set_intensity(lamp, glow * 1.0)
    Sprite.set_color(lamp, 3.0, 2.0, 0.9, glow * 0.9)
    local hour = Entity.get_custom(day, "DayCycle", "hour") or 0
    local hh = math.floor(hour)
    local mm = math.floor((hour - hh) * 60)
    UI.set_text(status, string.format("%02d:%02d", hh, mm))
end
