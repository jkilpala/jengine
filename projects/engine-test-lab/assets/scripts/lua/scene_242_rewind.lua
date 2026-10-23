-- Scene 242: `rewind` and `rewindable`. Space throws the ball; R held
-- rewinds, G held holds, anything else records. Left alone it throws, rewinds
-- and throws again.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/242-rewind.scene"
local clock, ball, line
local mode = "record"
local t = 0
local auto = 0

local function set_mode(m)
    if m ~= mode then
        mode = m
        Rewind.set(clock, "mode", m)
    end
end

function init(entity)
    clock = Entity.find_by_name("Clock")
    ball = Entity.find_by_name("Ball")
    line = Entity.find_by_name("Line0")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if Input.is_key_just_pressed("1") then Rewind.set(clock, "speed", 0.5) end
    if Input.is_key_just_pressed("2") then Rewind.set(clock, "speed", 1.0) end
    if Input.is_key_just_pressed("3") then Rewind.set(clock, "speed", 2.0) end
    -- Left alone: throw at 1 s, rewind from 4 s for 3 s, then again.
    auto = auto + dt
    local auto_mode = (auto > 4 and auto < 7) and "rewind" or "record"
    if auto > 7.5 then auto = 0 end
    if Input.is_key_pressed("R") then
        set_mode("rewind")
    elseif Input.is_key_pressed("G") then
        set_mode("hold")
    else
        set_mode(auto_mode)
    end
    if mode == "record" and (Input.is_key_just_pressed("Space") or (auto > 1 and auto - dt <= 1)) then
        Physics.set_velocity(ball, 900, 250)
    end
    UI.set_text(line, "mode " .. mode)
end
