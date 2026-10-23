-- Scene 207: controller identity, button prompts and action interactions.
-- 'lab_jump' (Space / Pad0_A) is held, tapped and double-tapped; the lines
-- read Input.get_last_device, get_gamepad_name / get_gamepad_type, and each
-- action's Input.get_prompt in the style in use and in every family.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/207-prompts-and-interactions.scene"
local ACTIONS = { "lab_jump", "lab_use" }
local STYLES = { "keyboard", "xbox", "playstation", "switch", "generic" }
local lines = {}
local counts = { h05 = 0, h1 = 0, h2 = 0, tap = 0, double = 0 }
local events = {}

local function note(s)
    table.insert(events, 1, s)
    if #events > 4 then table.remove(events) end
end

function init(entity)
    for k = 0, 11 do lines[k + 1] = Entity.find_by_name("Line" .. k) end
    Input.set_bindings("lab_jump", { "Space", "Pad0_A" })
    Input.set_bindings("lab_use", { "E", "Pad0_X" })
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_action_held("lab_jump", 0.5) then counts.h05 = counts.h05 + 1; note("held 0.5 s") end
    if Input.is_action_held("lab_jump", 1.0) then counts.h1 = counts.h1 + 1; note("held 1 s") end
    if Input.is_action_held("lab_jump", 2.0) then counts.h2 = counts.h2 + 1; note("held 2 s") end
    if Input.is_action_tapped("lab_jump") then counts.tap = counts.tap + 1; note("tap") end
    if Input.is_action_double_tapped("lab_jump") then counts.double = counts.double + 1; note("double tap") end

    local pad = Input.get_last_gamepad()
    UI.set_text(lines[1], "last device: " .. Input.get_last_device() .. "    last pad: " .. pad
        .. "    pad 0: " .. (Input.get_gamepad_type(0) ~= "" and (Input.get_gamepad_type(0) .. " - " .. Input.get_gamepad_name(0)) or "none"))
    for i, a in ipairs(ACTIONS) do
        local p = Input.get_prompt(a)
        local all = {}
        for _, s in ipairs(STYLES) do all[#all + 1] = s .. " " .. Input.get_prompt(a, s).label end
        UI.set_text(lines[1 + i], string.format("%s: now [%s] %s (%s)    %s", a, p.label, p.glyph, p.binding,
            table.concat(all, " | ")))
    end
    local held = Input.get_hold_time("lab_jump")
    local bar = string.rep("#", math.floor(math.min(held, 2.5) * 16)) .. string.rep(".", 40 - math.floor(math.min(held, 2.5) * 16))
    UI.set_text(lines[5], string.format("lab_jump held %.2f s  %s", held, bar))
    UI.set_text(lines[7], string.format("holds 0.5 s: %d    1 s: %d    2 s: %d    taps: %d    double taps: %d",
        counts.h05, counts.h1, counts.h2, counts.tap, counts.double))
    UI.set_text(lines[8], "latest: " .. table.concat(events, "  <  "))
end
