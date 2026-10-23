-- Scene 03: Camera
-- Moves the Player entity with WASD/arrow keys; CameraFollow on Player
-- causes the camera to track it (smoothed). Q / E zoom the active camera.
--
-- There is no `get_camera_zoom`, so the script keeps the value: a command can
-- set state but not read it back, and one number is cheaper than a getter
-- across four runtimes.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/03-camera.scene"

local SPEED = 250.0  -- world units / second
local ZOOM_STEP = 1.08
local ZOOM_MIN, ZOOM_MAX = 0.4, 3.0
local zoom = 1.0
local player

function init(entity)
    player = Entity.find_by_name("Player")
    log("[03-camera] ready  player=" .. tostring(player))
end

function update(entity, dt)
    nav.tick(SCENE_PATH)

    -- Held, not edged: zooming is something you hold until the framing looks
    -- right, and an edge would make it a hunt for the right number of taps.
    local before = zoom
    if Input.is_key_pressed("E") then zoom = math.min(zoom * ZOOM_STEP, ZOOM_MAX) end
    if Input.is_key_pressed("Q") then zoom = math.max(zoom / ZOOM_STEP, ZOOM_MIN) end
    if zoom ~= before then Camera.set_zoom(zoom) end

    if not player then return end

    local x, y = Transform.get_position(player)
    local dx, dy = 0.0, 0.0
    if Input.is_action_pressed("move_right") then dx = dx + 1 end
    if Input.is_action_pressed("move_left")  then dx = dx - 1 end
    if Input.is_action_pressed("move_up")    then dy = dy + 1 end
    if Input.is_action_pressed("move_down")  then dy = dy - 1 end
    if dx ~= 0 or dy ~= 0 then
        local n = math.sqrt(dx * dx + dy * dy)
        x = x + (dx / n) * SPEED * dt
        y = y + (dy / n) * SPEED * dt
        Transform.set_position(player, x, y)
    end
end
