-- Scene 218: an entity-UI menu as a screen reader reads it (the
-- `accessible` component), and Accessibility.announce on N.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/218-screen-readers.scene"
local e = {}
local names = {}
local announced = 0
local last = "nothing yet"

function init(entity)
    e.Line0 = Entity.find_by_name("Line0")
    e.Line1 = Entity.find_by_name("Line1")
    -- A screen reader's press arrives as any click does: `ui_click`.
    Event.on("ui_click", entity)
    for _, n in ipairs({ "Play", "Continue", "Credits", "Options" }) do
        local b = Entity.find_by_name("Btn" .. n)
        if b then names[b] = n end
    end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("N") then
        announced = announced + 1
        Accessibility.announce("Announcement " .. announced)
    end
    UI.set_text(e.Line0, "announcements made: " .. announced)
    UI.set_text(e.Line1, "last pressed: " .. last)
end

function on_ui_click(entity, data)
    if data ~= nil and names[data.button] then
        last = names[data.button]
        Accessibility.announce(last .. " pressed")
    end
end
