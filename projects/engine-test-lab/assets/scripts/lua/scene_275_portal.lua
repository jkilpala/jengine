-- Scene 275: `portal`. A ball moved by its velocity alone goes into A and
-- comes out of B turned; Space sends it again, C turns carrying off (the
-- portals stay windows).

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/275-portal.scene"
local line, ball, a, b
local carry = true
local t = 0

local function send()
    Transform.set_position(ball, 100, 20)
    Physics.set_velocity(ball, -260, 0)
    t = 0
end

function init(entity)
    line = Entity.find_by_name("Line0")
    ball = Entity.find_by_name("Ball")
    a = Entity.find_by_name("A")
    b = Entity.find_by_name("B")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if Input.is_key_just_pressed("C") then
        carry = not carry
        Portal.set(a, "carry", carry)
        Portal.set(b, "carry", carry)
    end
    local x, y = Transform.get_position(ball)
    if Input.is_key_just_pressed("Space") or t > 4 or x < -640 or y > 380 then send() end
    UI.set_text(line, "carry " .. (carry and "on" or "off") .. "   ball at " .. math.floor(x + 0.5) .. ", " .. math.floor(y + 0.5))
end
