-- 162 Instant Replay: a pinball table and an input replay. R records a
-- run from a freshly loaded table; P loads it again and plays the recording
-- back - every frame's input and frame time, so the physics and this script
-- go exactly the same way. The keys that steer the replay are read through
-- `Replay.live_key`, the player's own keyboard: everything else reads the
-- replayed input while it plays, and the R and P presses never end up in it.

local nav = require("nav")
local SCENE = "scenes/162-instant-replay.scene"
local BUMPERS = { { -100.0, 140.0 }, { 60.0, 140.0 }, { -20.0, 50.0 } }
local LANES = { -130.0, -60.0, 10.0 }
local LANE_Y = 285.0
local SLINGS = { { -205.0, -80.0, -150.0, -200.0, -1.0 }, { 165.0, -80.0, 110.0, -200.0, 1.0 } }
local FLIP_SPEED = 26
local LANE_X, LANE_Y0 = 227, -334
local e = {}
local captions = true
local score, ball_no, balls = 0, 1, 3
local flash = { 0, 0, 0 }
local sling_cool = { 0, 0 }
local bumper_cool = { 0, 0, 0 }
local lit = { false, false, false }
local lane_cool = { 0, 0, 0 }
local bonus_t = 0
local message, message_t = "", 0
local charge, launching = 0, false
local attract = true
local auto = { 0, 0 }
local auto_cool = { 0, 0 }
local auto_wait = 0
local over_t = 0
local rec_t, t = 0, 0

local function say(text, seconds)
    message, message_t = text, seconds
end

local function flipper(name, side, up)
    -- The motor turns the pivot relative to the flipper, so the flipper
    -- turns the other way: positive is down for the left flipper.
    local s = up and -FLIP_SPEED or FLIP_SPEED
    Physics.set_motor_speed(name, s * side)
end

local function reset_ball()
    Transform.set_position(e.Ball, LANE_X, LANE_Y0)
    Physics.set_velocity(e.Ball, 0, 0)
    charge = 0
end

local function kick(vx, vy, nx, ny, speed)
    -- Away along the normal, at least `speed`.
    local into = vx * nx + vy * ny
    if into < 0 then vx, vy = vx - 2 * into * nx, vy - 2 * into * ny end
    local along = vx * nx + vy * ny
    if along < speed then vx, vy = vx + (speed - along) * nx, vy + (speed - along) * ny end
    Physics.set_velocity(e.Ball, vx, vy)
end

function init(entity)
    for _, n in ipairs({ "Ball", "FlipperL", "FlipperR", "Score", "BallLabel", "Message", "RecLamp", "ReplayState",
        "Bar", "ReplayInfo", "Plunger", "Status", "Lane0", "Lane1", "Lane2", "Bumper0", "Bumper1", "Bumper2", "SlingL", "SlingR" }) do
        e[n] = Entity.find_by_name(n)
    end
    Physics.set_gravity(0, -6)
    flipper("FlipperL", 1, false)
    flipper("FlipperR", -1, false)
end

local function replay_controls(dt)
    local status = Replay.status()
    if status == "recording" then rec_t = rec_t + dt end
    if Replay.live_key("R") then
        if status == "recording" then
            Replay.stop()
        else
            Replay.record()
            Scene.load(SCENE)
        end
    elseif Replay.live_key("P") then
        if status == "playing" then
            Replay.stop()   -- take over: live input from here on
        elseif Replay.frames() > 0 then
            Replay.play()
            Scene.load(SCENE)
        end
    end
    -- Two minutes is a long run; stop there.
    if status == "recording" and rec_t > 120 then Replay.stop() end
end

local function replay_panel()
    local status = Replay.status()
    local frames = math.floor(Replay.frames())
    local seconds = string.format("%.1f", frames / 60)
    if status == "recording" then
        local on = math.floor(t * 2.5) % 2 == 0
        Sprite.set_color(e.RecLamp, on and 1.0 or 0.3, 0.05, 0.05, 1)
        Text.set(e.ReplayState, "REC  " .. string.format("%.1f", rec_t) .. " s")
        Sprite.set_fill(e.Bar, 0)
        Text.set(e.ReplayInfo, "Recording from a fresh table. R stops.")
    elseif status == "playing" then
        Sprite.set_color(e.RecLamp, 0.25, 1.0, 0.4, 1)
        Text.set(e.ReplayState, "REPLAY")
        Sprite.set_fill(e.Bar, Replay.progress())
        Text.set(e.ReplayInfo, "Frame " .. math.floor(Replay.progress() * frames + 0.5) .. " of " .. frames
            .. " - played back exactly. P takes over.")
    else
        Sprite.set_color(e.RecLamp, 0.3, 0.05, 0.05, 1)
        Sprite.set_fill(e.Bar, status == "finished" and 1 or 0)
        if status == "finished" then
            Text.set(e.ReplayState, "THE END")
            Text.set(e.ReplayInfo, "That was the replay. P plays it again, R records a new run.")
        elseif frames > 0 then
            Text.set(e.ReplayState, "READY")
            Text.set(e.ReplayInfo, "A run of " .. seconds .. " s is kept. P plays it back.")
        else
            Text.set(e.ReplayState, "READY")
            Text.set(e.ReplayInfo, "R records a run from a fresh table.")
        end
    end
end

