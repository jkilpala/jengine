-- Scene 188: an ik_chain on a chain's end bone bends every bone above it
-- (FABRIK) so the tip reaches the target. The middle chain follows the
-- cursor and the left one the same point in its own column, both through
-- Skeleton.set_ik_target; the third names an entity to follow.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/188-ik-chains.scene"
local NAMES = { "Free", "Limited", "Named" }
local ends, tips, targets = {}, {}, {}
local orbiter, status
local t, weight, want = 0, 1, 1

function init(entity)
    for i, n in ipairs(NAMES) do
        ends[i] = Entity.find_by_name(n .. "6")
        tips[i] = Entity.find_by_name(n .. "Tip")
        targets[i] = Entity.find_by_name(n .. "Target")
    end
    orbiter = Entity.find_by_name("Orbiter")
    status = Entity.find_by_name("Status")
end

local function miss(i, tx, ty)
    local x, y = Transform.get_world_position(tips[i])
    return math.floor(math.sqrt((x - tx) ^ 2 + (y - ty) ^ 2) + 0.5)
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    local mx, my = Input.get_mouse_world_position()
    -- The cursor drives the middle chain, and the left one the same point
    -- moved into its own column.
    for i = 1, 2 do
        local tx, ty = mx - (2 - i) * 420, my
        Skeleton.set_ik_target(ends[i], tx, ty)
        Transform.set_position(targets[i], tx, ty)
    end
    local ox, oy = 420 + 130 * math.cos(t * 0.9), -20 + 150 * math.sin(t * 1.3)
    Transform.set_position(orbiter, ox, oy)
    Transform.set_position(targets[3], ox, oy)
    if Input.is_key_just_pressed("W") then want = 1 - want end
    weight = weight + (want - weight) * math.min(1, dt * 4)
    Skeleton.set_ik_weight(ends[3], weight)
    if status then
        UI.set_text(status, "tip to target: free " .. miss(1, mx - 420, my) .. "   limited " .. miss(2, mx, my)
            .. "   named " .. miss(3, ox, oy) .. "   (weight " .. string.format("%.2f", weight) .. ")")
    end
end
