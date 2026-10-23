-- 117 Moor at Midnight: the light in the fog is the engine's - one `fog`
-- on "Mist" gathers the light round every point and veils what is behind it.
-- This script walks the traveller, swings and flickers her lantern, drifts
-- the wisps and, on F, thins, thickens or clears the fog.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/117-moor-at-midnight.scene"
local GROUND = -250.0

local traveller, lantern, mist, status
local wisps = {}
local captions = true
local t = 0
local hurry = 1
local fogs = { { 0.72, "a low fog" }, { 0.92, "a thick fog" }, { 0.35, "a thin mist" }, { 0.0, "clear night" } }
local fog_at = 1

function init(entity)
    traveller = Entity.find_by_name("Traveller")
    lantern = Entity.find_by_name("Lantern")
    mist = Entity.find_by_name("Mist")
    status = Entity.find_by_name("Status")
    for i = 1, 3 do wisps[i] = Entity.find_by_name("Wisp" .. (i - 1)) end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("Space") then hurry = hurry == 1 and 3 or 1 end
    if Input.is_key_just_pressed("F") then
        fog_at = fog_at % #fogs + 1
        Fog.set(mist, "density", fogs[fog_at][1])
    end
    -- The traveller walks across, and comes round again.
    local x, y = Transform.get_position(traveller)
    x = x + 48 * hurry * dt
    if x > 820 then x = -820 end
    Transform.set_position(traveller, x, y)
    -- The lantern swings at her side and flickers.
    local swing = math.sin(t * 3.1) * 6
    Transform.set_position(lantern, x + 20 + swing, GROUND + 22 + math.abs(math.sin(t * 3.1)) * 3)
    Lighting.set_intensity(lantern, 1.9 + 0.15 * math.sin(t * 17) + 0.1 * math.sin(t * 29 + 1))
    -- Wisps drift over the heather.
    for i = 1, 3 do
        local p = t * (0.18 + i * 0.05) + i * 2.1
        Transform.set_position(wisps[i], -300 + (i - 1) * 320 + math.sin(p) * 160, -170 + math.sin(p * 2.3) * 40)
    end
    UI.set_text(status, fogs[fog_at][2])
end
