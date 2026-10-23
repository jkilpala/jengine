-- Scene 27: Triggers
-- Nav-only script. The actual trigger demo lives in `trigger_zone.lua`,
-- which is attached to each `TriggerArea2D` entity and receives
-- `on_trigger_enter` / `on_trigger_exit` callbacks from the engine.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/27-triggers.scene"

function init(entity)
    log("[27-triggers] ready - 3 trigger zones, 1 wandering probe")
    log("[27-triggers] enter/exit events come from trigger_zone.lua")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
end