-- Scene 146: the fonts are assets (`fonts/bebasneue-sdf.font` was made by
-- `jengine font-sdf` from the bitmap beside it); this only navigates.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/146-sdf-custom-font.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
