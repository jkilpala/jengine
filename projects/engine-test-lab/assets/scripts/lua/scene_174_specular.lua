-- Scene 174: moves the lamp. The highlights are the engine's: `specular`
-- and `shininess` on each material, the plate's masked by its normal map.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/174-specular.scene"
local lamp, dot
local t = 0
local follow = false
local last_x, last_y

function init(entity)
    lamp = Entity.find_by_name("Lamp")
    dot = Entity.find_by_name("LampDot")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    local mx, my = Input.get_mouse_world_position()
    if last_x and (math.abs(mx - last_x) + math.abs(my - last_y)) > 1 then follow = true end
    last_x, last_y = mx, my
    local x, y
    if follow then
        x, y = mx, my
    else
        x, y = math.cos(t * 0.6) * 480, -20 + math.sin(t * 1.2) * 120
    end
    if lamp then Transform.set_position(lamp, x, y) end
    if dot then Transform.set_position(dot, x, y) end
end
