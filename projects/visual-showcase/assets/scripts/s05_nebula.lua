-- 05 Nebula: parallax, a moving star, breathing clouds, meteors.
--
-- Every layer is tagged `depth_<n>` (n in hundredths) by tools/gen_scenes.py;
-- the mouse shifts each by its depth, stars least and the planet most, which
-- is all a parallax needs. The sun swings slowly about the planet, and since
-- the planet is a lit, normal-mapped sprite, its day side follows.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/05-nebula.scene"

local PLANET_X, PLANET_Y = 380.0, -125.0   -- gen_scenes NEBULA_PLANET
local SUN_ORBIT = 850.0                    -- the sun's distance from the planet
local SUN_ANGLE, SUN_SWING = 2.72, 0.45    -- where it sits, and how far it swings
local SUN_PERIOD = 40.0
local SUN_DEPTH = 0.06
local PARALLAX = 1.0
local CLOUDS = { "VeilMagenta", "VeilTeal", "VeilViolet", "KnotEmber", "KnotBlue", "KnotHeart" }

local t = 0.0
local paused = false
local captions = true
local post_on = nav.post_all_on(SCENE)

local status
local layers = {}   -- { entity, x, y, depth }
local sun = {}      -- the entities that ride the sun
local flares = {}   -- { entity, phase, speed }
local clouds = {}   -- { entity, r, g, b, a, rot, spin, phase }
local meteor = { entity = nil, age = 99.0, life = 0.7, next = 3.0, x = 0, y = 0, vx = 0, vy = 0 }
local mx, my = 0.0, 0.0

local function atan2(y, x)
    if math.atan2 then return math.atan2(y, x) end
    return math.atan(y, x)
end

local function refresh_status()
    if status then Text.set(status, (paused and "paused   " or "") .. nav.post_status(post_on)) end
end

function init(entity)
    math.randomseed(5)
    status = Entity.find_by_name("Status")
    for n = 0, 30 do
        for _, e in ipairs(Entity.find_by_tag("depth_" .. n)) do
            local x, y = Transform.get_position(e)
            layers[#layers + 1] = { entity = e, x = x, y = y, depth = n / 100.0 }
        end
    end
    for _, name in ipairs({ "SunFlare", "SunGlow", "SunLight" }) do
        local e = Entity.find_by_name(name)
        if e then sun[#sun + 1] = e end
    end
    for i = 1, 6 do
        local e = Entity.find_by_name("Flare" .. i)
        if e then flares[#flares + 1] = { entity = e, phase = math.random() * 6.28, speed = 1.5 + math.random() * 2.5 } end
    end
    for i, name in ipairs(CLOUDS) do
        local e = Entity.find_by_name(name)
        if e then
            local r, g, b, a = Sprite.get_color(e)
            clouds[#clouds + 1] = { entity = e, r = r, g = g, b = b, a = a, rot = Transform.get_rotation(e),
                                    spin = (i % 2 == 0 and 1 or -1) * (0.002 + 0.001 * i), phase = i * 1.7 }
        end
    end
    meteor.entity = Entity.find_by_name("Meteor")
    refresh_status()
end

local function is_sun(e)
    for _, s in ipairs(sun) do if s == e then return true end end
    return false
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Space") then paused = not paused; refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    if not paused then t = t + dt end

    -- Parallax: ease toward the mouse, then offset each layer by its depth.
    local wx, wy = Input.get_mouse_world_position()
    if wx then
        local k = math.min(1.0, dt * 3.0)
        mx = mx + (wx - mx) * k
        my = my + (wy - my) * k
    end
    for _, l in ipairs(layers) do
        if not is_sun(l.entity) then
            Transform.set_position(l.entity, l.x - mx * l.depth * PARALLAX, l.y - my * l.depth * PARALLAX)
        end
    end

    -- The sun swings about the planet; its flare, glow and light go together.
    local a = SUN_ANGLE + SUN_SWING * math.sin(t / SUN_PERIOD * math.pi * 2.0)
    local sx = PLANET_X + math.cos(a) * SUN_ORBIT - mx * SUN_DEPTH * PARALLAX
    local sy = PLANET_Y + math.sin(a) * SUN_ORBIT - my * SUN_DEPTH * PARALLAX
    for _, e in ipairs(sun) do Transform.set_position(e, sx, sy) end

    -- Stars twinkle; clouds turn and breathe, very slowly.
    for _, f in ipairs(flares) do
        local s = 1.0 + 0.12 * math.sin(t * f.speed + f.phase) + 0.05 * math.sin(t * f.speed * 3.1 + f.phase)
        Transform.set_scale(f.entity, s, s)
    end
    for _, c in ipairs(clouds) do
        Transform.set_rotation(c.entity, c.rot + t * c.spin)
        local breathe = 1.0 + 0.1 * math.sin(t * 0.35 + c.phase)
        Sprite.set_color(c.entity, c.r, c.g, c.b, c.a * breathe)
    end

    -- Now and then, a meteor.
    if meteor.entity and not paused then
        meteor.age = meteor.age + dt
        meteor.next = meteor.next - dt
        if meteor.next <= 0 then
            meteor.next = 5.0 + math.random() * 5.0
            meteor.age = 0.0
            meteor.x = -500 + math.random() * 1000
            meteor.y = 380
            local ang = -2.0 - math.random() * 0.6
            meteor.vx, meteor.vy = math.cos(ang) * 1100, math.sin(ang) * 1100
            Transform.set_rotation(meteor.entity, atan2(meteor.vy, meteor.vx))
        end
        local k = meteor.age / meteor.life
        if k < 1.0 then
            Transform.set_position(meteor.entity, meteor.x + meteor.vx * meteor.age, meteor.y + meteor.vy * meteor.age)
            Sprite.set_color(meteor.entity, 2.6, 2.8, 3.0, math.sin(k * math.pi))
        else
            Sprite.set_color(meteor.entity, 2.6, 2.8, 3.0, 0.0)
        end
    end
end
