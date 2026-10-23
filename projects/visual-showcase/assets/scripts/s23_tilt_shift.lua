-- 23 Tilt-Shift Diorama: the focus band and the camera. The town runs
-- itself - cars and boats on patrol loops, turning with `face_velocity`,
-- chimneys smoking, cloud shadows drifting.
-- Mouse: the focus band.  T: tilt-shift on / off.  Up / Down: how much blur.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/23-tilt-shift-diorama.scene"

local post_on = nav.post_all_on(SCENE)
local captions = true
local status, cam
local tilt_on = true
local blur = 9.0
local focus, focus_to = 0.0, 0.0
local t = 0.0
local last_mx, last_my = nil, nil
local mouse_quiet = 99.0
local cam_x, cam_y = 0.0, 0.0

local function refresh_status()
    if status then
        UI.set_text(status, "tilt-shift " .. (tilt_on and ("on, blur " .. math.floor(blur + 0.5)) or "off")
            .. "   " .. nav.post_status(post_on))
    end
end

function init(entity)
    status = Entity.find_by_name("Status")
    cam = Entity.find_by_name("Camera")
    refresh_status()
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then
        captions = not captions
        for _, name in ipairs({ "Title", "Hint", "Status" }) do
            local e = Entity.find_by_name(name)
            if e then UI.set_visible(e, captions) end
        end
    end
    if Input.is_key_just_pressed("T") then
        tilt_on = not tilt_on
        PostProcess.set_enabled("tilt_shift", tilt_on)
        refresh_status()
    end
    local was = blur
    if Input.is_key_pressed("Up") then blur = math.min(20, blur + dt * 8) end
    if Input.is_key_pressed("Down") then blur = math.max(1, blur - dt * 8) end
    if blur ~= was then
        PostProcess.set_float("tilt_shift", "max_blur", blur)
        refresh_status()
    end
    t = t + dt

    -- The camera drifts slowly over the town, like a hand-held lens.
    cam_x = math.sin(t * 0.07) * 70
    cam_y = math.sin(t * 0.05 + 1.0) * 50
    if cam then Transform.set_position(cam, cam_x, cam_y) end

    -- The band: where the mouse is when it moves, else a slow sweep.
    local mx, my = Input.get_mouse_world_position()
    if last_mx and (math.abs(mx - last_mx) > 0.5 or math.abs(my - last_my) > 0.5) then mouse_quiet = 0 end
    last_mx, last_my = mx, my
    mouse_quiet = mouse_quiet + dt
    if mouse_quiet < 6 then
        focus_to = my
    else
        focus_to = cam_y + math.sin(t * 0.18) * 160
    end
    focus = focus + (focus_to - focus) * math.min(1, dt * 5)
    PostProcess.set_float("tilt_shift", "focus_y", focus)
end
