-- Scene 231: a transparent `paintable` canvas (`alpha_cutoff` 0): paint
-- gives it colour and alpha, "erase" takes alpha away, and an emitter whose
-- `emit_shape` is the canvas starts its sparks from what is painted now.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/231-paint-canvas.scene"
local e = {}
local t = 0
local stamps = 0

function init(entity)
    for _, n in ipairs({ "Canvas", "Painter", "Eraser", "Line0" }) do e[n] = Entity.find_by_name(n) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    local px, py = math.sin(t * 0.7) * 400, math.sin(t * 1.3) * 160 - 20
    local ex, ey = math.sin(t * 0.7 - 1.6) * 400, math.sin(t * 1.3 - 2.0) * 160 - 20
    Transform.set_position(e.Painter, px, py)
    Transform.set_position(e.Eraser, ex, ey)
    Sprite.paint(e.Canvas, px, py, 34, 1.0, 0.55, 0.15, 1.0, "disc", 0)
    Sprite.paint(e.Canvas, ex, ey, 60, 0, 0, 0, 1.0, "disc", 0, "erase")
    stamps = stamps + 2
    local mx, my = Input.get_mouse_world_position()
    if Input.is_mouse_left_pressed() then
        Sprite.paint(e.Canvas, mx, my, 40, 0.3, 0.9, 0.4, 1.0, "disc", 0, "over")
        stamps = stamps + 1
    end
    if Input.is_mouse_right_pressed() then
        Sprite.paint(e.Canvas, mx, my, 60, 0, 0, 0, 1.0, "disc", 0, "erase")
        stamps = stamps + 1
    end
    UI.set_text(e.Line0, string.format("stamps %d", math.floor(stamps)))
end
