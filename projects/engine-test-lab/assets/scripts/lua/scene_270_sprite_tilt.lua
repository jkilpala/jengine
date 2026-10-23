-- Scene 270: `sprite_tilt`. Three cards held at fixed turns, and one
-- driven: the arrows lean it, Space turns it over, A lets it sway.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/270-sprite-tilt.scene"
local line, card
local yaw, pitch, over = 0, 0, false
local auto, t = true, 0

function init(entity)
    line = Entity.find_by_name("Line0")
    card = Entity.find_by_name("Driven")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("A") then auto = not auto end
    if Input.is_key_just_pressed("Space") then over = not over end
    t = t + dt
    if auto then
        yaw, pitch = math.sin(t * 0.9) * 0.6, math.sin(t * 1.3) * 0.4
        if math.floor(t / 4) % 2 == 1 then over = true else over = false end
    else
        if Input.is_key_pressed("Left") then yaw = yaw - dt end
        if Input.is_key_pressed("Right") then yaw = yaw + dt end
        if Input.is_key_pressed("Up") then pitch = pitch + dt end
        if Input.is_key_pressed("Down") then pitch = pitch - dt end
    end
    local base = 0
    if over then base = math.pi end
    SpriteTilt.set(card, "target", { base + yaw, pitch })
    local side = "front"
    if over then side = "back" end
    UI.set_text(line, "yaw " .. math.floor(yaw * 100 + 0.5) / 100 .. "   pitch " .. math.floor(pitch * 100 + 0.5) / 100
        .. "   " .. side .. (auto and "   (swaying)" or ""))
end
