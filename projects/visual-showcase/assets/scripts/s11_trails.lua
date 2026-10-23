-- 11 Trails: spawns meteors and the odd fireball (their trails, lights and
-- lifetimes are all in the prefabs), leads the wisp with the mouse, and
-- swings the blade: a pivot turned through a slash while the tip's trail
-- emits, and stopped after.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/11-trails.scene"

-- tools/gen_scenes.py: TRAILS_METEORS, trails_prefabs().
local METEORS = { "TrMeteor1", "TrMeteor2", "TrMeteor3" }
-- The tip's trail lives exactly as long as a slash (tools/gen_scenes.py,
-- BladeTip), so the slash tapers to a point where it began.
local SWING_TIME = 0.16         -- seconds a slash takes
local SWING_ANGLE = 3.8         -- radians it sweeps
local SLASH_LIGHT = 1.8

local captions = true
local post_on = nav.post_all_on(SCENE)
local meteors_on = true
local status, wisp, pivot, tip
local wx, wy = 0.0, 60.0
local next_meteor, next_fireball = 0.2, 1.5
local swing_t = nil
local swing_from, swing_dir = 0.0, 1.0
local base_angle = 2.2

local function refresh_status()
    if status then
        Text.set(status, "meteors " .. (meteors_on and "on" or "off") .. "   " .. nav.post_status(post_on))
    end
end

local function spawn_meteor(name)
    local e = Entity.spawn_prefab(name)
    -- Along the top edge and down the right side, so every streak crosses
    -- the view on its way down to the left.
    if math.random() < 0.7 then
        Transform.set_position(e, -300 + math.random() * 1300, 400)
    else
        Transform.set_position(e, 700, 50 + math.random() * 350)
    end
end

local function slash()
    swing_t = 0.0
    swing_dir = -swing_dir
    swing_from = base_angle
    base_angle = base_angle + 1.3
    Transform.set_rotation(pivot, swing_from)
    -- The tip jumped to the new start: forget the last slash's points, or
    -- the first segment would run from where it ended.
    Trail.clear(tip)
    Trail.set_emitting(tip, true)
    Lighting.set_intensity(tip, SLASH_LIGHT)
end

function init(entity)
    status = Entity.find_by_name("Status")
    wisp = Entity.find_by_name("Wisp")
    pivot = Entity.find_by_name("BladePivot")
    tip = Entity.find_by_name("BladeTip")
    refresh_status()
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    if Input.is_key_just_pressed("M") then
        meteors_on = not meteors_on
        refresh_status()
    end

    if meteors_on then
        next_meteor = next_meteor - dt
        if next_meteor <= 0 then
            next_meteor = 0.12 + math.random() * 0.35
            spawn_meteor(METEORS[math.random(1, #METEORS)])
        end
        next_fireball = next_fireball - dt
        if next_fireball <= 0 then
            next_fireball = 2.5 + math.random() * 2.5
            spawn_meteor("TrFireball")
        end
    end

    -- The wisp eases after the mouse, so its ribbon curves.
    local mx, my = Input.get_mouse_world_position()
    local k = 1.0 - math.exp(-dt * 14.0)
    wx = wx + (mx - wx) * k
    wy = wy + (my - wy) * k
    if wisp then Transform.set_position(wisp, wx, wy) end
    if pivot then Transform.set_position(pivot, wx, wy) end

    if pivot and tip then
        if swing_t == nil and (Input.is_mouse_left_just_pressed() or Input.is_key_just_pressed("Space")) then
            slash()
        end
        if swing_t ~= nil then
            swing_t = swing_t + dt
            local s = math.min(swing_t / SWING_TIME, 1.0)
            -- Fast out of the wind-up, slowing into the follow-through.
            local e = 1.0 - (1.0 - s) * (1.0 - s) * (1.0 - s)
            Transform.set_rotation(pivot, swing_from + swing_dir * SWING_ANGLE * e)
            Lighting.set_intensity(tip, SLASH_LIGHT * (1.0 - s * s))
            if s >= 1.0 then
                Trail.set_emitting(tip, false)
                Lighting.set_intensity(tip, 0.0)
                swing_t = nil
            end
        end
    end
end
