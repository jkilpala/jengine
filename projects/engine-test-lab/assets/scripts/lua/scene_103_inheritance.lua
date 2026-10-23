-- Scene 103: script inheritance, `script = { extends = "..." }` and `super`.
--
-- Three boxes, three scripts, one module chain under lua/inherit/. Until
-- Sep 2026 only native honoured `extends`: in the editor and on the web all
-- three boxes stood still and grey. Compare native / editor / web - every
-- box spins, every box pulses once a second (an inherited `on_ping`), the
-- middle one bobs, and the right one is orange.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/103-script-inheritance.scene"

local since = 0

function update(entity, dt)
    nav.tick(SCENE_PATH)
    since = since + dt
    if since >= 1 then
        since = since - 1
        Event.send("ping", entity)
    end
end
