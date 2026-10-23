-- Scene 272: the `paint` (Kuwahara) and `halftone` post effects over a
-- test card. 1-4 pick the look; Up / Down change the paint's radius or the
-- halftone's cell.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/272-paint-and-halftone.scene"
local NAMES = { "none", "paint", "halftone CMYK", "halftone mono" }
local line
local mode = 2
local radius, cell = 5, 8

local function apply()
    PostProcess.set_enabled("paint", mode == 2)
    PostProcess.set_enabled("halftone", mode >= 3)
    PostProcess.set_float("paint", "radius", radius)
    PostProcess.set_float("halftone", "cell", cell)
    if mode == 3 then PostProcess.set_float("halftone", "mode", 1) end
    if mode == 4 then PostProcess.set_float("halftone", "mode", 0) end
    local extra = ""
    if mode == 2 then extra = "   radius " .. radius end
    if mode >= 3 then extra = "   cell " .. cell .. " px" end
    UI.set_text(line, NAMES[mode] .. extra)
end

function init(entity)
    line = Entity.find_by_name("Line0")
    apply()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local changed = false
    for k = 1, 4 do
        if Input.is_key_just_pressed(tostring(k)) then mode = k; changed = true end
    end
    local d = 0
    if Input.is_key_just_pressed("Up") then d = 1 end
    if Input.is_key_just_pressed("Down") then d = -1 end
    if d ~= 0 then
        if mode == 2 then radius = math.max(1, math.min(16, radius + d)) end
        if mode >= 3 then cell = math.max(2, math.min(32, cell + d)) end
        changed = true
    end
    if changed then apply() end
end
