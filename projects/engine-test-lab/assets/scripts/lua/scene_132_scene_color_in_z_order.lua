-- Scene 132: a Scene Color band at z 10, a ball behind it and a ball in
-- front of it. Nothing to drive: the oscillators move the balls.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/132-scene-color-in-z-order.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
