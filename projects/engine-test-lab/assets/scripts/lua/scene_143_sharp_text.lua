-- Scene 143: the text is the engine's (a distance-field font); this only
-- breathes one label's size.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/143-sharp-text.scene"
local grower
local t = 0.0

function init(entity)
    grower = Entity.find_by_name("Grower")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if grower then
        local s = 3 + 2 * math.sin(t * 0.8)
        Transform.set_scale(grower, s, s)
    end
end
