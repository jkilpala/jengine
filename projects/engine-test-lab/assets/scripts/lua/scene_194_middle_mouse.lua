-- Scene 194: Input.is_mouse_middle_pressed / _just_pressed /
-- _just_released, beside the left and right buttons. A middle drag pans the
-- camera.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/194-middle-mouse.scene"
local lamps, status, cam = {}, nil, nil
local presses = { left = 0, middle = 0, right = 0 }
local cx, cy, lastx, lasty = 0, 0, nil, nil

function init(entity)
    lamps.left = Entity.find_by_name("LampLeft")
    lamps.middle = Entity.find_by_name("LampMiddle")
    lamps.right = Entity.find_by_name("LampRight")
    status = Entity.find_by_name("Status")
    cam = Entity.find_by_name("Camera")
end

local function lamp(e, on, r, g, b)
    if e == nil then return end
    if on then Sprite.set_color(e, r, g, b, 1) else Sprite.set_color(e, 0.25, 0.27, 0.32, 1) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_mouse_left_just_pressed() then presses.left = presses.left + 1 end
    if Input.is_mouse_middle_just_pressed() then presses.middle = presses.middle + 1 end
    if Input.is_mouse_right_just_pressed() then presses.right = presses.right + 1 end
    lamp(lamps.left, Input.is_mouse_left_pressed(), 0.4, 0.8, 1.0)
    lamp(lamps.middle, Input.is_mouse_middle_pressed(), 1.0, 0.8, 0.3)
    lamp(lamps.right, Input.is_mouse_right_pressed(), 0.9, 0.4, 0.9)
    -- A middle drag pans the board (screen pixels, y down).
    local mx, my = Input.get_mouse_position()
    if Input.is_mouse_middle_pressed() and lastx then
        cx, cy = cx - (mx - lastx), cy + (my - lasty)
        if cam then Transform.set_position(cam, cx, cy) end
    end
    if Input.is_mouse_middle_just_released() then lastx = nil end
    lastx, lasty = mx, my
    if status then
        UI.set_text(status, "presses: left " .. presses.left .. "   middle " .. presses.middle .. "   right " .. presses.right)
    end
end
