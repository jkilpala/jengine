-- Scene 203: prefabs with parts. Lab203Cart is a hierarchy - a body, two
-- wheels (each an instance of Lab203Wheel) and a mast carrying a flag.
-- Space spawns one that rolls; the next frame Entity.find_child finds its
-- flag (a path, "Mast/Flag") and gives it a colour of its own.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/203-prefab-parts.scene"
local COLOURS = { { 0.95, 0.3, 0.3 }, { 0.3, 0.55, 1.0 }, { 1.0, 0.85, 0.25 }, { 0.8, 0.4, 0.95 } }
local status
local carts = {}
local fresh = nil
local spawned = 0

local function show(found)
    UI.set_text(status, "carts spawned: " .. spawned .. "   last flag found: " .. tostring(found))
end

function init(entity)
    status = Entity.find_by_name("Status")
    show("-")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    -- Parts exist once the spawn is applied: look for the flag a frame on.
    if fresh then
        local flag = Entity.find_child(fresh, "Mast/Flag")
        if flag then
            local c = COLOURS[(spawned - 1) % #COLOURS + 1]
            Sprite.set_color(flag, c[1], c[2], c[3], 1)
        end
        show(flag ~= nil)
        fresh = nil
    end
    if Input.is_key_just_pressed("Space") then
        local e = Entity.spawn_prefab("Lab203Cart")
        Transform.set_position(e, -700, -190)
        spawned = spawned + 1
        carts[#carts + 1] = { e = e, x = -700 }
        fresh = e
    end
    -- Roll each along the road; its parts ride with it. Destroyed at the
    -- far edge, its parts go with it.
    for i = #carts, 1, -1 do
        local c = carts[i]
        c.x = c.x + 140 * dt
        Transform.set_position(c.e, c.x, -190)
        if c.x > 720 then
            Entity.destroy(c.e)
            table.remove(carts, i)
        end
    end
end
