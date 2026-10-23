-- Scene 287: `path_morph`, `Morph.to` / `Morph.set` and `on_morph_end`.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/287-path-morph.scene"
local NAMES = { "Square", "Triangle", "Circle", "Star" }
local EASES = { "in_out", "linear", "out", "back" }
local line, shape
local ease, duration, ends = 1, 1.0, 0
local current = "Square"

function init(entity)
    line = Entity.find_by_name("Line0")
    shape = entity
end

function on_morph_end(entity)
    ends = ends + 1
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    for i, n in ipairs(NAMES) do
        if Input.is_key_just_pressed(tostring(i)) then
            current = n
            Morph.to(shape, n, duration)
        end
    end
    if Input.is_key_just_pressed("E") then
        ease = ease % #EASES + 1
        Morph.set(shape, "ease", EASES[ease])
    end
    if Input.is_key_just_pressed("Up") then duration = math.min(4, duration + 0.5) end
    if Input.is_key_just_pressed("Down") then duration = math.max(0.25, duration - 0.25) end
    UI.set_text(line, "into " .. current .. "   ease " .. EASES[ease] .. "   " .. duration .. " s   finished " .. ends .. " times")
end
