-- Scene 167: rumble and vibration. The playing is the engine's (Input.rumble
-- on gamepad 0, Input.vibrate on the device); this asks, and draws what it
-- asked for as discs swelling with each motor's strength.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/167-rumble.scene"
local strong_disc, weak_disc, phone_disc, readout
local strong, weak, phone = 0, 0, 0
local was = {}

-- A pad button's press this frame (the API reads only "held").
local function pad_tap(i)
    local now = Input.is_gamepad_connected(0) and Input.is_gamepad_button_pressed(0, i)
    local edge = now and not was[i]
    was[i] = now
    return edge
end

function init(entity)
    strong_disc = Entity.find_by_name("Strong")
    weak_disc = Entity.find_by_name("Weak")
    phone_disc = Entity.find_by_name("Phone")
    readout = Entity.find_by_name("Readout")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local said = nil
    if Input.is_key_just_pressed("1") or pad_tap(0) then
        Input.rumble(0, 0, 0.6, 0.12); weak = 0.6; said = "tap: weak 0.6 for 0.12 s"
    end
    if Input.is_key_just_pressed("2") or pad_tap(1) then
        Input.rumble(0, 1, 0.3, 0.35); strong = 1; weak = math.max(weak, 0.3); said = "thud: strong 1, weak 0.3 for 0.35 s"
    end
    local engine = Input.is_key_pressed("3") or (Input.is_gamepad_connected(0) and Input.is_gamepad_button_pressed(0, 2))
    if engine then
        -- Asked again every frame for a little longer: it stops by itself
        -- a moment after the button is let go.
        Input.rumble(0, 0.35, 0.15, 0.1); strong = math.max(strong, 0.35); weak = math.max(weak, 0.15)
    end
    if Input.is_key_just_pressed("4") then Input.rumble(0, 0, 0, 0); strong = 0; weak = 0; said = "stopped" end
    if Input.is_key_just_pressed("V") then Input.vibrate(0.3, 1); phone = 1; said = "phone: 0.3 s" end
    strong = math.max(0, strong - dt * 3)
    weak = math.max(0, weak - dt * 3)
    phone = math.max(0, phone - dt * 3)
    Transform.set_scale(strong_disc, 1 + strong * 3, 1 + strong * 3)
    Transform.set_scale(weak_disc, 1 + weak * 3, 1 + weak * 3)
    Transform.set_scale(phone_disc, 1 + phone * 3, 1 + phone * 3)
    local pad = Input.is_gamepad_connected(0) and "pad 0 connected" or "no pad (press a button on it to wake it)"
    if said then Text.set(readout, pad .. "   -   " .. said)
    elseif engine then Text.set(readout, pad .. "   -   engine")
    end
    if not said and not engine and strong == 0 and weak == 0 then Text.set(readout, pad) end
end
