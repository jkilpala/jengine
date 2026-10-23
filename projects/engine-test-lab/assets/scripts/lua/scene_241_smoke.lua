-- Scene 241: `smoke`. The paddle sweeps through the air and pushes the
-- smoke aside; F turns the fan (source 2) on and off, P puffs at the paddle.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/241-smoke.scene"
local air, paddle
local t = 0
local fan = false

function init(entity)
    air = Entity.find_by_name("Air")
    paddle = Entity.find_by_name("Paddle")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    local x = math.sin(t * 0.7) * 380
    Transform.set_position(paddle, x, 120)
    if Input.is_key_just_pressed("F") then
        fan = not fan
        Smoke.set(air, "sources.2.active", fan)
    end
    if Input.is_key_just_pressed("P") then Smoke.puff(air, x, 120, 60, 4, 0, 0) end
    if Input.is_key_just_pressed("R") then Scene.load(SCENE_PATH) end
end
