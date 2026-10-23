-- Scene 290: `Input.get_mouse_delta` and `Cursor.set_locked` - the mouse
-- read as motion. The dial turns with the mouse's sideways motion (a
-- spinner) and the ring moves by the motion (a trackball), both from the
-- delta, never from the position: push the mouse past the window edge and
-- they keep going, while the position stops. L locks the pointer (hidden and
-- held); in a browser the lock needs that key press or a click first, and
-- Esc lets it go - the readout says what the runtime has.

script = { tick_rate = 60 }   -- every frame: motion is read per frame

local nav = require("lua/nav")
local SCENE_PATH = "scenes/290-mouse-delta.scene"
local lines = {}
local dial, ring
local angle = 0
local rx, ry = 0, -60
local sum_x, sum_y = 0, 0
local locked = false

function init(entity)
    for i = 0, 2 do lines[i] = Entity.find_by_name("Line" .. i) end
    dial = Entity.find_by_name("Dial")
    ring = Entity.find_by_name("Ring")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local dx, dy = Input.get_mouse_delta()
    sum_x, sum_y = sum_x + dx, sum_y + dy
    -- A spinner: sideways motion turns the dial.
    angle = angle - dx * 0.01
    Transform.set_rotation(dial, angle)
    -- A trackball: the ring moves by the motion (screen y is down), wrapping
    -- round the play area so it never stops at an edge either.
    rx, ry = rx + dx, ry - dy
    if rx > 600 then rx = rx - 1200 elseif rx < -600 then rx = rx + 1200 end
    if ry > 200 then ry = ry - 480 elseif ry < -280 then ry = ry + 480 end
    Transform.set_position(ring, rx, ry)

    if Input.is_key_just_pressed("L") then
        locked = not locked
        Cursor.set_locked(locked)
    end
    if Input.is_key_just_pressed("C") then sum_x, sum_y = 0, 0 end

    local mx, my = Input.get_mouse_position()
    UI.set_text(lines[0], string.format("delta (%d, %d)   total (%d, %d)   position (%d, %d)",
        math.floor(dx + 0.5), math.floor(dy + 0.5), math.floor(sum_x + 0.5), math.floor(sum_y + 0.5),
        math.floor(mx + 0.5), math.floor(my + 0.5)))
    UI.set_text(lines[1], "lock asked: " .. tostring(locked) .. "   runtime can lock: "
        .. tostring(Display.supports("cursor_lock")))
    UI.set_text(lines[2], "dial angle " .. string.format("%.2f", angle) .. " rad")
end
