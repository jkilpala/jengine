-- Scene 102: where a particle starts.
--
-- A particle emitter's `emit_shape` (Sep 2026): `point`, `line`, `circle`
-- (optionally only its `edge`) and `rect`. Before it every particle started
-- at the emitter's exact position, so rain across a sky was a row of
-- emitters - one game's street had nine. Nothing here needs the script but
-- the hub keys.
--
-- Compare native / editor / web: the snow must fall from the whole top
-- edge, the ring must be a ring, and the fog must fill its rectangle.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/102-emit-shapes.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
