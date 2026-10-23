-- 133 Ember Storm: sparks that keep state. The cart's sparks and the ash
-- are GPU particles stepped by a compute pass (`simulate = "state"`), so
-- they are born where the brazier is (a rolling cart leaves a wake), bounce
-- down the steps and off the roof, settle on the cobbles and are swept
-- aside by the swinging ball. This script only rolls the cart and swings
-- the ball - by giving the ball its velocity, which the motion step moves
-- it by and the sparks read to be pushed. Space shows the closed form.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/133-ember-storm.scene"
local PX, PY, ARM = 40.0, 560.0, 762.0
local FLOOR = -260.0
local cart, ball, chain, status, sparks, ash, light
local captions, stateful, swinging = true, true, true
local t, swing = 0, 0

function init(entity)
    cart = Entity.find_by_name("Cart")
    ball = Entity.find_by_name("Ball")
    chain = Entity.find_by_name("Chain")
    sparks = Entity.find_by_name("Sparks")
    ash = Entity.find_by_name("Ash")
    light = Entity.find_by_name("FireLight")
    status = Entity.find_by_name("Status")
end

local function ball_at(a)
    return PX + math.sin(a) * ARM, PY - math.cos(a) * ARM
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("Space") then
        stateful = not stateful
        local mode = stateful and "state" or "closed"
        Particles.set(sparks, "simulate", mode)
        Particles.set(ash, "simulate", mode)
    end
    if Input.is_key_just_pressed("B") then swinging = not swinging end
    t = t + dt
    -- The cart rolls back and forth along the floor.
    Transform.set_position(cart, math.sin(t * 0.32) * 470, FLOOR)
    Lighting.set_intensity(light, 1.6 + math.sin(t * 17) * 0.15 + math.sin(t * 7.3) * 0.1)
    -- The ball swings low across the floor: its body is given the velocity
    -- that takes it to the next point of the arc.
    if swinging then swing = swing + dt end
    local a = math.sin(swing * 0.9) * 0.6
    local x, y = Transform.get_position(ball)
    local nx, ny = ball_at(math.sin((swing + (swinging and dt or 0)) * 0.9) * 0.6)
    if dt > 0 then Physics.set_velocity(ball, (nx - x) / dt, (ny - y) / dt) end
    local bx, by = ball_at(a)
    Line.set(chain, "points", { { PX, PY }, { bx, by } })
    UI.set_text(status, stateful and "sparks keep state: they bounce, settle and are swept"
        or "closed form: each spark a formula of its age - nothing to bounce or sweep")
end
