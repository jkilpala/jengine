-- Scene 152: an isometric tilemap. The diamonds, the pillars standing on
-- their cells and their sort with the walker are the engine's
-- (`"orientation": "isometric"`, a `y_sort` layer). This moves the walker
-- round the middle pillars and picks the cell under the mouse.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/152-isometric-tilemap.scene"
local N = 8
local CX, CY = 16.0, 30.0
local map, walker, cursor, picked
local t = 0

function init(entity)
    map = Entity.find_by_name("Map")
    walker = Entity.find_by_name("Walker")
    cursor = Entity.find_by_name("Cursor")
    picked = Entity.find_by_name("Picked")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if walker then
        -- An ellipse in the map's own proportions, so it circles on the ground.
        Transform.set_position(walker, CX + math.cos(t * 0.8) * 110, CY + math.sin(t * 0.8) * 55)
    end
    if map == nil then return end
    local wx, wy = Input.get_mouse_world_position()
    local c, r = Tilemap.world_to_tile(map, wx, wy)
    local inside = c >= 0 and r >= 0 and c < N and r < N
    if inside and cursor then
        local x, y = Tilemap.tile_to_world(map, c, r)
        Transform.set_position(cursor, x, y)
    end
    if picked then
        UI.set_text(picked, inside and ("cell " .. c .. ", " .. r) or "")
    end
    if inside and Input.is_mouse_left_just_pressed() then
        local v = Tilemap.get_tile(map, "pillars", c, r)
        Tilemap.set_tile(map, "pillars", c, r, v == 0 and 3 or 0)
    end
end