local function auto_play(dt, bx, by, vx, vy)
    -- Attract mode: flip when the ball comes down onto a flipper, launch
    -- when it waits in the lane. Any key of the player's turns it off.
    for i = 1, 2 do
        auto[i] = math.max(0, auto[i] - dt)
        auto_cool[i] = math.max(0, auto_cool[i] - dt)
    end
    if by < -205 and by > -300 and vy < 80 then
        local side = bx < -20 and 1 or 2
        if auto_cool[side] == 0 and math.abs(bx - (side == 1 and -75 or 35)) < 70 then
            auto[side], auto_cool[side] = 0.22, 0.6
        end
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    replay_controls(dt)
    replay_panel()

    local bx, by = Transform.get_position(e.Ball)
    local vx, vy = Physics.get_velocity(e.Ball)
    if Input.is_key_just_pressed("Left") or Input.is_key_just_pressed("Right") or Input.is_key_just_pressed("Space") then
        attract = false
    end
    if attract then auto_play(dt, bx, by, vx, vy) end
    local left = Input.is_key_pressed("Left") or Input.is_key_pressed("A") or (attract and auto[1] > 0)
    local right = Input.is_key_pressed("Right") or Input.is_key_pressed("D") or (attract and auto[2] > 0)
    flipper("FlipperL", 1, left)
    flipper("FlipperR", -1, right)

    -- The plunger: hold Space while the ball waits in the lane.
    local waiting = bx > 205 and by < -300 and math.abs(vy) < 30
    local pull = Input.is_key_pressed("Space")
    if attract and waiting then
        auto_wait = auto_wait + dt
        pull = auto_wait < 1.6
        if not pull then auto_wait = 0 end
    end
    if waiting and pull then
        charge = math.min(1, charge + dt * 1.1)
    elseif waiting and charge > 0 then
        Physics.set_velocity(e.Ball, 0, 760 + 520 * charge)
        charge = 0
    end
    Transform.set_position(e.Plunger, LANE_X, -372 - charge * 14)

    -- Bumpers kick the ball away and flash.
    for i, b in ipairs(BUMPERS) do
        bumper_cool[i] = math.max(0, bumper_cool[i] - dt)
        local dx, dy = bx - b[1], by - b[2]
        local d = math.sqrt(dx * dx + dy * dy)
        if d < 44 and bumper_cool[i] == 0 then
            kick(vx, vy, dx / d, dy / d, 620)
            bumper_cool[i], flash[i] = 0.12, 1
            score = score + 100
        end
        flash[i] = math.max(0, flash[i] - dt * 5)
        local f = flash[i]
        Sprite.set_color(e["Bumper" .. (i - 1)], 1.0 + f * 2, 0.35 + f * 2, 0.7 + f * 1.5, 1)
    end
    -- Slingshots kick off their long face.
    for i, s in ipairs(SLINGS) do
        sling_cool[i] = math.max(0, sling_cool[i] - dt)
        local ax, ay, cx, cy = s[1], s[2], s[3], s[4]
        local ex, ey = cx - ax, cy - ay
        local len = math.sqrt(ex * ex + ey * ey)
        local ux, uy = ex / len, ey / len
        local nx, ny = uy * s[5], -ux * s[5]
        local along = (bx - ax) * ux + (by - ay) * uy
        local off = (bx - ax) * nx + (by - ay) * ny
        local name = i == 1 and "SlingL" or "SlingR"
        if along > 8 and along < len - 8 and off < 17 and off > 0 and sling_cool[i] == 0 then
            kick(vx, vy, nx, ny, 480)
            sling_cool[i] = 0.15
            score = score + 10
            Sprite.set_color(e[name], 1.2, 1.4, 1.6, 1)
        elseif sling_cool[i] == 0 then
            Sprite.set_color(e[name], 0.3, 0.85, 1.0, 1)
        end
    end
    -- Rollover lanes: light all three for a bonus.
    for i, x in ipairs(LANES) do
        lane_cool[i] = math.max(0, lane_cool[i] - dt)
        if math.abs(bx - x) < 22 and math.abs(by - LANE_Y) < 16 and lane_cool[i] == 0 then
            lane_cool[i] = 0.5
            if not lit[i] then lit[i] = true; score = score + 500 end
        end
    end
    if lit[1] and lit[2] and lit[3] then
        score = score + 5000
        lit = { false, false, false }
        bonus_t = 1.2
        say("LANES COMPLETE\n+5000", 2)
    end
    bonus_t = math.max(0, bonus_t - dt)
    for i = 1, 3 do
        local on = lit[i] or (bonus_t > 0 and math.floor(bonus_t * 8) % 2 == 0)
        if on then
            Sprite.set_color(e["Lane" .. (i - 1)], 1.6, 1.2, 0.4, 1)
        else
            Sprite.set_color(e["Lane" .. (i - 1)], 0.35, 0.25, 0.1, 1)
        end
    end

    -- The drain.
    if by < -380 then
        if ball_no >= balls then
            say("GAME OVER\nscore " .. score, 3)
            over_t = 3
            score, ball_no = 0, 1
        else
            ball_no = ball_no + 1
            say("BALL " .. ball_no, 1.5)
        end
        reset_ball()
    end
    over_t = math.max(0, over_t - dt)

    message_t = math.max(0, message_t - dt)
    Text.set(e.Message, message_t > 0 and message or "")
    Text.set(e.Score, tostring(score))
    Text.set(e.BallLabel, "BALL " .. ball_no .. " OF " .. balls)
    UI.set_text(e.Status, attract and "Attract mode - press a flipper to play" or "")
end
