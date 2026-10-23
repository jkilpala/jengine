-- Scene 130: the camera pans and zooms, so a reflection found with
-- WorldToScreen has to follow it - a mirror fixed to a fraction of the
-- screen would slide off its objects.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/130-graph-position.scene"

local cam
local t = 0.0

function init(entity)
    cam = Entity.find_by_name("Camera")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if cam then Transform.set_position(cam, math.sin(t * 0.3) * 120.0, math.sin(t * 0.2) * 30.0) end
    Camera.set_zoom(1.0 + 0.15 * math.sin(t * 0.25))
end
