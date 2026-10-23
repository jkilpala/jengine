-- Scene 292: a tileset's `margin` and `spacing` - two linear-filtered maps of
-- the same tiles, one packed edge to edge (seams bleed) and one with gutters.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/292-tile-spacing.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
