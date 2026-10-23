-- 126 Billiard Hall: a player lines up shot after shot. While the cue
-- swings round onto its ball, the cue ball's `trajectory` is set to the
-- velocity the shot would give it, and the engine steps a copy of the table
-- ahead to draw where it goes (the dotted line), the ball it strikes and
-- that ball's path (the gold one), and where they touch (the ring). Then
-- the shot is struck with exactly that velocity and goes down the line.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/126-billiard-hall.scene"
local POCKETS = { { -520.0, 270.0 }, { 0.0, 276.0 }, { 520.0, 270.0 }, { -520.0, -270.0 }, { 0.0, -276.0 }, { 520.0, -270.0 } }
local R, W, H = 13.0, 520.0, 270.0
local HOME = { -260, 0 }

local cue, stick, stick_shadow, ring, aim, ghost, status
local balls = {}
local captions, lines = true, true
local state, t = "aim", 0
local angle, target_angle, power, target_power = 0, 0, 900, 900
local pull, quiet, shots = 0, 0, 0
local seed = 1

local function rand()
    seed = (seed * 1103515245 + 12345) % 2147483648
    return seed / 2147483648
end

local function pos(e)
    local x, y = Transform.get_position(e)
    return x, y
end

local function potted(x, y)
    if math.abs(x) > W - 8 and math.abs(y) > H - 8 then return true end
    if math.abs(x) < 34 and math.abs(y) > H - 4 then return true end
    return false
end

-- The shot: the ghost-ball aim at a pocket, scored by how straight the cut
-- is and how short, a little chance mixed in.
local function choose()
    local cx, cy = pos(cue)
    local best, best_score = nil, -1e9
    for _, b in ipairs(balls) do
        if b.alive then
            local bx, by = pos(b.e)
            for _, p in ipairs(POCKETS) do
                local px, py = p[1] - bx, p[2] - by
                local pl = math.sqrt(px * px + py * py)
                local gx, gy = bx - px / pl * 2 * R, by - py / pl * 2 * R
                local dx, dy = gx - cx, gy - cy
                local dl = math.sqrt(dx * dx + dy * dy)
                if dl > 1 then
                    local cut = (dx * px + dy * py) / (dl * pl)
                    local score = cut * 3 - (dl + pl) / 900 + rand() * 0.6
                    if cut > 0.35 and score > best_score then
                        best_score, best = score, { dx / dl, dy / dl, dl + pl }
                    end
                end
            end
        end
    end
    if best == nil then
        local a = rand() * 6.283
        return math.cos(a), math.sin(a), 700
    end
    return best[1], best[2], math.min(1900, 700 + best[3] * 1.2)
end

local function begin_aim()
    local dx, dy, p = choose()
    target_angle = math.atan(dy, dx)
    if math.atan2 ~= nil then target_angle = math.atan2(dy, dx) end
    -- Swing in from a little to one side, as a hand does.
    angle = target_angle + (rand() - 0.5) * 1.2
    target_power, power = p, p * 0.6
    state, t = "aim", 0
    Trajectory.set(cue, "active", true)
end

function init(entity)
    cue = Entity.find_by_name("Cue")
    stick = Entity.find_by_name("Stick")
    stick_shadow = Entity.find_by_name("StickShadow")
    ring = Entity.find_by_name("Ring")
    aim = Entity.find_by_name("Aim")
    ghost = Entity.find_by_name("Ghost")
    status = Entity.find_by_name("Status")
    for k = 1, 15 do balls[#balls + 1] = { e = Entity.find_by_name("Ball" .. k), alive = true } end
    begin_aim()
end

local function place_stick(cx, cy, back, show)
    local ux, uy = math.cos(angle), math.sin(angle)
    local tx, ty = cx - ux * (R + 4 + back), cy - uy * (R + 4 + back)
    if not show then tx, ty = -9999, 0 end
    Transform.set_position(stick, tx, ty)
    Transform.set_rotation(stick, angle)
    Transform.set_position(stick_shadow, tx + 6, ty - 10)
    Transform.set_rotation(stick_shadow, angle)
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("A") then
        lines = not lines
        Line.set(aim, "color", { 1, 1, 1, lines and 0.75 or 0 })
        Line.set(ghost, "color", { 1, 0.69, 0.07, lines and 0.8 or 0 })
    end
    t = t + dt
    local cx, cy = pos(cue)
    if state == "aim" then
        -- Ease onto the line, then hold it a moment.
        local k = 1 - math.exp(-dt * 2.2)
        angle = angle + (target_angle - angle) * k
        power = power + (target_power - power) * k
        local vx, vy = math.cos(angle) * power, math.sin(angle) * power
        Trajectory.set(cue, "velocity", { vx, vy })
        place_stick(cx, cy, 6 + 30 * (power / 1500), true)
        if t > 2.6 or Input.is_key_just_pressed("Space") then state, t = "draw", 0 end
    elseif state == "draw" then
        pull = math.min(1, t / 0.5)
        place_stick(cx, cy, 6 + 30 * (power / 1500) + 50 * pull, true)
        if t > 0.65 then state, t = "strike", 0 end
    elseif state == "strike" then
        local f = math.min(1, t / 0.07)
        place_stick(cx, cy, (6 + 30 * (power / 1500) + 50) * (1 - f), true)
        if f >= 1 then
            Physics.set_velocity(cue, math.cos(angle) * power, math.sin(angle) * power)
            Trajectory.set(cue, "active", false)
            state, t, quiet, shots = "roll", 0, 0, shots + 1
        end
    elseif state == "roll" then
        place_stick(cx, cy, 30 + math.min(1, t * 2) * 80, t < 0.5)
        local moving = false
        for _, b in ipairs(balls) do
            if b.alive then
                local bx, by = pos(b.e)
                if potted(bx, by) then
                    b.alive = false
                    Entity.destroy(b.e)
                else
                    local vx, vy = Physics.get_velocity(b.e)
                    if vx * vx + vy * vy > 64 then moving = true end
                end
            end
        end
        local vx, vy = Physics.get_velocity(cue)
        if potted(cx, cy) then
            Transform.set_position(cue, 0, -9999)
            Physics.set_velocity(cue, 0, 0)
        elseif vx * vx + vy * vy > 64 then
            moving = true
        end
        quiet = moving and 0 or quiet + dt
        if quiet > 0.5 then
            local left = 0
            for _, b in ipairs(balls) do if b.alive then left = left + 1 end end
            if left == 0 or shots >= 14 then Scene.load(SCENE) return end
            if cy < -5000 then
                Transform.set_position(cue, HOME[1], HOME[2])
                Physics.set_velocity(cue, 0, 0)
            end
            begin_aim()
        end
    end
    -- The ring where the cue ball meets the struck ball.
    local hit = Entity.get_custom(cue, "Trajectory", "hit")
    local showing = lines and state ~= "roll" and hit ~= nil and hit ~= ""
    if showing then
        Transform.set_position(ring, Entity.get_custom(cue, "Trajectory", "hit_x"), Entity.get_custom(cue, "Trajectory", "hit_y"))
    else
        Transform.set_position(ring, -9999, 0)
    end
    UI.set_text(status, state == "roll" and "rolling" or ("lining up on " .. (hit ~= nil and hit ~= "" and hit or "a cushion")))
end
