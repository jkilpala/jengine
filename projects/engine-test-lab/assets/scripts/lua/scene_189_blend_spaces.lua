-- Scene 189: a skeletal player whose clip is empty plays its blend_space -
-- five clips placed at points, mixed by where the position is, all at one
-- phase. The mouse over the pad moves the position
-- (Animation.set_blend_position); with the mouse elsewhere it circles.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/189-blend-spaces.scene"
local PX, PY, U = -300.0, -10.0, 120.0
local rig, cursor, status
local t, spinning = 0, false

function init(entity)
    rig = Entity.find_by_name("Rig")
    cursor = Entity.find_by_name("Cursor")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    local mx, my = Input.get_mouse_world_position()
    local bx, by = (mx - PX) / U, (my - PY) / U
    local mode = "the mouse"
    if math.abs(bx) > 1.4 or math.abs(by) > 1.4 then
        -- Off the pad: a loop that passes over every point and outside them.
        bx, by = 1.25 * math.cos(t * 0.5), 1.25 * math.sin(t * 0.8)
        mode = "circling"
    end
    Animation.set_blend_position(rig, bx, by)
    Transform.set_position(cursor, PX + bx * U, PY + by * U)
    if Input.is_key_just_pressed("Space") then
        spinning = not spinning
        Animation.crossfade(rig, spinning and "animations/lab189_spin.skelanim" or "", 0.6)
    end
    if status then
        UI.set_text(status, "position " .. string.format("%.2f, %.2f", bx, by) .. " (" .. mode .. ")   playing "
            .. (spinning and "the spin clip" or "the blend space"))
    end
end
