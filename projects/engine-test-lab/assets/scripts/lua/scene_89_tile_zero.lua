-- Scene 89 - a tile id of zero is empty
--
-- The renderer resolves a gid against each tileset's `first_gid`, which is
-- always 1 or more, so gid 0 names no tile and nothing is drawn. The
-- collider used to ask `Tilemap.get_tile(x, y).is_some()` instead - and `Some(0)`
-- is `is_some()`. A map whose empty cells were written as `0` rather than
-- `null` therefore **drew as open air and collided as solid rock**: an
-- invisible wall over the whole level.
--
-- It is the natural way to write one. Every tilemap format in the world
-- spells empty as 0, a generator filling an array starts with zeroes, and
-- `Tilemap.set_tile(e, layer, x, y, 0)` is what a script means by "clear it".
--
-- The two floors here are the same shape and differ only in how their air
-- is spelled. `get_tile` answers for both, and a probe dropped down each
-- column has to stop at the same height on both sides. Before the fix the
-- right-hand one stopped at the top of the screen, everywhere.
--
-- Found by somebody playing a run-and-gun platformer: "you can jump up, and
-- when you get on top of something you cannot run forward." They were
-- inside the level. BUGS #141.
--
-- [1] [2] [3] also prove the other half, BUGS #142: a script has three
-- ways to say "clear this cell" - `nil`, `0` and a negative - and until
-- 2026-09-20 they did DIFFERENT things in different runtimes. `nil` was
-- a hard error on native, thrown every frame it was called, while the
-- browsers turned the `-1` native accepted into a gid of four billion:
-- no art, so nothing drawn, and solid to the collider. Press each one in
-- each runtime; all three must punch the same hole.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/89-tile-zero.scene"
local TILE = 32
local W, H = 14, 6

local readout
local floors = {}
local t = 0
local report = ""
local cleared = ""
local show_probe = true


function init(entity)
    readout = Entity.find_by_name("Readout")
    floors = {
        { name = "null", e = Entity.find_by_name("FloorNull") },
        { name = "0", e = Entity.find_by_name("FloorZero") },
    }
    log("[scene_89] ready - both floors should answer the same")
end

-- Walk down a column and report the first solid cell, which is what a
-- falling body would land on. `get_tile` answers 0 for an empty cell (and
-- -1 off the map), never nil - this probe used to test `~= nil`, so every
-- cell looked solid, both floors "agreed" at row 0 everywhere, and the
-- scene proved nothing.
local function first_solid(floor, col)
    for row = 0, H - 1 do
        local gid = Tilemap.get_tile(floor.e, "ground", col, row)
        if gid ~= nil and gid > 0 then
            return row
        end
    end
    return nil
end

-- The probe, drawn: a bar on top of the first solid cell of every column,
-- which is where a falling body would land. The two floors' bars must sit
-- at the same heights.
local function draw_probe()
    for i, floor in ipairs(floors) do
        local r, g, b = 0.4, 1.0, 0.5
        if i == 2 then r, g, b = 1.0, 0.75, 0.3 end
        for col = 0, W - 1 do
            local row = first_solid(floor, col)
            if row ~= nil then
                local cx, cy = Tilemap.tile_to_world(floor.e, col, row)
                if cx ~= nil then
                    local top = cy + TILE * 0.5 + 3
                    Debug.draw_line(cx - TILE * 0.4, top, cx + TILE * 0.4, top, r, g, b)
                end
            end
        end
    end
end

local function compare()
    local lines = {}
    local same = true
    for col = 0, W - 1 do
        local a = first_solid(floors[1], col)
        local b = first_solid(floors[2], col)
        if a ~= b then
            same = false
            lines[#lines + 1] = string.format("column %d: null says %s, 0 says %s",
                col, tostring(a), tostring(b))
        end
    end
    if same then
        report = "BOTH FLOORS AGREE: a zero is empty, exactly as a null is."
    else
        report = "THEY DISAGREE - " .. table.concat(lines, "; ")
    end
end

-- The three spellings of "nothing here". Each punches a hole in the SAME
-- cell of both floors; the probe below then has to agree that it went.
-- A runtime where one spelling errors, or leaves a gid nothing has art
-- for, fails visibly rather than quietly.
local function clear_with(value, label)
    for _, floor in ipairs(floors) do
        Tilemap.set_tile(floor.e, "ground", 2, H - 1, value)
    end
    cleared = label
    compare()
end

function update(entity, dt)
    t = t + dt
    if Input.is_key_just_pressed("Space") then
        show_probe = not show_probe
    end
    if report == "" then
        compare()
    end
    if Input.is_key_just_pressed("1") then
        clear_with(nil, "nil")
    elseif Input.is_key_just_pressed("2") then
        clear_with(0, "0")
    elseif Input.is_key_just_pressed("3") then
        clear_with(-1, "-1")
    elseif Input.is_key_just_pressed("4") then
        -- Put it back, so the scene can be run again without a reload.
        for _, floor in ipairs(floors) do
            Tilemap.set_tile(floor.e, "ground", 2, H - 1, 1)
        end
        cleared = "restored"
        compare()
    end
    if show_probe then
        draw_probe()
    end
    if readout ~= nil then
        local extra = ""
        if cleared ~= "" then
            extra = "   (column 2 last cleared with `" .. cleared .. "`)"
        end
        Text.set(readout, report .. extra)
    end
    nav.tick(SCENE_PATH)
end
