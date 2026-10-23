-- Scene 273: `Scene.transition`. Each key reloads this scene as its other
-- side through a transition; which side is up, and the auto mode, live in a
-- save slot never committed (kept in memory for the session, every
-- runtime), since the reload starts the script afresh.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/273-scene-transition.scene"
local SLOT = "lab273"
local STYLES = { "fade", "iris", "diamond", "slide", "wipe", "dissolve" }
local MASK = "masks/lab273_mask.png"
local line
local side, auto, last
local t = 0
local leaving = false

local function go(style)
    if leaving then return end
    leaving = true
    Save.set(SLOT, "side", side == "A" and "B" or "A")
    Save.set(SLOT, "last", style)
    local texture = ""
    if style == "dissolve" then texture = MASK end
    Scene.transition(SCENE_PATH, style, 0.9, texture)
end

function init(entity)
    line = Entity.find_by_name("Line0")
    side = Save.get(SLOT, "side") or "A"
    auto = Save.get(SLOT, "auto") ~= "off"
    last = Save.get(SLOT, "last") or "-"
    local other = side == "A" and "B" or "A"
    for _, n in ipairs({ "Side" .. other, "Side" .. other .. "Letter" }) do
        Transform.set_position(Entity.find_by_name(n), 0, -3000)
    end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if Input.is_key_just_pressed("A") then
        auto = not auto
        Save.set(SLOT, "auto", auto and "on" or "off")
    end
    for k = 1, #STYLES do
        if Input.is_key_just_pressed(tostring(k)) then go(STYLES[k]) end
    end
    if auto and t > 2.0 then
        local i = 1
        for k = 1, #STYLES do
            if STYLES[k] == last then i = k % #STYLES + 1 end
        end
        go(STYLES[i])
    end
    UI.set_text(line, "side " .. side .. "   arrived by " .. last .. (auto and "   (auto)" or ""))
end
