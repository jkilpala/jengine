-- Scene 276: `jelly`. Two blocks dropped on a floor, one plain and one a
-- jelly; D drops them again, Space pokes the jelly. They drop by
-- themselves every few seconds.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/276-jelly.scene"
local line, plain, jelly
local t = 0

local function drop()
    Transform.set_position(plain, -220, 150)
    Transform.set_position(jelly, 220, 150)
    Physics.set_velocity(plain, 0, 0)
    Physics.set_velocity(jelly, 0, 0)
    t = 0
end

local function poke()
    local x, y = Transform.get_position(jelly)
    Jelly.poke(jelly, x + 70, y, 500, 60)
end

function init(entity)
    line = Entity.find_by_name("Line0")
    plain = Entity.find_by_name("Plain")
    jelly = Entity.find_by_name("Jelly")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if Input.is_key_just_pressed("D") or t > 4 then drop() end
    if Input.is_key_just_pressed("Space") or (t > 2.2 and t - dt <= 2.2) then poke() end
    UI.set_text(line, "drop in " .. math.floor(4 - t + 0.5))
end
