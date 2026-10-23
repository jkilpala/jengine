-- 10 Clay: the sun follows the mouse - placed far out along the mouse's
-- direction from the middle, so the shadows point away from it - and A / S
-- turn the ambient occlusion and the sun off and on, to see each alone.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/10-clay.scene"

-- tools/gen_scenes.py: CLAY_CAPTIONS for all three (dark, on white).
local CAPTION = { 0.0273, 0.0273, 0.0331 }
local CAPTIONS = { Title = CAPTION, Hint = CAPTION, Status = CAPTION }

local captions = true
local post_on = nav.post_all_on(SCENE)
local occlusion_on = true
local sun_on = true
local status, sun

local function refresh_status()
    if status then
        Text.set(status, "occlusion " .. (occlusion_on and "on" or "off") .. "   sun "
            .. (sun_on and "on" or "off") .. "   " .. nav.post_status(post_on))
    end
end

function init(entity)
    status = Entity.find_by_name("Status")
    sun = Entity.find_by_name("Sun")
    refresh_status()
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions, CAPTIONS) end
    if Input.is_key_just_pressed("A") then
        occlusion_on = not occlusion_on
        -- tools/gen_scenes.py: the scene's ambient_occlusion.
        Lighting.set_occlusion(52, occlusion_on and 0.6 or 0)
        refresh_status()
    end
    if Input.is_key_just_pressed("S") and sun then
        sun_on = not sun_on
        Lighting.set_intensity(sun, sun_on and 0.85 or 0.0)
        refresh_status()
    end

    -- The sun, far out beyond the mouse: its light nearly parallel over
    -- the room, so every shadow points the same way.
    if sun then
        local mx, my = Input.get_mouse_world_position()
        local len = math.sqrt(mx * mx + my * my)
        if len < 1 then mx, my, len = -1, 1, 1.414 end
        Transform.set_position(sun, mx / len * 2600, my / len * 2600)
    end
end
