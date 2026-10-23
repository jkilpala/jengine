-- 124 The Paper Theatre: the cut-outs are flat paper with no normal maps;
-- each carries an `auto_normal`, so the engine rounds its edges from its own
-- silhouette and the candle carried across catches the rims on its side.
-- N lays them flat (strength 0) to compare; the dragon bobs on its rod.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/124-paper-theatre.scene"
local NAMES = { "Moon", "Cloud0", "Cloud1", "HillsBack", "Castle", "HillsFront", "Tree", "Dragon", "Proscenium" }

local candle, dragon, status
local cuts = {}
local captions = true
local rounded, held = true, false
local t = 0

function init(entity)
    candle = Entity.find_by_name("Candle")
    dragon = Entity.find_by_name("Dragon")
    status = Entity.find_by_name("Status")
    for _, n in ipairs(NAMES) do cuts[#cuts + 1] = Entity.find_by_name(n) end
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("Space") then held = not held end
    if Input.is_key_just_pressed("N") then
        rounded = not rounded
        for _, e in ipairs(cuts) do AutoNormal.set(e, "strength", rounded and 2.2 or 0.0) end
    end
    if not held then t = t + dt end
    -- The candle goes to and fro along a low arc, its flame flickering.
    local x = math.sin(t * 0.35) * 460
    local y = -130 + math.cos(t * 0.35) * 40 + math.sin(t * 1.3) * 25
    Transform.set_position(candle, x, y)
    local flicker = 1.0 + math.sin(t * 17) * 0.05 + math.sin(t * 29) * 0.04
    Lighting.set_intensity(candle, 1.5 * flicker)
    -- The dragon bobs and drifts on its rod.
    Transform.set_position(dragon, 60 + math.sin(t * 0.5) * 120, 10 + math.sin(t * 1.4) * 14)
    Transform.set_rotation(dragon, math.sin(t * 1.4) * 0.05)
    UI.set_text(status, rounded and "rounded from their silhouettes" or "flat")
end
