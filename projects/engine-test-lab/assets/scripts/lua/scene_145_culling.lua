-- Scene 145: the layers are the engine's (`render_layers`, `cull_mask`);
-- this only navigates.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/145-culling-masks.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
