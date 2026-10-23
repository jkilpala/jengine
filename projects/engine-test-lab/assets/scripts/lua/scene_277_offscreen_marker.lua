-- Scene 277: `offscreen_marker`. Three dots far apart, each with a
-- marker; the arrows move the camera, or it wanders by itself (A).

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/277-offscreen-marker.scene"
local line, cam
local x, y, t = 0, 0, 0
local wander = true

function init(entity)
    line = Entity.find_by_name("Line0")
    cam = Entity.find_by_name("Camera")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if Input.is_key_just_pressed("A") then wander = not wander end
    local dx, dy = 0, 0
    if Input.is_key_pressed("Left") then dx = -1 end
    if Input.is_key_pressed("Right") then dx = 1 end
    if Input.is_key_pressed("Up") then dy = 1 end
    if Input.is_key_pressed("Down") then dy = -1 end
    if dx ~= 0 or dy ~= 0 then
        wander = false
        x, y = x + dx * 500 * dt, y + dy * 500 * dt
    elseif wander then
        x, y = math.sin(t * 0.35) * 900, math.sin(t * 0.5) * 700
    end
    Transform.set_position(cam, x, y)
    local seen = {}
    for _, n in ipairs({ "Red", "Green", "Blue" }) do
        if Entity.get_custom(Entity.find_by_name(n .. "Marker"), "OffscreenMarker", "visible") then table.insert(seen, n) end
    end
    UI.set_text(line, "in view: " .. (#seen > 0 and table.concat(seen, ", ") or "none") .. (wander and "   (wandering)" or ""))
end
