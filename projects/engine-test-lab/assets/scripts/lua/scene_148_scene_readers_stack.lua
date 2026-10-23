-- Scene 148: the ordering is the engine's (`SceneReaderPlan`); this only
-- navigates.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/148-scene-readers-stack.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
