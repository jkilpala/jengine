-- Scene 05: Prefabs
--
-- Demonstrates `Entity.spawn_prefab(name)` for the three prefabs under
-- `assets/prefabs/` (Coin, Block, Spark). Key 1/2/3 spawns the matching
-- prefab at the current mouse position; left mouse re-spawns the last
-- type chosen. Spawned entities carry Velocity2D so they move and bounce via
-- the native motion + BounceArea systems.
--
-- The three differ on purpose, and the difference is the demo: the coin is
-- light and damps to a stop, the block is heavy and settles fast, the spark
-- is quick and loses nothing on a bounce. They all used to crawl - the block
-- had no velocity and no bounce area at all, so the spawn kick was the only
-- thing moving it, slowly, off the edge and out of the scene for good.
--
-- The script keeps its own ring buffer of spawn ids and destroys the oldest
-- entity once MAX_LIVE is exceeded. This used to add "a `Lifetime` on the
-- prefab would be cleaner, but ScriptCommand::SpawnEntity does not attach
-- Lifetime" - that is **stale**, the native spawn arm attaches it (and
-- `bounce_area`, which is what keeps these three on screen). The ring buffer
-- stays anyway because it caps the live *count*, which an age-based Lifetime
-- does not: holding 1 spawns as fast as the key repeats.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/05-prefabs.scene"

local MAX_LIVE = 60
local KICK = 260.0   -- spread on spawn, in world units / second
local TYPES = { "Coin", "Block", "Spark" }

-- Ring buffer of live spawn ids.
local live = {}
local head = 1
local last_type = "Coin"

-- Track per-frame key edges so a single press spawns exactly one entity
-- even though `is_key_pressed` reports level state.
local prev_1, prev_2, prev_3, prev_c = false, false, false, false

local function mouse_to_world()
    -- Use the engine-provided helper so the mapping accounts for the
    -- active camera's viewport size / zoom / position, not just the
    -- raw canvas pixel size (which can differ from the camera's
    -- authored 1280×720 in the editor).
    local wx, wy = Input.get_mouse_world_position()
    return wx or 0.0, wy or 0.0
end

local function clear_all()
    for i = 1, #live do
        if live[i] then Entity.destroy(live[i]) end
        live[i] = nil
    end
    head = 1
    log("[05-prefabs] cleared")
end

local function spawn(name)
    last_type = name
    local id = Entity.spawn_prefab(name)
    if not id then
        log("[05-prefabs] spawn_prefab returned nil for " .. tostring(name))
        return
    end
    local wx, wy = mouse_to_world()
    Transform.set_position(id, wx, wy)
    -- A random kick so identical prefabs do not stack perfectly. It has to
    -- scale with the prefab speeds or a fistful of coins leaves as one clump.
    local jx = (math.random() - 0.5) * KICK
    local jy = (math.random() - 0.5) * KICK
    -- Preserve the prefab's authored velocity by reading it back first.
    local vx, vy = Physics.get_velocity(id)
    Physics.set_velocity(id, (vx or 0) + jx, (vy or 0) + jy)

    -- Ring-buffer cleanup of the oldest entry when full.
    if live[head] then Entity.destroy(live[head]) end
    live[head] = id
    head = (head % MAX_LIVE) + 1
    log(string.format("[05-prefabs] spawn %s id=%s at (%.0f, %.0f)", name, tostring(id), wx, wy))
end

function init(entity)
    math.randomseed(123)
    log("[05-prefabs] ready - press 1/2/3 to spawn, mouse-left re-spawns, C clears")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)

    local k1 = Input.is_key_pressed("1")
    local k2 = Input.is_key_pressed("2")
    local k3 = Input.is_key_pressed("3")
    local kc = Input.is_key_pressed("C")

    if k1 and not prev_1 then spawn("Coin") end
    if k2 and not prev_2 then spawn("Block") end
    if k3 and not prev_3 then spawn("Spark") end
    if kc and not prev_c then clear_all() end
    prev_1, prev_2, prev_3, prev_c = k1, k2, k3, kc

    if Input.is_mouse_left_just_pressed() then
        spawn(last_type)
    end
end
