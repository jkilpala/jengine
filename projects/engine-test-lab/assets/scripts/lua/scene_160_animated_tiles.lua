-- Scene 160: the tiles animate on their own (`animations` on the tileset).

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/160-animated-tiles.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
