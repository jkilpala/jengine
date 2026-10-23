-- Scene 31: Shadows
-- Demonstrates ShadowCaster: skew + coverage-weighted multiply blend.
-- Static visualization — nav passthrough only.

script = { tick_rate = 10 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/31-shadows.scene"

function init(entity)
    log("[31-shadows] ready - compare baseline / skewed / stretched shadows over the bright backdrop")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
