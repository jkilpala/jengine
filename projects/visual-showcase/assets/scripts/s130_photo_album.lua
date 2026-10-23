-- 130 Photo Album: a table of snapshots played with the fingers. Every
-- move here is read from `Input.get_gestures()` - this frame's taps,
-- double taps, long presses, swipes, pans, pinches and twists, recognised
-- by the engine from the touches (and from the mouse and wheel on a
-- desktop) - so no finger is tracked by hand. Left alone it plays the
-- same gestures itself, through the same handler, a touch marker showing
-- where.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/130-photo-album.scene"
local N = 6
local DECK_X, DECK_Y = -500.0, -235.0
local START = { { -150.0, 40.0, 0.12 }, { 180.0, -60.0, -0.20 }, { 360.0, 150.0, 0.30 } }
local HW, HH = 120, 88 -- half a photo at scale 1
local photos, order, deck, cards = {}, {}, {}, {}
local status, logui, hand -- the touch marker
local captions = true
local log = {}
local drag, twist, panned_photo, zoomed = nil, nil, false, nil
local idle, demo_t = 4.0, 0
local t = 0

local function note(text)
    table.insert(log, 1, text)
    if #log > 7 then table.remove(log) end
    UI.set_text(logui, table.concat(log, "\n"))
end

local function restack()
    for i, p in ipairs(order) do
        Sprite.set_z_order(p.s, 9 + 2 * i)
        Sprite.set_z_order(p.e, 10 + 2 * i)
    end
end

