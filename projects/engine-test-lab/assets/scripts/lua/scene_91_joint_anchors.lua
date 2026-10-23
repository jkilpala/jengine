-- Scene 91 - a joint has two ends
--
-- The same vehicle twice, and the only difference is one field on the
-- wheel joints.
--
-- A revolute joint pins a point on one body to a point on the other.
-- `anchor` is the point on the jointed entity - the wheel, which turns
-- about its own centre, so `[0, 0]`. `anchor_b` is the mount on the
-- target: the back of the chassis, the front of it.
--
-- `JointType::Revolute` used to carry only `anchor`, and rapier's
-- `local_anchor2` was hardcoded to `[0, 0]`. `Fixed`, `Rope` and `Spring`
-- all carried both ends; these two did not. That is invisible for the one
-- revolute joint this repo had - a test pendulum hanging from a peg,
-- where the far end *is* the origin - and it makes a vehicle impossible:
-- both wheels pin to the middle of the chassis, and the only way round it
-- from the other side would be a `Joint2D` per wheel on one entity, which
-- hecs cannot hold.
--
-- Left: `anchor_b` left at its default. Both wheels sit at the chassis's
-- centre, so it has no wheelbase, it cannot stand up, and the driven
-- wheel just spins the whole thing.
--
-- Right: `anchor_b` set to each mount. A wheel at each end, and it
-- drives.
--
-- Press F4 to outline the joints. Each one draws a line between its two
-- anchors, which is the clearest possible picture of what this is about -
-- and `debug_draw` hardcoded the same zero, so before this it drew the
-- joint where the simulation was not.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/91-joint-anchors.scene"

-- Where each rig is put back when it is reset, matching the scene.
local RIGS = {
    { tag = "Bad", x = -330, label = "no anchor_b" },
    { tag = "Good", x = 180, label = "anchor_b set" },
}
local Y = -140
local REAR_X, FRONT_X, MOUNT_Y = -68, 68, -28

local readout
local parts = {}
local t = 0
local placed_at = 0

local function reset()
    for _, rig in ipairs(RIGS) do
        local p = parts[rig.tag]
        if p ~= nil then
            Transform.set_position(p.chassis, rig.x, Y)
            Transform.set_rotation(p.chassis, 0)
            Physics.set_velocity(p.chassis, 0, 0)
            Physics.set_angular_velocity(p.chassis, 0)
            -- The broken rig's wheels are pinned to the chassis centre
            -- whatever we put them at; putting them where they *should*
            -- be is what makes that obvious on the first frame.
            Transform.set_position(p.rear, rig.x + REAR_X, Y + MOUNT_Y)
            Transform.set_position(p.front, rig.x + FRONT_X, Y + MOUNT_Y)
            for _, w in ipairs({ p.rear, p.front }) do
                Physics.set_velocity(w, 0, 0)
                Physics.set_angular_velocity(w, 0)
            end
        end
    end
    placed_at = t
end

function init(entity)
    readout = Entity.find_by_name("Readout")
    for _, rig in ipairs(RIGS) do
        parts[rig.tag] = {
            chassis = Entity.find_by_name("Chassis" .. rig.tag),
            rear = Entity.find_by_name("Rear" .. rig.tag),
            front = Entity.find_by_name("Front" .. rig.tag),
        }
    end
    reset()
    log("[scene_91] ready - the left rig should collapse, the right one should drive")
end

-- How far apart the two wheels are. That single number is the bug: with
-- one anchor it is zero, because both wheels pin to the same point.
local function wheelbase(p)
    if p.rear == nil or p.front == nil then
        return 0
    end
    local rx, ry = Transform.get_position(p.rear)
    local fx, fy = Transform.get_position(p.front)
    local dx, dy = fx - rx, fy - ry
    return math.sqrt(dx * dx + dy * dy)
end

function update(entity, dt)
    t = t + dt
    if Input.is_key_just_pressed("Space") then
        reset()
    end
    -- Let them drive off, then put them back, so the scene is watchable
    -- rather than two shapes that left the screen once.
    if t - placed_at > 7.0 then
        reset()
    end

    local want = FRONT_X - REAR_X
    local bits = {}
    local ok = true
    for _, rig in ipairs(RIGS) do
        local p = parts[rig.tag]
        if p ~= nil then
            local base = wheelbase(p)
            local expected = (rig.tag == "Good") and want or 0
            if math.abs(base - expected) > 20 then
                ok = false
            end
            bits[#bits + 1] = string.format("%s: wheelbase %.0f", rig.label, base)
        end
    end
    if readout ~= nil then
        local head = ok and "AS EXPECTED" or "UNEXPECTED"
        Text.set(readout, string.format("%s - %s   (it should be 0 and %d)",
            head, table.concat(bits, "   |   "), want))
    end
    nav.tick(SCENE_PATH)
end
