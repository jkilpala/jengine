-- Scene 105: on-screen touch controls (`touch_controls` component).
--
-- The script knows nothing about touch. It reads the keyboard the way any
-- game here does - actions and raw keys - and the controls press those keys:
-- the stick is bound to the `move_*` actions (so it presses W / A / S / D,
-- their first keys), button A to the `confirm` action (Enter), button B to
-- the raw key X. The layout says `show: "always"`, so a mouse works it on a
-- desktop and in the editor; on a phone the fingers do, several at once.
--
-- Must match in every runtime: the box moves with the stick (diagonals too),
-- turns yellow while A is held, grows while B is held, and the readout names
-- exactly what is held. Holding the stick and a button together must do both.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/105-touch-controls.scene"

local SPEED = 260
local box, readout
local x, y = 0, -20

function init(entity)
    box = Entity.find_by_name("Box")
    readout = Entity.find_by_name("Readout")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)

    local held = {}
    local dx, dy = 0, 0
    for _, a in ipairs({ "move_left", "move_right", "move_up", "move_down" }) do
        if Input.is_action_pressed(a) then
            held[#held + 1] = a
        end
    end
    if Input.is_action_pressed("move_left") then dx = dx - 1 end
    if Input.is_action_pressed("move_right") then dx = dx + 1 end
    if Input.is_action_pressed("move_up") then dy = dy + 1 end
    if Input.is_action_pressed("move_down") then dy = dy - 1 end
    x = math.max(-560, math.min(560, x + dx * SPEED * dt))
    y = math.max(-220, math.min(200, y + dy * SPEED * dt))
    Transform.set_position(box, x, y)

    local confirm = Input.is_action_pressed("confirm")
    local x_key = Input.is_key_pressed("X")
    if confirm then held[#held + 1] = "confirm (A)" end
    if x_key then held[#held + 1] = "key X (B)" end

    if confirm then
        Sprite.set_color(box, 1.0, 0.85, 0.2, 1.0)
    else
        Sprite.set_color(box, 0.35, 0.75, 1.0, 1.0)
    end
    local s = x_key and 1.6 or 1.0
    Transform.set_scale(box, s, s)

    Text.set(readout, "held: " .. (#held > 0 and table.concat(held, ", ") or "nothing"))
end
