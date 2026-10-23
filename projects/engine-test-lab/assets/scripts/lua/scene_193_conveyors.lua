-- Scene 193: a `conveyor` on a static collider carries what rests on it
-- along its +x. Crates are fed onto the top belt; Space reverses the lower
-- belt with Physics.set_conveyor_speed.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/193-conveyors.scene"
local lower, status
local crates = {}
local feed, speed = 0, -120

function init(entity)
    lower = Entity.find_by_name("Lower")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    feed = feed - dt
    if feed <= 0 then
        local c = Entity.spawn_prefab("PhysCrate")
        Transform.set_position(c, -220, 170)
        crates[#crates + 1] = c
        -- Ten at a time: the oldest goes.
        if #crates > 10 then Entity.destroy(table.remove(crates, 1)) end
        feed = 1.6
    end
    if Input.is_key_just_pressed("Space") and lower then
        speed = -speed
        Physics.set_conveyor_speed(lower, speed)
    end
    if status then UI.set_text(status, "lower belt speed " .. speed .. "   crates " .. #crates) end
end
