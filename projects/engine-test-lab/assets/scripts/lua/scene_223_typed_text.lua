-- Scene 223 - typed text
--
-- Input.get_typed() is the text typed this frame, as the keyboard layout
-- made it; Input.get_just_pressed() is which physical keys went down. A
-- command that is a character (a roguelike's `?`, `<`, `>`) reads the first: on
-- a Finnish keyboard `?` is shift and the key beside 0, a key no code calls
-- `Slash`. A roguelike found it - its help never opened there.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/223-typed-text.scene"

local typed_ui, keys_ui, help_ui
local typed, keys = "", ""

function init(entity)
    typed_ui = Entity.find_by_name("Typed")
    keys_ui = Entity.find_by_name("Keys")
    help_ui = Entity.find_by_name("Help")
    typed, keys = "", ""
end

function update(entity, dt)
    local now = Input.get_typed()
    if now ~= "" then
        typed = (typed .. now):sub(-24)
        if now:find("?", 1, true) then UI.set_text(help_ui, "'?' typed - a help screen would open here") end
    end
    for _, k in ipairs(Input.get_just_pressed()) do
        keys = (keys .. " " .. k):sub(-40)
    end
    if Input.is_key_just_pressed("Escape") then typed, keys = "", "" UI.set_text(help_ui, "") end
    UI.set_text(typed_ui, typed)
    UI.set_text(keys_ui, keys)
    nav.tick(SCENE_PATH)
end
