-- 160 Shape Shifter: the big shape's `path_morph` eases its own path into
-- another `vector_path`'s shape - `Morph.to(e, name, seconds)` - colour and
-- all, cut into evenly spaced points and lined up so it never twists. This
-- script only says which shape is next.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/160-shape-shifter.scene"
local NAMES = { "Circle", "Star", "Heart", "Bird", "Crescent", "Drop", "Hexagon" }
local status
local captions = true
local shifter, pick
local at = 1
local wait = 1.4
local paused = false

local function become(i)
    at = i
    Morph.to(shifter, NAMES[i], 1.1)
    Transform.set_position(pick, -480 + (i - 1) * 160, -270)
    UI.set_text(status, NAMES[i])
end

function init(entity)
    status = Entity.find_by_name("Status")
    shifter = Entity.find_by_name("Shifter")
    pick = Entity.find_by_name("Pick")
    UI.set_text(status, NAMES[1])
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("Space") then paused = not paused end
    if Input.is_mouse_left_just_pressed() then
        local mx, my = Input.get_mouse_world_position()
        if my < -200 then
            local i = math.floor((mx + 480) / 160 + 0.5) + 1
            if i >= 1 and i <= #NAMES then
                become(i)
                wait = 2.6
            end
        end
    end
    if not paused then
        wait = wait - dt
        if wait <= 0 then
            become(at % #NAMES + 1)
            wait = 2.2
        end
    end
end
