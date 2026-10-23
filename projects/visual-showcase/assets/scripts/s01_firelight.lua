-- 01 Firelight: makes the brazier, the sconces and the lantern alive.
--
-- The fire's light flickers in brightness and reach, and also *moves* a
-- few pixels. That last part is what sells it: a light that only changes
-- brightness lights the room like a dimmer switch, while one that jitters
-- makes every shadow edge tremble the way real firelight does.
--
-- Flicker is a sum of a few sines plus a smoothed random walk. Sines alone
-- repeat visibly within a few seconds; the random walk alone looks like
-- noise. Together they read as flame.

local nav = require("nav")
local SCENE = "scenes/01-firelight.scene"

local BRAZIER_X, BRAZIER_Y = 40.0, -30.0   -- tools/gen_scenes.py BRAZIER
local FIRE_LIGHT_Y = BRAZIER_Y + 24.0
local FIRE_INTENSITY, FIRE_RADIUS = 3.2, 720.0
local SCONCE_INTENSITY = 2.2
local LANTERN_INTENSITY = 2.4
-- The scene is HDR, so a glow's colour may exceed 1.0 - these are how many
-- times brighter than white each one burns (tools/gen_scenes.py `emissive`).
local HALO_HEAT, CORE_HEAT, COAL_HEAT = 1.6, 2.5, 3.0
local SCONCE_HEAT, LANTERN_HEAT = 1.6, 2.0
local PILLARS = 6
local PILLAR_BASE = { 0.40, 0.36, 0.33 }   -- linear; the scene authors the same

local t = 0.0
local flicker_on = true
local lantern_on = true
local captions = true
local post_on = nav.post_all_on(SCENE)

local fire, halo, halo_core, coals, lantern, lantern_glow, status
local sconce_lights, sconce_glows = {}, {}
local flames = {}      -- { entity, phase, speed, base_rot }
local pillars = {}     -- { entity, falloff }

-- A random walk eased toward a new target a few times a second.
local walk = { value = 0.0, target = 0.0, timer = 0.0 }
local function step_walk(w, dt, every, amount)
    w.timer = w.timer - dt
    if w.timer <= 0 then
        w.timer = every * (0.6 + math.random() * 0.8)
        w.target = (math.random() * 2.0 - 1.0) * amount
    end
    w.value = w.value + (w.target - w.value) * math.min(1.0, dt * 14.0)
    return w.value
end

local jitter_x = { value = 0.0, target = 0.0, timer = 0.0 }
local jitter_y = { value = 0.0, target = 0.0, timer = 0.0 }

local function flicker_at(time, phase)
    return 0.07 * math.sin(time * 11.3 + phase)
        + 0.05 * math.sin(time * 23.7 + phase * 1.7)
        + 0.04 * math.sin(time * 4.1 + phase * 0.3)
end

local function collect_flames(prefix, count)
    for i = 1, count do
        local e = Entity.find_by_name(prefix .. "Flame" .. i)
        if e then
            flames[#flames + 1] = {
                entity = e,
                phase = math.random() * 6.28,
                speed = 7.0 + math.random() * 6.0,
            }
        end
    end
end

local function refresh_status()
    if status then Text.set(status, nav.post_status(post_on)) end
end

