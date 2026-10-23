-- Scene 183: ragdolls (`ragdoll` on a skeleton root, Ragdoll.set) and
-- joints that snap past a `break_force` (on_joint_break).

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/183-ragdolls-and-breaking-joints.scene"
local figure, spine, status
local limp, heavy, snaps = false, false, 0

function init(entity)
    figure = Entity.find_by_name("Figure")
    spine = Entity.find_by_name("Spine")
    status = Entity.find_by_name("Status")
    Event.on("snapped", entity)
end

function on_snapped(entity, data)
    snaps = snaps + 1
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("R") then
        Scene.load(SCENE_PATH)
        return
    end
    if Input.is_key_just_pressed("Space") and figure then
        limp = not limp
        Ragdoll.set(figure, "active", limp)
        -- A kick as it goes: an impulse is mass times speed, and a bone at
        -- the default density weighs about a thousand.
        if limp and spine then Physics.apply_impulse(spine, 900000, 250000) end
    end
    if Input.is_key_just_pressed("G") then
        heavy = not heavy
        Physics.set_gravity(0, heavy and -14.715 or -9.81)
    end
    if status then
        UI.set_text(status, (limp and "limp" or "standing") .. "   gravity " .. (heavy and "x1.5" or "x1")
            .. "   joints snapped: " .. snaps)
    end
end
