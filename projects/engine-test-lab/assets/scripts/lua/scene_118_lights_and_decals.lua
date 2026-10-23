-- Scene 118: a light that casts no shadows, and multiply decals with soft
-- edges. Nothing moves; the script only navigates.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/118-lights-and-decals.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
