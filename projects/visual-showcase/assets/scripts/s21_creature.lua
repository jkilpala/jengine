-- 21 The Creature: pace, stops and the ground. The legs are not this
-- script's: the clips animate them, `Animation.set_blend` mixes the walk
-- into the run by pace, and each paw's foot IK plants them on the terrain.
-- This keeps the body over the ground, moves it at the speed the blended
-- gait walks at, and follows it with the camera.
-- Up / Down set the pace, Space stops and starts, I turns foot IK off.

script = { tick_rate = 60 }

local nav = require("nav")
local T = require("cr_terrain")
local SCENE = "scenes/21-the-creature.scene"

local WALK, RUN, IDLE = "cr_walk", "cr_run", "cr_idle"
local FADE = 0.5                  -- seconds a stop or a start crossfades over
local HIND_X, FRONT_X = 8, 136    -- where the hips and shoulders stand (tools/creature.py)
local PAWS = { "Cat_hn_paw", "Cat_hf_paw", "Cat_fn_paw", "Cat_ff_paw" }

local captions = true
local post_on = nav.post_all_on(SCENE)
local cat, cam, far, mid, status
local paws = {}
local x, y, tilt = 0.0, 0.0, 0.0
local cam_x, cam_y = 0.0, 0.0
local speed = 0.0
local pace, target_pace = 0.0, 0.0
local moving = false
local ik_on = true
local idle_input = 10.0           -- seconds since a key; the demo drives after ten (and at first)
local demo_t = 0.0

-- The demo, when nobody is driving: (seconds, moving, pace).
local DEMO = {
    { 2.5, false, 0.0 }, { 5.0, true, 0.0 }, { 5.0, true, 1.0 }, { 4.0, true, 0.35 },
    { 4.0, true, 0.0 }, { 3.0, false, 0.0 },
}

local function wrap(v, m) return v - math.floor(v / m) * m end

-- The terrain under x, from the table the scene was built from.
local function ground(px)
    local n = #T.H
    local f = wrap(px, T.PERIOD) / T.STEP
    local i = math.floor(f)
    local a = T.H[(i % n) + 1]
    local b = T.H[((i + 1) % n) + 1]
    return a + (b - a) * (f - i)
end

-- How fast the blended gait's feet go by: stride over stance time, each
-- between the walk's and the run's by the blend.
local function gait_speed(p)
    return T.WALK_SPEED + (T.RUN_SPEED - T.WALK_SPEED) * p * p
end

local function refresh_status()
    if not status then return end
    local what
    if not moving then what = "standing"
    elseif pace < 0.05 then what = "walking"
    elseif pace > 0.95 then what = "running"
    else what = "walk " .. math.floor((1 - pace) * 100 + 0.5) .. "% / run " .. math.floor(pace * 100 + 0.5) .. "%" end
    Text.set(status, what .. "   foot IK " .. (ik_on and "on" or "off") .. "   " .. nav.post_status(post_on))
end

local function set_moving(on)
    if on == moving or not cat then return end
    moving = on
    if on then
        Animation.crossfade(cat, WALK, FADE)
        Animation.set_blend(cat, RUN, pace)
    else
        -- The fade keeps the gait it is leaving, blend and all.
        Animation.crossfade(cat, IDLE, FADE)
        Animation.set_blend(cat, "", 0)
    end
end

function init(entity)
    cat = Entity.find_by_name("Cat")
    cam = Entity.find_by_name("Camera")
    far = Entity.find_by_name("HillsFar")
    mid = Entity.find_by_name("HillsMid")
    status = Entity.find_by_name("Status")
    for _, n in ipairs(PAWS) do
        local p = Entity.find_by_name(n)
        if p then paws[#paws + 1] = p end
    end
    if cat then x, y = Transform.get_position(cat) end
    cam_x, cam_y = x + 140, y + 60
    refresh_status()
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    if not cat then return end

    -- Driving: keys take over from the demo for a while.
    idle_input = idle_input + dt
    if Input.is_key_pressed("Up") then
        target_pace = math.min(1.0, target_pace + dt * 0.6); idle_input = 0
        set_moving(true)
    end
    if Input.is_key_pressed("Down") then
        target_pace = math.max(0.0, target_pace - dt * 0.6); idle_input = 0
    end
    if Input.is_key_just_pressed("Space") then
        set_moving(not moving); idle_input = 0
    end
    if Input.is_key_just_pressed("I") then
        ik_on = not ik_on
        for _, p in ipairs(paws) do Skeleton.set_ik_weight(p, ik_on and 1 or 0) end
        idle_input = 0
    end
    if idle_input > 10.0 then
        demo_t = demo_t + dt
        local total = 0
        for _, s in ipairs(DEMO) do total = total + s[1] end
        local t = wrap(demo_t, total)
        for _, s in ipairs(DEMO) do
            if t < s[1] then
                set_moving(s[2]); target_pace = s[3]
                break
            end
            t = t - s[1]
        end
    end

    -- Pace eases, and the blend follows it.
    local was = pace
    pace = pace + (target_pace - pace) * math.min(1.0, dt * 1.5)
    if moving and math.abs(pace - was) > 0.0005 then Animation.set_blend(cat, RUN, pace) end

    -- Speed: the blended gait's, eased so a start and a stop take time.
    local want = moving and gait_speed(pace) or 0.0
    speed = speed + (want - speed) * math.min(1.0, dt * 3.0)
    x = x + speed * dt

    -- The body rides the ground under hips and shoulders, pitched with it.
    local gh, gf = ground(x + HIND_X), ground(x + FRONT_X)
    local body_y = (gh + gf) * 0.5 - T.FLOOR
    local want_tilt = math.atan((gf - gh) / (FRONT_X - HIND_X)) * 0.85
    y = y + (body_y - y) * math.min(1.0, dt * 10.0)
    tilt = tilt + (want_tilt - tilt) * math.min(1.0, dt * 8.0)

    -- The ground repeats every period: step back one, with the camera.
    if x > T.PERIOD + 600 then
        x = x - T.PERIOD
        cam_x = cam_x - T.PERIOD
    end
    Transform.set_position(cat, x, y)
    Transform.set_rotation(cat, tilt)

    -- The camera leads a little, more at speed.
    local lead = 140 + speed * 0.25
    cam_x = cam_x + (x + lead - cam_x) * math.min(1.0, dt * 2.5)
    cam_y = cam_y + (y + 70 - cam_y) * math.min(1.0, dt * 2.0)
    if cam then Transform.set_position(cam, cam_x, cam_y) end

    -- Hills: far ones barely move, the middle ones a little more. Each is
    -- tiled 1024 wide, so wrapping by that is invisible.
    if far then Transform.set_position(far, cam_x - wrap(cam_x * 0.08, 1024), cam_y * 0.92 - 200) end
    if mid then Transform.set_position(mid, cam_x - wrap(cam_x * 0.25, 1024), cam_y * 0.8 - 310) end

    refresh_status()
end
