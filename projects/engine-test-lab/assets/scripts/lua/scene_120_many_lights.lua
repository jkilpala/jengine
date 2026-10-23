-- Scene 120: more lights than a frame draws, and a camera that pans over
-- them. The pass draws the ones in view; the count line says how many there
-- are in all.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/120-many-lights.scene"

local camera, count
local t = 0.0

function init(entity)
    camera = Entity.find_by_name("Camera")
    count = Entity.find_by_name("Count")
    if count then Text.set(count, "400 point lights in the world; a frame draws those that reach the view (1024 at most)") end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if camera then Transform.set_position(camera, math.sin(t * 0.15) * 1350, 0) end
end
