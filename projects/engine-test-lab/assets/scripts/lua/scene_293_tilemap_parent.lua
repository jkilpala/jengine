-- Scene 293: a tilemap carried by a parent (the raft sways; its map, trees
-- and flag must sway with it, never sitting in the red frame), and a mesh
-- tinted by Sprite.set_color and re-ordered by Sprite.set_z_order.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/293-tilemap-parent.scene"

local raft, mesh
local t = 0.0
local front = false
local next_swap = 1.0

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if raft == nil then
        raft = Entity.find_by_name("Raft")
        mesh = Entity.find_by_name("Tinted")
    end
    t = t + dt
    if raft ~= nil then
        Transform.set_position(raft, -300 + 80 * math.sin(t * 0.9), 10)
    end
    if mesh ~= nil then
        local r = 0.6 + 0.4 * math.sin(t * 1.3)
        local g = 0.6 + 0.4 * math.sin(t * 1.3 + 2.1)
        local b = 0.6 + 0.4 * math.sin(t * 1.3 + 4.2)
        Sprite.set_color(mesh, r, g, b, 1)
        if t >= next_swap then
            next_swap = next_swap + 1.0
            front = not front
            Sprite.set_z_order(mesh, front and 7 or 3)
        end
    end
end
