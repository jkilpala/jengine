-- Scene 178: a shader graph's VertexOffset. The swaying is all in the
-- materials; this only sets how far the grass leans - a material parameter
-- the vertex stage reads.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/178-vertex-offset.scene"
local blades = {}
local sway = 18
local status

function init(entity)
    status = Entity.find_by_name("Status")
    for i = 1, 14 do blades[i] = Entity.find_by_name("Blade" .. i) end
end

local function apply()
    for _, b in ipairs(blades) do
        if b then Material.set_float(b, "sway", sway) end
    end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Up") then sway = math.min(48, sway + 6); apply() end
    if Input.is_key_just_pressed("Down") then sway = math.max(0, sway - 6); apply() end
    if status then UI.set_text(status, "sway: " .. sway .. " world units at the tip") end
end
