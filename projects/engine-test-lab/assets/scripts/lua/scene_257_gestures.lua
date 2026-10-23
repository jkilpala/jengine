-- Scene 257: gestures. Everything here is read from `Input.get_gestures()`
-- - this frame's taps, double taps, long presses, swipes, pans, pinches and
-- twists, recognised by the engine from the fingers (and the mouse and
-- wheel) - and logged with their numbers.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/257-gestures.scene"
local card, mark, counts_ui, log_ui
local x, y, rot, sc, vx, vy = 200, -20, 0, 1, 0, 0
local held, dragging = false, false
local counts = {}
local log = {}
local KINDS = { "tap", "double_tap", "long_press", "swipe", "pan", "pinch", "rotate" }

local function note(s)
    table.insert(log, 1, s)
    if #log > 12 then table.remove(log) end
    UI.set_text(log_ui, table.concat(log, "\n"))
end

local function inside(px, py)
    local c, s = math.cos(-rot), math.sin(-rot)
    local lx = (px - x) * c - (py - y) * s
    local ly = (px - x) * s + (py - y) * c
    return math.abs(lx) < 110 * sc and math.abs(ly) < 75 * sc
end

function init(entity)
    card = Entity.find_by_name("Card")
    mark = Entity.find_by_name("CardMark")
    counts_ui = Entity.find_by_name("Counts")
    log_ui = Entity.find_by_name("Log")
    for _, k in ipairs(KINDS) do counts[k] = 0 end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    for _, g in ipairs(Input.get_gestures()) do
        counts[g.kind] = (counts[g.kind] or 0) + 1
        if g.kind == "pan" then
            if g.phase == "began" then dragging = inside(g.wx - g.wdx, g.wy - g.wdy) end
            if dragging then x, y, vx, vy = x + g.wdx, y + g.wdy, 0, 0 end
            if g.phase ~= "changed" then
                note(string.format("pan %s at (%d, %d)", g.phase, math.floor(g.wx + 0.5), math.floor(g.wy + 0.5)))
            end
            if g.phase == "ended" then dragging = false end
        elseif g.kind == "swipe" then
            vx, vy = g.wdx * 3, g.wdy * 3
            note(string.format("swipe (%d, %d) at %d px/s", math.floor(g.dx + 0.5), math.floor(g.dy + 0.5), math.floor(g.speed + 0.5)))
        elseif g.kind == "pinch" then
            sc = math.max(0.3, math.min(4, sc * g.scale))
            if g.phase ~= "changed" or g.scale ~= 1 then note(string.format("pinch %s x%.3f (card x%.2f)", g.phase, g.scale, sc)) end
        elseif g.kind == "rotate" then
            rot = rot + g.angle
            if g.phase ~= "changed" then note(string.format("rotate %s (card %d deg)", g.phase, math.floor(math.deg(rot) + 0.5))) end
        elseif g.kind == "double_tap" then
            x, y, rot, sc, vx, vy = 200, -20, 0, 1, 0, 0
            note("double tap - reset")
        elseif g.kind == "long_press" then
            held = inside(g.wx, g.wy)
            note(string.format("long press at (%d, %d)%s", math.floor(g.wx + 0.5), math.floor(g.wy + 0.5), held and " on the card" or ""))
        else
            note(string.format("%s at (%d, %d)", g.kind, math.floor(g.wx + 0.5), math.floor(g.wy + 0.5)))
        end
    end
    if held and not Input.is_mouse_left_pressed() and #Input.get_touches() == 0 then held = false end
    x, y = x + vx * dt, y + vy * dt
    local damp = math.exp(-3 * dt)
    vx, vy = vx * damp, vy * damp
    x = math.max(-600, math.min(600, x))
    y = math.max(-300, math.min(260, y))
    Transform.set_position(card, x, y)
    Transform.set_rotation(card, rot)
    Transform.set_scale(card, sc, sc)
    -- A mark on the card's corner shows which way it is turned.
    local c, s = math.cos(rot), math.sin(rot)
    Transform.set_position(mark, x + (90 * c - 55 * s) * sc, y + (90 * s + 55 * c) * sc)
    Transform.set_rotation(mark, rot)
    if held then Sprite.set_color(card, 1.0, 0.8, 0.3, 1) else Sprite.set_color(card, 0.35, 0.55, 0.85, 1) end
    local parts = {}
    for _, k in ipairs(KINDS) do parts[#parts + 1] = k .. " " .. counts[k] end
    UI.set_text(counts_ui, table.concat(parts, "   "))
end
