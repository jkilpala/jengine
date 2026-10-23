-- Scene 187: fixed_update(entity, dt) runs once before each physics step,
-- with the step's dt; update runs once a frame. The two controllers here
-- step with the physics.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/187-fixed-update.scene"
local ball, drone, status
local updates, fixeds, clock, last_dt = 0, 0, 0, 0
local ups, fps = 0, 0
local G = 981

function init(entity)
    ball = Entity.find_by_name("Ball")
    drone = Entity.find_by_name("Drone")
    status = Entity.find_by_name("Status")
end

function fixed_update(entity, dt)
    fixeds = fixeds + 1
    last_dt = dt
    -- A spring toward the mark at (-300, 40), damped, through the velocity
    -- (gravity included: it pulls between the steps).
    local x, y = Transform.get_position(ball)
    local vx, vy = Physics.get_velocity(ball)
    Physics.set_velocity(ball, vx + ((-300 - x) * 30 - vx * 4) * dt, vy + ((40 - y) * 30 - vy * 4 + G) * dt)
    -- A hover: hold y = 60 against gravity with a force for this step.
    local _, y = Transform.get_position(drone)
    local _, vy = Physics.get_velocity(drone)
    Physics.apply_force(drone, 0, G + (60 - y) * 40 - vy * 9)
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    updates = updates + 1
    clock = clock + dt
    if clock >= 1 then
        ups, fps = updates / clock, fixeds / clock
        updates, fixeds, clock = 0, 0, 0
    end
    if Input.is_key_just_pressed("Space") and drone then Physics.apply_impulse(drone, 0, -900) end
    if Input.is_key_just_pressed("K") and ball then Physics.apply_impulse(ball, 900, 0) end
    if status then
        UI.set_text(status, "update " .. math.floor(ups + 0.5) .. " a second   fixed_update "
            .. math.floor(fps + 0.5) .. " a second (dt " .. string.format("%.4f", last_dt) .. ")")
    end
end
