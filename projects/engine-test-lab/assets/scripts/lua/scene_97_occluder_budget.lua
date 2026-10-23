-- Scene 97 - the occluder pool, and the budget it replaced.
--
-- Twenty `light_occluder` boxes under one light, numbered. **All twenty cast.**
--
-- Until Sep 2026 they did not. The lighting pass kept `MAX_OCCLUDER_SEGMENTS`
-- (64) edges for the whole screen and bound that same set for *every* light,
-- dropping the rest in silence. There is no way to author a bare edge - an
-- occluder is a closed outline, so a box is four segments - so sixty-four was
-- **sixteen boxes**, and four of these pillars cast nothing, nothing was
-- logged, and which four depended on ECS query order. Past the budget it did
-- not look like a limit; it looked like shadows that worked in one room and
-- stopped in the next.
--
-- Now each light is handed only the edges within its reach, from a pool of
-- `OCCLUDER_POOL` (448) counted per light, nearest first; when many lights see
-- little geometry they share one list instead (`pack_light_occluders`). The
-- cull is exact - an edge out of a light's reach cannot block it - so this
-- scene's shadows are what testing every edge would give.
--
-- The green square on the right is `enabled: false`: `gather_lights` skips it
-- outright. A lantern-lit game was built against the old limit and spends
-- the pool lantern-first.

script = { tick_rate = 10 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/97-occluder-budget.scene"

function init(entity)
    log("[97-occluder-budget] 20 boxes authored = 80 edges, all in one light's reach, pool of 448")
    log("[97-occluder-budget] count the shadows: all twenty pillars cast")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
