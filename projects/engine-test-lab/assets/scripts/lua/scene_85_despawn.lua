-- Scene 85 - Despawn offscreen: things that clean themselves up
--
-- `Lifetime` covers "this goes away after a while". The other half - "this
-- goes away once nobody can see it" - had no component at all until Sep
-- 2026, so every scrolling game in this repo wrote the same loop: keep your
-- own list of what you spawned, compare each one against the camera every
-- frame, despawn the ones behind.
--
-- The component is authored on the prefab, and nothing here tracks what it
-- spawns:
--
--   "despawn_offscreen": { "margin": 60.0, "behind_only": true }
--
-- `behind_only` is what a scrolling game wants: whatever is ahead of the
-- camera has not had its turn yet and must not be swept up early. A camera
-- that is not moving has no behind, so behind-only things wait for it to
-- move; "behind" is per axis, the side the camera is moving away from.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/85-despawn.scene"

local camera, readout
local panning = true
local behind_only = true
local spawned = { keeper = 0, tidy = 0 }
local cam_x = 0
local next_spawn = 0
local t = 0

function init(entity)
    camera = Entity.find_by_name("Camera")
    readout = Entity.find_by_name("Readout")
    log("[scene_85] ready - Space pans, 1 spawns a hundred, 2 toggles behind-only, R clears")
end

local function spawn_one(ahead)
    -- One that looks after itself, and one that does not, side by side.
    local x = cam_x + ahead
    local tidy = Entity.spawn_prefab(behind_only and "LabBehind" or "LabTidy")
    Transform.set_position(tidy, x, -60)
    spawned.tidy = spawned.tidy + 1
    local keeper = Entity.spawn_prefab("LabKeeper")
    Transform.set_position(keeper, x, -110)
    spawned.keeper = spawned.keeper + 1
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt

    if Input.is_key_just_pressed("Space") then
        panning = not panning
    end
    if Input.is_key_just_pressed("2") then
        behind_only = not behind_only
    end
    if Input.is_key_just_pressed("1") then
        for i = 1, 100 do
            spawn_one(200 + i * 40)
        end
    end
    if Input.is_key_just_pressed("R") then
        -- Panning back the other way sweeps up everything that has a rule;
        -- the keepers stay, which is the point.
        cam_x = 0
        Transform.set_position(camera, cam_x, 0)
        spawned.keeper = 0
        spawned.tidy = 0
    end

    if panning then
        cam_x = cam_x + 260 * dt
        Transform.set_position(camera, cam_x, 0)
        next_spawn = next_spawn - dt
        if next_spawn <= 0 then
            next_spawn = 0.12
            spawn_one(760)
        end
    end

    -- `get_entity_count` is the honest measure: it counts what is really in
    -- the world, not what this script believes it spawned.
    local alive = Debug.get_entity_count ~= nil and Debug.get_entity_count() or -1
    Text.set(readout, string.format(
        "camera %.0f    spawned %d + %d    entities alive %d    %s    %s",
        cam_x, spawned.tidy, spawned.keeper, alive,
        behind_only and "behind-only" or "any direction",
        panning and "panning" or "stopped"))
end
