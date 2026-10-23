-- Scene 140: the camera pans; P switches its pixel size between 6 and off
-- (`Camera.set_pixel_size`), S the pan's speed.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/140-pixel-perfect-camera.scene"
local cam, status
local pixels = true
local fast = false
local t = 0.0

local function show()
    if status then
        UI.set_prop(status, "text", (pixels and "pixel size 6" or "screen resolution") .. (fast and "   fast pan" or ""))
    end
end

function init(entity)
    cam = Entity.find_by_name("Camera")
    status = Entity.find_by_name("Status")
    show()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("P") then
        pixels = not pixels
        Camera.set_pixel_size(pixels and 6 or 0)
        show()
    end
    if Input.is_key_just_pressed("S") then
        fast = not fast
        show()
    end
    t = t + dt * (fast and 1.5 or 0.3)
    if cam then Transform.set_position(cam, math.sin(t) * 60, math.cos(t * 0.7) * 20) end
end
