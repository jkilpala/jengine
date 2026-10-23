-- Scene 159: the cookies are drawn by the engine (`cookie` on a light); this
-- moves the point light to the mouse and turns it.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/159-light-cookies.scene"
local lamp
local turning = true
local angle = 0

function init(entity)
    lamp = Entity.find_by_name("Lamp")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then turning = not turning end
    if lamp == nil then return end
    if turning then angle = angle + dt * 0.6 end
    local x, y = Input.get_mouse_world_position()
    if x > -600 and x < 0 then Transform.set_position(lamp, x, y) end
    Transform.set_rotation(lamp, angle)
end
