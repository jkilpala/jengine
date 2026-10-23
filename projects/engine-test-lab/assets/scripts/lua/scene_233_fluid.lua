-- Scene 233: a `fluid` poured down a ramp into a tank, drawn as one
-- surface by a `canvas_group` with a `threshold`.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/233-fluid.scene"
local e = {}
local threshold, pouring = true, true

function init(entity)
    for _, n in ipairs({ "Water", "Group", "Line0" }) do e[n] = Entity.find_by_name(n) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("T") then
        threshold = not threshold
        CanvasGroup.set(e.Group, "threshold", threshold and 0.45 or 0)
    end
    if Input.is_key_just_pressed("C") then Fluid.set(e.Water, "clear", true) end
    if Input.is_key_just_pressed("P") then
        pouring = not pouring
        Fluid.set(e.Water, "pour_rate", pouring and 90 or 0)
    end
    UI.set_text(e.Line0, (threshold and "one surface" or "soft dots") .. (pouring and "   pouring" or "   not pouring"))
end
