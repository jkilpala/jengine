-- Scene 253: `trajectory`. The ball's path is predicted from its launch
-- velocity as the aim sweeps; thrown, its trail runs down the prediction.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/253-trajectory.scene"
local HOME_X, HOME_Y = -480.0, -150.0
local CRATE_X, CRATE_Y = 400, -220
local ball, crate, line
local t, state, shown = 0, "aim", true
local angle, speed = 0.7, 620
local aimed = "-"

function init(entity)
    ball = Entity.find_by_name("Ball")
    crate = Entity.find_by_name("Crate")
    line = Entity.find_by_name("Line0")
end

local function velocity()
    return math.cos(angle) * speed, math.sin(angle) * speed
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("V") then
        shown = not shown
        Trajectory.set(ball, "active", shown)
    end
    t = t + dt
    if state == "aim" then
        angle = 0.75 + math.sin(t * 0.9) * 0.35
        speed = 700 + math.sin(t * 0.37) * 160
        local vx, vy = velocity()
        Trajectory.set(ball, "velocity", { vx, vy })
        if t > 3.0 or Input.is_key_just_pressed("Space") then
            Trail.clear(ball)
            Trail.set_emitting(ball, true)
            Physics.set_velocity(ball, vx, vy)
            -- Keep the line drawn at the throw: stop writing it.
            Trajectory.set(ball, "line", "")
            Trajectory.set(ball, "hit_line", "")
            state, t = "fly", 0
        end
    elseif t > 3.0 then
        Trail.set_emitting(ball, false)
        Transform.set_position(ball, HOME_X, HOME_Y)
        Physics.set_velocity(ball, 0, 0)
        Transform.set_position(crate, CRATE_X, CRATE_Y)
        Transform.set_rotation(crate, 0)
        Physics.set_velocity(crate, 0, 0)
        Trajectory.set(ball, "line", "Seen")
        Trajectory.set(ball, "hit_line", "SeenHit")
        state, t = "aim", 0
    end
    -- What the aim predicts; kept through the flight, when the prediction
    -- is of a ball already in the air.
    if state == "aim" then
        local hit = Entity.get_custom(ball, "Trajectory", "hit")
        aimed = (hit ~= nil and hit ~= "") and hit or "-"
    end
    UI.set_text(line, state .. "   prediction " .. (shown and "on" or "off") .. "   hits " .. aimed)
end
