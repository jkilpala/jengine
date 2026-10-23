-- Scene 197: camera priorities, blends and a confining shape. The close
-- camera is confined to the L; 1 and 2 hand the screen between it and the
-- wide camera by priority, each easing in over its own blend.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/197-camera-blend-confine.scene"
local L = { { -600.0, -350.0 }, { 600.0, -350.0 }, { 600.0, 0.0 }, { 0.0, 0.0 }, { 0.0, 350.0 }, { -600.0, 350.0 } }
local SPEED = 320
local dot, close, wide, status
local confined = true
local main = "close"

function init(entity)
    dot = Entity.find_by_name("Dot")
    close = Entity.find_by_name("CamClose")
    wide = Entity.find_by_name("CamWide")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local x, y = Transform.get_position(dot)
    local dx, dy = 0, 0
    if Input.is_key_pressed("Left") then dx = dx - 1 end
    if Input.is_key_pressed("Right") then dx = dx + 1 end
    if Input.is_key_pressed("Down") then dy = dy - 1 end
    if Input.is_key_pressed("Up") then dy = dy + 1 end
    x = math.max(-580, math.min(580, x + dx * SPEED * dt))
    y = math.max(-330, math.min(330, y + dy * SPEED * dt))
    Transform.set_position(dot, x, y)
    if Input.is_key_just_pressed("1") then Camera.set(wide, "priority", 5); main = "wide" end
    if Input.is_key_just_pressed("2") then Camera.set(wide, "priority", 0); main = "close" end
    if Input.is_key_just_pressed("C") then
        confined = not confined
        Camera.set(close, "confine", confined and L or {})
    end
    local cx, cy = Camera.get_position()
    if status then
        UI.set_text(status, string.format("main: %s   confine: %s   view at %d, %d   zoom %.2f", main,
            confined and "on" or "off", math.floor(cx + 0.5), math.floor(cy + 0.5), Camera.get_zoom()))
    end
end
