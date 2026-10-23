-- Scene 111: an `Unlit` sprite sorts with lit ones by z_order.
--
-- The far tower's windows are unlit (z 1); the near building is lit (z 2)
-- and must hide them wherever it slides. Until Sep 2026 unlit things were
-- drawn after the lighting composite, above every lit sprite, so the
-- windows showed through the building in front.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/111-unlit-sorting.scene"

local building, sign, sign_text, lamp
local t = 0
local slide = 0      -- 0 = covering the tower's right half, 1 = moved right
local target = 0

function init(entity)
    building = Entity.find_by_name("NearBuilding")
    sign = Entity.find_by_name("Sign")
    sign_text = Entity.find_by_name("SignText")
    lamp = Entity.find_by_name("Lamp")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if Input.is_key_just_pressed("Space") then
        target = 1 - target
    end
    slide = slide + (target - slide) * math.min(1, dt * 4)
    local x = 90 - slide * 260
    if building then Transform.set_position(building, x, -120) end
    if sign then Transform.set_position(sign, x, 20) end
    if sign_text then Transform.set_position(sign_text, x, 38) end
    if lamp then Transform.set_position(lamp, math.sin(t * 0.6) * 360 - 60, 60) end
end
