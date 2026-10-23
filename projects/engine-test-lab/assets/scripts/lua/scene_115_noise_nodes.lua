-- Scene 115: the shader graph's Noise and Voronoi nodes, each alone on a
-- swatch. Everything is in the graphs; the script only navigates.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/115-noise-nodes.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
