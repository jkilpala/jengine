-- 140 Kart Circuit: the track is a `mode7` plane - one quad in perspective
-- - that follows the player's kart; the karts and the trackside are
-- `mode7_sprite`s, placed and scaled by the engine from their ground points,
-- each showing the view of itself the camera sees. The racing is here:
-- every kart steers for the next waypoint along the centre line, the player
-- by hand (Up / Left / Right / Down) or, left alone, by the same autopilot.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/140-kart-circuit.scene"
local WAYS = { { -700.0, -150.0 }, { -698.8, -248.0 }, { -695.6, -337.2 }, { -684.4, -417.4 }, { -659.5, -488.2 }, { -615.7, -549.6 }, { -555.2, -602.6 }, { -480.9, -646.2 }, { -395.0, -679.1 }, { -300.0, -700.0 }, { -189.0, -707.8 }, { -61.7, -703.4 }, { 70.0, -688.5 }, { 194.3, -664.7 }, { 302.8, -633.4 }, { 406.7, -593.7 }, { 503.1, -545.1 }, { 586.2, -487.3 }, { 650.0, -420.0 }, { 692.2, -338.7 }, { 716.4, -244.2 }, { 725.9, -144.1 }, { 724.1, -46.0 }, { 713.4, 45.3 }, { 691.1, 138.5 }, { 657.4, 229.8 }, { 613.4, 312.5 }, { 560.0, 380.0 }, { 492.7, 433.6 }, { 411.5, 476.8 }, { 324.4, 506.7 }, { 239.3, 520.2 }, { 162.4, 511.3 }, { 88.1, 467.0 }, { 15.9, 406.5 }, { -53.7, 353.0 }, { -120.0, 330.0 }, { -181.4, 352.6 }, { -238.6, 405.4 }, { -294.1, 465.6 }, { -350.6, 510.4 }, { -411.6, 522.6 }, { -480.7, 518.9 }, { -550.9, 499.8 }, { -613.5, 461.5 }, { -660.0, 400.0 }, { -686.9, 303.2 }, { -699.7, 175.2 }, { -703.0, 35.9 }, { -701.3, -94.4 } }
local NAMES = { "Player", "Blue", "Green", "Yellow" }
local TOP = { 215, 228, 220, 234 }
local karts = {}
local status, track
local skies = {}
local captions = true
local idle = 3

local function heading(dx, dy)
    if math.atan2 ~= nil then return math.atan2(dy, dx) end
    return math.atan(dy, dx)
end

local function wrap(a)
    while a > math.pi do a = a - 2 * math.pi end
    while a < -math.pi do a = a + 2 * math.pi end
    return a
end

local function nearest(x, y)
    local best, bi = 1e18, 1
    for i, w in ipairs(WAYS) do
        local d = (w[1] - x) ^ 2 + (w[2] - y) ^ 2
        if d < best then best, bi = d, i end
    end
    return bi
end

function init(entity)
    status = Entity.find_by_name("Status")
    track = Entity.find_by_name("Track")
    for i = 0, 2 do skies[#skies + 1] = Entity.find_by_name("Sky" .. i) end
    for i, n in ipairs(NAMES) do
        local e = Entity.find_by_name(n)
        karts[i] = { e = e, x = 0, y = 0, a = 0, v = 0, way = 1, lap = 0, lane = (i - 2.5) * 9, placed = false }
    end
end

local function autopilot(k, dt, top)
    -- Aim two waypoints ahead, shifted into the kart's own lane.
    local w = WAYS[(k.way + 1) % #WAYS + 1]
    local tx, ty = w[1] + math.sin(k.a) * k.lane, w[2] - math.cos(k.a) * k.lane
    local turn = wrap(heading(tx - k.x, ty - k.y) - k.a)
    k.a = k.a + math.max(-2.2 * dt, math.min(2.2 * dt, turn * 4 * dt))
    local corner = math.min(1, math.abs(turn) * 1.5)
    local target = top * (1 - 0.35 * corner)
    k.v = k.v + (target - k.v) * math.min(1, dt * 1.2)
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    local up, down = Input.is_key_pressed("Up"), Input.is_key_pressed("Down")
    local left, right = Input.is_key_pressed("Left"), Input.is_key_pressed("Right")
    if up or down or left or right then idle = 0 else idle = idle + dt end
    for i, k in ipairs(karts) do
        if not k.placed then
            k.x = ({ -684.6, -716.6, -685.3, -717.2 })[i]
            k.y = ({ -119.7, -120.3, -85.7, -86.3 })[i]
            k.a = -1.5512
            k.way = nearest(k.x, k.y)
            -- Behind the line: crossing it starts lap one, not two.
            if k.way ~= 1 then k.lap = -1 end
            k.placed = true
        end
        if i == 1 and idle < 3 then
            local steer = (left and 1 or 0) - (right and 1 or 0)
            k.a = k.a + steer * 2.0 * dt * math.min(1, k.v / 60)
            local target = up and TOP[i] or (down and 0 or k.v * 0.985)
            k.v = k.v + (target - k.v) * math.min(1, dt * (down and 2.5 or 1.1))
        else
            autopilot(k, dt, TOP[i])
        end
        k.x = k.x + math.cos(k.a) * k.v * dt
        k.y = k.y + math.sin(k.a) * k.v * dt
        -- The waypoint passed: the next one is nearer.
        local nxt = k.way % #WAYS + 1
        local w, n = WAYS[k.way], WAYS[nxt]
        if (n[1] - k.x) ^ 2 + (n[2] - k.y) ^ 2 < (w[1] - k.x) ^ 2 + (w[2] - k.y) ^ 2 then
            if nxt == 1 then k.lap = k.lap + 1 end
            k.way = nxt
        end
        Mode7.place(k.e, k.x, k.y, k.a)
    end
    -- The sky turns with the camera: a full turn is one picture across.
    local a = karts[1].a
    local off = ((-a / (2 * math.pi)) * 2560) % 2560
    for i, sky in ipairs(skies) do
        local x, y = Transform.get_position(sky)
        Transform.set_position(sky, off + (i - 2) * 2560, y)
    end
    local place = 1
    local me = karts[1].lap * 1000 + karts[1].way
    for i = 2, #karts do
        if karts[i].lap * 1000 + karts[i].way > me then place = place + 1 end
    end
    UI.set_text(status, "lap " .. (karts[1].lap + 1) .. "    place " .. place .. " of 4"
        .. (idle >= 3 and "    (autopilot)" or ""))
end
