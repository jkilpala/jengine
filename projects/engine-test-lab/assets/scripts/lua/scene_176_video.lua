-- Scene 176: the video is the engine's (`video_player`); this drives it
-- with Video.set.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/176-video.scene"
local screen, status
local playing, looping, speed = true, true, 1.0
local ended = 0

function init(entity)
    screen = Entity.find_by_name("Screen")
    status = Entity.find_by_name("Status")
    Event.on("video_ended", entity)
end

function on_video_ended(entity, data)
    ended = ended + 1
    playing = false
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if not screen then return end
    if Input.is_key_just_pressed("Space") then
        playing = not playing
        Video.set(screen, "playing", playing)
    end
    if Input.is_key_just_pressed("R") then
        Video.set(screen, "seek", 0)
        playing = true
        Video.set(screen, "playing", true)
    end
    if Input.is_key_just_pressed("L") then
        looping = not looping
        Video.set(screen, "looping", looping)
    end
    if Input.is_key_just_pressed("Up") then speed = math.min(4, speed * 2); Video.set(screen, "speed", speed) end
    if Input.is_key_just_pressed("Down") then speed = math.max(0.25, speed / 2); Video.set(screen, "speed", speed) end
    if status then
        UI.set_text(status, (playing and "playing" or "paused") .. "   loop " .. (looping and "on" or "off")
            .. "   speed " .. speed .. "x   ended " .. ended .. " time" .. (ended == 1 and "" or "s"))
    end
end
