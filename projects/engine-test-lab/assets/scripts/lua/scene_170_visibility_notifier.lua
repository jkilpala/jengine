-- Scene 170: pans the camera along the street and counts what the lanterns
-- say. Lighting up and greying out is each lantern's own script, called by
-- the engine through `visibility_notifier`.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/170-visibility-notifier.scene"
local cam, status
local t, x = 0, 0
local auto = true
local lit, entered, exited = 0, 0, 0

function init(entity)
    cam = Entity.find_by_name("Camera")
    status = Entity.find_by_name("Status")
    Event.on("lantern_seen", entity)
end

function on_lantern_seen(entity, data)
    local d = (data and data.d) or 0
    lit = lit + d
    if d > 0 then entered = entered + 1 else exited = exited + 1 end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then auto = not auto end
    t = t + dt
    if auto then
        x = math.sin(t * 0.35) * 1300
    else
        if Input.is_key_pressed("Left") then x = x - 600 * dt end
        if Input.is_key_pressed("Right") then x = x + 600 * dt end
    end
    Transform.set_position(cam, x, 0)
    if status then
        UI.set_text(status, "lit " .. lit .. "   entered " .. entered .. "   exited " .. exited
            .. (auto and "   (auto-pan)" or ""))
    end
end
