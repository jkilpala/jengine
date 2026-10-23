-- 114 Smoke Chamber: one `smoke` grid is all the air in the room. The
-- brazier's smoke and the pipe's steam are its sources; the pillars, the
-- floor and the weight are colliders it flows round, and the weight pushes
-- it as it swings. This script swings the weight, turns the fan (F, and
-- every 9 s), lets the pipe hiss (S, and in bursts) and puffs smoke at the
-- weight (P).

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/114-smoke-chamber.scene"
local PIVOT = { 0.000000, 410.000000 }
local LEN = 430.000000

local air, weight, chain, fan
local t = 0
local captions = true
local fan_on = false
local fan_speed, fan_angle = 0, 0
local next_fan = 5
local steam_on = false
local steam_forced = 0

local function set_fan(on)
    fan_on = on
    Smoke.set(air, "sources.3.active", on)
end

local function set_steam(on)
    if on ~= steam_on then
        steam_on = on
        Smoke.set(air, "sources.2.active", on)
    end
end

function init(entity)
    air = Entity.find_by_name("Air")
    weight = Entity.find_by_name("Weight")
    chain = Entity.find_by_name("Chain")
    fan = Entity.find_by_name("Fan")
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    -- The weight swings on its chain.
    local a = 0.78 * math.sin(t * 1.45)
    Transform.set_position(weight, PIVOT[1] + LEN * math.sin(a), PIVOT[2] - LEN * math.cos(a))
    Transform.set_rotation(weight, a)
    Transform.set_rotation(chain, a)
    -- The fan, on and off.
    next_fan = next_fan - dt
    if Input.is_key_just_pressed("F") or next_fan <= 0 then
        set_fan(not fan_on)
        next_fan = fan_on and 4 or 9
    end
    local want = fan_on and 16 or 0
    fan_speed = fan_speed + (want - fan_speed) * math.min(1, dt * 1.5)
    fan_angle = fan_angle + fan_speed * dt
    Transform.set_rotation(fan, fan_angle)
    -- The pipe hisses in bursts, or while S is held.
    if Input.is_key_just_pressed("S") then steam_forced = 2.5 end
    steam_forced = math.max(0, steam_forced - dt)
    set_steam(steam_forced > 0 or (t % 6) < 2.2)
    -- A puff of smoke at the weight.
    if Input.is_key_just_pressed("P") then
        local p = { Transform.get_position(weight) }
        Smoke.puff(air, p[1], p[2], 70, 3, 0, 0)
    end
end
