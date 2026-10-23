-- Scene 205: text fields. Entity-UI ui_text_input fields with a caret, a
-- selection, the clipboard, a limit and a password mask (ui::text_edit).
-- The status lines read UI.get_prop: focused, caret, selected, submitted.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/205-text-fields.scene"
local NAMES = { "NameField", "PassField", "MsgField" }
local fields, status = {}, {}
local log = {}

function init(entity)
    for i, n in ipairs(NAMES) do fields[i] = Entity.find_by_name(n) end
    for k = 0, 3 do status[k + 1] = Entity.find_by_name("Status" .. k) end
    UI.set_prop(fields[1], "focused", true)
end

function update(entity, dt)
    -- The hub keys only when no field has the keyboard: a field consumes
    -- what is typed into it, so H and Esc here are letters and "leave".
    nav.tick(SCENE_PATH)
    local which = nil
    for i, f in ipairs(fields) do
        if UI.get_prop(f, "focused") then which = i end
        if UI.get_prop(f, "submitted") then
            local text = UI.get_prop(f, "text") or ""
            table.insert(log, 1, NAMES[i] .. ": " .. (i == 2 and string.rep("*", #text) or text))
            while #log > 3 do table.remove(log) end
        end
    end
    if which then
        local f = fields[which]
        UI.set_text(status[1], string.format("%s focused   caret %d   selected \"%s\"",
            NAMES[which], math.floor((UI.get_prop(f, "caret") or 0) + 0.5), UI.get_prop(f, "selected") or ""))
    else
        UI.set_text(status[1], "no field focused - click one (H / Esc now go to the hub)")
    end
    for k = 1, 3 do UI.set_text(status[k + 1], log[k] and ("submitted  " .. log[k]) or "") end
end
