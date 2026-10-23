-- Scene 46 - Sort Layers
--
-- A `.sortlayers` asset is a list of named buckets, each with a base_z. The
-- scene points at one with a top-level "sort_layers" key and every sprite names
-- its bucket in `sort_layer`. Effective depth is base_z + z_order, so the
-- bucket dominates and z_order only orders sprites inside it.
--
-- The right-hand column is the case worth checking: a background sprite with
-- z_order 900 still draws *behind* a foreground sprite with z_order -50,
-- because -1000 + 900 < 1000 - 50. If layers were ignored the stack would
-- invert, which is exactly the regression this scene catches.
--
-- Nothing here is script-driven.

script = { tick_rate = 5 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/46-sort-layers.scene"

function init(entity)
    local readout = Entity.find_by_name("Readout")
    if readout then
        Text.set(readout,
            "effective depth = base_z + z_order   |   orange (foreground) must be on top in both columns")
    end
    log("[scene_46] ready - check both columns stack orange > green > blue")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
