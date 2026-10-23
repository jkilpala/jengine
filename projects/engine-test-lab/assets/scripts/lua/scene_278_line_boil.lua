-- Scene 278: the `boil` post effect over ruled lines and world text.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/278-line-boil.scene"
local RATES = { 3, 8, 16 }
local line
local on, rate, amount = true, 8, 2.0

local function apply()
    PostProcess.set_enabled("boil", on)
    PostProcess.set_float("boil", "rate", rate)
    PostProcess.set_float("boil", "amount", amount)
    UI.set_text(line, on and ("boil " .. rate .. " a second, " .. amount .. " px") or "boil off")
end

function init(entity)
    line = Entity.find_by_name("Line0")
    apply()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local changed = false
    if Input.is_key_just_pressed("B") then on = not on; changed = true end
    for k = 1, 3 do
        if Input.is_key_just_pressed(tostring(k)) then rate = RATES[k]; on = true; changed = true end
    end
    if Input.is_key_just_pressed("Up") then amount = math.min(6, amount + 0.5); changed = true end
    if Input.is_key_just_pressed("Down") then amount = math.max(0.5, amount - 0.5); changed = true end
    if changed then apply() end
end
