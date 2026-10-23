-- Scene 280: the `lens_rain` post effect over a grid of squares.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/280-lens-rain.scene"
local line
local amount, size, mist, refract = 0.7, 12, false, true

local function apply()
    PostProcess.set_float("lens_rain", "amount", amount)
    PostProcess.set_float("lens_rain", "size", size)
    PostProcess.set_float("lens_rain", "mist", mist and 0.6 or 0)
    PostProcess.set_float("lens_rain", "refraction", refract and 1.5 or 0)
    UI.set_text(line, "amount " .. math.floor(amount * 100 + 0.5) .. "%   size " .. size .. " px"
        .. (mist and "   mist" or "") .. (refract and "" or "   refraction off"))
end

function init(entity)
    line = Entity.find_by_name("Line0")
    apply()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local changed = false
    if Input.is_key_just_pressed("Up") then amount = math.min(1, amount + 0.1); changed = true end
    if Input.is_key_just_pressed("Down") then amount = math.max(0, amount - 0.1); changed = true end
    if Input.is_key_just_pressed("Right") then size = math.min(40, size + 2); changed = true end
    if Input.is_key_just_pressed("Left") then size = math.max(4, size - 2); changed = true end
    if Input.is_key_just_pressed("M") then mist = not mist; changed = true end
    if Input.is_key_just_pressed("R") then refract = not refract; changed = true end
    if changed then apply() end
end
