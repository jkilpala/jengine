-- Scene 228: `paintable` sprites and Sprite.paint - stains written into a
-- sprite's own copy of its texture, kept for good. Left floor paints `over`,
-- right floor `multiply`; each has a hole the paint must leave alone.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/228-paint.scene"
local atan2 = math.atan2 or function(y, x) return math.atan(y, x) end
local SHAPES = { "disc", "splat", "streak" }
local e = {}
local t = 0
local shape = 2
local stamps = 0
local px, py, qx, qy = -300, -10, 300, -10
local next_a, next_b = 0, 0

local function brush(cx, phase)
    return cx + math.sin(t * 0.9 + phase) * 220, -10 + math.sin(t * 1.4 + phase * 2) * 130
end

function init(entity)
    for _, n in ipairs({ "Over", "Multiply", "Brush", "Brush2", "Line0" }) do e[n] = Entity.find_by_name(n) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    for i = 1, 3 do
        if Input.is_key_just_pressed(tostring(i)) then shape = i end
    end
    local ax, ay = brush(-300, 0)
    local bx, by = brush(300, 1.3)
    Transform.set_position(e.Brush, ax, ay)
    Transform.set_position(e.Brush2, bx, by)
    next_a = next_a - dt
    if next_a <= 0 then
        Sprite.paint(e.Over, ax, ay, 26, 0.35, 0.01, 0.01, 0.9, "splat", atan2(ay - py, ax - px))
        next_a, stamps = 0.25, stamps + 1
    end
    next_b = next_b - dt
    if next_b <= 0 then
        Sprite.paint(e.Multiply, bx, by, 60, 0.15, 0.12, 0.3, 0.5, "streak", atan2(by - qy, bx - qx))
        next_b, stamps = 0.2, stamps + 1
    end
    px, py, qx, qy = ax, ay, bx, by
    if Input.is_mouse_left_just_pressed() then
        local mx, my = Input.get_mouse_world_position()
        local target = mx < 0 and e.Over or e.Multiply
        Sprite.paint(target, mx, my, 50, 0.1, 0.5, 0.9, 1.0, SHAPES[shape], 0.6)
        stamps = stamps + 1
    end
    UI.set_text(e.Line0, string.format("shape %s   stamps %d", SHAPES[shape], math.floor(stamps)))
end
