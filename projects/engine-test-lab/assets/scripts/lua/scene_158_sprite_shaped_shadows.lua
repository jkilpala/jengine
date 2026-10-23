-- Scene 158: the shadows' shapes are traced from the sprites' pixels by the
-- engine (`light_occluder.from_alpha`); this only moves the light.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/158-sprite-shaped-shadows.scene"
local lamp

function init(entity)
    lamp = Entity.find_by_name("Lamp")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if lamp then
        local x, y = Input.get_mouse_world_position()
        Transform.set_position(lamp, x, y)
    end
end
