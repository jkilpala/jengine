-- 158 First Snow: every roof, sill, rail and bough here carries a
-- `snow_cover`, and the snow settles on each one's upward edges by itself.
-- This script only says whether it snows or thaws (`Snow.set`).

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/158-first-snow.scene"
local NAMES = { "Ground", "PineL", "PineR", "CottageA", "CottageB", "Fence", "Lamp", "Bench", "Cart" }
local status
local captions = true
local things = {}
local fall
local snowing = true
local t = 0

local function set_weather(snow)
    snowing = snow
    for _, e in ipairs(things) do
        Snow.set(e, "rate", snow and 0.6 or -2.5)
    end
    Particles.set(fall, "emission_rate", snow and 90 or 0)
end

function init(entity)
    status = Entity.find_by_name("Status")
    fall = Entity.find_by_name("Snowfall")
    for _, n in ipairs(NAMES) do table.insert(things, Entity.find_by_name(n)) end
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("S") then set_weather(true) end
    if Input.is_key_just_pressed("M") then set_weather(false) end
    t = t + dt
    UI.set_text(status, snowing and "snowing" or "thawing")
end
