-- Scene 284: `flow_path` and `Flow.set`.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/284-flow-path.scene"
local line, water
local speed, grip = 120, 3
local floats = {}

local function show()
    Flow.set(water, "speed", speed)
    Flow.set(water, "grip", grip)
    UI.set_text(line, "speed " .. speed .. "   grip " .. grip)
end

function init(entity)
    line = Entity.find_by_name("Line0")
    water = Entity.find_by_name("Water")
    for i = 0, 5 do table.insert(floats, Entity.find_by_name("Float" .. i)) end
    show()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local changed = false
    if Input.is_key_just_pressed("Up") then speed = speed + 40; changed = true end
    if Input.is_key_just_pressed("Down") then speed = speed - 40; changed = true end
    if Input.is_key_just_pressed("Right") then grip = math.min(12, grip + 1); changed = true end
    if Input.is_key_just_pressed("Left") then grip = math.max(0, grip - 1); changed = true end
    if Input.is_key_just_pressed("R") then speed = -speed; changed = true end
    if Input.is_key_just_pressed("Space") then
        for i, e in ipairs(floats) do
            Transform.set_position(e, -540 + (i - 1) * 60, 120 + ((i - 1) % 3 - 1) * 30)
        end
    end
    if changed then show() end
end
