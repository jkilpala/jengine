-- 08 Underwater: bends the kelp, schools the fish, floats the jellyfish on
-- the mouse and sways the god rays. The caustics and the water's wobble are
-- shader graphs and run on their own clock.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/08-underwater.scene"

-- tools/gen_scenes.py: UW_KELP (x, y, segments) and UW_FISH.
local KELP = { 6, 5, 7, 6, 7, 4 }
local FISH = 26
local RAYS = 5

-- Luau has math.atan2; Fengari (Lua 5.3) has only math.atan(y, x).
local function atan2(y, x)
    if math.atan2 then return math.atan2(y, x) end
    return math.atan(y, x)
end

local t = 0.0
local captions = true
local post_on = nav.post_all_on(SCENE)
local wobble_on = true
local status, wobble, jelly, jelly_glow, jelly_light
local kelp = {}      -- kelp[i][k] = entity
local rays = {}      -- { e, rot }
local fish = {}      -- { e, x, y, vx, vy }
local jx, jy = 0.0, 100.0

local function refresh_status()
    if status then
        Text.set(status, (wobble_on and "water wobble on   " or "water wobble off   ") .. nav.post_status(post_on))
    end
end

function init(entity)
    status = Entity.find_by_name("Status")
    wobble = Entity.find_by_name("Wobble")
    jelly = Entity.find_by_name("Jelly")
    jelly_glow = Entity.find_by_name("JellyGlow")
    jelly_light = Entity.find_by_name("JellyLight")
    for i, count in ipairs(KELP) do
        kelp[i] = {}
        for k = 1, count do kelp[i][k] = Entity.find_by_name("Kelp" .. i .. "_" .. k) end
    end
    for i = 1, RAYS do
        local e = Entity.find_by_name("Ray" .. i)
        if e then rays[#rays + 1] = { e = e, rot = Transform.get_rotation(e) } end
    end
    for i = 1, FISH do
        local e = Entity.find_by_name("Fish" .. i)
        if e then
            local x, y = Transform.get_position(e)
            fish[#fish + 1] = { e = e, x = x, y = y, vx = 60 + math.random() * 20, vy = (math.random() - 0.5) * 20 }
        end
    end
    refresh_status()
end

-- The school: each fish steers toward the school's middle, matches its
-- neighbours' heading, keeps its distance, follows a slow wandering goal,
-- and bolts from the jellyfish.
local function school(dt)
    local n = #fish
    if n == 0 then return end
    local cx, cy, avx, avy = 0, 0, 0, 0
    for _, f in ipairs(fish) do
        cx, cy, avx, avy = cx + f.x, cy + f.y, avx + f.vx, avy + f.vy
    end
    cx, cy, avx, avy = cx / n, cy / n, avx / n, avy / n
    local gx = math.sin(t * 0.13) * 430
    local gy = 40 + math.sin(t * 0.21 + 1.0) * 110
    for i, f in ipairs(fish) do
        local ax = (cx - f.x) * 0.6 + (avx - f.vx) * 0.9 + (gx - f.x) * 0.25
        local ay = (cy - f.y) * 0.6 + (avy - f.vy) * 0.9 + (gy - f.y) * 0.25
        for j, o in ipairs(fish) do
            if j ~= i then
                local dx, dy = f.x - o.x, f.y - o.y
                local d2 = dx * dx + dy * dy
                if d2 < 900 and d2 > 0.01 then
                    ax, ay = ax + dx / d2 * 2600, ay + dy / d2 * 2600
                end
            end
        end
        local dx, dy = f.x - jx, f.y - jy
        local d2 = dx * dx + dy * dy
        if d2 < 40000 then
            local k = (40000 - d2) / 40000 * 900 / math.sqrt(d2 + 1)
            ax, ay = ax + dx * k, ay + dy * k
        end
        f.vx, f.vy = f.vx + ax * dt, f.vy + ay * dt
        local sp = math.sqrt(f.vx * f.vx + f.vy * f.vy)
        local max = d2 < 40000 and 320 or 150
        if sp > max then f.vx, f.vy = f.vx / sp * max, f.vy / sp * max end
        if sp < 40 and sp > 0 then f.vx, f.vy = f.vx / sp * 40, f.vy / sp * 40 end
        f.x, f.y = f.x + f.vx * dt, f.y + f.vy * dt
        Transform.set_position(f.e, f.x, f.y)
        -- Art faces right: mirror when swimming left, tilt with the climb.
        local left = f.vx < 0
        Transform.set_scale(f.e, left and -1 or 1, 1)
        local tilt = atan2(f.vy, math.abs(f.vx)) * 0.6
        Transform.set_rotation(f.e, left and -tilt or tilt)
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    if Input.is_key_just_pressed("W") and wobble then
        wobble_on = not wobble_on
        -- A Scene Color sprite draws the scene, not its colour: it is turned
        -- off by moving it out of view.
        Transform.set_position(wobble, 0, wobble_on and 0 or 5000)
        refresh_status()
    end

    -- Kelp: every segment turned a little, a wave running up the strand.
    for i, strand in ipairs(kelp) do
        for k, e in ipairs(strand) do
            if e then
                local a = math.sin(t * 1.1 - k * 0.45 + i * 1.7) * (0.05 + 0.012 * k)
                Transform.set_rotation(e, a + (k == 1 and 0 or 0.02 * math.sin(t * 0.4 + i)))
            end
        end
    end

    -- God rays sway and breathe.
    for i, r in ipairs(rays) do
        Transform.set_rotation(r.e, r.rot + math.sin(t * 0.3 + i * 1.3) * 0.05)
        local a = 0.12 + 0.06 * math.sin(t * 0.7 + i * 2.1)
        Sprite.set_color(r.e, 0.84, 1.6, 1.6, a)
    end

    -- The jellyfish drifts after the mouse and pulses.
    local mx, my = Input.get_mouse_world_position()
    local k = math.min(1.0, dt * 1.6)
    jx, jy = jx + (mx - jx) * k, jy + (my - jy) * k + math.sin(t * 2.2) * 12 * dt
    local pulse = 1.0 + 0.08 * math.sin(t * 3.0)
    for _, e in ipairs({ jelly, jelly_glow, jelly_light }) do
        if e then Transform.set_position(e, jx, jy) end
    end
    if jelly then Transform.set_scale(jelly, pulse, 2.0 - pulse) end
    if jelly_light then Lighting.set_intensity(jelly_light, 2.0 + 0.4 * math.sin(t * 3.0)) end

    school(math.min(dt, 0.05))
end
