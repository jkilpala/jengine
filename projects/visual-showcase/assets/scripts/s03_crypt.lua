-- 03 Crypt: torches that flicker, a wisp on the mouse, chains that sway,
-- and N to take every normal map away and give it back.
--
-- tools/gen_scenes.py tags each surface `bump_<texture stem>`; N takes the
-- normal maps away with Material.set_normal_map(e, nil) and hands each back
-- by its stem. (Until Sep 2026 a script could not change a normal map, and
-- this scene authored every surface twice and faded one copy out.)

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/03-crypt.scene"

local TORCHES = 4
local TORCH_INTENSITY = 2.4
local TORCH_HEAT = 1.6          -- the glow's brightness past white (gen_scenes `emissive`)
local WISP_INTENSITY = 2.6
local WISP_HEAT = 2.2
local CHAINS = 3
local CANDLES = 3
local CANDLE_INTENSITY = 2.2

local t = 0.0
local normals_on = true
local torches_on = true
local captions = true
local post_on = nav.post_all_on(SCENE)

local status
local torch_lights, torch_glows, torch_embers = {}, {}, {}
local candle_lights = {}
local flames = {}   -- { entity, phase, speed }
local chains = {}   -- { entities, phase, speed, amp }
local wisp = {}     -- the entities that ride the mouse
local wisp_light, wisp_glow
local wx, wy = -200.0, 0.0

local function flicker_at(time, phase)
    return 0.08 * math.sin(time * 11.3 + phase)
        + 0.05 * math.sin(time * 23.7 + phase * 1.7)
        + 0.04 * math.sin(time * 4.1 + phase * 0.3)
end

local SURFACES = { "bricks", "flagstones", "column", "barrel", "crate", "chain" }

local function set_normals(on)
    normals_on = on
    for _, stem in ipairs(SURFACES) do
        local normal = on and ("sprites/env/" .. stem .. "_n.png") or nil
        for _, e in ipairs(Entity.find_by_tag("bump_" .. stem)) do
            Material.set_normal_map(e, normal)
        end
    end
end

local function refresh_status()
    if status then
        Text.set(status, (normals_on and "normal maps on" or "normal maps OFF")
            .. "   " .. nav.post_status(post_on))
    end
end

function init(entity)
    math.randomseed(3)
    status = Entity.find_by_name("Status")
    for i = 1, TORCHES do
        torch_lights[i] = Entity.find_by_name("Torch" .. i .. "Light")
        torch_glows[i] = Entity.find_by_name("Torch" .. i .. "Glow")
        torch_embers[i] = Entity.find_by_name("Torch" .. i .. "Embers")
        for k = 1, 3 do
            local e = Entity.find_by_name("Torch" .. i .. "Flame" .. k)
            if e then
                flames[#flames + 1] = { entity = e, torch = i, phase = math.random() * 6.28,
                                        speed = 7.0 + math.random() * 6.0 }
            end
        end
    end
    for i = 1, CANDLES do
        candle_lights[i] = Entity.find_by_name("Candle" .. i .. "Light")
        for k = 1, 4 do
            local e = Entity.find_by_name("Candle" .. i .. "Flame" .. k)
            if e then
                flames[#flames + 1] = { entity = e, candle = i, phase = math.random() * 6.28,
                                        speed = 5.0 + math.random() * 4.0 }
            end
        end
    end
    for i = 1, CHAINS do
        local list = {}
        local e = Entity.find_by_name("Chain" .. i)
        if e then list[1] = e end
        chains[i] = { entities = list, phase = math.random() * 6.28,
                      speed = 0.7 + math.random() * 0.5, amp = 0.025 + math.random() * 0.02 }
    end
    for _, name in ipairs({ "WispGlow", "WispCore", "WispTrail", "Wisp" }) do
        local e = Entity.find_by_name(name)
        if e then wisp[#wisp + 1] = e end
    end
    wisp_light = Entity.find_by_name("Wisp")
    wisp_glow = Entity.find_by_name("WispGlow")
    refresh_status()
end

local function handle_keys()
    local changed = nav.post_keys(post_on)
    if Input.is_key_just_pressed("N") then
        set_normals(not normals_on)
        changed = true
    end
    if Input.is_key_just_pressed("L") then
        torches_on = not torches_on
        for i = 1, TORCHES do
            if torch_embers[i] then Particles.set_active(torch_embers[i], torches_on) end
        end
        changed = true
    end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    if changed then refresh_status() end
end

function update(entity, dt)
    nav.tick(SCENE)
    handle_keys()
    t = t + dt

    -- Torches, each on its own phase; L puts them out.
    for i = 1, TORCHES do
        local f = torches_on and (1.0 + flicker_at(t, i * 2.3)) or 0.0
        if torch_lights[i] then Lighting.set_intensity(torch_lights[i], TORCH_INTENSITY * f) end
        if torch_glows[i] then
            Sprite.set_color(torch_glows[i], TORCH_HEAT, 0.19 * TORCH_HEAT, 0.02 * TORCH_HEAT, 0.45 * f)
        end
    end
    for i = 1, CANDLES do
        if candle_lights[i] then
            Lighting.set_intensity(candle_lights[i], CANDLE_INTENSITY * (1.0 + 0.6 * flicker_at(t * 0.8, i * 1.3)))
        end
    end
    for _, fl in ipairs(flames) do
        if fl.candle or torches_on then
            local stretch = 1.0 + 0.16 * math.sin(t * fl.speed + fl.phase)
                + 0.08 * math.sin(t * fl.speed * 2.3 + fl.phase * 2.0)
            Transform.set_scale(fl.entity, 1.0 - 0.06 * math.sin(t * fl.speed * 1.4 + fl.phase), stretch)
            Transform.set_rotation(fl.entity, 0.09 * math.sin(t * fl.speed * 0.45 + fl.phase))
        else
            Transform.set_scale(fl.entity, 0.0, 0.0)
        end
    end

    -- Chains sway about the beam they hang from (anchored at the top).
    for _, c in ipairs(chains) do
        local r = c.amp * math.sin(t * c.speed + c.phase)
        for _, e in ipairs(c.entities) do Transform.set_rotation(e, r) end
    end

    -- The wisp eases after the mouse and bobs a little.
    local mx, my = Input.get_mouse_world_position()
    if mx then
        local k = math.min(1.0, dt * 6.0)
        wx = wx + (mx - wx) * k
        wy = wy + (my - wy) * k
    end
    local bx, by = wx + 4.0 * math.sin(t * 1.3), wy + 6.0 * math.sin(t * 2.1)
    for _, e in ipairs(wisp) do Transform.set_position(e, bx, by) end
    local pulse = 1.0 + 0.06 * math.sin(t * 3.7)
    if wisp_light then Lighting.set_intensity(wisp_light, WISP_INTENSITY * pulse) end
    if wisp_glow then
        Sprite.set_color(wisp_glow, 0.21 * WISP_HEAT, 0.6 * WISP_HEAT, WISP_HEAT, 0.6 * pulse)
    end
end
