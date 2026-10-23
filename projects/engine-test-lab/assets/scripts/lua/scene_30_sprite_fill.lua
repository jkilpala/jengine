-- Scene 30: SpriteFill
-- Demonstrates Stretch / NineSlice / Tile fill modes on `SpriteComponent`.
-- Static visualization — nav passthrough only.

script = { tick_rate = 10 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/30-sprite-fill.scene"

function init(entity)
    log("[30-sprite-fill] ready - compare stretch / nine-slice / tile against the checker texture")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
