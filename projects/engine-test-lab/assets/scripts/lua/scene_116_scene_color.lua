-- Scene 116: the shader graph's Scene Color node - three squares that draw
-- what is already behind them, moved. Everything is in the graphs; the
-- script only navigates.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/116-scene-color.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
