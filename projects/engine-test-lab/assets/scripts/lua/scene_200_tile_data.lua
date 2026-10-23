-- Scene 200: tile data. The tileset gives its tiles fields (data), draws
-- grass as one of four (random) and turns crate tiles into the Lab200Crate
-- prefab when the scene loads (spawns). Pointing reads the cell's data.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/200-tile-data.scene"
local map, cursor, status

local function describe(d)
    if d == nil then return "(no data)" end
    local parts = {}
    for _, k in ipairs({ "kind", "walk", "burns", "damage" }) do
        if d[k] ~= nil then parts[#parts + 1] = k .. "=" .. tostring(d[k]) end
    end
    return table.concat(parts, "  ")
end

function init(entity)
    map = Entity.find_by_name("Map")
    cursor = Entity.find_by_name("Cursor")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local mx, my = Input.get_mouse_world_position()
    local cx, cy = Tilemap.world_to_tile(map, mx, my)
    local wx, wy = Tilemap.tile_to_world(map, cx, cy)
    Transform.set_position(cursor, wx, wy)
    local gid = Tilemap.get_tile(map, "ground", cx, cy)
    local d = Tilemap.get_tile_data(map, "ground", cx, cy)
    local burns = Tilemap.get_tile_data(map, "ground", cx, cy, "burns")
    if status then
        UI.set_text(status, "cell " .. cx .. ", " .. cy .. "   tile " .. gid .. "   " .. describe(d)
            .. (burns and "   (it burns)" or ""))
    end
end
