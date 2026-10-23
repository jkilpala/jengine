-- Scene 136: .gui styles. The middle card's opacity and the right card's
-- scale are animated with `GUI.set`; Recolour sets a colour from a Lua table.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/136-gui-styles.scene"
local GUI_ID = "ui/scene_136_styles.gui"

local t = 0.0
local hue = 0

function init(entity)
    GUI.on_click(entity, GUI_ID, "b_three", "on_gui_click")
end

function on_gui_click(entity, node)
    hue = (hue + 1) % 3
    local colours = { { 0.2, 0.35, 0.7, 1 }, { 0.7, 0.3, 0.15, 1 }, { 0.2, 0.6, 0.3, 1 } }
    GUI.set(GUI_ID, "rounded", "bg_color", colours[hue + 1])
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    GUI.set(GUI_ID, "fading", "opacity", 0.5 + 0.5 * math.cos(t * 1.6))
    GUI.set(GUI_ID, "popping", "scale", 1.0 + 0.12 * math.sin(t * 2.2))
end
