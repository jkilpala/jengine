-- 121 The Endless Road: the car drives on forever, and the world is
-- built ahead of it a stretch at a time and dropped behind it
-- (`despawn_offscreen`). Its `floating_origin` moves the whole world back
-- whenever it strays 4,000 units out, so nothing ever stands far from the
-- origin; this script works in TRUE distance - an x plus the total the
-- world has been moved, which the engine keeps in the car's custom
-- `FloatingOrigin.x` - so the stretches it builds land where they belong,
-- and so it can say how far the car has really come.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/121-the-endless-road.scene"
local ROAD, GROUND = -150.0, -205.0
local CHUNK = 1280
local SPEED, HURRY = 600, 2400

local car, body, wheels, cam, head, status
local clouds, far, near = {}, {}, {}
local captions = true
local hurry = false
local next_chunk = -2 * CHUNK     -- true x of the next stretch to build
local scroll = { cloud = 0, far = 0, near = 0 }
local shifts, last_origin = 0, 0
local lit, glow = false, 0
local t = 0

-- A number in 0..1 for stretch `i`, slot `k`: the same every run.
local function hash(i, k)
    local s = math.sin(i * 12.9898 + k * 78.233) * 43758.5453
    return s - math.floor(s)
end

local function origin()
    return Entity.get_custom(car, "FloatingOrigin", "x") or 0
end

local function place(prefab, x, y)
    local e = Entity.spawn_prefab(prefab)
    Transform.set_position(e, x, y)
    return e
end

-- Build the stretch starting at true x `tx`.
local function build(tx)
    local i = math.floor(tx / CHUNK + 0.5)
    local x = tx - origin()          -- where that is in the world now
    place("ErRoad", x + CHUNK / 2, ROAD - 128)
    place("ErField", x + CHUNK / 2, ROAD - 128 - 300 + 4)
    -- Two telegraph poles a stretch, then a few trees, maybe a house and a fence.
    for k = 0, 1 do place("ErPole", x + 300 + k * 640, ROAD - 20) end
    if hash(i, 1) < 0.35 then
        place("ErHouse", x + 200 + hash(i, 2) * 800, ROAD - 14)
    end
    if hash(i, 3) < 0.5 then
        place("ErFence", x + 300 + hash(i, 4) * 600, ROAD - 22)
    end
    local n = 2 + math.floor(hash(i, 5) * 4)
    for k = 1, n do
        local px = x + hash(i, 10 + k) * CHUNK
        if hash(i, 20 + k) < 0.5 then
            place("ErPoplar", px, ROAD - 16)
        else
            place("ErTree", px, ROAD - 18)
        end
    end
end

function init(entity)
    car = Entity.find_by_name("Car")
    body = Entity.find_by_name("CarBody")
    wheels = { Entity.find_by_name("Wheel0"), Entity.find_by_name("Wheel1") }
    cam = Entity.find_by_name("Camera")
    head = Entity.find_by_name("Headlight")
    status = Entity.find_by_name("Status")
    for i = 0, 2 do
        clouds[#clouds + 1] = Entity.find_by_name("Cloud" .. i)
        far[#far + 1] = Entity.find_by_name("FarHill" .. i)
        near[#near + 1] = Entity.find_by_name("NearHill" .. i)
    end
end

function on_dusk(entity) lit = true end
function on_dawn(entity) lit = false end

-- Three tiles of a band about the camera, `width` apart, slid by `s`.
local function band(tiles, cx, s, width, y)
    local off = s % width
    for k, e in ipairs(tiles) do
        Transform.set_position(e, cx - off + (k - 2) * width, y)
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("Space") then hurry = not hurry end
    t = t + dt
    local speed = hurry and HURRY or SPEED
    -- The car drives itself on; the world comes back to meet it when it
    -- has gone too far.
    local cx = Transform.get_position(car) + speed * dt
    Transform.set_position(car, cx, GROUND)
    local o = origin()
    if o ~= last_origin then
        shifts = shifts + 1
        last_origin = o
    end
    local true_x = cx + o
    -- Build ahead.
    while next_chunk < true_x + 2 * CHUNK do
        build(next_chunk)
        next_chunk = next_chunk + CHUNK
    end
    -- The camera rides ahead of the car (its own transform, so the sky
    -- the day cycle draws about it is always the sky).
    local camx, camy = cx + 360, -30
    Transform.set_position(cam, camx, camy)
    -- The bands scroll by their own counters.
    scroll.cloud = scroll.cloud + speed * 0.04 * dt + 8 * dt
    scroll.far = scroll.far + speed * 0.12 * dt
    scroll.near = scroll.near + speed * 0.35 * dt
    band(clouds, camx, scroll.cloud, 700, 230)
    band(far, camx, scroll.far, 1280, ROAD - 40)
    band(near, camx, scroll.near, 1280, ROAD - 60)
    -- The body bobs; the wheels turn by the true distance, folded into one
    -- turn - a rotation of thousands of radians in a float is itself the
    -- kind of large number this scene is about.
    Transform.set_position(body, 0, 6 + math.sin(t * 9) * 1.2 + math.sin(t * 23) * 0.5)
    local turn = -(true_x / 24) % (2 * math.pi)
    for _, w in ipairs(wheels) do Transform.set_rotation(w, turn) end
    -- Headlights at dusk.
    glow = glow + ((lit and 1 or 0) - glow) * math.min(1, dt * 1.5)
    Lighting.set_intensity(head, glow * 1.6)
    Sprite.set_color(head, 3.0, 2.6, 1.8, glow * 0.9)
    local m = math.floor(true_x / 100 + 0.5)
    UI.set_text(status, string.format("%d m from the start   car at x %d   world moved back %d times",
        m, math.floor(cx + 0.5), shifts))
end
