-- Scene 251: `auto_normal`. Flat shapes with no normal maps, rounded from
-- their own silhouettes; the lamp circles them.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/251-auto-normal.scene"
local SHAPES = { "round", "linear", "pillow" }
local lamp, line
local shapes = {}
local t, rounded, shape = 0, true, 1

function init(entity)
    lamp = Entity.find_by_name("Lamp")
    line = Entity.find_by_name("Line0")
    for i = 0, 2 do shapes[#shapes + 1] = Entity.find_by_name("Shape" .. i) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("N") then
        rounded = not rounded
        for _, e in ipairs(shapes) do AutoNormal.set(e, "strength", rounded and 2.0 or 0.0) end
    end
    if Input.is_key_just_pressed("B") then
        shape = shape % #SHAPES + 1
        for _, e in ipairs(shapes) do AutoNormal.set(e, "shape", SHAPES[shape]) end
    end
    t = t + dt
    Transform.set_position(lamp, math.cos(t * 0.6) * 520, -20 + math.sin(t * 0.6) * 220)
    UI.set_text(line, (rounded and "rounded" or "flat") .. "   bevel " .. SHAPES[shape])
end
