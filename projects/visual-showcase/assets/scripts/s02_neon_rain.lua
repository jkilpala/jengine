-- 02 Neon Rain: the broken sign, the wet road, and the weather.
--
-- The rain, the splashes and the steam are particles the scene authors; this
-- script does what needs a clock: the OPEN sign that keeps failing, the
-- reflections trembling as rain hits them, ripple rings recycled across the
-- road, and lightning.

local nav = require("nav")
local SCENE = "scenes/02-neon-rain.scene"

local STREET_Y = -150.0              -- tools/gen_scenes.py STREET_Y
local PINK = { 1.0, 0.1005, 0.2140 } -- lin(1.0, 0.35, 0.5)
local SIGN_HEAT = 4.0
local AMBIENT = { 0.1005, 0.1329, 0.4480, 0.10 }  -- lin(0.35, 0.40, 0.70), 0.10

local t = 0.0
local captions = true
local post_on = nav.post_all_on(SCENE)
local raining = true

local status
local open_text, open_light, open_halo
local rain, splash
local streaks = {}   -- { entity, base_alpha, color, phase }
local ripples = {}   -- { entity, age, life, delay }

-- The OPEN sign: mostly lit, and now and then a stutter of quick failures.
local open_on = true
local open_timer = 2.0
local stutter = 0

-- Lightning: a short list of (time, ambient intensity) steps.
local bolt = nil
local bolt_t = 0.0
local next_bolt = 14.0
local BOLT = { { 0.00, 0.95 }, { 0.05, 0.12 }, { 0.11, 0.75 }, { 0.20, 0.10 } }

local function refresh_status()
    if status then
        Text.set(status, nav.post_status(post_on) .. "   rain " .. (raining and "on" or "off"))
    end
end

local function set_open(on)
    open_on = on
    local k = on and SIGN_HEAT or 0.12
    if open_text then Text.set_color(open_text, PINK[1] * k, PINK[2] * k, PINK[3] * k, 1.0) end
    if open_light then Lighting.set_intensity(open_light, on and 1.6 or 0.05) end
    if open_halo then Sprite.set_color(open_halo, PINK[1], PINK[2], PINK[3], on and 0.35 or 0.02) end
end

local function respawn_ripple(r)
    r.age = 0.0
    r.life = 0.5 + math.random() * 0.4
    r.delay = math.random() * 0.8
    local x = -620 + math.random() * 1240
    local y = STREET_Y - 20 - math.random() * 180
    Transform.set_position(r.entity, x, y)
    Sprite.set_color(r.entity, 0.47, 0.60, 1.0, 0.0)
end

function init(entity)
    math.randomseed(1981)
    status = Entity.find_by_name("Status")
    open_text = Entity.find_by_name("OpenText")
    open_light = Entity.find_by_name("OpenLight")
    open_halo = Entity.find_by_name("OpenHalo")
    rain = Entity.find_by_name("Rain")
    splash = Entity.find_by_name("Splash")
    for i, spec in ipairs({
        { "ReflNoodle", 0.55, { 1.0, 0.0272, 0.5225 } },
        { "ReflBar", 0.50, { 0.0331, 0.7874, 1.0 } },
        { "ReflOpen", 0.45, PINK },
        { "ReflLamp", 0.60, { 1.0, 0.4770, 0.1193 } },
    }) do
        local e = Entity.find_by_name(spec[1])
        if e then
            streaks[#streaks + 1] = { entity = e, base = spec[2], color = spec[3], phase = i * 1.7 }
        end
    end
    for k = 0, 13 do
        local e = Entity.find_by_name("Ripple" .. k)
        if e then
            local r = { entity = e }
            respawn_ripple(r)
            r.delay = math.random() * 1.5
            ripples[#ripples + 1] = r
        end
    end
    refresh_status()
end

local function handle_keys()
    local changed = nav.post_keys(post_on)
    if Input.is_key_just_pressed("R") then
        raining = not raining
        if rain then Particles.set_active(rain, raining) end
        if splash then Particles.set_active(splash, raining) end
        changed = true
    end
    if Input.is_key_just_pressed("T") then
        bolt, bolt_t = 1, 0.0
    end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    if changed then refresh_status() end
end

local function tick_open(dt)
    open_timer = open_timer - dt
    if open_timer > 0 then return end
    if stutter > 0 then
        stutter = stutter - 1
        set_open(not open_on)
        open_timer = 0.03 + math.random() * 0.09
        if stutter == 0 then set_open(math.random() < 0.8) end
    elseif open_on then
        -- Time for a failure: a burst of quick flickers.
        stutter = 3 + math.floor(math.random() * 6)
        open_timer = 0.0
    else
        set_open(true)
        open_timer = 1.5 + math.random() * 4.0
    end
end

local function tick_lightning(dt)
    next_bolt = next_bolt - dt
    if next_bolt <= 0 and not bolt then
        bolt, bolt_t = 1, 0.0
        next_bolt = 12.0 + math.random() * 14.0
    end
    if not bolt then return end
    bolt_t = bolt_t + dt
    while BOLT[bolt + 1] and bolt_t >= BOLT[bolt + 1][1] do bolt = bolt + 1 end
    local level = BOLT[bolt][2]
    if bolt == #BOLT and bolt_t > BOLT[#BOLT][1] + 0.05 then
        bolt = nil
        Lighting.set_ambient(AMBIENT[1], AMBIENT[2], AMBIENT[3], AMBIENT[4])
        return
    end
    -- A flash is white-blue and lights everything the lamps do not.
    Lighting.set_ambient(0.75, 0.82, 1.0, level)
end

function update(entity, dt)
    nav.tick(SCENE)
    handle_keys()
    t = t + dt

    tick_open(dt)
    tick_lightning(dt)

    -- Reflections tremble as the rain breaks them up.
    for _, s in ipairs(streaks) do
        local wobble = 1.0 + 0.10 * math.sin(t * 9.0 + s.phase) + 0.05 * math.sin(t * 23.0 + s.phase * 2.0)
        Transform.set_scale(s.entity, 1.0, wobble)
        local a = s.base * (0.85 + 0.15 * math.sin(t * 13.0 + s.phase))
        if s.color == PINK and not open_on then a = a * 0.05 end
        Sprite.set_color(s.entity, s.color[1], s.color[2], s.color[3], a)
    end

    -- Ripples: grow and fade, then land somewhere else.
    for _, r in ipairs(ripples) do
        if r.delay > 0 then
            r.delay = r.delay - dt
        elseif raining or r.age > 0 then
            r.age = r.age + dt
            local k = r.age / r.life
            if k >= 1.0 then
                respawn_ripple(r)
                if not raining then r.delay = 1e9 end
            else
                local grow = 0.25 + k * 1.4
                Transform.set_scale(r.entity, grow, grow * 0.28)
                Sprite.set_color(r.entity, 0.47, 0.60, 1.0, 0.35 * (1.0 - k))
            end
        end
        if raining and r.delay > 1e8 then r.delay = math.random() * 0.8 end
    end
end
