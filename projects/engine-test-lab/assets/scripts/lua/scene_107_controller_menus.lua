-- Scene 107 - Controller menus
--
-- A menu used without a pointer. The engine does all of it
-- (`ecs::systems::navigate_ui`); this script only listens, exactly as it
-- would for clicks:
--
--   * arrows, a D-pad or the left stick move the focus ring to the nearest
--     button that way; Enter, Space or A press it, and the press arrives as
--     the same `on_gui_click` a click sends
--   * ECHO is disabled - focus can land on it, a press does nothing
--   * the list is longer than its frame: walking down scrolls it so the
--     ring stays in view
--   * [U] swaps to entity UI - a button (`ui_click`), a slider (left / right
--     slide it while focused) and a toggle (A flips it)
--
-- Until Sep 2026 a `.gui` document could be walked with Tab and nothing
-- pressed the focused button, and entity UI had no focus at all: no menu in
-- any game here could be used from a controller without its own cursor.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/107-controller-menus.scene"
local GUI_DOC = "ui/scene_107_menu.gui"

local IDS = { "g_a", "g_b", "g_c", "g_d", "g_e", "g_f", "g_g" }
for i = 0, 13 do
    IDS[#IDS + 1] = "l_" .. i
end

local readout, eu_button, eu_slider, eu_toggle
local showing_entity_ui = false
local presses = 0

local function say(what)
    presses = presses + 1
    Text.set(readout, string.format("pressed: %s   (%d so far)", what, presses))
    log("[scene_107] pressed " .. what)
end

function init(entity)
    readout = Entity.find_by_name("Readout")
    eu_button = Entity.find_by_name("EUButton")
    eu_slider = Entity.find_by_name("EUSlider")
    eu_toggle = Entity.find_by_name("EUToggle")
    for _, id in ipairs(IDS) do
        GUI.on_click(entity, GUI_DOC, id, "on_gui_click")
    end
    Event.on("ui_click", entity)
    log("[scene_107] ready - arrows / D-pad / stick, Enter / Space / A; U swaps UIs")
end

local function show_entity_ui(on)
    showing_entity_ui = on
    if on then
        GUI.hide(GUI_DOC)
    else
        GUI.show(GUI_DOC)
    end
    for _, e in ipairs({ eu_button, eu_slider, eu_toggle }) do
        UI.set_visible(e, on)
    end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("U") then
        show_entity_ui(not showing_entity_ui)
    end
    if showing_entity_ui then
        local v = UI.get_prop(eu_slider, "value") or 0
        local on = UI.get_prop(eu_toggle, "checked")
        Text.set(readout, string.format("slider %d%%   toggle %s   (%d presses)",
            math.floor(v * 100 + 0.5), on and "on" or "off", presses))
    end
end

function on_gui_click(self_entity, node_id)
    say(node_id)
end

function on_ui_click(self_entity, data)
    if data ~= nil and data.button == eu_button then
        say("the entity-UI button")
    end
end
