-- Scene 291: `Sprite.set_size(entity, w, h)` - the size a sprite is drawn
-- at, in world units, without touching its transform. The blue disc breathes
-- through set_size and its yellow child keeps its size; the red disc
-- breathes through its transform's scale, and its child grows with it -
-- which is the difference. The green square takes width and height apart.
-- Found missing by an online agar-like (Oct 2026), whose cells are
-- `online_slot`s: jengine-online owns a slot's scale.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/291-sprite-size.scene"
local grower, scaled, wide
local lines = {}
local t = 0

function init(entity)
    grower = Entity.find_by_name("Grower")
    scaled = Entity.find_by_name("Scaled")
    wide = Entity.find_by_name("Wide")
    for i = 0, 2 do lines[i] = Entity.find_by_name("Line" .. i) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    local k = 1.0 + 0.5 * math.sin(t * 1.6)
    Sprite.set_size(grower, 120 * k, 120 * k)
    Transform.set_scale(scaled, k, k)
    Sprite.set_size(wide, 60 + 120 * k, 180 - 60 * k)
    UI.set_text(lines[0], string.format("set_size %d x %d", math.floor(120 * k + 0.5), math.floor(120 * k + 0.5)))
    UI.set_text(lines[1], string.format("scale %.2f", k))
    UI.set_text(lines[2], string.format("wide %d x %d", math.floor(60 + 120 * k + 0.5), math.floor(180 - 60 * k + 0.5)))
end
