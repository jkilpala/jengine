-- Scene 185: a keyframe timeline. The `timeline` on this entity animates
-- every other thing in the scene; its event keys call this script
-- (`on_beat`), and `on_timeline_end` comes when a clip finishes.
-- Timeline.set switches clips, pauses and loops.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/185-keyframe-timeline.scene"
local beats, ended, playing, looping = 0, false, true, false
local clip = "show"
local status

function init(entity)
    status = Entity.find_by_name("Status")
end

function on_beat(entity)
    beats = beats + 1
end

function on_timeline_end(entity)
    ended = true
end

local function play(entity, name)
    clip = name
    beats, ended, playing = 0, false, true
    Timeline.set(entity, "clip", name)
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then
        playing = not playing
        Timeline.set(entity, "playing", playing)
    end
    if Input.is_key_just_pressed("R") then play(entity, "show") end
    if Input.is_key_just_pressed("Q") then play(entity, "quick") end
    if Input.is_key_just_pressed("L") then
        looping = not looping
        Timeline.set(entity, "looping", looping)
    end
    if status then
        UI.set_text(status, "clip " .. clip .. "   beats " .. beats .. "   " ..
            (ended and "ended" or (playing and "playing" or "paused")) .. (looping and "   looping" or ""))
    end
end
