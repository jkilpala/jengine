-- Scene 192: an fxaa post effect smooths the stairs on turned edges.
-- F switches it (PostProcess.set_enabled); the shapes turn slowly so the
-- stairs crawl when it is off.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/192-anti-aliasing.scene"
local square, star, status
local spokes = {}
local on, t = true, 0

function init(entity)
    square = Entity.find_by_name("Square")
    star = Entity.find_by_name("Star")
    status = Entity.find_by_name("Status")
    for k = 0, 11 do spokes[k] = Entity.find_by_name("Spoke" .. k) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if Input.is_key_just_pressed("F") then
        on = not on
        PostProcess.set_enabled("fxaa", on)
    end
    Transform.set_rotation(square, t * 0.15)
    Transform.set_rotation(star, -t * 0.1)
    for k = 0, 11 do
        if spokes[k] then Transform.set_rotation(spokes[k], k * math.pi / 12 + t * 0.08) end
    end
    if status then UI.set_text(status, "FXAA " .. (on and "on" or "off")) end
end
