-- Scene 68 - Graph commands and triggers.
--
-- Every effect in this scene is a `.behavior` graph (behaviors/cmd-*.behavior);
-- this script only handles navigation. Until Sep 2026 none of what the scene
-- shows happened in any runtime (BUGS #93):
--
--   * `SpawnEntity` made an entity with a Name and a Transform, not the
--     prefab it named - nothing visible appeared.
--   * `PlaySound`, `ChangeScene`, `PlayAnimation` and `StopAnimation` were
--     recorded "for the host to forward", and no host did.
--   * `OnKey`, `OnMouseClick` and `OnTimer` had no caller at all.
--   * In the editor and on the web, a despawned entity kept its graph
--     forever and `OnDespawn` never fired.
--
-- What to check, in every runtime:
--   S       a gold coin appears at the teal box, bounces, and a blip plays
--   A / X   the orange box starts spinning / the purple one stops
--   click   a small pink spark appears under the pointer and flies off
--   clock   the hand steps 30 degrees clockwise once a second
--   4s      the red box vanishes with a blip; the grey box then turns green
--   L       the scene reloads: coins and sparks gone, red box back, grey grey

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/68-graph-commands.scene"

function init(entity)
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
