-- Scene 51 - Tilemap Queries & Pathfinding
--
-- Scene 09 draws a tilemap. This one interrogates it.
--
--   Tilemap.world_to_tile(e, wx, wy) -> tx, ty      tile coords under a world point
--   Tilemap.tile_to_world(e, tx, ty) -> wx, wy      centre of a tile in world space
--   Tilemap.get_tile(e, layer, x, y)                the GID; 0 for an empty cell, -1 outside
--   Tilemap.get_tile_at_world(e, layer, wx, wy)     same, addressed by world point
--   Tilemap.set_tile(e, layer, x, y, gid)           write one cell
--   Tilemap.find_path(e, layer, sx, sy, gx, gy, mode)        -> array of tiles
--   Tilemap.find_path_world(e, layer, sx, sy, gx, gy, mode)  -> tile centres
--
-- WALKABILITY IS "EMPTY". A cell is open when it holds no tile - JSON `null`,
-- or a gid of 0, which names no tile (BUGS #141 made the two agree; before
-- that a 0 drew as open air and pathed as solid rock).
--
-- The maze has a solid outer wall, so no route goes round it; the walker and
-- the goal sit at two dead ends 40 steps apart, and walling any of six cells
-- forces a longer route - click one and watch it re-plan.
--
-- mode is "manhattan" (4-way) or "diagonal" (8-way, octile, with corner
-- cutting disabled so a path cannot slip through a 1-tile diagonal gap).

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/51-tilemap-pathfinding.scene"

local LAYER = "ground"
local WALL_GID = 2

local maze, walker, goal
local mode = "manhattan"
local path = nil
local walk_i = 1
local walk_t = 0.0
local STEP_TIME = 0.12
local paused = false

local function recompute()
    if maze == nil or walker == nil or goal == nil then return end
    local wx, wy = Transform.get_position(walker)
    local gx, gy = Transform.get_position(goal)
    local sx, sy = Tilemap.world_to_tile(maze, wx, wy)
    local tx, ty = Tilemap.world_to_tile(maze, gx, gy)
    if sx == nil or tx == nil then
        path = nil
        return
    end
    path = Tilemap.find_path(maze, LAYER, sx, sy, tx, ty, mode)
    walk_i = 1
    walk_t = 0.0
end

function init(entity)
    maze = Entity.find_by_name("Maze")
    walker = Entity.find_by_name("Walker")
    goal = Entity.find_by_name("Goal")
    recompute()
    log("[scene_51] ready - M switches mode, Space pauses the walker, click or C toggles a wall, R resets")
end

function update(entity, dt)
    if maze == nil then return end

    if Input.is_key_just_pressed("Space") then
        paused = not paused
    end
    if Input.is_key_just_pressed("M") then
        mode = (mode == "manhattan") and "diagonal" or "manhattan"
        recompute()
    elseif Input.is_key_just_pressed("R") then
        Scene.load(SCENE_PATH)
        return
    end

    -- Click (or C at the cursor) toggles the tile under the mouse between a
    -- wall and empty, then re-plans - the clearest way to see A* react.
    -- Through the camera, rather than assuming it sits at the origin unzoomed.
    local wx, wy = Input.get_mouse_world_position()
    local tx, ty = Tilemap.world_to_tile(maze, wx, wy)

    if (Input.is_mouse_left_just_pressed() or Input.is_key_just_pressed("C")) and tx ~= nil then
        local cur = Tilemap.get_tile(maze, LAYER, tx, ty)
        -- nil or 0 means empty/walkable; anything else is a wall.
        local empty = cur == nil or cur == 0
        Tilemap.set_tile(maze, LAYER, tx, ty, empty and WALL_GID or 0)
        recompute()
    end

    -- Walk the path one tile at a time so the result is visibly a route.
    if path ~= nil and #path > 0 then
        if not paused then walk_t = walk_t + dt end
        if walk_t >= STEP_TIME then
            walk_t = 0.0
            walk_i = walk_i + 1
            if walk_i > #path then walk_i = 1 end
            local step = path[walk_i]
            if type(step) == "table" and step[1] ~= nil then
                local cx, cy = Tilemap.tile_to_world(maze, step[1], step[2])
                if cx ~= nil and walker ~= nil then Transform.set_position(walker, cx, cy) end
            end
        end
        -- Draw the route with debug lines (scene 52 covers these in their own
        -- right; here they are doing the job they exist for).
        for i = 1, #path - 1 do
            local a, b = path[i], path[i + 1]
            if type(a) == "table" and type(b) == "table" then
                local ax, ay = Tilemap.tile_to_world(maze, a[1], a[2])
                local bx, by = Tilemap.tile_to_world(maze, b[1], b[2])
                if ax ~= nil and bx ~= nil then
                    Debug.draw_line(ax, ay, bx, by, 0.3, 1.0, 0.6)
                end
            end
        end
    end

    local pl = Entity.find_by_name("PathLabel")
    if pl then
        Text.set(pl, string.format("mode: %s   path: %s%s",
            mode, path and (#path .. " tiles") or "none (blocked)", paused and "   (paused)" or ""))
    end
    local tl = Entity.find_by_name("TileLabel")
    if tl then
        if tx ~= nil then
            local gid = Tilemap.get_tile(maze, LAYER, tx, ty)
            Text.set(tl, string.format("cursor tile (%d, %d) = %s",
                math.floor(tx + 0.5), math.floor(ty + 0.5),
                (gid == nil or gid == 0) and "empty (walkable)" or ("gid " .. math.floor(gid + 0.5))))
        else
            Text.set(tl, "cursor is outside the tilemap")
        end
    end
    local r = Entity.find_by_name("Readout")
    if r then
        Text.set(r, "click a tile to toggle a wall (the path re-plans)   M mode   Space pause   R reset")
    end

    nav.tick(SCENE_PATH)
end
