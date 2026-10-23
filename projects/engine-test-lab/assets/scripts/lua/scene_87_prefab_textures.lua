-- Scene 87 - Prefab textures
--
-- A runtime preloads textures by walking the world, which is right for
-- everything a scene names and wrong for everything it does not. Nothing
-- references a prefab's sheet until a script spawns one, and that happens
-- mid-frame - far too late to read an image off disk.
--
-- So a prefab whose texture no scene entity uses spawned, lived, moved and
-- drew *nothing*. Silently: the missing-texture warning only fires for ids
-- something asked to load, and nothing ever asked. The web runtime had its
-- own scan for this and the editor viewport asks the editor for whatever
-- the renderer finds missing, so both were fine and only native was blind -
-- which is the worst arrangement, because native is where it shipped
-- (BUGS #137).
--
-- Two columns. The left spawns `LabStranger`, whose sheet this scene names
-- nowhere. The right spawns `LabKnown`, whose sheet the grey bar under it
-- uses. If the left column is empty, the preload has stopped looking at
-- prefabs.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/87-prefab-textures.scene"

local readout
local spawned = 0
local t = 0

local function drop(prefab, x, y)
    local e = Entity.spawn_prefab(prefab)
    Transform.set_position(e, x + (math.random() - 0.5) * 90, y + (math.random() - 0.5) * 40)
    spawned = spawned + 1
    return e
end

local function wave(count)
    for _ = 1, count do
        drop("LabStranger", -260, -20)
        drop("LabKnown", 260, -20)
    end
end

function init(entity)
    readout = Entity.find_by_name("Readout")
    wave(1)
    log("[scene_87] ready - the left column is the one that used to be invisible")
end

function update(entity, dt)
    t = t + dt
    if Input.is_key_just_pressed("1") then
        wave(1)
    end
    if Input.is_key_just_pressed("2") then
        wave(20)
    end
    if Input.is_key_just_pressed("R") then
        spawned = 0
    end
    -- They carry `Lifetime`, so the count drifts back down on its own.
    Text.set(readout, string.format("spawned %d   entities now %d", spawned, Debug.get_entity_count()))
    nav.tick(SCENE_PATH)
end
