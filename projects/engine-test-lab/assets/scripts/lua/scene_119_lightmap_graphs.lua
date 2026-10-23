-- Scene 119: a lightmap drawn by a graph material, beside a plain one.
-- The rings run on the graph's own clock; the script only navigates.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/119-lightmap-graphs.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
