-- Scene 260: GPU particles that keep state (`simulate = "state"`), beside
-- the same emitter in closed form. M swings both emitters left and right, P
-- runs the paddles that push through each cloud.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/260-stateful-gpu-particles.scene"
local line, left, right, pl, pr
local moving, paddles, t, tp = false, true, 0, 0

function init(entity)
    line = Entity.find_by_name("Line0")
    left = Entity.find_by_name("Stateful")
    right = Entity.find_by_name("Closed")
    pl = Entity.find_by_name("PaddleL")
    pr = Entity.find_by_name("PaddleR")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("M") then moving = not moving end
    if Input.is_key_just_pressed("P") then paddles = not paddles end
    t = t + dt
    local dx = moving and math.sin(t * 1.5) * 160 or 0
    Transform.set_position(left, -300 + dx, 150)
    Transform.set_position(right, 300 + dx, 150)
    if paddles then tp = tp + dt end
    local v = paddles and math.cos(tp * 1.2) * 1.2 * 220 or 0
    Physics.set_velocity(pl, v, 0)
    Physics.set_velocity(pr, v, 0)
    UI.set_text(line, string.format("emitters %s   paddles %s", moving and "moving" or "still", paddles and "on" or "off"))
end
