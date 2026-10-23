-- 19 God Rays: the sun follows the mouse, and the light shafts' source goes
-- with it - `light_shafts` takes a world point, so the beams swing to follow.
-- G turns the shafts off and on; Space sends a gust through the leaves.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/19-god-rays.scene"

-- tools/gen_scenes.py: GR_SUN, and the Leaves emitter's rate.
local SUN_X, SUN_Y = 380.0, 250.0
local LEAF_RATE, GUST_RATE = 4.0, 60.0
local GUST_TIME = 1.2
local EASE = 3.0                    -- how quickly the sun catches the mouse

local captions = true
local post_on = nav.post_all_on(SCENE)
local shafts_on = true
local status, sun, glow, leaves
local sx, sy = SUN_X, SUN_Y
local gust = 0.0

local function refresh_status()
    if status then
        Text.set(status, "light shafts " .. (shafts_on and "on" or "off") .. "   " .. nav.post_status(post_on))
    end
end

function init(entity)
    status = Entity.find_by_name("Status")
    sun = Entity.find_by_name("Sun")
    glow = Entity.find_by_name("SunGlow")
    leaves = Entity.find_by_name("Leaves")
    refresh_status()
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    if Input.is_key_just_pressed("G") then
        shafts_on = not shafts_on
        PostProcess.set_enabled("light_shafts", shafts_on)
        refresh_status()
    end
    if Input.is_key_just_pressed("Space") and leaves then
        gust = GUST_TIME
        Particles.set_rate(leaves, GUST_RATE)
    end
    if gust > 0 then
        gust = gust - dt
        if gust <= 0 and leaves then Particles.set_rate(leaves, LEAF_RATE) end
    end

    -- The sun eases after the mouse, kept in the upper part of the sky.
    local mx, my = Input.get_mouse_world_position()
    local tx = math.max(-600, math.min(600, mx))
    local ty = math.max(40, math.min(330, my))
    local k = math.min(1.0, dt * EASE)
    sx, sy = sx + (tx - sx) * k, sy + (ty - sy) * k
    if sun then Transform.set_position(sun, sx, sy) end
    if glow then Transform.set_position(glow, sx, sy) end
    PostProcess.set_float("light_shafts", "source_x", sx)
    PostProcess.set_float("light_shafts", "source_y", sy)
end
