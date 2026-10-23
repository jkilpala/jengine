-- Scene 108 - Multi-touch
--
-- Every finger on the screen, as `Input.get_touches()` reports it: a ring
-- under each, its id and its phase ("began", "moved", "held", "ended"),
-- and - with two down - the distance and angle between them, which is all a
-- pinch or a twist is.
--
-- Before Sep 2026 a script saw one finger, as the left mouse button, and a
-- package the same: no game could pinch, play a chord, or take two taps at
-- once. The list is kept by `TouchControlsState::tick`, which every host
-- calls, and reaches scripts through the snapshot natively and through
-- `input_mirror::publish` in the browser hosts.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/108-multi-touch.scene"
local MAX = 10

local rings, labels = {}, {}
local readout, mouse
local seen_max = 0

local PHASE_COLOR = {
    began = { 0.45, 1.0, 0.55, 0.85 },
    moved = { 0.4, 0.8, 1.0, 0.85 },
    held = { 0.95, 0.85, 0.4, 0.85 },
    ended = { 1.0, 0.4, 0.4, 0.85 },
}

function init(entity)
    for i = 1, MAX do
        rings[i] = Entity.find_by_name("Finger" .. i)
        labels[i] = Entity.find_by_name("FingerLabel" .. i)
    end
    readout = Entity.find_by_name("Readout")
    mouse = Entity.find_by_name("Mouse")
    log("[scene_108] ready - touch the screen with one or more fingers")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local list = Input.get_touches()
    if #list > seen_max then seen_max = #list end
    for i = 1, MAX do
        local t = list[i]
        if t ~= nil then
            Transform.set_position(rings[i], t.wx, t.wy)
            local c = PHASE_COLOR[t.phase] or PHASE_COLOR.held
            Sprite.set_color(rings[i], c[1], c[2], c[3], c[4])
            Transform.set_position(labels[i], t.wx, t.wy + 90)
            local tag = t.on_control and " (control)" or ""
            Text.set(labels[i], "#" .. math.floor(t.id) .. " " .. t.phase .. tag)
        else
            Transform.set_position(rings[i], 0, -2000)
            Transform.set_position(labels[i], 0, -2000)
            Text.set(labels[i], "")
        end
    end
    local line = "fingers: " .. #list .. "   (most at once: " .. seen_max .. ")"
    if #list >= 2 then
        local a, b = list[1], list[2]
        local dx, dy = b.wx - a.wx, b.wy - a.wy
        local dist = math.sqrt(dx * dx + dy * dy)
        -- Fengari has no math.atan2 and Luau's math.atan takes one argument:
        -- probe, as lib code elsewhere does.
        local angle
        if math.atan2 ~= nil then angle = math.atan2(dy, dx) else angle = math.atan(dy, dx) end
        line = line .. string.format("   pinch: %d units, %d degrees", math.floor(dist + 0.5),
            math.floor(angle * 180 / math.pi + 0.5))
    end
    Text.set(readout, line)
    local mx, my = Input.get_mouse_world_position()
    Text.set(mouse, string.format("mouse (the first free finger): %d, %d  %s", math.floor(mx + 0.5),
        math.floor(my + 0.5), Input.is_mouse_left_pressed() and "down" or "up"))
end
