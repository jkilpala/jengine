-- 17 Murmuration: the flock is the jengine-swarm package - thousands of
-- birds flocking off a spatial grid, none of them known to this script. What
-- the script does is steer the whole flock through the package's `Swarm`
-- custom fields, which every runtime reads: a point that wanders the sky for
-- the flock to wheel after, and the falcon, a threat every nearby bird flees.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/17-murmuration.scene"
-- Luau has math.atan2; Lua 5.3 (the editor, the web) has a two-argument atan.
local atan2 = math.atan2 or math.atan

-- tools/gen_scenes.py: MURM_COUNT.
local COUNT_STEP, COUNT_MIN, COUNT_MAX = 1500, 1500, 12000
local THREAT_RADIUS = 150.0
local FALCON_SPEED = 520.0          -- how fast it closes on the mouse
local IDLE_HUNT = 3.0               -- seconds of a still mouse before it hunts alone

local captions = true
local post_on = nav.post_all_on(SCENE)
local status, flock, falcon
local count = 4500
local falcon_on = true
local t = 0.0
local fx, fy = 500.0, 250.0
local last_mx, last_my = 0.0, 0.0
local still = 0.0

local function refresh_status()
    if status then
        Text.set(status, string.format("%d starlings   falcon %s   ", count, falcon_on and "on" or "off")
            .. nav.post_status(post_on))
    end
end

function init(entity)
    status = Entity.find_by_name("Status")
    flock = Entity.find_by_name("Flock")
    falcon = Entity.find_by_name("Falcon")
    refresh_status()
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    if Input.is_key_just_pressed("Up") and count < COUNT_MAX then count = count + COUNT_STEP; refresh_status() end
    if Input.is_key_just_pressed("Down") and count > COUNT_MIN then count = count - COUNT_STEP; refresh_status() end
    if Input.is_key_just_pressed("Space") then
        falcon_on = not falcon_on
        if falcon then Sprite.set_color(falcon, 0.05, 0.035, 0.05, falcon_on and 1 or 0) end
        refresh_status()
    end
    t = t + dt

    -- Where the flock is drawn: a slow figure over the fields.
    local tx = math.sin(t * 0.11) * 430.0
    local ty = 70.0 + math.sin(t * 0.23 + 1.0) * 150.0

    -- The falcon: after the mouse, or - if the mouse has been still - a
    -- loop of its own that dives through the flock's heart.
    local mx, my = Input.get_mouse_world_position()
    if math.abs(mx - last_mx) + math.abs(my - last_my) > 0.5 then still = 0.0 else still = still + dt end
    last_mx, last_my = mx, my
    local gx, gy = mx, my
    if still > IDLE_HUNT then
        gx = tx + math.cos(t * 0.9) * 260.0
        gy = ty + math.sin(t * 1.8) * 120.0
    end
    local dx, dy = gx - fx, gy - fy
    local d = math.sqrt(dx * dx + dy * dy)
    local step = math.min(d, FALCON_SPEED * dt)
    if d > 1e-3 then
        fx, fy = fx + dx / d * step, fy + dy / d * step
        if falcon and d > 4 then Transform.set_rotation(falcon, atan2(dy, dx)) end
    end
    if falcon then Transform.set_position(falcon, fx, fy) end

    if flock then
        Entity.set_custom(flock, "Swarm", "count", count)
        Entity.set_custom(flock, "Swarm", "target_x", tx)
        Entity.set_custom(flock, "Swarm", "target_y", ty)
        Entity.set_custom(flock, "Swarm", "threat_x", fx)
        Entity.set_custom(flock, "Swarm", "threat_y", fy)
        Entity.set_custom(flock, "Swarm", "threat_radius", falcon_on and THREAT_RADIUS or 0)
    end
end
