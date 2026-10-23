-- Scene 43 - Profiling & Debug Overlay
--
-- The overlays are engine-side: F3 draws frame stats (fps / frame ms / scripts
-- / physics / render / draws), F12 the entity inspector, and Build Settings ->
-- Debug Overlay decides whether they ship. Each runtime fills its own
-- FrameStats, so a row that stays flat on one runtime usually means that
-- runtime does not fill it.
--
-- The script side reads the same numbers. get_fps / get_frame_time /
-- get_entity_count / get_draw_calls / log_stats are bound in every runtime
-- (docs claimed otherwise for a long time; verified by probing the VM rather
-- than reading the source). The guards below stay anyway: each runtime fills
-- its own FrameStats, so a counter can legitimately be absent or zero on one
-- and not another, and naming which is which on screen is the comparison this
-- scene exists to make.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/43-profiling.scene"

local spawned = 0
local lbl_fps, lbl_frame, lbl_entities, lbl_draws, lbl_spawned, readout

-- Call `fn` only if the global exists in this runtime.
local function try(fn, fmt)
    if fn == nil then
        return "nil in this runtime"
    end
    local ok, v = pcall(fn)
    if not ok or v == nil then
        return "nil in this runtime"
    end
    return string.format(fmt, v)
end

function init(entity)
    lbl_fps = Entity.find_by_name("Fps")
    lbl_frame = Entity.find_by_name("Frame")
    lbl_entities = Entity.find_by_name("Entities")
    lbl_draws = Entity.find_by_name("Draws")
    lbl_spawned = Entity.find_by_name("Spawned")
    readout = Entity.find_by_name("Readout")
    log("[scene_43] ready - Space spawns 100 sprites, L logs stats, R resets")
end

function update(entity, dt)
    if Input.is_key_just_pressed("Space") then
        for i = 1, 100 do
            -- spawn_entity takes a NAME and returns a pre-allocated id that
            -- is usable in the same frame, even though the spawn is deferred.
            local e = Entity.spawn("Blob" .. (spawned + i))
            if e ~= nil then
                Transform.set_position(e, math.random(-560, 560), math.random(-230, 230))
                Sprite.set_color(e, math.random(), math.random(), math.random(), 1.0)
            end
        end
        spawned = spawned + 100
    elseif Input.is_key_just_pressed("L") then
        if Debug.log_stats ~= nil then
            Debug.log_stats()
        else
            log("[scene_43] log_stats is nil in this runtime")
        end
    elseif Input.is_key_just_pressed("R") then
        Scene.load(SCENE_PATH)
        return
    end

    if lbl_fps then Text.set(lbl_fps, "get_fps(): " .. try(Debug.get_fps, "%.0f")) end
    if lbl_frame then Text.set(lbl_frame, "get_frame_time(): " .. try(Debug.get_frame_time, "%.2f ms")) end
    if lbl_entities then Text.set(lbl_entities, "get_entity_count(): " .. try(Debug.get_entity_count, "%d")) end
    if lbl_draws then Text.set(lbl_draws, "get_draw_calls(): " .. try(Debug.get_draw_calls, "%d")) end
    if lbl_spawned then Text.set(lbl_spawned, "spawned by this scene: " .. spawned) end
    if readout then
        Text.set(readout,
            "F3 = frame overlay, F12 = inspector (engine-side, always available)")
    end

    nav.tick(SCENE_PATH)
end