local function raise(p)
    for i, q in ipairs(order) do
        if q == p then table.remove(order, i) break end
    end
    order[#order + 1] = p
    restack()
end

-- The top photo under a world point, tested in its own turned frame.
local function hit(x, y)
    for i = #order, 1, -1 do
        local p = order[i]
        if p.dealt and not p.returning then
            local c, s = math.cos(-p.rot), math.sin(-p.rot)
            local lx = (x - p.x) * c - (y - p.y) * s
            local ly = (x - p.x) * s + (y - p.y) * c
            local k = p.sc * (1 + 0.06 * p.lift)
            if math.abs(lx) < HW * k and math.abs(ly) < HH * k then return p end
        end
    end
    return nil
end

local function on_deck(x, y)
    return math.abs(x - DECK_X) < HW + 10 and math.abs(y - DECK_Y) < HH + 10
end

local function show_deck()
    for i, c in ipairs(cards) do Sprite.set_color(c, 1, 1, 1, i <= #deck and 1 or 0) end
end

local function deal(dx, dy)
    if #deck == 0 then
        -- The deck is out: every photo slides home and the next swipe deals.
        for _, p in ipairs(photos) do
            if p.dealt then p.returning = true; p.goal = nil; p.lifted = false end
        end
        zoomed = nil
        return "gather the photos"
    end
    local p = photos[table.remove(deck)]
    p.dealt, p.returning, p.goal = true, false, nil
    p.x, p.y, p.rot, p.sc = DECK_X, DECK_Y, 0, 1
    local l = math.sqrt(dx * dx + dy * dy)
    if l < 1 then dx, dy, l = 1, 0.5, 1.118 end
    p.vx, p.vy, p.vr = dx / l * 950, dy / l * 950, (math.random() - 0.5) * 4
    raise(p)
    show_deck()
    return "deal"
end

-- One gesture, from the fingers or from the demo: rows as
-- `Input.get_gestures()` gives them (world points and moves).
local function handle(g, dt)
    local k = g.kind
    if k == "tap" then
        if on_deck(g.wx, g.wy) then
            note("tap  - " .. deal(1, 0.45))
        else
            local p = hit(g.wx, g.wy)
            if p then raise(p) end
            note("tap")
        end
    elseif k == "double_tap" then
        local p = hit(g.wx, g.wy)
        if p and p == zoomed then
            p.goal = p.home
            zoomed = nil
            note("double tap  - put it back")
        elseif p then
            if zoomed then zoomed.goal = zoomed.home end
            p.home = { p.x, p.y, p.rot, p.sc }
            p.goal = { 0, 10, 0, 2.6 }
            p.vx, p.vy, p.vr = 0, 0, 0
            zoomed = p
            raise(p)
            note("double tap  - look closer")
        end
    elseif k == "long_press" then
        local p = hit(g.wx, g.wy)
        if p then
            p.lifted = true
            raise(p)
            note("long press  - lift")
        end
    elseif k == "pan" then
        if g.phase == "began" then
            drag = hit(g.wx - g.wdx, g.wy - g.wdy)
            panned_photo = drag ~= nil
            if drag then
                raise(drag)
                drag.goal = nil
                if drag == zoomed then zoomed = nil end
                note("pan  - drag")
            end
        end
        if drag then
            drag.x, drag.y = drag.x + g.wdx, drag.y + g.wdy
            if dt > 0 then
                drag.vx = math.max(-1400, math.min(1400, g.wdx / dt))
                drag.vy = math.max(-1400, math.min(1400, g.wdy / dt))
                drag.vr = 0
            end
        end
        if g.phase == "ended" then drag = nil end
    elseif k == "swipe" then
        if not panned_photo then
            note(string.format("swipe %d px/s  - %s", math.floor(g.speed + 0.5), deal(g.wdx, g.wdy)))
        end
        panned_photo = false
    elseif k == "pinch" or k == "rotate" then
        if g.phase == "began" then twist = hit(g.wx, g.wy) end
        local p = twist or hit(g.wx, g.wy)
        if p then
            p.goal = nil
            if p == zoomed then zoomed = nil end
            if k == "pinch" then
                p.sc = math.max(0.5, math.min(3.2, p.sc * g.scale))
                if g.phase ~= "changed" or twist == nil then note(string.format("pinch  %s x%.2f", g.phase, p.sc)) end
            else
                p.rot = p.rot + g.angle
                if g.phase == "began" then note("rotate  - twist") end
            end
        end
        if g.phase == "ended" then twist = nil end
    end
end

-- The demo: a list of moves, each a stretch of time in which the marker goes
-- somewhere and makes gestures. Every gesture goes through `handle`.
local demo = {}
local cur_x, cur_y = 0, -300
local function hand_to(x, y, a)
    cur_x, cur_y = x, y
    Transform.set_position(hand, x, y)
    Sprite.set_color(hand, 1, 1, 1, a)
end
local function g_at(kind, x, y, extra)
    local g = { kind = kind, wx = x, wy = y, wdx = 0, wdy = 0, scale = 1, angle = 0, speed = 0, phase = "ended" }
    for key, v in pairs(extra or {}) do g[key] = v end
    return g
end
local function top_photo()
    for i = #order, 1, -1 do
        if order[i].dealt and not order[i].returning then return order[i] end
    end
    return nil
end
local hx, hy = 0, -300
local function travel(tx, ty, f)
    local e = f * f * (3 - 2 * f)
    return hx + (tx - hx) * e, hy + (ty - hy) * e
end
-- Each move: duration, and a function of (local time, dt, first frame,
-- last frame).
demo = {
    { 1.2, function(u, dt, first, last)
        local x, y = travel(DECK_X + 20, DECK_Y + 10, math.min(1, u / 0.8))
        hand_to(x, y, 1)
        if u >= 0.9 and not demo.tapped then demo.tapped = true; handle(g_at("tap", DECK_X + 20, DECK_Y + 10), dt) end
    end },
    { 1.4, function(u, dt, first, last)
        -- A swipe from beside the deck, up and to the right.
        -- A flick across bare table above the deck (a real one is a pan too,
        -- but the pan of a finger on no photo moves nothing).
        local sx, sy = -520, -20
        if u < 0.6 then
            local x, y = travel(sx, sy, u / 0.6)
            hand_to(x, y, 1)
        else
            local f = math.min(1, (u - 0.6) / 0.25)
            hand_to(sx + 300 * f, sy + 120 * f, 1)
            if f >= 1 and not demo.swiped then
                demo.swiped = true
                panned_photo = false
                handle(g_at("swipe", sx + 300, sy + 120, { wdx = 300, wdy = 120, speed = 1600 }), dt)
            end
        end
    end },
    { 2.4, function(u, dt, first, last)
        -- Drag the top photo across the table.
        local p = demo.p
        if first then demo.p = top_photo(); p = demo.p; if p then demo.from = { p.x, p.y } end end
        if p == nil then return end
        if u < 0.7 then
            local x, y = travel(demo.from[1], demo.from[2], u / 0.7)
            hand_to(x, y, 1)
            return
        end
        local f = math.min(1, (u - 0.7) / 1.4)
        local e = f * f * (3 - 2 * f)
        local tx, ty = demo.from[1] + (-260 - demo.from[1]) * e, demo.from[2] + (120 - demo.from[2]) * e
        local x, y = cur_x, cur_y
        local phase = demo.panning and "changed" or "began"
        if f >= 1 then phase = "ended" end
        if not demo.done_pan then
            handle(g_at("pan", tx, ty, { wdx = tx - x, wdy = ty - y, phase = phase }), dt)
            demo.panning = true
            if f >= 1 then demo.done_pan = true; p.vx, p.vy = 0, 0 end
        end
        hand_to(tx, ty, 1)
    end },
    { 2.0, function(u, dt, first, last)
        -- Twist it, two fingers turning.
        local p = demo.p
        if p == nil then return end
        local phase = first and "began" or (last and "ended" or "changed")
        local a = (u < 1.6) and dt * 0.45 or 0
        handle(g_at("rotate", p.x, p.y, { angle = a, phase = phase }), dt)
        hand_to(p.x + 70 * math.cos(p.rot + 0.6), p.y + 70 * math.sin(p.rot + 0.6), 1)
    end },
    { 2.0, function(u, dt, first, last)
        -- Pinch it larger, then back.
        local p = demo.p
        if p == nil then return end
        local phase = first and "began" or (last and "ended" or "changed")
        local rate = (u < 0.9) and 0.45 or ((u < 1.8) and -0.45 or 0)
        handle(g_at("pinch", p.x, p.y, { scale = math.exp(rate * dt), phase = phase }), dt)
        hand_to(p.x + 60 * p.sc, p.y - 40 * p.sc, 1)
    end },
    { 3.2, function(u, dt, first, last)
        -- Double tap to look closer, and again to put it back.
        local p = demo.p
        if p == nil then return end
        if first then demo.tap1, demo.tap2 = false, false end
        if u < 0.5 then
            local x, y = cur_x, cur_y
            hand_to(x + (p.x - x) * math.min(1, dt * 8), y + (p.y - y) * math.min(1, dt * 8), 1)
        elseif not demo.tap1 then
            demo.tap1 = true
            handle(g_at("double_tap", p.x, p.y), dt)
        elseif u > 2.6 and not demo.tap2 then
            demo.tap2 = true
            handle(g_at("double_tap", p.x, p.y), dt)
        end
        if u >= 0.5 then hand_to(p.x + 40, p.y - 30, u < 2.6 and 0.5 or 1) end
    end },
    { 2.6, function(u, dt, first, last)
        -- Hold another photo until it lifts, and carry it.
        if first then
            demo.q = nil
            for i = #order - 1, 1, -1 do
                if order[i].dealt and not order[i].returning then demo.q = order[i]; break end
            end
            if demo.q then demo.from = { demo.q.x, demo.q.y } end
            demo.held, demo.carry = false, false
        end
        local q = demo.q
        if q == nil then return end
        if u < 0.6 then
            local x, y = travel(demo.from[1], demo.from[2], u / 0.6)
            hand_to(x, y, 1)
        elseif u < 1.3 then
            if u > 1.1 and not demo.held then demo.held = true; handle(g_at("long_press", q.x, q.y), dt) end
            hand_to(q.x, q.y, 1)
        else
            local f = math.min(1, (u - 1.3) / 1.1)
            local e = f * f * (3 - 2 * f)
            local tx, ty = demo.from[1] + 200 * e, demo.from[2] - 90 * e
            local x, y = cur_x, cur_y
            if not demo.carry_done then
                handle(g_at("pan", tx, ty, { wdx = tx - x, wdy = ty - y,
                                             phase = (not demo.carry) and "began" or (f >= 1 and "ended" or "changed") }), dt)
                demo.carry = true
                if f >= 1 then demo.carry_done = true; q.vx, q.vy, q.lifted = 0, 0, false end
            end
            hand_to(tx, ty, 1)
        end
    end },
}
local function reset_demo()
    demo.step, demo.u = 1, 0
    demo.tapped, demo.swiped, demo.panning, demo.done_pan = false, false, false, false
    demo.carry_done = false
end

local function run_demo(dt)
    local step = demo[demo.step]
    local first = demo.u == 0
    demo.u = demo.u + dt
    local last = demo.u >= step[1]
    step[2](demo.u, dt, first, last)
    if last then
        hx, hy = cur_x, cur_y
        demo.step = demo.step + 1
        demo.u = 0
        if demo.step > #demo then reset_demo() end
    end
end

function init(entity)
    math.randomseed(130)
    for i = 0, N - 1 do
        local p = { i = i + 1, e = Entity.find_by_name("Photo" .. i), s = Entity.find_by_name("Shadow" .. i),
                    x = DECK_X, y = DECK_Y, rot = 0, sc = 1, vx = 0, vy = 0, vr = 0, lift = 0,
                    dealt = false, lifted = false }
        photos[#photos + 1] = p
        order[#order + 1] = p
        cards[#cards + 1] = Entity.find_by_name("Card" .. i)
    end
    for i = N, 1, -1 do deck[#deck + 1] = i end
    -- A few already on the table.
    for _, s in ipairs(START) do
        local p = photos[table.remove(deck)]
        p.dealt, p.x, p.y, p.rot = true, s[1], s[2], s[3]
        raise(p)
    end
    show_deck()
    status = Entity.find_by_name("Status")
    logui = Entity.find_by_name("Log")
    hand = Entity.find_by_name("Touch")
    reset_demo()
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    local gestures = Input.get_gestures()
    if #gestures > 0 then
        if idle >= 4.0 then
            -- A real finger: the demo lets go of whatever it held.
            hand_to(0, -2000, 0)
            drag, twist, panned_photo = nil, nil, false
            for _, p in ipairs(photos) do p.lifted = false end
            hx, hy = 0, -300
            reset_demo()
        end
        idle = 0
        for _, g in ipairs(gestures) do handle(g, dt) end
    else
        idle = idle + dt
    end
    local touching = #Input.get_touches() > 0 or Input.is_mouse_left_pressed()
    if idle >= 4.0 and not touching then run_demo(dt) end
    for _, p in ipairs(photos) do
        if p.lifted and not touching and idle < 4.0 then p.lifted = false end
        p.lift = p.lift + ((p.lifted and 1 or 0) - p.lift) * math.min(1, dt * 10)
        if p.returning then
            local f = math.min(1, dt * 5)
            p.x, p.y = p.x + (DECK_X - p.x) * f, p.y + (DECK_Y - p.y) * f
            p.rot, p.sc = p.rot * (1 - f), p.sc + (1 - p.sc) * f
            if math.abs(p.x - DECK_X) + math.abs(p.y - DECK_Y) < 4 then
                p.returning, p.dealt = false, false
                deck[#deck + 1] = p.i
                show_deck()
            end
        elseif p.goal then
            local f = math.min(1, dt * 7)
            p.x, p.y = p.x + (p.goal[1] - p.x) * f, p.y + (p.goal[2] - p.y) * f
            p.rot, p.sc = p.rot + (p.goal[3] - p.rot) * f, p.sc + (p.goal[4] - p.sc) * f
            if math.abs(p.sc - p.goal[4]) < 0.002 and math.abs(p.x - p.goal[1]) < 0.5 then p.goal = nil end
        elseif p ~= drag and p.dealt then
            -- Sliding on the cloth: slowed, kept on the table.
            p.x, p.y = p.x + p.vx * dt, p.y + p.vy * dt
            p.rot = p.rot + p.vr * dt
            local damp = math.exp(-3.2 * dt)
            p.vx, p.vy, p.vr = p.vx * damp, p.vy * damp, p.vr * damp
            if math.abs(p.x) > 560 then p.x = (p.x > 0 and 560 or -560); p.vx = -p.vx * 0.4 end
            if math.abs(p.y) > 290 then p.y = (p.y > 0 and 290 or -290); p.vy = -p.vy * 0.4 end
        end
        local shown = p.dealt or p.returning
        local k = p.sc * (1 + 0.06 * p.lift)
        Transform.set_position(p.e, p.x, p.y)
        Transform.set_rotation(p.e, p.rot)
        Transform.set_scale(p.e, k, k)
        Sprite.set_color(p.e, 1, 1, 1, shown and 1 or 0)
        local off = (6 + 22 * p.lift) * p.sc
        Transform.set_position(p.s, p.x + off, p.y - off * 1.3)
        Transform.set_rotation(p.s, p.rot)
        Transform.set_scale(p.s, k * (1 + 0.04 * p.lift), k * (1 + 0.04 * p.lift))
        Sprite.set_color(p.s, 1, 1, 1, shown and (0.5 - 0.18 * p.lift) or 0)
    end
    if idle >= 4.0 then
        UI.set_text(status, "left alone, it plays the gestures itself")
    else
        UI.set_text(status, string.format("%d in the deck", #deck))
    end
end
