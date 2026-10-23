-- 18 Graveyard Watch: the lantern follows the mouse, snapped to the art's
-- pixel grid; the torches flicker; D steps the lighting between dithered
-- bands, hard bands and smooth light (Lighting.set_toon), and P / C turn the
-- pixelate pass and the CRT off and on.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/18-graveyard-watch.scene"

-- tools/gen_scenes.py: PP (world units per art pixel) and RT_TOON.
local PP = 4.0
local MODES = {
    { "dithered", 4, 1.0, PP },
    { "hard bands", 4, 0.0, 0 },
    { "smooth", 0 },
}
local TORCH = 1.5

local captions = true
local post_on = nav.post_all_on(SCENE)
local mode = 1
local pixelate_on, crt_on = true, true
local status, lantern, lantern_light
local torches, flames = {}, {}
local t = 0.0

local function refresh_status()
    if status then
        UI.set_text(status, "light " .. MODES[mode][1] .. "   pixelate " .. (pixelate_on and "on" or "off")
            .. "   crt " .. (crt_on and "on" or "off"))
    end
end

function init(entity)
    status = Entity.find_by_name("Status")
    lantern = Entity.find_by_name("Lantern")
    lantern_light = Entity.find_by_name("LanternLight")
    for i = 1, 2 do
        torches[i] = Entity.find_by_name("TorchLight" .. i)
        flames[i] = Entity.find_by_name("Flame" .. i)
    end
    refresh_status()
end

function update(entity, dt)
    nav.tick(SCENE)
    nav.post_keys(post_on)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("D") then
        mode = mode % #MODES + 1
        local m = MODES[mode]
        Lighting.set_toon(m[2], m[3], m[4])
        refresh_status()
    end
    if Input.is_key_just_pressed("P") then
        pixelate_on = not pixelate_on
        PostProcess.set_enabled("pixelate", pixelate_on)
        refresh_status()
    end
    if Input.is_key_just_pressed("C") then
        crt_on = not crt_on
        PostProcess.set_enabled("crt", crt_on)
        refresh_status()
    end
    t = t + dt

    -- The lantern, on the art's grid: it moves a whole art pixel at a time.
    local mx, my = Input.get_mouse_world_position()
    local x = math.floor(mx / PP + 0.5) * PP
    local y = math.floor(my / PP + 0.5) * PP
    if lantern then Transform.set_position(lantern, x, y) end
    if lantern_light then Transform.set_position(lantern_light, x, y) end

    -- Torches: two sines and a little noise, never quite in step.
    for i, e in ipairs(torches) do
        local f = 0.82 + 0.1 * math.sin(t * 9.0 + i * 2.1) + 0.06 * math.sin(t * 23.0 + i) + 0.05 * math.random()
        Lighting.set_intensity(e, TORCH * f)
        local fl = flames[i]
        if fl then Transform.set_scale(fl, 1.0, 0.85 + 0.25 * f) end
    end
end
