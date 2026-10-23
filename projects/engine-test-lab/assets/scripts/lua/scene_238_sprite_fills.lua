-- Scene 238: `radial` and `bar` sprite fills, swept by Sprite.set_fill.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/238-sprite-fills.scene"
local ids = {}
local t = 0
local paused = false

function init(entity)
    for _, n in ipairs({ "Radial0", "Radial1", "Radial2", "Bar0", "Bar1", "Bar2", "Bar3", "Line0" }) do
        ids[n] = Entity.find_by_name(n)
    end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then paused = not paused end
    if not paused then t = t + dt end
    local a = 0.5 - 0.5 * math.cos(t * 0.9)
    for _, n in ipairs({ "Radial0", "Radial1", "Radial2", "Bar0", "Bar1", "Bar2", "Bar3" }) do
        Sprite.set_fill(ids[n], a)
    end
    UI.set_text(ids.Line0, string.format("amount %.2f", a))
end