function init(entity)
    math.randomseed(7)
    fire = Entity.find_by_name("FireLight")
    halo = Entity.find_by_name("Halo")
    halo_core = Entity.find_by_name("HaloCore")
    coals = Entity.find_by_name("Coals")
    lantern = Entity.find_by_name("Lantern")
    lantern_glow = Entity.find_by_name("LanternGlow")
    status = Entity.find_by_name("Status")
    for i = 1, 2 do
        sconce_lights[i] = Entity.find_by_name("Sconce" .. i .. "Light")
        sconce_glows[i] = Entity.find_by_name("Sconce" .. i .. "Glow")
    end
    collect_flames("Brazier", 5)
    collect_flames("Sconce1", 3)
    collect_flames("Sconce2", 3)
    for i = 1, PILLARS do
        local e = Entity.find_by_name("Pillar" .. i)
        if e then
            local px, py = Transform.get_position(e)
            local d = math.sqrt((px - BRAZIER_X) ^ 2 + (py - BRAZIER_Y) ^ 2)
            -- How much of the fire a cap catches: unlit caps are coloured
            -- by the script instead: an occluder shaded its own sprite when this was made.
            pillars[#pillars + 1] = { entity = e, falloff = math.max(0.0, 1.0 - d / FIRE_RADIUS) }
        end
    end
    refresh_status()
end

local function handle_keys()
    nav.post_keys(post_on)
    if Input.is_key_just_pressed("F") then flicker_on = not flicker_on end
    if Input.is_key_just_pressed("L") or Input.is_mouse_right_just_pressed() then
        lantern_on = not lantern_on
    end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    refresh_status()
end

function update(entity, dt)
    nav.tick(SCENE)
    handle_keys()
    t = t + dt

    -- The brazier.
    local f = 1.0
    local jx, jy = 0.0, 0.0
    if flicker_on then
        f = 1.0 + flicker_at(t, 0.0) + step_walk(walk, dt, 0.09, 0.10)
        jx = step_walk(jitter_x, dt, 0.07, 5.0)
        jy = step_walk(jitter_y, dt, 0.07, 4.0)
    end
    if fire then
        Lighting.set_intensity(fire, FIRE_INTENSITY * f)
        Lighting.set_radius(fire, FIRE_RADIUS * (0.97 + 0.03 * f))
        Transform.set_position(fire, BRAZIER_X + jx, FIRE_LIGHT_Y + jy)
    end
    if halo then Sprite.set_color(halo, HALO_HEAT, 0.14 * HALO_HEAT, 0.01 * HALO_HEAT, 0.42 * f) end
    if halo_core then Sprite.set_color(halo_core, CORE_HEAT, 0.52 * CORE_HEAT, 0.13 * CORE_HEAT, 0.55 * f) end
    if coals then
        Sprite.set_color(coals, COAL_HEAT, 0.10 * COAL_HEAT, 0.004 * COAL_HEAT,
            0.75 + 0.2 * math.sin(t * 1.7) * (flicker_on and 1 or 0))
    end

    -- Every tongue of fire stretches and sways on its own clock.
    for _, fl in ipairs(flames) do
        local s = flicker_on and 1 or 0
        local stretch = 1.0 + s * (0.16 * math.sin(t * fl.speed + fl.phase)
            + 0.08 * math.sin(t * fl.speed * 2.3 + fl.phase * 2.0))
        local squeeze = 1.0 - s * 0.06 * math.sin(t * fl.speed * 1.4 + fl.phase)
        Transform.set_scale(fl.entity, squeeze, stretch)
        Transform.set_rotation(fl.entity, s * 0.09 * math.sin(t * fl.speed * 0.45 + fl.phase))
    end

    -- Sconces, each on its own phase.
    for i = 1, 2 do
        local sf = flicker_on and (1.0 + flicker_at(t, i * 2.1) * 1.4) or 1.0
        if sconce_lights[i] then Lighting.set_intensity(sconce_lights[i], SCONCE_INTENSITY * sf) end
        if sconce_glows[i] then
            Sprite.set_color(sconce_glows[i], SCONCE_HEAT, 0.21 * SCONCE_HEAT, 0.02 * SCONCE_HEAT, 0.5 * sf)
        end
    end

    -- Pillar caps catch the fire as it flickers.
    for _, p in ipairs(pillars) do
        local warm = p.falloff * p.falloff * 0.9 * f
        Sprite.set_color(p.entity,
            PILLAR_BASE[1] * (0.45 + warm * 1.3),
            PILLAR_BASE[2] * (0.45 + warm * 0.95),
            PILLAR_BASE[3] * (0.45 + warm * 0.7), 1.0)
    end

    -- The lantern rides the mouse.
    local wx, wy = Input.get_mouse_world_position()
    if lantern and wx then
        Transform.set_position(lantern, wx, wy)
        Lighting.set_intensity(lantern, lantern_on and LANTERN_INTENSITY or 0.0)
    end
    if lantern_glow and wx then
        Transform.set_position(lantern_glow, wx, wy)
        Sprite.set_color(lantern_glow, 0.32 * LANTERN_HEAT, 0.6 * LANTERN_HEAT, LANTERN_HEAT,
            lantern_on and 0.55 or 0.0)
    end
end
