-- Scene 28: Spotlights & Lightmap
-- Three SpotLight2D cones (R/G/B) shine down on a 9x2 checker grid.
-- AmbientLight raises the floor brightness; a wandering PointLight2D
-- (driven by Oscillator) traverses the grid for variety.
-- LightmapComponent is serialised but not yet integrated with the
-- renderer — left in to document the format.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/28-spotlights.scene"

function init(entity)
    log("[28-spotlights] ready - 3 spotlights + ambient + wandering point light")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
