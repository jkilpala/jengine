-- Scene 177: per-tile shapes (the tileset's `shapes`) and tilemap shadows
-- (`tilemap_occluder`). The script only moves the lamp, breaks tiles and
-- drops the balls; the slopes, the ledge, the grass and the shadows are data.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/177-tile-shapes.scene"
local START = { { -300, 150 }, { 40, 150 }, { 210, 150 }, { 300, 150 } }
local map, lamp, status
local balls = {}
local broken = {}
local edits = 0

function init(entity)
    map = Entity.find_by_name("Map")
    lamp = Entity.find_by_name("Lamp")
    status = Entity.find_by_name("Status")
    for i = 1, #START do balls[i] = Entity.find_by_name("Ball" .. i) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local mx, my = Input.get_mouse_world_position()
    if lamp and mx then Transform.set_position(lamp, mx, my) end
    if map and mx and Input.is_mouse_left_just_pressed() then
        local tx, ty = Tilemap.world_to_tile(map, mx, my)
        -- The outer wall stays, so nothing falls out of the world.
        if tx and tx > 0 and ty > 0 and tx < 23 and ty < 11 then
            local key = tx .. "," .. ty
            local here = Tilemap.get_tile(map, "rock", tx, ty)
            if here and here > 0 then
                broken[key] = here
                Tilemap.set_tile(map, "rock", tx, ty, nil)
                edits = edits + 1
            elseif broken[key] then
                Tilemap.set_tile(map, "rock", tx, ty, broken[key])
                broken[key] = nil
                edits = edits + 1
            end
        end
    end
    if Input.is_key_just_pressed("B") then
        for i, b in ipairs(balls) do
            if b then
                Transform.set_position(b, START[i][1], START[i][2])
                Physics.set_velocity(b, 0, 0)
            end
        end
    end
    if status then
        UI.set_text(status, "tiles edited: " .. edits)
    end
end
