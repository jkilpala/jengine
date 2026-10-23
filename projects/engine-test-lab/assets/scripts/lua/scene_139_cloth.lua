-- Scene 139: the cloth and the ropes are the engine's (`cloth`, `rope`);
-- this swings the pole, bobs the weight, and W switches the wind through
-- `Cloth.set`.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/139-cloth-and-rope.scene"
local pole, weight, flag, banner, status
local t = 0.0
local wind = true

function init(entity)
    pole = Entity.find_by_name("Pole")
    weight = Entity.find_by_name("Weight")
    flag = Entity.find_by_name("Flag")
    banner = Entity.find_by_name("Banner")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if pole then Transform.set_position(pole, -420 + 90 * math.sin(t * 0.8), -40) end
    if weight then Transform.set_position(weight, 320, 60 + 50 * math.sin(t * 1.3)) end
    if Input.is_key_just_pressed("W") then
        wind = not wind
        if flag then
            Cloth.set(flag, "wind", wind and { 900, 40 } or { 0, 0 })
            Cloth.set(flag, "wave", wind and 4000 or 0)
        end
        if banner then Cloth.set(banner, "wind", wind and { 300, 0 } or { 0, 0 }) end
        if status then UI.set_prop(status, "text", wind and "wind on" or "wind off") end
    end
end
