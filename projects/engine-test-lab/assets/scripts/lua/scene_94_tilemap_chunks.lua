-- Scene 94 - a tilemap costs what you changed, not what it is
--
-- `PhysicsWorld::sync_tilemap_colliders` decides every frame whether a
-- colliding tilemap's body is still correct. It used to decide that by
-- **hashing every tile of every colliding layer**, and then, on any
-- difference at all, removing the body and rebuilding every collider on the
-- map. For a world of a million tiles that is a per-frame walk of a million
-- cells to discover that nothing moved, and one pick swing rebuilding the
-- whole level - so a game that digs, farms or builds was not possible on the
-- engine whatever else it had.
--
-- Now each `TilemapLayer` carries a revision counter and one counter per
-- 32x32 chunk, and the colliders are grouped per chunk. An idle frame reads
-- one integer per layer; an edit rebuilds the chunks it touched. Measured on
-- a million tiles: 966us -> 884ns idle, and a one-tile edit 11.6ms -> 1.1us.
--
-- The numbers are in `crates/jengine-core/tests/tilemap_colliders_rebuild_
-- per_chunk.rs`. This scene is for the two things a test cannot show:
--
--  1. **The frame time does not move while you dig.** 14,400 tiles is small
--     enough that the old code survived it and big enough that it was
--     working for its living. Hold [LMB] and watch the readout.
--  2. **A crate resting on rock that is dug out falls.** That is the half a
--     rebuild can get *wrong* rather than slow: rapier does not wake a
--     sleeping body when a collider it was resting on is removed unless the
--     removal asks it to, so without one boolean a digging game leaves its
--     furniture hanging in the air.
--
-- Run it in all three runtimes. The map, the digging and the falling should
-- be indistinguishable between them - `set_tile` funnels through
-- `tilemap::script_tile_id` in all four hosts (BUGS #142) and the collider
-- sync is core code every runtime calls.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/94-tilemap-chunks.scene"

local TILE = 16
local W, H = 160, 90
-- Must match the scene's transform: the map's top-left corner in world units.
local OX, OY = -W * TILE / 2, H * TILE / 2
-- The brush is a disc rather than a single cell, because one tile a frame
-- never straddles a chunk border and straddling is the interesting case.
local BRUSH = 3

local rock, readout
-- A rolling average, because a single frame's time is noise. `get_frame_time`
-- is the same number the F3 overlay shows.
local avg_ms = 0
local worst_ms = 0
local changed = 0
local digs = 0

function init(entity)
    rock = Entity.find_by_name("Rock")
    readout = Entity.find_by_name("Readout")
    if rock == nil then
        log("[scene_94] no Rock entity - the scene is not the one this script expects")
    end
    log("[scene_94] ready - " .. tostring(W * H) .. " tiles, hold LMB to dig")
end

-- World position -> tile column/row. Rows run downwards from the origin,
-- which is how `sync_tilemap_colliders` places its boxes.
local function tile_at(wx, wy)
    return math.floor((wx - OX) / TILE), math.floor((OY - wy) / TILE)
end

-- Paint a disc of `value` and return how many cells actually changed.
-- `set_tile` on a cell that already holds that value is deliberately not a
-- change and costs no rebuild, so this counts the real work.
local function paint(cx, cy, value)
    local n = 0
    for dy = -BRUSH, BRUSH do
        for dx = -BRUSH, BRUSH do
            if dx * dx + dy * dy <= BRUSH * BRUSH then
                local x, y = cx + dx, cy + dy
                if x >= 0 and x < W and y >= 0 and y < H then
                    -- get_tile answers 0 for an empty cell, never nil; this
                    -- compared against nil, so a fill never saw a hole to fill.
                    local was_solid = Tilemap.get_tile(rock, "rock", x, y) > 0
                    local want = value
                    if was_solid ~= (want ~= nil) then
                        Tilemap.set_tile(rock, "rock", x, y, want)
                        n = n + 1
                    end
                end
            end
        end
    end
    return n
end

-- [R]: put the whole map back. Deliberately the expensive case - every chunk
-- at once, which is what a level load costs and what the old code charged for
-- a single tile. Note that most of this frame's cost is the *bridge*: a
-- `set_tile` per cell is one deferred command per cell, which is the
-- whole-world script mirror in `docs/architecture-debt.md` and not something
-- the chunking touches. Judge the digging, not this.
local function refill()
    local n = 0
    for y = 0, H - 1 do
        for x = 0, W - 1 do
            if Tilemap.get_tile(rock, "rock", x, y) == 0 then
                Tilemap.set_tile(rock, "rock", x, y, 1)
                n = n + 1
            end
        end
    end
    changed = changed + n
    log("[scene_94] refilled " .. tostring(n) .. " tiles in one frame")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)

    local ms = Debug.get_frame_time() * 1000.0
    -- ~1s of smoothing at 60Hz, so holding the button shows a trend rather
    -- than a flicker.
    avg_ms = avg_ms + (ms - avg_ms) * 0.06
    if ms > worst_ms then
        worst_ms = ms
    end

    if rock ~= nil then
        local dig = Input.is_mouse_left_pressed()
        local fill = Input.is_mouse_right_pressed()
        if dig or fill then
            local mx, my = Input.get_mouse_world_position()
            local cx, cy = tile_at(mx, my)
            -- Not `dig and nil or 1`: in Lua that is `1` either way, because
            -- `true and nil` is nil and `nil or 1` is 1. It is the one idiom
            -- that cannot carry a nil, which is exactly the value wanted here.
            local want = 1
            if dig then
                want = nil
            end
            local n = paint(cx, cy, want)
            if n > 0 then
                changed = changed + n
                digs = digs + 1
            end
        end
        if Input.is_key_just_pressed("R") then
            refill()
            worst_ms = 0
        end
    end

    if readout ~= nil then
        Text.set(readout, string.format(
            "frame %.2f ms (avg)   worst %.2f ms   %d tiles changed over %d edits",
            avg_ms, worst_ms, changed, digs))
    end
end
