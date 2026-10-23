-- Scene 249: `Time.set_scale` slows the world; a `time_scale` keeps an
-- entity's own pace (`ignore_world` is real time); `Audio.follow_time`
-- bends the music and new one-shots with the world.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/249-time-scale.scene"
local dots = {}
local line
local follow = false

function init(entity)
    for _, n in ipairs({ "World", "Hero", "Quick" }) do dots[#dots + 1] = Entity.find_by_name(n) end
    line = Entity.find_by_name("Line0")
    Audio.play_music("sounds/hum.wav", 0.4)
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("1") then Time.set_scale(1.0) end
    if Input.is_key_just_pressed("2") then Time.set_scale(0.25) end
    if Input.is_key_just_pressed("3") then Time.set_scale(0.0) end
    if Input.is_key_just_pressed("A") then
        follow = not follow
        Audio.follow_time(follow)
    end
    if Input.is_key_just_pressed("B") then Audio.play_sound("sounds/blip.wav", 0.6) end
    -- Wrap each dot back to the left edge.
    for _, d in ipairs(dots) do
        local x, y = Transform.get_position(d)
        if x > 560 then
            Transform.set_position(d, -560, y)
            Trail.clear(d)
        end
    end
    UI.set_text(line, string.format("Time.get_scale() = %.2f   audio follows time: %s", Time.get_scale(), follow and "on" or "off"))
end
