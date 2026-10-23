-- Scene 265: `ui_draggable` / `ui_drop_slot`. Items in slots dragged with
-- the mouse or carried by focus; D sends the green one round the open slots
-- with `UI.drop`, by the same rules.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/265-drag-and-drop.scene"
local NAMES = { "Red", "Red2", "Blue", "Green" }
local line
local next_slot = 1

function init(entity)
    line = Entity.find_by_name("Line0")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("D") then
        UI.drop(Entity.find_by_name("Green"), "Slot" .. next_slot)
        next_slot = next_slot % 5 + 1
    end
    local parts = {}
    for _, n in ipairs(NAMES) do
        local e = Entity.find_by_name(n)
        local slot = Entity.get_custom(e, "Drag", "slot") or "?"
        local count = Entity.get_custom(e, "Drag", "count") or 0
        local held = Entity.get_custom(e, "Drag", "held") == true
        parts[#parts + 1] = n .. " " .. tostring(slot) .. " x" .. math.floor(count + 0.5) .. (held and " (held)" or "")
    end
    UI.set_text(line, table.concat(parts, "    "))
end
