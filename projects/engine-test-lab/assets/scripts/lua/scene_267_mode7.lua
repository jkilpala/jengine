-- Scene 267: `mode7`. A checkered ground in perspective and posts
-- (`mode7_sprite`) standing on it; the camera circles on its own (A), or
-- Left / Right turn and Up / Down move it with `Mode7.place`.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/267-mode7.scene"
local line, ground
local auto, t = true, 0
local x, y, a = -350, 0, 0

function init(entity)
    line = Entity.find_by_name("Line0")
    ground = Entity.find_by_name("Ground")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("A") then auto = not auto end
    if auto then
        t = t + dt * 0.3
        x, y = -350 * math.cos(t), -350 * math.sin(t)
        a = t
    else
        if Input.is_key_pressed("Left") then a = a + dt end
        if Input.is_key_pressed("Right") then a = a - dt end
        local v = 0
        if Input.is_key_pressed("Up") then v = 160 end
        if Input.is_key_pressed("Down") then v = -160 end
        x, y = x + math.cos(a) * v * dt, y + math.sin(a) * v * dt
    end
    Mode7.place(ground, x, y, a)
    UI.set_text(line, "camera at (" .. math.floor(x + 0.5) .. ", " .. math.floor(y + 0.5) .. ") facing "
        .. math.floor((a * 180 / math.pi) % 360 + 0.5) .. " degrees" .. (auto and "   (circling)" or ""))
end
