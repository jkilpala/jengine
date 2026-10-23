-- Scene 54 - Input Rebinding
--
-- The action layer is editable at runtime, which is what a key-config menu
-- needs:
--
--   Input.get_actions()                 -> every action name
--   Input.get_bindings(action)          -> that action's key list
--   Input.add_binding(action, key)      -> append one
--   Input.remove_binding(action, key)   -> drop one
--   Input.set_bindings(action, keys)    -> replace the list
--   Input.reset_bindings(action)        -> back to the project default
--   Input.set_dead_zone(action, v) / Input.get_dead_zone(action)
--
-- The dead zone only matters for axis bindings (Pad0_LeftStickX+ and friends):
-- it is the magnitude the axis must exceed to count as pressed. 0 means unset,
-- and the engine falls back to DEFAULT_AXIS_DEAD_ZONE (0.35).

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/54-input-rebinding.scene"

local ACTION = "confirm"
-- Keys offered for capture. Deliberately not every key: nav needs its own.
local CAPTURE_KEYS = { "Q", "W", "E", "R", "T", "Y", "Z", "X", "C", "V",
                       "1", "2", "3", "4", "5", "Space", "Enter" }

local capturing = false
local last = "-"

local function join(list)
    if type(list) ~= "table" then return tostring(list) end
    return #list > 0 and table.concat(list, ", ") or "(none)"
end

function init(entity)
    log("[scene_54] ready - B captures a key for 'confirm', R resets, D cycles dead zone")
end

function update(entity, dt)
    if capturing then
        for _, k in ipairs(CAPTURE_KEYS) do
            if Input.is_key_just_pressed(k) then
                -- Append rather than replace, so the original binding still
                -- works and the list visibly grows.
                Input.add_binding(ACTION, k)
                capturing = false
                last = "input_add_binding(\"" .. ACTION .. "\", \"" .. k .. "\")"
                break
            end
        end
    else
        if Input.is_key_just_pressed("B") then
            capturing = true
            last = "waiting for a key..."
        elseif Input.is_key_just_pressed("R") then
            Input.reset_bindings(ACTION)
            last = "input_reset_bindings(\"" .. ACTION .. "\")"
        elseif Input.is_key_just_pressed("X") then
            -- Clears every binding for the action; the light goes dark until
            -- B adds one back or R restores the project default.
            Input.clear_bindings(ACTION)
            last = "input_clear_bindings(\"" .. ACTION .. "\")"
        elseif Input.is_key_just_pressed("D") then
            local dz = tonumber(Input.get_dead_zone(ACTION)) or 0.0
            local next_dz = dz >= 0.6 and 0.0 or dz + 0.2
            Input.set_dead_zone(ACTION, next_dz)
            last = string.format("input_set_dead_zone(%.1f)", next_dz)
        end
    end

    local al = Entity.find_by_name("ActionsLabel")
    if al then
        local acts = Input.get_actions()
        local n = type(acts) == "table" and #acts or 0
        Text.set(al, string.format("input_get_actions(): %d actions", n))
    end
    local bl = Entity.find_by_name("BindingsLabel")
    if bl then
        Text.set(bl, "confirm bindings: " .. join(Input.get_bindings(ACTION)))
    end
    local dl = Entity.find_by_name("DeadZoneLabel")
    if dl then
        local dz = tonumber(Input.get_dead_zone(ACTION)) or 0.0
        Text.set(dl, string.format("dead zone: %.2f%s", dz,
            dz == 0.0 and "  (unset - engine uses 0.35)" or ""))
    end
    local ml = Entity.find_by_name("ModeLabel")
    if ml then
        Text.set(ml, capturing and "PRESS A KEY to bind it to 'confirm'"
            or "B = capture   R = reset   X = clear all   D = cycle dead zone")
    end

    -- The light proves the rebinding took: it follows the action, not a key.
    local light = Entity.find_by_name("Light")
    if light then
        if Input.is_action_pressed(ACTION) then
            Sprite.set_color(light, 0.4, 1.0, 0.55, 1.0)
        else
            Sprite.set_color(light, 0.25, 0.28, 0.36, 1.0)
        end
    end

    -- The release-edge and held predicates, which the press-edge ones above
      -- do not cover. `is_action_just_released` is what a charge-and-fire
      -- input reads; the mouse pair is the same idea for clicks.
    if Input.is_action_just_released(ACTION) then
        last = "is_action_just_released(\"" .. ACTION .. "\")"
    end
    if Input.is_mouse_left_just_released() then
        last = "is_mouse_left_just_released()"
    end

    local r = Entity.find_by_name("Readout")
    if r then
        Text.set(r, string.format("last: %s   |   mouse held: %s   wheel: %+.0f",
            last, tostring(Input.is_mouse_left_pressed()), Input.get_mouse_wheel() or 0))
    end
    nav.tick(SCENE_PATH)
end
