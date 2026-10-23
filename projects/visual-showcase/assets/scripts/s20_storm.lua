-- 20 Storm: lightning, gusts and the grass. The rain needs nothing from
-- here - it lands on the roofs, walls and street by itself (`collision`) and
-- its splashes are a sub-emitter - so this only changes the weather.
-- L calls lightning, G a gust, R stops and starts the rain.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/20-storm.scene"

-- tools/gen_scenes.py: the ambient light, ST_RAIN_ANGLE.
local AMBIENT = { 0.17, 0.235, 0.52, 0.42 }
local RAIN_ANGLE = -1.40
local GUST_LEAN = 0.38                -- how much further a full gust leans the rain
local GRASS_W, BLADES = 1320, 170

local captions = true
local post_on = nav.post_all_on(SCENE)
local rain_on = true
local status, sky, bolt, rain, rain_far, grass
local puddles = {}
local t = 0.0
local rng = 12345

local function rand()
    -- Park-Miller: every product stays under 2^53, so Luau's doubles and
    -- Fengari's integers agree and the storm is the same in every runtime.
    rng = (rng * 16807) % 2147483647
    return rng / 2147483647
end

-- Lightning: a strike is a list of flashes, each (start, length, strength).
local next_strike = 3.0
local strike_t = -1.0
local strike = nil

-- Wind: a steady lean plus gusts that rise and fall.
local gust_t = -1.0
local next_gust = 6.0
local GUST_LEN = 3.5
local wind = 0.0
local last_angle = nil

-- The grass: blades along the verge, each with its own height. Laid out once;
-- the bending is the material's (materials/storm_grass), steered by `wind`.
local indices = {}
local sway_phase = 0.0
local grass_y = 0.0

local function refresh_status()
    if status then
        Text.set(status, "rain " .. (rain_on and "on" or "off") .. "   " .. nav.post_status(post_on))
    end
end

