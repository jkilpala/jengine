-- 118 The Endless Dungeon: the map is decided by the `wfc` generator on
-- this entity, eight columns at a time (`Wfc.generate`), each stretch
-- fitting the end of the last. This script walks the explorer through what
-- has been decided - a search over the tiles' open sides, which are the
-- bits of (gid - 1): 1 up, 2 right, 4 down, 8 left - asks for the next
-- stretch as she nears the end of the map so far, breaks through a wall
-- where the dungeon has cut her off, and lights the dead ends.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/118-the-endless-dungeon.scene"
local TS, COLS, ROWS = 48, 400, 13
local X0, Y0 = -144.0, 312.0
local CHUNK = 8
local BIT = { 1, 2, 4, 8 }
local STEP = { { 0, -1 }, { 1, 0 }, { 0, 1 }, { -1, 0 } }

local map, explorer, torch, dust, cam, status
local captions = true
local hurry = 1
local generated, asked = 0, 0
local cell = { 1, 6 }
local path = {}
local overrides = {}   -- tiles this script set and the snapshot has not caught up with
local lit = {}
local t = 0

local function centre(c, r)
    return X0 + (c + 0.5) * TS, Y0 - (r + 0.5) * TS
end

local function tile(c, r)
    local k = c .. "," .. r
    if overrides[k] then return overrides[k] end
    local g = Tilemap.get_tile(map, "ground", c, r)
    if g == nil or g <= 0 then return 0 end
    return g
end

local function open(g, d)
    return g > 0 and math.floor((g - 1) / BIT[d]) % 2 == 1
end

local function ask()
    if asked * CHUNK < COLS then
        Wfc.generate(map, asked * CHUNK, 0, CHUNK, ROWS)
        asked = asked + 1
    end
end

-- Every cell reachable from `from`, with the step that reached it.
local function search(from)
    local seen = { [from[1] .. "," .. from[2]] = { from[1], from[2], nil } }
    local queue = { from }
    local head = 1
    local best = from
    while head <= #queue do
        local c = queue[head]
        head = head + 1
        local g = tile(c[1], c[2])
        if c[1] > best[1] then best = c end
        for d = 1, 4 do
            local n = { c[1] + STEP[d][1], c[2] + STEP[d][2] }
            local k = n[1] .. "," .. n[2]
            if not seen[k] and n[2] >= 0 and n[2] < ROWS and n[1] >= 0 and open(g, d) then
                local d2 = (d + 1) % 4 + 1
                if open(tile(n[1], n[2]), d2) then
                    seen[k] = { n[1], n[2], c }
                    queue[#queue + 1] = n
                end
            end
        end
    end
    return seen, best
end

local function route_to(seen, goal)
    local out = {}
    local k = goal[1] .. "," .. goal[2]
    while seen[k] and seen[k][3] do
        table.insert(out, 1, { seen[k][1], seen[k][2] })
        local p = seen[k][3]
        k = p[1] .. "," .. p[2]
    end
    return out
end

-- Open the wall between a cell and the one to its right, on both tiles.
local function break_through(c)
    local a, b = tile(c[1], c[2]), tile(c[1] + 1, c[2])
    if a == 0 or b == 0 then return false end
    if not open(a, 2) then a = a + 2 end
    if not open(b, 4) then b = b + 8 end
    Tilemap.set_tile(map, "ground", c[1], c[2], a)
    Tilemap.set_tile(map, "ground", c[1] + 1, c[2], b)
    overrides[c[1] .. "," .. c[2]] = a
    overrides[(c[1] + 1) .. "," .. c[2]] = b
    local x, y = centre(c[1] + 0.5, c[2])
    Transform.set_position(dust, x, y)
    Particles.emit_burst(dust, 60)
    return true
end

local function plan()
    local seen, best = search(cell)
    if best[1] <= cell[1] and generated > cell[1] + 2 then
        -- Cut off: through the wall from the farthest cell she can reach.
        if break_through(best) then
            seen, best = search(cell)
        end
    end
    path = route_to(seen, best)
end

function init(entity)
    map = entity
    explorer = Entity.find_by_name("Explorer")
    torch = Entity.find_by_name("Torch")
    dust = Entity.find_by_name("Dust")
    cam = Entity.find_by_name("Camera")
    status = Entity.find_by_name("Status")
    for _ = 1, 4 do ask() end
end

function on_wfc_done(entity)
    generated = generated + CHUNK
    -- Crystals in the stretch's dead ends.
    local from = generated - CHUNK
    for c = from, generated - 1 do
        for r = 0, ROWS - 1 do
            local g = tile(c, r)
            local bits = g - 1
            local n = 0
            for d = 1, 4 do if open(g, d) then n = n + 1 end end
            if n == 1 and (c * 7 + r * 13) % 3 == 0 and not lit[c .. "," .. r] then
                lit[c .. "," .. r] = true
                local e = Entity.spawn_prefab("EdCrystal")
                local x, y = centre(c, r)
                Transform.set_position(e, x, y)
            end
        end
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("Space") then hurry = hurry == 1 and 3 or 1 end
    -- Keep two stretches ahead of her.
    if generated > 0 and asked * CHUNK - cell[1] < 3 * CHUNK and asked * CHUNK <= generated + CHUNK then ask() end
    if generated < CHUNK then return end
    if #path == 0 then plan() end
    local x, y = Transform.get_position(explorer)
    if #path > 0 then
        local tx, ty = centre(path[1][1], path[1][2])
        local dx, dy = tx - x, ty - y
        local d = math.sqrt(dx * dx + dy * dy)
        local step = 80 * hurry * dt
        if d <= step then
            x, y = tx, ty
            cell = { path[1][1], path[1][2] }
            table.remove(path, 1)
        else
            x, y = x + dx / d * step, y + dy / d * step
            if math.abs(dx) > 1 then Transform.set_scale(explorer, dx > 0 and 1 or -1, 1) end
        end
        Transform.set_position(explorer, x, y)
    end
    Transform.set_position(torch, x, y)
    Lighting.set_intensity(torch, 1.7 + 0.12 * math.sin(t * 15) + 0.08 * math.sin(t * 23))
    -- The camera follows, a little ahead.
    local cx = Transform.get_position(cam)
    Transform.set_position(cam, cx + (x + 180 - cx) * math.min(1, dt * 2), 0)
    if cell[1] > COLS - 6 then Scene.load(SCENE) end
    UI.set_text(status, string.format("column %d of %d decided", generated, COLS))
end
