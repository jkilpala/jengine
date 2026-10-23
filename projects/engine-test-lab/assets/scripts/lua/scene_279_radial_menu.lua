-- Scene 279: `ui_radial_menu`, this script on the wheel itself (it is
-- what `on_radial_pick` calls). Q opens and closes it; left still, it aims
-- itself round with `Radial.set(wheel, "aim", degrees)`.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/279-radial-menu.scene"
local line, wheel
local open = true
local t, idle = 0, 99
local last_mx, last_my
local aiming = false
local picks = 0

function init(entity)
    wheel = entity
    line = Entity.find_by_name("Line0")
end

function on_radial_pick(entity)
    picks = picks + 1
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if Input.is_key_just_pressed("Q") then
        open = not open
        Radial.set(wheel, "open", open)
    end
    local mx, my = Input.get_mouse_position()
    if last_mx ~= nil and (math.abs(mx - last_mx) > 1 or math.abs(my - last_my) > 1) then idle = 0 else idle = idle + dt end
    last_mx, last_my = mx, my
    if idle > 3 then
        aiming = true
        Radial.set(wheel, "aim", 90 - t * 60)
    elseif aiming then
        aiming = false
        Radial.set(wheel, "aim", nil)
    end
    local h = Entity.get_custom(wheel, "RadialMenu", "hovered")
    local label = Entity.get_custom(wheel, "RadialMenu", "label") or ""
    UI.set_text(line, "hovered " .. ((h ~= nil and h >= 0) and tostring(math.floor(h)) or "-")
        .. "   picks " .. picks .. (label ~= "" and ("   last " .. label) or "") .. (aiming and "   (aiming itself)" or ""))
end
