-- Scene 248: `floating_origin`. The traveller strays past 1,200 units
-- about once a second and the engine moves the whole world back under it;
-- the total moved is its custom `FloatingOrigin.x`.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/248-floating-origin.scene"
local traveller, line0, line1
local on, paused = true, false
local shifts, last = 0, 0

function init(entity)
    traveller = Entity.find_by_name("Traveller")
    line0 = Entity.find_by_name("Line0")
    line1 = Entity.find_by_name("Line1")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("O") then
        on = not on
        FloatingOrigin.set(traveller, "enabled", on)
    end
    if Input.is_key_just_pressed("Space") then
        paused = not paused
        Physics.set_velocity(traveller, paused and 0 or 1500, 0)
    end
    local x = Transform.get_position(traveller)
    local moved = Entity.get_custom(traveller, "FloatingOrigin", "x") or 0
    if moved ~= last then
        shifts = shifts + 1
        last = moved
    end
    UI.set_text(line0, string.format("x %d   true distance %d", math.floor(x + 0.5), math.floor(x + moved + 0.5)))
    UI.set_text(line1, string.format("origin %s   world moved back %d times", on and "on" or "OFF", shifts))
end
