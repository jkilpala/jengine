-- Scene 234: `motion_blur` and its velocity pass. The camera pans; one
-- block rides with it, one crosses the screen; the posts stand still.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/234-motion-blur.scene"
local e = {}
local t = 0
local on = true
local cam_x = 0

function init(entity)
    for _, n in ipairs({ "Camera", "Rider", "Crosser", "Line0" }) do
        e[n] = Entity.find_by_name(n)
    end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if Input.is_key_just_pressed("B") then
        on = not on
        PostProcess.set_enabled("motion_blur", on)
    end
    -- The camera sweeps back and forth over the posts.
    cam_x = math.sin(t * 0.8) * 900
    Transform.set_position(e.Camera, cam_x, 0)
    Transform.set_position(e.Rider, cam_x, 80)
    local cx = cam_x + math.sin(t * 2.6) * 420
    Transform.set_position(e.Crosser, cx, 200)
    UI.set_text(e.Line0, on and "motion blur on" or "motion blur off")
end
