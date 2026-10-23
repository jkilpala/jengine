-- Scene 212: four joints. A cart on wheel joints (sprung, driven); two
-- boxes on target joints following a point (the mouse while it is held); an
-- arm on a motor joint held at an offset; three pucks on friction joints.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/212-more-joints.scene"
local X2 = -160
local X4 = 470
local e = {}
local speed = 5
local flip = 0
local t = 0

local function drop_pucks()
    for k = 0, 2 do
        Transform.set_position(e["Puck" .. k], X4 - 80 + 80 * k, 150)
        Physics.set_velocity(e["Puck" .. k], 0, 0)
    end
end

function init(entity)
    for _, n in ipairs({ "Orbit", "Arm3", "Puck0", "Puck1", "Puck2", "Line0" }) do e[n] = Entity.find_by_name(n) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    local key = Input.is_key_just_pressed
    flip = flip + dt
    if key("1") or flip > 2.6 then
        flip = 0
        speed = -speed
        Physics.set_motor_speed("Rear1", speed)
        Physics.set_motor_speed("Front1", speed)
    end
    if key("2") then Physics.apply_impulse(e.Arm3, 2400, 0) end
    if key("3") then drop_pucks() end
    if Input.is_mouse_left_pressed() then
        local mx, my = Input.get_mouse_world_position()
        Transform.set_position(e.Orbit, mx, my)
    else
        Transform.set_position(e.Orbit, X2 + math.cos(t * 1.4) * 80, math.sin(t * 1.4) * 80)
    end
    local vx, vy = Physics.get_velocity(e.Puck1)
    UI.set_text(e.Line0, "cart motor " .. speed .. "   middle puck falling at " .. math.floor(-vy + 0.5) .. " a second")
end
