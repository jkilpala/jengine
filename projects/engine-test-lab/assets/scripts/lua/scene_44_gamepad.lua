-- Scene 44 - Gamepad
--
-- Two markers, side by side, driven by the same input:
--
--   left  (yellow) - is_action_pressed, so a stick past its dead zone reads as
--                    a press and the marker snaps to one of nine positions
--   right (blue)   - get_action_axis, the same actions read as signed analog
--                    values, so a stick moves it smoothly and a *key* still
--                    drives it at full deflection
--
-- A pad binding is a name in the *action* namespace, so `pad_a` binds both
-- "Pad0_A" and a keyboard fallback and either resolves it. Bindings live in
-- project.jengine, and the IJKL / ZXCV / Q / E fallbacks keep the scene
-- checkable with no pad attached.
--
-- The bottom two lines are the raw device API - connected, axes, buttons -
-- which bypasses bindings entirely. Prefer actions unless a stick really is a
-- stick.
--
-- Until Sep 2026 there was no analog API at all, and in the *browser* runtimes
-- no gamepad input reached scripts whatsoever: the preamble resolved actions by
-- testing their bindings against a keyboard-only key map, so "Pad0_A" was never
-- found. Every light in this scene stayed dark in the editor and in a web build
-- while working natively (BUGS #9). If they light up on native and not here,
-- that has come back.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/44-gamepad.scene"

local ACTIONS = {
    "pad_up", "pad_down", "pad_left", "pad_right",
    "pad_a", "pad_b", "pad_x", "pad_y", "pad_lt", "pad_rt",
}

local ON = { 0.4, 0.95, 0.5, 1.0 }
local OFF = { 0.2, 0.22, 0.3, 1.0 }
local MARKER_HOME = { -120.0, 30.0 }
local ANALOG_HOME = { 150.0, 30.0 }
local MARKER_RANGE = 70.0

local marker, analog_marker, buttons_lbl, readout, readout2

function init(entity)
    marker = Entity.find_by_name("Marker")
    analog_marker = Entity.find_by_name("AnalogMarker")
    buttons_lbl = Entity.find_by_name("Buttons")
    readout = Entity.find_by_name("Readout")
    readout2 = Entity.find_by_name("Readout2")
    log("[scene_44] ready - pad or IJKL / ZXCV / Q / E")
end

function update(entity, dt)
    local held = {}
    for _, action in ipairs(ACTIONS) do
        local down = Input.is_action_pressed(action)
        local light = Entity.find_by_name("Light_" .. action)
        if light ~= nil then
            local c = down and ON or OFF
            Sprite.set_color(light, c[1], c[2], c[3], c[4])
        end
        if down then
            held[#held + 1] = action
        end
    end

    if marker then
        -- Thresholded: the stick reaches this one as a pressed/not-pressed
        -- action once past the dead zone, so it snaps.
        local dx = (Input.is_action_pressed("pad_right") and 1 or 0)
            - (Input.is_action_pressed("pad_left") and 1 or 0)
        local dy = (Input.is_action_pressed("pad_up") and 1 or 0)
            - (Input.is_action_pressed("pad_down") and 1 or 0)
        Transform.set_position(marker,
            MARKER_HOME[1] + dx * MARKER_RANGE,
            MARKER_HOME[2] + dy * MARKER_RANGE)
    end

    if analog_marker then
        -- The same actions, read as signed analog values. A stick moves this
        -- one anywhere in the well; a key still pins it to the edge, because a
        -- key binding reads 1.0.
        local ax = (Input.get_action_axis("pad_right") or 0.0)
            - (Input.get_action_axis("pad_left") or 0.0)
        local ay = (Input.get_action_axis("pad_up") or 0.0)
            - (Input.get_action_axis("pad_down") or 0.0)
        Transform.set_position(analog_marker,
            ANALOG_HOME[1] + ax * MARKER_RANGE,
            ANALOG_HOME[2] + ay * MARKER_RANGE)
    end

    if buttons_lbl then
        Text.set(buttons_lbl, "held: " .. (#held > 0 and table.concat(held, ", ") or "-"))
    end
    if readout then
        Text.set(readout, string.format(
            "action axes: x=%.2f  y=%.2f   (bound, dead-zoned - the path a game should use)",
            (Input.get_action_axis("pad_right") or 0.0) - (Input.get_action_axis("pad_left") or 0.0),
            (Input.get_action_axis("pad_up") or 0.0) - (Input.get_action_axis("pad_down") or 0.0)))
    end
    if readout2 then
        -- Raw device, no bindings involved. `is_gamepad_connected` is a
        -- separate question from the axis value on purpose: a stick reading
        -- 0.00 because nothing is plugged in looks exactly like one at centre.
        local n = Input.get_gamepad_count() or 0
        if n > 0 and Input.is_gamepad_connected(0) then
            local btns = {}
            for b = 0, 7 do
                if Input.is_gamepad_button_pressed(0, b) then btns[#btns + 1] = tostring(b) end
            end
            Text.set(readout2, string.format(
                "raw pad 0: connected  axes 0..3 = %.2f %.2f %.2f %.2f  buttons: %s",
                Input.get_gamepad_axis(0, 0) or 0.0, Input.get_gamepad_axis(0, 1) or 0.0,
                Input.get_gamepad_axis(0, 2) or 0.0, Input.get_gamepad_axis(0, 3) or 0.0,
                #btns > 0 and table.concat(btns, ",") or "-"))
        else
            Text.set(readout2, string.format(
                "raw pad 0: not connected (%d slot(s) seen) - keyboard fallbacks still drive the actions", n))
        end
    end

    nav.tick(SCENE_PATH)
end
