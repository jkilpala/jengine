-- 99 Night Watch: nobody in this courtyard is scripted. Each guard and the
-- thief is a behaviour tree (`behavior_tree` in the scene) walking a
-- nav_agent round the walls. This script keeps score, empties and refills
-- the chests, sends the nearest guard to look where one was robbed (its
-- blackboard's `noise`), sends a caught thief home, flickers the torches
-- and writes each actor's running leaf above its head.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/99-night-watch.scene"
local GUARDS, CHESTS, TORCHES = 3, 4, 5
local HOME_X, HOME_Y = 0.000000, -290.000000

local e = {}
local guards = {}
local chests = {}
local gold, caught = 0, 0
local captions = true
local t = 0

local function status()
    UI.set_text(e.Status, string.format("gold stolen %d   caught %d", math.floor(gold + 0.5), math.floor(caught + 0.5)))
end

function init(entity)
    for _, n in ipairs({ "Thief", "ThiefLabel", "Status" }) do e[n] = Entity.find_by_name(n) end
    for i = 0, GUARDS - 1 do
        guards[i] = { e = Entity.find_by_name("Guard" .. i), label = Entity.find_by_name("GuardLabel" .. i) }
    end
    for i = 0, CHESTS - 1 do
        chests[i] = { shut = Entity.find_by_name("Chest" .. i), open = Entity.find_by_name("ChestOpen" .. i), refill = 0 }
    end
    for i = 0, TORCHES - 1 do e["Torch" .. i] = Entity.find_by_name("Torch" .. i) end
    Event.on("thief_caught", entity)
    Event.on("thief_steals", entity)
    status()
end

local function dist(a, bx, by)
    local x, y = Transform.get_position(a)
    return math.sqrt((x - bx) ^ 2 + (y - by) ^ 2)
end

function on_thief_steals(entity, data)
    local tx, ty = Transform.get_position(e.Thief)
    local best, bd = nil, 1e9
    for i = 0, CHESTS - 1 do
        local d = dist(chests[i].shut, tx, ty)
        if d < bd then best, bd = i, d end
    end
    local c = best and chests[best]
    if c and c.refill <= 0 and bd < 80 then
        gold = gold + 1
        c.refill = 9
        Sprite.set_color(c.open, 1, 1, 1, 1)
        Sprite.set_color(c.shut, 1, 1, 1, 0)
        -- Whoever is nearest hears it.
        local cx, cy = Transform.get_position(c.shut)
        local near, nd = nil, 1e9
        for i = 0, GUARDS - 1 do
            local d = dist(guards[i].e, cx, cy)
            if d < nd then near, nd = guards[i].e, d end
        end
        if near then Entity.set_custom(near, "Blackboard", "noise", { cx, cy }) end
        status()
    end
end

function on_thief_caught(entity, data)
    caught = caught + 1
    Transform.set_position(e.Thief, HOME_X, HOME_Y)
    BehaviorTree.set(e.Thief, "restart", true)
    status()
end

local function label(text_e, actor, dy)
    local x, y = Transform.get_position(actor)
    Transform.set_position(text_e, x, y + dy)
    Text.set(text_e, tostring(Entity.get_custom(actor, "Blackboard", "_running") or ""))
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    for i = 0, GUARDS - 1 do label(guards[i].label, guards[i].e, 44) end
    label(e.ThiefLabel, e.Thief, 38)
    for i = 0, CHESTS - 1 do
        local c = chests[i]
        if c.refill > 0 then
            c.refill = c.refill - dt
            if c.refill <= 0 then
                Sprite.set_color(c.open, 1, 1, 1, 0)
                Sprite.set_color(c.shut, 1, 1, 1, 1)
            end
        end
    end
    for i = 0, TORCHES - 1 do
        local f = 1.25 + 0.12 * math.sin(t * 9.0 + i * 2.1) + 0.06 * math.sin(t * 23.0 + i)
        Lighting.set_intensity(e["Torch" .. i], f)
    end
end
