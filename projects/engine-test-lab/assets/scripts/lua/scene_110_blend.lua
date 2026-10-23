-- Scene 110: sprite blend modes, and particles that blend, curve and stretch.
--
-- Everything here is authored in the scene; the script only navigates.
-- Until Sep 2026 all five discs drew identically (no renderer read
-- `blend_mode`) and every particle was alpha-blended and square.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/110-blend-and-particles.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
