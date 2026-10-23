-- Scene 123: texture filters. The same two images, sampled nearest (the
-- project default) and linear (an override for sprites/filter_linear/ in
-- project.jengine). Nothing to drive; this only runs the scene navigation.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/123-texture-filters.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
