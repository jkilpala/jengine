-- Scene 10 - GUI Document Showcase
--
-- Demonstrates every GuiNodeType (Panel/Text/Image/Button/ProgressBar/
-- Slider/Toggle/TextInput/Circle) driven from Lua via gui_get/gui_set/
-- gui_on_click/gui_focus.
--
-- Interactive behaviour:
--   loading_bar       : auto-animated 0 -> 1 -> 0 ping-pong via gui_set
--   brightness_slider : click to step; tints preview_panel via bg_color
--   deco_toggle       : click flips `checked`; stats_label reports ON/OFF
--   name_input        : focused on start (gui_focus), so typing goes straight
--                       into it. Text entry is engine-side: characters reach
--                       InputState and `apply_text_input` writes them to
--                       whichever node the GuiComponent has focused.
--                       Backspace deletes.
--
--                       `GUI.set(..., "text", ...)` fills the field and
--                       `""` clears it - watch the greeting under it
--                       change, since that echoes `gui_get`. Until Sep
--                       2026 the same call wrote the *placeholder*
--                       instead, which looks identical on an empty box:
--                       Randomize appeared to work and Clear did nothing
--                       at all (BUGS #138). [C] clears it from the
--                       keyboard.
--   btn_randomize     : sets slider, toggle, name input via gui_set
--   btn_reset         : restores defaults + clears click_log
--   btn_toggle_prev   : hides/shows preview row (image + circle + label)
--   click_log         : shows last button action

script = { tick_rate = 60 }

local nav = require("lua/nav")
local GUI_DOC = "ui/scene_10.gui"
local SCENE_PATH = "scenes/10-gui.scene"

local t = 0.0
local last_echo = ""
local preview_visible = true
local greet_names = { "Aria", "Bryn", "Cara", "Doran", "Eve", "Finn" }

local function reset_defaults()
    GUI.set(GUI_DOC, "brightness_slider", "value", 0.5)
    GUI.set(GUI_DOC, "deco_toggle",       "checked", true)
    GUI.set(GUI_DOC, "name_input",        "text", "Player1")
    GUI.set(GUI_DOC, "echo_label",        "text", "(type a name)")
    GUI.set(GUI_DOC, "click_log",         "text", "(click Randomize / Reset / Toggle Preview)")
    preview_visible = true
    GUI.set(GUI_DOC, "preview_panel", "visible", true)
    GUI.set(GUI_DOC, "demo_image",    "visible", true)
    GUI.set(GUI_DOC, "demo_circle",   "visible", true)
end

function init(entity)
    GUI.on_click(entity, GUI_DOC, "btn_randomize",       "on_gui_click")
    GUI.on_click(entity, GUI_DOC, "btn_reset",           "on_gui_click")
    GUI.on_click(entity, GUI_DOC, "btn_toggle_preview",  "on_gui_click")
    -- Slider/Toggle have no built-in click behaviour - scripts wire them up
    -- the same way buttons do, then mutate state via gui_set.
    GUI.on_click(entity, GUI_DOC, "deco_toggle",         "on_gui_click")
    GUI.on_click(entity, GUI_DOC, "brightness_slider",   "on_gui_click")
    GUI.focus(GUI_DOC, "name_input")
    reset_defaults()
    log("[scene_10] ready - click slider/toggle/buttons, type a name")
end

-- gui_show / gui_hide toggle a whole document's visibility, as opposed to
-- GUI.set(..., "visible", ...) which hides one node. Bound to G below.
local doc_visible = true

function update(entity, dt)
    -- Clearing the field is the half of `gui_set` that used to be a no-op.
    if Input.is_key_just_pressed("C") then
        GUI.set(GUI_DOC, "name_input", "text", "")
        GUI.set(GUI_DOC, "click_log", "text", "cleared the field")
    end
    if Input.is_key_just_pressed("G") then
        doc_visible = not doc_visible
        if doc_visible then GUI.show(GUI_DOC) else GUI.hide(GUI_DOC) end
    end
    t = t + dt

    -- Ping-pong loading bar 0..1 over 2.4s.
    local phase = (t % 2.4) / 1.2
    local v = phase <= 1.0 and phase or (2.0 - phase)
    GUI.set(GUI_DOC, "loading_bar", "value", v)

    -- Echo name input into the greeting label whenever it changes.
    local nm = GUI.get(GUI_DOC, "name_input", "text") or ""
    local greeting
    if nm == "" then
        greeting = "(type a name)"
    else
        greeting = "Hello, " .. nm .. "!"
    end
    if greeting ~= last_echo then
        GUI.set(GUI_DOC, "echo_label", "text", greeting)
        last_echo = greeting
    end

    -- Mirror slider + toggle state into the stats label.
    local br_s = GUI.get(GUI_DOC, "brightness_slider", "value") or "0.5"
    local br = tonumber(br_s) or 0.5
    local on_s = GUI.get(GUI_DOC, "deco_toggle", "checked") or "false"
    local on = (on_s == "true")
    GUI.set(GUI_DOC, "stats_label", "text",
        string.format("brightness %.2f   decoration %s   loading %.2f",
            br, on and "ON" or "OFF", v))

    nav.tick(SCENE_PATH)
end

function on_gui_click(self_entity, node_id)
    if node_id == "btn_randomize" then
        local nm = greet_names[math.random(#greet_names)]
        local br = math.random()
        GUI.set(GUI_DOC, "brightness_slider", "value", br)
        GUI.set(GUI_DOC, "preview_panel", "bg_color",
            string.format("[%.3f, %.3f, %.3f, 1.0]", br * 0.9, br * 0.6, 1.0 - br * 0.6))
        GUI.set(GUI_DOC, "deco_toggle",       "checked", math.random() > 0.5)
        GUI.set(GUI_DOC, "name_input",        "text", nm)
        GUI.set(GUI_DOC, "click_log",         "text", "randomize -> " .. nm)
    elseif node_id == "btn_reset" then
        reset_defaults()
        GUI.set(GUI_DOC, "click_log", "text", "reset")
    elseif node_id == "btn_toggle_preview" then
        preview_visible = not preview_visible
        GUI.set(GUI_DOC, "preview_panel", "visible", preview_visible)
        GUI.set(GUI_DOC, "demo_image",    "visible", preview_visible)
        GUI.set(GUI_DOC, "demo_circle",   "visible", preview_visible)
        GUI.set(GUI_DOC, "click_log",     "text",
            "preview -> " .. (preview_visible and "shown" or "hidden"))
    elseif node_id == "deco_toggle" then
        -- Flip the checked state on click.
        local on_s = GUI.get(GUI_DOC, "deco_toggle", "checked") or "false"
        GUI.set(GUI_DOC, "deco_toggle", "checked", on_s ~= "true")
        GUI.set(GUI_DOC, "click_log", "text",
            "toggle -> " .. (on_s ~= "true" and "ON" or "OFF"))
    elseif node_id == "brightness_slider" then
        -- No drag tracking yet; clicking the slider bumps it by 0.1 and
        -- wraps back to 0 past the end.
        local br_s = GUI.get(GUI_DOC, "brightness_slider", "value") or "0.5"
        local br = tonumber(br_s) or 0.5
        br = br + 0.1
        if br > 1.0 then br = 0.0 end
        GUI.set(GUI_DOC, "brightness_slider", "value", br)
        -- Colour properties take a JSON array; gui_set stringifies its value
        -- and every runtime parses it back as JSON before applying.
        GUI.set(GUI_DOC, "preview_panel", "bg_color",
            string.format("[%.3f, %.3f, %.3f, 1.0]", br * 0.9, br * 0.6, 1.0 - br * 0.6))
        GUI.set(GUI_DOC, "click_log", "text",
            string.format("slider -> %.2f", br))
    end
end
