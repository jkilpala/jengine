-- Scene 202: global illumination. The lamp lights the floor red; the lit
-- floor shines again (radiance cascades) and stops at the wall.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/202-global-illumination.scene"
local lamp, status
local on = true
local x, y = -300, -170

local function show()
    if status then UI.set_text(status, "bounce " .. (on and "on" or "off")) end
end

function init(entity)
    lamp = Entity.find_by_name("Lamp")
    status = Entity.find_by_name("Status")
    show()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("G") then
        on = not on
        Lighting.set_global_illumination(on and 2 or 0)
        show()
    end
    local dx, dy = 0, 0
    if Input.is_key_pressed("Left") then dx = dx - 1 end
    if Input.is_key_pressed("Right") then dx = dx + 1 end
    if Input.is_key_pressed("Down") then dy = dy - 1 end
    if Input.is_key_pressed("Up") then dy = dy + 1 end
    x = math.max(-560, math.min(560, x + dx * 200 * dt))
    y = math.max(-190, math.min(300, y + dy * 200 * dt))
    Transform.set_position(lamp, x, y)
end
