-- Scene 239: `post_volume`. The camera pans from a cold volume through the
-- scene's own grade to a warm one; the grade follows with no script.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/239-post-volumes.scene"
local cam, line
local t = 0
local paused = false

function init(entity)
    cam = Entity.find_by_name("Camera")
    line = Entity.find_by_name("Line0")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then paused = not paused end
    if not paused then t = t + dt end
    local x = math.sin(t * 0.35) * 1100
    Transform.set_position(cam, x, 0)
    UI.set_text(line, string.format("camera x %d", math.floor(x + 0.5)))
end
