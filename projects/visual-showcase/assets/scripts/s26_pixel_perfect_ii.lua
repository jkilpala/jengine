-- 26 Pixel Perfect II: scene 12's world with a pixel-perfect camera. The
-- drawing is the engine's (`pixel_size` on the camera); this pans, and M
-- goes round the three ways to draw it: at the art's resolution, at the
-- screen's with `pixelate` after, and at the screen's with neither.
-- S speeds the pan up; C lays the CRT over it.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/26-pixel-perfect-ii.scene"
local PIXEL = 4.0          -- world units per art pixel (tools/gen_scenes.py: PP)

local PAN = 44.0           -- world units either side
local speeds = { 0.25, 1.2 }
local speed = 1

local captions = true
local post_on = nav.post_all_on(SCENE)
local modes = { "art resolution", "pixelate", "screen resolution" }
local mode = 1
local crt_on = false
local status, cam
local t = 0.0

local function refresh_status()
    if status then
        UI.set_text(status, modes[mode] .. "   pan " .. (speed == 1 and "slow" or "fast")
            .. "   crt " .. (crt_on and "on" or "off"))
    end
end

local function apply_mode()
    Camera.set_pixel_size(mode == 1 and PIXEL or 0)
    PostProcess.set_enabled("pixelate", mode == 2)
    refresh_status()
end

function init(entity)
    status = Entity.find_by_name("Status")
    cam = Entity.find_by_name("Camera")
    apply_mode()
end

function update(entity, dt)
    nav.tick(SCENE)
    nav.post_keys(post_on)
    if Input.is_key_just_pressed("Tab") then
        captions = not captions
        for _, name in ipairs({ "Title", "Hint", "Status" }) do
            local e = Entity.find_by_name(name)
            if e then UI.set_visible(e, captions) end
        end
    end
    if Input.is_key_just_pressed("M") then
        mode = mode % #modes + 1
        apply_mode()
    end
    if Input.is_key_just_pressed("S") then
        speed = speed % #speeds + 1
        refresh_status()
    end
    if Input.is_key_just_pressed("C") then
        crt_on = not crt_on
        PostProcess.set_enabled("crt", crt_on)
        refresh_status()
    end
    t = t + dt * speeds[speed]
    if cam then Transform.set_position(cam, math.sin(t) * PAN, 0.0) end
end
