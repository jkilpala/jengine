-- Scene 61 - Swarm AI (perf)
--
-- `jengine-swarm` is an engine package (packages/jengine-swarm): it registers a
-- `swarm_spawner` component and one PreUpdate system that keeps the boid
-- population matched to the spawner's count and flocks them with a spatial
-- grid.
--
-- WHY THIS SCENE EXISTS. Two things at once:
--
--   1. A real feature built as a package rather than baked into the engine.
--   2. Somewhere to find out where the frame rate falls over, and whether it
--      falls over in the same place natively as in the editor.
--
-- WHY THE SCRIPT DOES NOT SPAWN ANYTHING. Creating thousands of entities
-- through the script bridge would be thousands of deferred commands a frame,
-- and the bridge would be what you were measuring. The package spawns them in
-- Rust; this script only asks for a number.
--
-- HOW THE NUMBER GETS THERE. `Entity.set_custom(entity, "Swarm", "count", n)`. The
-- package reads that custom field in preference to the authored count.
-- Custom components rather than a package script function because `set_custom`
-- is a core global that exists in all three runtimes, whereas a package's own
-- script functions are installed by the native Lua bridge and nothing else.
--
-- READING THE NUMBERS. `Debug.get_fps()` is smoothed, so give it a second after a
-- step change. The engine overlay (F3) breaks the frame down further; the
-- editor has the same numbers in its profiler panel. Compare like with like:
-- an editor viewport built with `-DevWasm` is unoptimised and proves nothing.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/61-swarm.scene"

local PRESETS = { 500, 1000, 2000, 4000, 8000, 16000 }
local KEYS = { "1", "2", "3", "4", "5", "6" }

local swarm
local readout, readout2
local count = 1000
local worst_ms = 0.0
local settle = 0.0

local function apply(n)
    if n < 0 then n = 0 end
    if n > 200000 then n = 200000 end
    count = n
    if swarm ~= nil then
        Entity.set_custom(swarm, "Swarm", "count", count)
    end
    -- The flock takes a moment to redistribute after a resize, and fps is
    -- smoothed, so ignore the worst-case reading briefly rather than recording
    -- a spike that says nothing about steady state.
    worst_ms = 0.0
    settle = 1.0
end

function init(entity)
    swarm = Entity.find_by_name("Swarm")
    readout = Entity.find_by_name("Readout")
    readout2 = Entity.find_by_name("Readout2")
    apply(count)
    log("[scene_61] ready - 1-6 for presets, Minus/Equal to step by 500, 0 to stop")
end

function update(entity, dt)
    if settle > 0.0 then
        settle = settle - dt
    end

    for i, key in ipairs(KEYS) do
        if Input.is_key_just_pressed(key) then
            apply(PRESETS[i])
        end
    end
    if Input.is_key_just_pressed("0") then
        apply(0)
    -- Canonical KeyCode names, not the printed characters: "Minus"/"Equal",
    -- never "-"/"=". A wrong name is simply a key that never fires.
    elseif Input.is_key_just_pressed("Equal") then
        apply(count + 500)
    elseif Input.is_key_just_pressed("Minus") then
        apply(count - 500)
    end

    local fps = 0.0
    if Debug.get_fps ~= nil then
        fps = Debug.get_fps() or 0.0
    end
    local ms = 0.0
    if fps > 0.001 then
        ms = 1000.0 / fps
    end
    if settle <= 0.0 and ms > worst_ms then
        worst_ms = ms
    end

    if readout ~= nil then
        -- `count` is what we ASKED for. `live` is what the world actually
        -- holds. Reporting only the request is how a screen full of nothing
        -- came with a confident "16000 boids" next to it - the package had
        -- spawned none, and the readout could not tell you that.
        local live = -1
        if Debug.get_entity_count ~= nil then
            live = Debug.get_entity_count()
        end
        local pop = string.format("%d asked", count)
        if live >= 0 then
            -- Entity total, not boids alone: there is no per-component count
            -- in the script API. The scene has 10 authored entities, so
            -- anything near that means the flock is empty.
            pop = string.format("%d asked / %d entities", count, live)
        end
        Text.set(readout, string.format(
            "%s   %.0f fps   %.1f ms/frame   worst %.1f ms",
            pop, fps, ms, worst_ms))
    end
    if readout2 ~= nil then
        local budget = ""
        if ms > 0.001 then
            budget = string.format("  (%.0f%% of a 60Hz frame)", ms / 16.67 * 100.0)
        end
        Text.set(readout2, string.format(
            "one package system moves all of them%s", budget))
    end

    nav.tick(SCENE_PATH)
end
