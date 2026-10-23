-- Scene 281: `speech_bubble` and `Bubble.say`. Three speakers close
-- together, one at the screen's edge, one with a lot to say.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/281-speech-bubble.scene"
local LINES = {
    A = "I was here first.",
    B = "Excuse me!",
    C = "Hello, hello.",
    Edge = "I'm right at the edge!",
    Wordy = "Now this is a rather long thing to say, and it ought to wrap onto a few lines in its bubble.",
}
local line, b
local bx, by = 0, -120
local typing = false

local function all_speak()
    for name, words in pairs(LINES) do
        Bubble.say(Entity.find_by_name(name), words, 0)
    end
end

function init(entity)
    line = Entity.find_by_name("Line0")
    b = Entity.find_by_name("B")
    all_speak()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_pressed("Left") then bx = bx - 200 * dt end
    if Input.is_key_pressed("Right") then bx = bx + 200 * dt end
    if Input.is_key_pressed("Up") then by = by + 200 * dt end
    if Input.is_key_pressed("Down") then by = by - 200 * dt end
    Transform.set_position(b, bx, by)
    if Input.is_key_just_pressed("T") then
        typing = not typing
        for name, _ in pairs(LINES) do
            Bubble.set(Entity.find_by_name(name), "reveal_speed", typing and 30 or 0)
        end
        all_speak()
    end
    if Input.is_key_just_pressed("Space") then all_speak() end
    UI.set_text(line, "typewriter " .. (typing and "on" or "off"))
end
