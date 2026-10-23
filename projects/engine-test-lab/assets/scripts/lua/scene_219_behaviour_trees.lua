-- Scene 219: the guard's behaviour tree is in the scene (its
-- `behavior_tree` component); this script, on the guard, only moves the
-- thief, makes noises (into the guard's blackboard) and answers the tree's
-- `call` leaf, `catch`.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/219-behaviour-trees.scene"
local e = {}
local guard
local caught = 0

function init(entity)
    guard = entity
    for _, n in ipairs({ "Thief", "Noise", "Line0", "Line1" }) do e[n] = Entity.find_by_name(n) end
end

-- The tree's `call` leaf: it waits until the blackboard says how it went.
function on_catch(entity)
    caught = caught + 1
    Transform.set_position(e.Thief, -300, -240)
    Entity.set_custom(guard, "Blackboard", "catch", "success")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local x, y = Transform.get_position(e.Thief)
    local dx, dy = 0, 0
    if Input.is_key_pressed("Left") then dx = dx - 1 end
    if Input.is_key_pressed("Right") then dx = dx + 1 end
    if Input.is_key_pressed("Up") then dy = dy + 1 end
    if Input.is_key_pressed("Down") then dy = dy - 1 end
    if dx ~= 0 or dy ~= 0 then
        Transform.set_position(e.Thief, x + dx * 230 * dt, y + dy * 230 * dt)
    end
    if Input.is_key_just_pressed("Space") then
        Entity.set_custom(guard, "Blackboard", "noise", { x, y })
    end
    -- The marker shows the noise the guard remembers (it clears it after
    -- searching).
    local noise = Entity.get_custom(guard, "Blackboard", "noise")
    if type(noise) == "table" then
        Transform.set_position(e.Noise, noise[1], noise[2])
    else
        Transform.set_position(e.Noise, 0, -5000)
    end
    UI.set_text(e.Line0, "caught: " .. caught)
    UI.set_text(e.Line1, "running: " .. tostring(Entity.get_custom(guard, "Blackboard", "_running") or ""))
end
