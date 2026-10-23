-- Scene 09 - Tilemaps
-- Demonstrates: Tilemap component with two layers + tileset, plus the
-- script-side tilemap API (`set_tile` / `get_tile`) used to drive a
-- cursor-based paint tool and an interactive layer toggle.
--
-- Controls:
--   WASD / Arrow keys  : move 1-tile cursor across the 12x8 grid
--   1 / 2 / 3 / 4      : select brush (grass / water / sand / stone)
--   SPACE              : paint brush onto ground layer at cursor
--   X                  : clear ground tile at cursor (set_tile nil) - rare:
--                        empty ground reveals the page-clear background
--   R                  : restore the authored ground layer
--   V                  : toggle decoration layer (snapshot + clear, restore)

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/09-tilemap.scene"

local TILE = 48            -- world units per tile (matches scene tile_size)
local MAP_X = -288.0       -- tilemap entity x (top-left of tile (0,0))
local MAP_Y = 192.0        -- tilemap entity y
local COLS = 12
local ROWS = 8

-- Brush palette: { tile_gid, r, g, b, label } - mirrors the tileset.
local BRUSHES = {
    [1] = { 1, 0.298, 0.612, 0.282, "grass" },
    [2] = { 2, 0.204, 0.424, 0.769, "water" },
    [3] = { 3, 0.871, 0.761, 0.502, "sand"  },
    [4] = { 4, 0.549, 0.549, 0.565, "stone" },
}

-- Authored ground pattern (matches the JSON in 09-tilemap.scene). Kept
-- here so R can restore it without reloading the scene.
local GROUND_PATTERN = {
    {1,1,1,1,1,1,1,1,1,1,1,1},
    {1,1,1,1,1,1,1,1,1,1,1,1},
    {1,1,1,1,3,3,3,3,1,1,1,1},
    {1,1,1,3,2,2,2,3,1,1,1,1},
    {1,1,1,3,2,2,2,3,1,1,1,1},
    {1,1,1,1,3,3,3,3,1,1,1,1},
    {1,1,1,1,1,1,1,1,1,1,1,1},
    {1,1,1,1,1,1,1,1,1,1,1,1},
}

-- Authored decoration cells (1-based tx, ty, gid).
local DECORATION = {
    {2, 2, 4}, {10, 2, 4}, {12, 4, 4}, {2, 5, 4},
    {11, 6, 4}, {4, 7, 4}, {8, 7, 4},
}

local cx, cy = 0, 0          -- cursor tile coords (0..COLS-1, 0..ROWS-1)
local brush = 1              -- current brush index (1..4)
local decoration_on = true   -- toggle state for V

local map_entity = nil
local cursor_entity = nil
local indicator_entity = nil

-- Edge-press tracking. is_action_just_pressed isn't always wired for raw
-- keys, so we track previous state ourselves.
local prev = {}
local function edge(key)
    local now = Input.is_key_pressed(key)
    local was = prev[key] or false
    prev[key] = now
    return now and not was
end

local function clamp(v, lo, hi)
    if v < lo then return lo end
    if v > hi then return hi end
    return v
end

-- Centre of tile (tx, ty) in world space, given the map entity position.
local function tile_center(tx, ty)
    return MAP_X + tx * TILE + TILE * 0.5,
           MAP_Y - ty * TILE - TILE * 0.5
end

local function update_cursor_visual()
    if cursor_entity then
        local wx, wy = tile_center(cx, cy)
        Transform.set_position(cursor_entity, wx, wy)
    end
end

local function set_brush(i)
    brush = i
    local b = BRUSHES[brush]
    if indicator_entity and b then
        Sprite.set_color(indicator_entity, b[2], b[3], b[4], 1.0)
    end
    log("[scene_09] brush -> " .. b[5])
end

local function restore_ground()
    if not map_entity then return end
    for ty = 0, ROWS - 1 do
        for tx = 0, COLS - 1 do
            Tilemap.set_tile(map_entity, "ground", tx, ty, GROUND_PATTERN[ty + 1][tx + 1])
        end
    end
    log("[scene_09] ground restored")
end

local function set_decoration(enabled)
    if not map_entity then return end
    if enabled then
        for _, cell in ipairs(DECORATION) do
            Tilemap.set_tile(map_entity, "decoration", cell[1] - 1, cell[2] - 1, cell[3])
        end
        log("[scene_09] decoration ON")
    else
        for ty = 0, ROWS - 1 do
            for tx = 0, COLS - 1 do
                Tilemap.set_tile(map_entity, "decoration", tx, ty, nil)
            end
        end
        log("[scene_09] decoration OFF")
    end
end

function init(entity)
    map_entity = Entity.find_by_name("Tilemap")
    cursor_entity = Entity.find_by_name("Cursor")
    indicator_entity = Entity.find_by_name("BrushIndicator")
    if not map_entity then
        log("[scene_09] ERROR: Tilemap entity not found")
        return
    end
    set_brush(1)
    update_cursor_visual()
    log("[scene_09] ready - WASD move, 1/2/3/4 brush, SPACE paint, X clear, R reset, V toggle decoration")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local moved = false
    if edge("A") or edge("Left")  then cx = clamp(cx - 1, 0, COLS - 1); moved = true end
    if edge("D") or edge("Right") then cx = clamp(cx + 1, 0, COLS - 1); moved = true end
    if edge("W") or edge("Up")    then cy = clamp(cy - 1, 0, ROWS - 1); moved = true end
    if edge("S") or edge("Down")  then cy = clamp(cy + 1, 0, ROWS - 1); moved = true end
    if moved then update_cursor_visual() end

    if edge("1") then set_brush(1) end
    if edge("2") then set_brush(2) end
    if edge("3") then set_brush(3) end
    if edge("4") then set_brush(4) end

    if edge("Space") and map_entity then
        local b = BRUSHES[brush]
        Tilemap.set_tile(map_entity, "ground", cx, cy, b[1])
    end
    if edge("X") and map_entity then
        Tilemap.set_tile(map_entity, "ground", cx, cy, nil)
    end
    if edge("R") then
        restore_ground()
    end
    if edge("V") then
        decoration_on = not decoration_on
        set_decoration(decoration_on)
    end
end
