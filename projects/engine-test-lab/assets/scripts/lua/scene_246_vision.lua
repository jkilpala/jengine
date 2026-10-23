-- Scene 246: `vision`. The eye in the middle turns; the target circles;
-- the engine counts the target in the eye's custom `Vision.seen`.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/246-vision.scene"
local eye, target, line
local t = 0
local paused = false
local all_round = false

function init(entity)
    eye = Entity.find_by_name("Eye")
    target = Entity.find_by_name("Target")
    line = Entity.find_by_name("Line0")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then paused = not paused end
    if Input.is_key_just_pressed("W") then
        all_round = not all_round
        Vision.set(eye, "fov", all_round and 6.2832 or 1.4)
    end
    if not paused then t = t + dt end
    Transform.set_rotation(eye, t * 0.6)
    Transform.set_position(target, math.cos(t * 0.37) * 330, math.sin(t * 0.37) * 230)
    local seen = Entity.get_custom(eye, "Vision", "seen") or 0
    UI.set_text(line, seen > 0 and "the target is SEEN" or "the target is hidden")
end
