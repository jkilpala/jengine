-- Scene 229: Sprite.press - a dent (or a ridge) pressed into a paintable
-- sprite's normal map, so the light catches its rim. The floor carries no
-- colour change at all: what you see is light on the shape.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/229-press.scene"
local atan2 = math.atan2 or function(y, x) return math.atan(y, x) end
local SHAPES = { "disc", "splat", "streak" }
local e = {}
local t = 0
local shape = 1
local depth = 1.5
local presses = 0
local gone = 0
local foot = 1
local px, py = 0, 0

function init(entity)
    for _, n in ipairs({ "Floor", "Lamp", "LampDot", "Brush", "Line0" }) do e[n] = Entity.find_by_name(n) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    for i = 1, 3 do
        if Input.is_key_just_pressed(tostring(i)) then shape = i end
    end
    if Input.is_key_just_pressed("R") then depth = -depth end
    -- The lamp goes round low over the floor.
    local lx, ly = math.cos(t * 0.6) * 420, math.sin(t * 0.6) * 200
    Transform.set_position(e.Lamp, lx, ly)
    Transform.set_position(e.LampDot, lx, ly)
    -- The brush walks a figure of eight, a footprint every 26 units.
    local bx, by = math.sin(t * 0.45) * 380, math.sin(t * 0.9) * 170 - 10
    Transform.set_position(e.Brush, bx, by)
    local dx, dy = bx - px, by - py
    gone = gone + math.sqrt(dx * dx + dy * dy)
    if gone > 26 then
        gone = 0
        local a = atan2(dy, dx)
        local side = foot == 1 and 8 or -8
        foot = 3 - foot
        Sprite.press(e.Floor, bx - math.sin(a) * side, by + math.cos(a) * side, 18, depth, "streak", a)
        presses = presses + 1
    end
    px, py = bx, by
    if Input.is_mouse_left_just_pressed() then
        local mx, my = Input.get_mouse_world_position()
        Sprite.press(e.Floor, mx, my, 60, depth, SHAPES[shape], 0.5)
        presses = presses + 1
    end
    UI.set_text(e.Line0, string.format("%s   %s   presses %d", SHAPES[shape], depth > 0 and "dents" or "ridges",
        math.floor(presses)))
end
