-- Scene 179: light layers. The figures are on layer 2 (`light_layers`),
-- the wall on layer 1; the torch's `light_mask` says which it lights, set
-- with Lighting.set_mask. No bit operators: Luau and Lua 5.3 spell them
-- differently, so the mask is added up.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/179-light-layers.scene"
local torch, status
local wall, figures = false, true

function init(entity)
    torch = Entity.find_by_name("Torch")
    status = Entity.find_by_name("Status")
end

local function apply()
    local mask = (wall and 1 or 0) + (figures and 2 or 0)
    if torch then Lighting.set_mask(torch, mask) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local mx, my = Input.get_mouse_world_position()
    if torch and mx then Transform.set_position(torch, mx, my) end
    if Input.is_key_just_pressed("1") then wall = not wall; apply() end
    if Input.is_key_just_pressed("2") then figures = not figures; apply() end
    if status then
        UI.set_text(status, "torch lights: " .. (wall and "the wall  " or "") .. (figures and "the figures" or "")
            .. ((not wall and not figures) and "nothing" or ""))
    end
end
