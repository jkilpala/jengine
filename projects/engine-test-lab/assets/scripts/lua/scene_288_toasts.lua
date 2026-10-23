-- Scene 288: `toast_stack`, `Toast.show` / `Toast.set` and `on_toast`.
-- The script is on the stack, so `on_toast` reaches it.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/288-toasts.scene"
local CORNERS = { "top_right", "bottom_right", "bottom_left", "top_left" }
local line, stack
local n, shown, corner = 0, 0, 1

local function toast(long)
    n = n + 1
    local text = long and "A much longer line of text than fits on one row, so the toast wraps it onto more."
        or "Something happened."
    Toast.show(stack, "Toast " .. n, text, "sprites/circle16.png")
end

function init(entity)
    stack = entity
    line = Entity.find_by_name("Line0")
end

function on_toast(entity)
    shown = shown + 1
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then toast(false) end
    if Input.is_key_just_pressed("L") then toast(true) end
    if Input.is_key_just_pressed("5") then for _ = 1, 5 do toast(false) end end
    if Input.is_key_just_pressed("C") then
        corner = corner % #CORNERS + 1
        Toast.set(stack, "corner", CORNERS[corner])
    end
    UI.set_text(line, "sent " .. n .. "   shown " .. shown .. "   corner " .. CORNERS[corner])
end
