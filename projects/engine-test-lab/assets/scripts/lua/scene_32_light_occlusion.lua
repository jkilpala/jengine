-- Scene 32: Light Occlusion
-- Demonstrates dynamic light blocking / cast shadows:
--   * soft penumbra (point light with shadow_softness) vs hard shadow
--   * spot light occlusion
--   * occluder shapes: box, sprite-derived bounds, and polygon
-- Static visualization — nav passthrough only.

script = { tick_rate = 10 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/32-light-occlusion.scene"

function init(entity)
    log("[32-light-occlusion] ready - compare soft / hard / spot / polygon occluders")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
