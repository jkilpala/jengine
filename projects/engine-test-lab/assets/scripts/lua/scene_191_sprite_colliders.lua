-- Scene 191: a sprite_collider writes the outline of its sprite's opaque
-- pixels into the entity's collider; the shapes on the right keep their
-- authored boxes. Space reloads the scene to drop them again.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/191-sprite-colliders.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then
        Scene.load(SCENE_PATH)
    end
end
