-- Scene 58 - Batch Scripts (tier 3)
--
-- jEngine's scripting model has three tiers and this is the third:
--
--   1. Entity scripts     - ScriptComponent, init/update(entity, dt) per entity.
--   2. Tick-rate scripts  - the same, scheduled at <= N Hz.
--   3. Batch scripts      - BatchScriptComponent, ONE update(ctx, q) call for
--                           every entity carrying the same (lang, path).
--
-- Tier 3 hands the script typed-array views over mirror buffers gathered from
-- the ECS columns, so the cost is a handful of host crossings per frame
-- regardless of how many entities are in the batch. That is the whole point:
-- tier 1 is hundreds of entities, tier 3 is thousands.
--
-- The 24 drifters here share `scripts/batch/drift_batch.js`, so they are one
-- batch and one call.
--
-- PLATFORM NOTE: batch scripts are native-only. The dispatcher needs the
-- worker pool, which is threads plus a QuickJS runtime, and neither exists on
-- wasm. In the editor and on web the drifters do not move. That is the
-- expected difference, not a bug - this scene is here partly to make that
-- divergence visible rather than surprising.
--
-- This *is* the scene that would previously have shown nothing anywhere:
-- `run_batch_scripts` had no caller in any runtime, so BatchScriptComponent
-- was inert everywhere. See BUGS.md #11.

script = { tick_rate = 10 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/58-batch-scripts.scene"

local readout
local sample
local t = 0.0
local start_x = nil

function init(entity)
    readout = Entity.find_by_name("Readout")
    -- One drifter is watched so the readout can prove the batch actually ran:
    -- if its x never changes, the dispatcher is not running.
    sample = Entity.find_by_name("Drifter0")
    if sample ~= nil then
        start_x = Transform.get_position(sample)
    end
    log("[scene_58] ready - 24 entities, one batch script, one update() call per frame")
end

function update(entity, dt)
    t = t + dt
    if readout == nil then
        nav.tick(SCENE_PATH)
        return
    end

    if sample == nil or start_x == nil then
        Text.set(readout, "batch: no drifters found")
    else
        local x = Transform.get_position(sample)
        local moved = math.abs(x - start_x) > 0.5
        Text.set(readout, string.format(
            "24 entities / 1 batch call   sample x %.1f (start %.1f)   %s",
            x, start_x,
            moved and "batch script IS running (native)"
                  or "not moving - expected in editor/web, a bug natively"))
    end

    nav.tick(SCENE_PATH)
end
