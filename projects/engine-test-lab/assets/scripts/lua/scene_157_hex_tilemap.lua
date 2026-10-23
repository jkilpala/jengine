-- Scene 157: the hexagons, picking, neighbours, distance, the path and the
-- colliders are the engine's (`"orientation": "hexagonal"`); this only marks
-- what it answers. R reloads.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/157-hex-tilemap.scene"
local COLS, ROWS = 9, 7
local NEIGHBOUR, PATH, CURSOR = 3, 4, 5
local maps = {}
local readout
local last = {}

function init(entity)
    maps = { Entity.find_by_name("Pointy"), Entity.find_by_name("Flat") }
    readout = Entity.find_by_name("Readout")
end

local function mark(map, c, r)
    for y = 0, ROWS - 1 do
        for x = 0, COLS - 1 do Tilemap.set_tile(map, "marks", x, y, 0) end
    end
    if c == nil then return "" end
    local path = Tilemap.find_path(map, "walls", 0, 0, c, r)
    if path then
        for _, p in ipairs(path) do Tilemap.set_tile(map, "marks", p[1], p[2], PATH) end
    end
    for _, n in ipairs(Tilemap.neighbors(map, c, r)) do
        Tilemap.set_tile(map, "marks", n[1], n[2], NEIGHBOUR)
    end
    Tilemap.set_tile(map, "marks", c, r, CURSOR)
    local steps = path and (#path - 1) or -1
    return "hex " .. c .. "," .. r .. "   " .. #Tilemap.neighbors(map, c, r) .. " neighbours   "
        .. Tilemap.distance(map, 0, 0, c, r) .. " steps from the corner   path " .. steps
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("R") then Scene.load(SCENE_PATH) return end
    local wx, wy = Input.get_mouse_world_position()
    for i, map in ipairs(maps) do
        if map then
            local c, r = Tilemap.world_to_tile(map, wx, wy)
            if c < 0 or r < 0 or c >= COLS or r >= ROWS then c, r = nil, nil end
            local k = tostring(c) .. "," .. tostring(r)
            if last[i] ~= k then
                last[i] = k
                local text = mark(map, c, r)
                if text ~= "" and readout then UI.set_text(readout, (i == 1 and "pointy: " or "flat: ") .. text) end
            end
        end
    end
end
