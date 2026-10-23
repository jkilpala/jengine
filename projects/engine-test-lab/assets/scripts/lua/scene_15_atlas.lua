-- Scene 15 - Runtime sprite atlas regions.
--
-- The scene file declares `atlas_refs: ["sprites/_atlases/demo.atlas"]`
-- at its top level. WgpuSceneRunner pre-loads the .atlas + its sibling
-- demo.png and registers the AtlasData under the PNG path before
-- ECS load, so by the time this script first ticks the engine knows
-- about all 8 regions (tile_0..tile_7).
--
-- The top row of 8 sprites proves the scene-author path:
-- SpriteComponent.texture_id all point at the shared atlas PNG, each
-- with its own hand-computed uv_rect carving out one tile.
--
-- The bottom row of 4 sprites proves the runtime swap path: their
-- regions are cycled at runtime via `Sprite.set_region(entity,
-- atlas_id, region_name)` -- the engine looks up the region's UV
-- rect from the loaded AtlasData and writes both `texture_id` and
-- `uv_rect` onto the SpriteComponent.
--
-- Controls:
--   SPACE        pause/resume auto-cycle (default ON)
--   1/2/3/4      bump cycler 1/2/3/4 forward by one region (works
--                whether auto-cycle is on or off)
--   H / Esc      hub
--   [ / ]        prev / next scene

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/15-atlas.scene"
local ATLAS_ID   = "sprites/_atlases/demo.png"
local REGIONS    = { "tile_0", "tile_1", "tile_2", "tile_3",
                     "tile_4", "tile_5", "tile_6", "tile_7" }

local cyclers     = { 0, 0, 0, 0 }     -- entity ids (filled on first tick)
local indices     = { 1, 2, 3, 4 }     -- 1-based region index per cycler
local status_id, status2_id
local auto_cycle  = true
local accum       = 0.0
local STEP_PERIOD = 0.6

local prev = {}
local function edge(key)
    local now = Input.is_key_pressed(key)
    local pressed = now and not prev[key]
    prev[key] = now
    return pressed
end

local function apply(slot)
    local id = cyclers[slot]
    if id and id ~= 0 then
        Sprite.set_region(id, ATLAS_ID, REGIONS[indices[slot]])
    end
end

local function refresh_status()
    if status_id and status_id ~= 0 then
        Text.set(status_id, string.format(
            "regions: %d %d %d %d   atlas: %s   auto-cycle: %s",
            indices[1] - 1, indices[2] - 1, indices[3] - 1, indices[4] - 1,
            ATLAS_ID,
            auto_cycle and "ON" or "OFF"))
    end
end

local function bump(slot)
    indices[slot] = (indices[slot] % #REGIONS) + 1
    apply(slot)
end

local ready_logged = false

function update(entity, dt)
    if status_id == nil then
        for i = 1, 4 do
            cyclers[i] = Entity.find_by_name("Cycler" .. (i - 1))
        end
        status_id  = Entity.find_by_name("Status")
        status2_id = Entity.find_by_name("Status2")
        for i = 1, 4 do apply(i) end
        refresh_status()
    end

    if not ready_logged then
        log("[scene_15] ready - SPACE pause auto-cycle, 1/2/3/4 bump one slot")
        ready_logged = true
    end

    local dirty = false

    if edge("Space") then
        auto_cycle = not auto_cycle
        dirty = true
    end
    if edge("1") then bump(1); dirty = true end
    if edge("2") then bump(2); dirty = true end
    if edge("3") then bump(3); dirty = true end
    if edge("4") then bump(4); dirty = true end

    if auto_cycle then
        accum = accum + dt
        while accum >= STEP_PERIOD do
            accum = accum - STEP_PERIOD
            for i = 1, 4 do bump(i) end
            dirty = true
        end
    else
        accum = 0.0
    end

    if dirty then refresh_status() end

    nav.tick(SCENE_PATH)
end
