-- Scene 147: the masks are the engine's (`sprite_mask` with a `Texture`
-- shape, `masked`); this only navigates.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/147-texture-masks.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
