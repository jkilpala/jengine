-- Scene 266: `sprite_stack`. Three towers of slices; Left / Right turn the
-- camera (they keep rising up the screen), S spins them.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/266-sprite-stack.scene"
local line, cam
local turn, spinning = 0, true
local angles = { 0, 0.5, 1.0 }

function init(entity)
    line = Entity.find_by_name("Line0")
    cam = Entity.find_by_name("Camera")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_pressed("Left") then turn = turn + dt end
    if Input.is_key_pressed("Right") then turn = turn - dt end
    if Input.is_key_just_pressed("S") then spinning = not spinning end
    Transform.set_rotation(cam, turn)
    for i = 1, 3 do
        if spinning then angles[i] = angles[i] + dt * 0.8 end
        Transform.set_rotation(Entity.find_by_name("Stack" .. (i - 1)), angles[i])
    end
    UI.set_text(line, "camera turned " .. math.floor(turn * 180 / math.pi + 0.5) .. " degrees"
        .. (spinning and "   towers spinning" or "   towers still"))
end
