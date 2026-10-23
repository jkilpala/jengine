-- Scene 142: the masks are the engine's (`sprite_mask`, `masked`); this
-- only navigates.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/142-sprite-masks.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
