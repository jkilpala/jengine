-- Placeholder scene script — used by any feature scene that hasn't
-- been built yet. Only purpose: handle the hub-return key bindings.

local SCENE_PATH = "scenes/placeholder.scene"
local nav = require("lua/nav")

function init(entity)
    log("[placeholder] press H or Esc to return to hub")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
