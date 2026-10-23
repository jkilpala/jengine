-- Scene 282: `sprite_outline` and `Outline.set`.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/282-sprite-outline.scene"
local NAMES = { "Steady", "Pulsing", "Cell" }
local COLOURS = { { 1.0, 0.8, 0.2, 1.0 }, { 0.3, 0.8, 1.0, 1.0 }, { 1.0, 1.0, 1.0, 0.8 } }
local line
local ents = {}
local width, glow, colour, on, turning, angle = 3, 8, 1, true, false, 0

local function apply()
    for _, e in ipairs(ents) do
        Outline.set(e, "enabled", on)
        Outline.set(e, "width", width)
        Outline.set(e, "glow", glow)
    end
    Outline.set(ents[1], "color", COLOURS[colour])
    UI.set_text(line, "width " .. width .. "   glow " .. glow .. (on and "" or "   off"))
end

function init(entity)
    line = Entity.find_by_name("Line0")
    for _, n in ipairs(NAMES) do table.insert(ents, Entity.find_by_name(n)) end
    apply()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local changed = false
    if Input.is_key_just_pressed("Up") then width = math.min(12, width + 1); changed = true end
    if Input.is_key_just_pressed("Down") then width = math.max(0, width - 1); changed = true end
    if Input.is_key_just_pressed("Right") then glow = math.min(32, glow + 2); changed = true end
    if Input.is_key_just_pressed("Left") then glow = math.max(0, glow - 2); changed = true end
    if Input.is_key_just_pressed("C") then colour = colour % #COLOURS + 1; changed = true end
    if Input.is_key_just_pressed("O") then on = not on; changed = true end
    if Input.is_key_just_pressed("R") then turning = not turning end
    if turning then
        angle = angle + dt * 0.6
        for _, e in ipairs(ents) do Transform.set_rotation(e, angle) end
    end
    if changed then apply() end
end
