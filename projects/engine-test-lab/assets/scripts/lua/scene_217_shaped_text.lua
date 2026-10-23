-- Scene 217: shaped and bidirectional text from font files - nothing to
-- drive, the scene is the test.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/217-shaped-text.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
