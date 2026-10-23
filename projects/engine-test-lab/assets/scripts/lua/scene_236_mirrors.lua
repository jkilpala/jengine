-- Scene 236: `mirror`. Two mirrors face each other across a corridor, each
-- reaching past the other, so the corridor repeats; M turns them off.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/236-mirrors.scene"
local e = {}
local t = 0
local on = true

function init(entity)
    for _, n in ipairs({ "Block", "Arrow", "MirrorTop", "MirrorBottom", "Line0" }) do
        e[n] = Entity.find_by_name(n)
    end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if Input.is_key_just_pressed("M") then
        on = not on
        Mirror.set(e.MirrorTop, "enabled", on)
        Mirror.set(e.MirrorBottom, "enabled", on)
    end
    Transform.set_position(e.Block, math.sin(t * 0.8) * 200, 20 + math.sin(t * 1.7) * 18)
    Transform.set_rotation(e.Arrow, t * 0.9)
    UI.set_text(e.Line0, on and "mirrors on" or "mirrors off")
end
