-- Scene 232: `gloss` stamps wet a paintable's normal map (its specular
-- mask), and `dry_seconds` dries them; `spray` is a speckled stamp.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/232-wet-paint.scene"
local e = {}
local t = 0

function init(entity)
    for _, n in ipairs({ "Floor", "Lamp", "LampDot", "Wetter", "Sprayer" }) do e[n] = Entity.find_by_name(n) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    local lx, ly = math.cos(t * 0.5) * 300, math.sin(t * 0.5) * 140
    Transform.set_position(e.Lamp, lx, ly)
    Transform.set_position(e.LampDot, lx, ly)
    local wx, wy = math.sin(t * 0.8) * 420, math.sin(t * 1.7) * 180 - 10
    Transform.set_position(e.Wetter, wx, wy)
    Sprite.paint(e.Floor, wx, wy, 34, 1, 1, 1, 1.0, "disc", 0, "gloss")
    local sx, sy = math.sin(t * 0.6 + 2) * 420, math.cos(t * 1.1) * 180 - 10
    Transform.set_position(e.Sprayer, sx, sy)
    if math.floor(t * 20) % 2 == 0 then
        Sprite.paint(e.Floor, sx, sy, 50, 0.9, 0.2, 0.45, 0.6, "spray", 0)
    end
    if Input.is_mouse_left_just_pressed() then
        local mx, my = Input.get_mouse_world_position()
        Sprite.paint(e.Floor, mx, my, 90, 1, 1, 1, 1.0, "disc", 0, "gloss")
    end
end
