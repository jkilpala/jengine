-- Scene 243: `occlusion` and `muffle` on an audio source, `room_reverb`
-- on the listener. The ear walks in and out of the room; D opens and shuts
-- its door, W raises and lowers a second wall.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/243-wall-occlusion.scene"
local ear, door, extra, line
local t = 0
local paused = false
local door_open, wall_up = false, true

function init(entity)
    ear = Entity.find_by_name("Ear")
    door = Entity.find_by_name("Door")
    extra = Entity.find_by_name("Extra")
    line = Entity.find_by_name("Line0")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then paused = not paused end
    if Input.is_key_just_pressed("D") then door_open = not door_open end
    if Input.is_key_just_pressed("W") then wall_up = not wall_up end
    if not paused then t = t + dt end
    local x = -50 + math.sin(t * 0.3) * 420
    Transform.set_position(ear, x, 0)
    -- Out of the way, rather than gone: a door lifted into the wall above.
    Transform.set_position(door, 0, door_open and 300 or 0)
    Transform.set_position(extra, -250, wall_up and 0 or 600)
    local walls = 0
    if x < 0 and not door_open then walls = walls + 1 end
    if x < -250 and wall_up then walls = walls + 1 end
    UI.set_text(line, string.format("ear x %d   walls between %d", math.floor(x + 0.5), walls))
end
