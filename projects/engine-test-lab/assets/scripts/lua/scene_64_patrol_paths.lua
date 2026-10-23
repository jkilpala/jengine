-- Scene 64 - Patrol Paths
--
-- Three walkers on the same four waypoints, one per traversal mode:
--
--   Loop     - wraps from the last waypoint back to the first
--   PingPong - reverses at each end
--   Once     - stops on the last waypoint and stays there
--
-- Nothing here is scripted. The routes are declared in the scene's top-level
-- `paths` block and the entities carry a `patrol_path` component naming one;
-- `update_patrol_paths` in the engine does the walking. This script only reads
-- positions back so the readout can show what the engine did.
--
-- Until Sep 2026 all of this was inert. `Path2D`, `Path2DLibrary`,
-- `PatrolPathRef` and the walk system all existed, with seven tests covering
-- exactly these three modes -- and no runtime ever filled the library, so
-- nothing moved anywhere and there was no way to author a route at all
-- (BUGS #7). Worth remembering as a shape: a system whose resource is optional
-- and never provided compiles, runs, passes its unit tests, and is dead.
--
-- If the three walkers sit still, that has come back.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/64-patrol-paths.scene"

local MODES = { "Loop", "PingPong", "Once" }
local walkers = {}
local readout

function init(entity)
    for _, mode in ipairs(MODES) do
        walkers[mode] = Entity.find_by_name("Walker_" .. mode)
    end
    readout = Entity.find_by_name("Readout")
    log("[scene_64] ready - three walkers, three traversal modes, no script driving them")
end

function update(entity, dt)
    if readout then
        local parts = {}
        for _, mode in ipairs(MODES) do
            local e = walkers[mode]
            if e ~= nil then
                local x = Transform.get_position(e)
                parts[#parts + 1] = string.format("%s x=%.0f", mode, x or 0.0)
            end
        end
        Text.set(readout, table.concat(parts, "   "))
    end

    nav.tick(SCENE_PATH)
end
