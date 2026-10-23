-- 12 Pixel Perfect: pans the camera slowly, so you can see the pixelated
-- frame scroll smoothly without shimmering, and toggles the two effects the
-- scene is about - P the pixelate pass, C the CRT.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/12-pixel-perfect.scene"

local PAN = 44.0          -- world units either side
local PAN_SPEED = 0.25    -- radians per second

local captions = true
local post_on = nav.post_all_on(SCENE)
local pixelate_on, crt_on = true, false
local status, cam
local t = 0.0

local function refresh_status()
    if status then
        UI.set_text(status, "pixelate " .. (pixelate_on and "on" or "off") .. "   crt "
            .. (crt_on and "on" or "off"))
    end
end

function init(entity)
    status = Entity.find_by_name("Status")
    cam = Entity.find_by_name("Camera")
    refresh_status()
end

function update(entity, dt)
    nav.tick(SCENE)
    nav.post_keys(post_on)
    -- This scene's captions are entity UI (crisp over the pixelated world),
    -- so Tab hides them rather than fading world text.
    if Input.is_key_just_pressed("Tab") then
        captions = not captions
        for _, name in ipairs({ "Title", "Hint", "Status" }) do
            local e = Entity.find_by_name(name)
            if e then UI.set_visible(e, captions) end
        end
    end
    if Input.is_key_just_pressed("P") then
        pixelate_on = not pixelate_on
        PostProcess.set_enabled("pixelate", pixelate_on)
        refresh_status()
    end
    if Input.is_key_just_pressed("C") then
        crt_on = not crt_on
        PostProcess.set_enabled("crt", crt_on)
        refresh_status()
    end
    t = t + dt
    if cam then Transform.set_position(cam, math.sin(t * PAN_SPEED) * PAN, 0.0) end
end
