-- Scene 171: the riding is the engine's (`path_follow`). This pauses and
-- plays them all, moves the shuttle between tracks, and counts arrivals.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/171-path-follow.scene"
local RIDERS = { "Inner", "Middle", "Outer", "Shuttle", "Runner" }
local playing = true
local upper = true
local arrivals = 0
local status, shuttle

function init(entity)
    status = Entity.find_by_name("Status")
    shuttle = Entity.find_by_name("Shuttle")
    Event.on("arrived", entity)
end

function on_arrived(entity, data)
    arrivals = arrivals + 1
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then
        playing = not playing
        for _, name in ipairs(RIDERS) do
            local e = Entity.find_by_name(name)
            if e then PathFollow.set(e, "playing", playing) end
        end
    end
    if Input.is_key_just_pressed("T") and shuttle then
        upper = not upper
        PathFollow.set(shuttle, "path", upper and "Upper" or "Lower")
    end
    if status then
        UI.set_text(status, (playing and "playing" or "paused") .. "   shuttle on " .. (upper and "Upper" or "Lower")
            .. "   runner arrivals " .. arrivals)
    end
end
