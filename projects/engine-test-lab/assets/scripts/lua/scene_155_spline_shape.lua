-- Scene 155: the shapes, their tiling and their colliders are the engine's
-- (`spline_shape`); R reloads to drop the balls again.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/155-spline-shape.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("R") then Scene.load(SCENE_PATH) end
end
