-- Scene 172: walks the blue characters across the red ones. The ordering
-- is the engine's: on the right each character has a `sorting_group`.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/172-sorting-groups.scene"
local loose_b, group_b, status
local t, paused, depth = 0, false, -90

function init(entity)
    loose_b = Entity.find_by_name("LooseB")
    group_b = Entity.find_by_name("GroupB")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then paused = not paused end
    if Input.is_key_pressed("Up") then depth = math.min(10, depth + 120 * dt) end
    if Input.is_key_pressed("Down") then depth = math.max(-140, depth - 120 * dt) end
    if not paused then t = t + dt end
    local dx = math.sin(t * 0.7) * 110
    if loose_b then Transform.set_position(loose_b, -330 + dx, depth) end
    if group_b then Transform.set_position(group_b, 330 + dx, depth) end
    if status then
        UI.set_text(status, "blue " .. (depth < -40 and "nearer (lower)" or "further (higher)") .. " than red")
    end
end
