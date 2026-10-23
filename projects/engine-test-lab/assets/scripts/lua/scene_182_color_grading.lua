-- Scene 182: colour look-up tables (`color_lut`) and film grain. The
-- strips are named by the thumbnails below the card, which is what loads
-- them; this switches between them with PostProcess.set_lut.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/182-color-grading.scene"
local LUTS = { "neutral", "warm", "night", "noir" }
local which, strength, grain = 1, 1.0, false
local status

function init(entity)
    status = Entity.find_by_name("Status")
end

local function apply()
    PostProcess.set_lut("luts/" .. LUTS[which] .. ".png", strength)
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    for i = 1, 4 do
        if Input.is_key_just_pressed(tostring(i)) then which = i; apply() end
    end
    if Input.is_key_just_pressed("Up") then strength = math.min(1, strength + 0.25); apply() end
    if Input.is_key_just_pressed("Down") then strength = math.max(0, strength - 0.25); apply() end
    if Input.is_key_just_pressed("G") then
        grain = not grain
        PostProcess.set_enabled("film_grain", grain)
    end
    if status then
        UI.set_text(status, LUTS[which] .. "   strength " .. math.floor(strength * 100 + 0.5) .. "%   grain " .. (grain and "on" or "off"))
    end
end
