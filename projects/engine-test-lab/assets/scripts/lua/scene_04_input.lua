-- Scene 04: Input
-- Lights up colored squares for each action / key, and moves a sprite cursor
-- to follow the mouse position, and prints what both mouse buttons are
-- doing: held, the press edge and the release edge. (The right button only
-- became readable from a script in Sep 2026; the engine tracked it all
-- along.)
--
-- FOCUS CHECK (BUGS #62): hold a direction key, alt-tab away, release it, come
-- back. The square must be dark. It used to stay lit - and the player kept
-- walking - because an unfocused window never receives the key-up. Same for
-- holding the left mouse button and releasing it outside the window.
-- Native only: Escape no longer closes the window (BUGS #63).

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/04-input.scene"

local up, down, left, right, confirm, mouseL, mouseR, space, cursor, readout

local NEWLINE = string.char(10)
local IDLE = { 0.2, 0.2, 0.25, 1.0 }
local ON   = { 1.0, 0.85, 0.25, 1.0 }
local IDLE_C = { 0.2, 0.25, 0.2, 1.0 }
local ON_C   = { 0.3, 1.0, 0.5, 1.0 }
local IDLE_M = { 0.25, 0.2, 0.25, 1.0 }
local ON_M   = { 1.0, 0.4, 0.9, 1.0 }
local IDLE_S = { 0.25, 0.25, 0.2, 1.0 }
local ON_S   = { 1.0, 1.0, 0.4, 1.0 }

local function tint(e, pressed, off, on)
    if not e then return end
    if pressed then
        Sprite.set_color(e, on[1], on[2], on[3], on[4])
    else
        Sprite.set_color(e, off[1], off[2], off[3], off[4])
    end
end

function init(entity)
    up      = Entity.find_by_name("ActUp")
    down    = Entity.find_by_name("ActDown")
    left    = Entity.find_by_name("ActLeft")
    right   = Entity.find_by_name("ActRight")
    confirm = Entity.find_by_name("ActConfirm")
    mouseL  = Entity.find_by_name("ActMouseL")
    mouseR  = Entity.find_by_name("ActMouseR")
    readout = Entity.find_by_name("MouseReadout")
    space   = Entity.find_by_name("ActSpace")
    cursor  = Entity.find_by_name("MouseCursor")
    log("[04-input] ready")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    tint(up,      Input.is_action_pressed("move_up"),    IDLE, ON)
    tint(down,    Input.is_action_pressed("move_down"),  IDLE, ON)
    tint(left,    Input.is_action_pressed("move_left"),  IDLE, ON)
    tint(right,   Input.is_action_pressed("move_right"), IDLE, ON)
    tint(confirm, Input.is_action_pressed("confirm"),    IDLE_C, ON_C)
    tint(mouseL,  Input.is_mouse_left_pressed(),         IDLE_M, ON_M)
    -- The right button: the engine has always tracked it, but scripts could
    -- not read it until Sep 2026.
    tint(mouseR,  Input.is_mouse_right_pressed(),        IDLE_M, ON_M)
    tint(space,   Input.is_key_pressed("Space"),         IDLE_S, ON_S)

    if cursor then
        local wx, wy = Input.get_mouse_world_position()
        Transform.set_position(cursor, wx or 0.0, wy or 0.0)
    end
    if readout then
        local function mark(on)
            return on and "yes" or "no "
        end
        Text.set(readout, string.format(
            "left  held %s  pressed %s  released %s" .. NEWLINE ..
            "right held %s  pressed %s  released %s",
            mark(Input.is_mouse_left_pressed()), mark(Input.is_mouse_left_just_pressed()), mark(Input.is_mouse_left_just_released()),
            mark(Input.is_mouse_right_pressed()), mark(Input.is_mouse_right_just_pressed()), mark(Input.is_mouse_right_just_released())))
    end
end