-- The verge, upright, once. The material bends each vertex by how far up its
-- blade it is (1 - v), so the mid vertices sit at v 0.45: 0.55 of the way up.
-- This used to rebuild all 850 vertices in Lua every frame and send them
-- across the bridge, which was most of the frame in a web build.
local function build_grass()
    local vs = {}
    for i = 1, BLADES do
        local x = -GRASS_W / 2 + (i - 0.5) * GRASS_W / BLADES + (rand() - 0.5) * 10
        -- Thicker where the verge is, thinner under the street's middle.
        local h = 34 + rand() * 60
        local hw = 3 + rand() * 3
        local s = 0.55 + rand() * 0.45
        vs[#vs + 1] = { x = x - hw, y = 0, u = 0, v = 1, r = s * 0.6, g = s * 0.6, b = s * 0.6 }
        vs[#vs + 1] = { x = x + hw, y = 0, u = 1, v = 1, r = s * 0.6, g = s * 0.6, b = s * 0.6 }
        vs[#vs + 1] = { x = x - hw * 0.6, y = h * 0.55, u = 0, v = 0.45, r = s, g = s, b = s }
        vs[#vs + 1] = { x = x + hw * 0.6, y = h * 0.55, u = 1, v = 0.45, r = s, g = s, b = s }
        vs[#vs + 1] = { x = x, y = h, u = 0.5, v = 0, r = s * 1.1, g = s * 1.1, b = s * 1.1 }
        -- Four vertices up the blade (two sides at base, two mid, one tip = 5).
        local b = (i - 1) * 5
        for _, k in ipairs({ 1, 2, 3, 2, 4, 3, 3, 4, 5 }) do indices[#indices + 1] = b + k end
    end
    Mesh.set(grass, vs, indices)
end

-- The wind into the material: (lean, sway, sway phase, the verge's world y).
-- The phase is added up here: time times a speed that the wind keeps
-- changing jumps further the longer the scene has run.
local function steer_grass(dt)
    sway_phase = sway_phase + dt * (2.2 + wind * 3.0)
    Material.set_vec4(grass, "wind", wind * 1.1, 0.12 + wind * 0.25, sway_phase % 6.283185, grass_y)
end

local function start_strike()
    strike_t = 0.0
    strike = { { 0.0, 0.07, 1.0 }, { 0.12, 0.05, 0.55 }, { 0.22, 0.30, 0.85 } }
    if bolt then
        Transform.set_position(bolt, -520 + rand() * 1040, 470)
        Transform.set_scale(bolt, rand() < 0.5 and -1 or 1, 0.9 + rand() * 0.2)
    end
end

-- How bright the lightning is now, 0..1: each flash fades over its length.
local function flash_level()
    if not strike then return 0.0 end
    local level = 0.0
    for _, f in ipairs(strike) do
        local s, len, k = f[1], f[2], f[3]
        if strike_t >= s and strike_t < s + len then
            level = math.max(level, k * (1.0 - (strike_t - s) / len) ^ 0.5)
        end
    end
    return level
end

local function apply_flash(k)
    local a = AMBIENT
    Lighting.set_ambient(a[1] + (0.75 - a[1]) * k, a[2] + (0.8 - a[2]) * k, a[3] + (1.0 - a[3]) * k, a[4] + 1.6 * k)
    if sky then
        local b = 0.9 + 3.5 * k
        Sprite.set_color(sky, b, b, b * 1.08, 1.0)
    end
    if bolt then Sprite.set_color(bolt, 0.8 * 3.0, 0.85 * 3.0, 3.0, math.min(1.0, k * 1.6)) end
    for _, p in ipairs(puddles) do
        local v = 0.2 + 1.4 * k
        Sprite.set_color(p, v, v * 1.1, v * 1.5, 1.0)
    end
end

function init(entity)
    status = Entity.find_by_name("Status")
    sky = Entity.find_by_name("Sky")
    bolt = Entity.find_by_name("Bolt")
    rain = Entity.find_by_name("Rain")
    rain_far = Entity.find_by_name("RainFar")
    grass = Entity.find_by_name("Grass")
    for i = 1, 3 do
        local p = Entity.find_by_name("Puddle" .. i)
        if p then puddles[#puddles + 1] = p end
    end
    if grass then
        local _, gy = Transform.get_position(grass)
        grass_y = gy
        build_grass()
    end
    refresh_status()
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    t = t + dt

    -- Lightning.
    next_strike = next_strike - dt
    if Input.is_key_just_pressed("L") or next_strike <= 0 then
        start_strike()
        next_strike = 5.0 + rand() * 7.0
    end
    if strike then
        strike_t = strike_t + dt
        apply_flash(flash_level())
        if strike_t > 0.6 then
            strike = nil
            apply_flash(0.0)
        end
    end

    -- Gusts: rise over a second, hold, die away.
    next_gust = next_gust - dt
    if Input.is_key_just_pressed("G") or next_gust <= 0 then
        gust_t = 0.0
        next_gust = 8.0 + rand() * 6.0
    end
    local target = 0.15 + 0.08 * math.sin(t * 0.7)
    if gust_t >= 0 then
        gust_t = gust_t + dt
        local g = math.min(1.0, gust_t / 1.0) * math.min(1.0, math.max(0.0, (GUST_LEN - gust_t) / 1.5))
        target = target + 0.85 * g
        if gust_t > GUST_LEN then gust_t = -1.0 end
    end
    wind = wind + (target - wind) * math.min(1.0, dt * 4.0)

    -- The rain leans with the wind. A new angle only when it has moved.
    local angle = RAIN_ANGLE + GUST_LEAN * (wind - 0.15)
    if last_angle == nil or math.abs(angle - last_angle) > 0.004 then
        last_angle = angle
        if rain then Particles.set(rain, "angle", { angle - 0.02, angle + 0.02 }) end
        if rain_far then Particles.set(rain_far, "angle", { angle - 0.02, angle + 0.02 }) end
    end

    if Input.is_key_just_pressed("R") then
        rain_on = not rain_on
        if rain then Particles.set_active(rain, rain_on) end
        if rain_far then Particles.set_active(rain_far, rain_on) end
        refresh_status()
    end

    if grass then steer_grass(dt) end
end
