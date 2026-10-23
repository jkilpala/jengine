-- Scene 34 - Skeleton & IK
--
-- The rig is authored in the scene: SkeletonRoot on "Arm", three Bone
-- entities chained by `parent`, and an IkTwoBone on the tip. The solver
-- (animation/ik.rs, run by all three runtimes) walks up two parents from the
-- entity carrying IkTwoBone and rotates root + mid so the chain's tip reaches
-- the target. IkLookAt on the pupil just aims one bone.
--
-- This script only moves the targets, via Skeleton.set_ik_target(entity, x, y).
--
-- The rig also carries a SkeletalPlayer bound to animations/arm_wave.skelanim.
-- It starts paused so the IK is legible; P plays it, and while it runs the
-- clip and the IK solver both write bone rotations — the solver runs after the
-- player, so IK wins. That ordering is the thing worth seeing here.
--
-- A third arm, drawn in yellow lines on the right, is not in the scene at
-- all: Skeleton.spawn_rig builds it from assets/rigs/spawned_arm.rig, a bone
-- per entity. Until Oct 2026 no runtime loaded a .rig, so the call bound
-- everywhere and spawned nothing anywhere.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/34-skeleton-ik.scene"
local CLIP = "animations/arm_wave.skelanim"

local RIG = "rigs/spawned_arm.rig"

local tip, mirror_tip, pupil, cursor, readout
local spawned
local flip = false
local playing = false
local arm

function init(entity)
    arm = Entity.find_by_name("Arm")
    tip = Entity.find_by_name("ArmTip")
    -- The same rig mirrored and scaled: IK measures bones in world units and
    -- turns a mirrored bone round (BUGS #152), so this one reaches too.
    mirror_tip = Entity.find_by_name("MirrorArmTip")
    pupil = Entity.find_by_name("EyePupil")
    cursor = Entity.find_by_name("Cursor")
    readout = Entity.find_by_name("Readout")
    -- spawn_rig builds a .rig by its path under assets/; the handle names
    -- its root bone, and the bones exist from the next frame.
    spawned = Skeleton.spawn_rig(RIG)
    log("[scene_34] ready - mouse drives the IK, P plays the skeletal clip")
end

function update(entity, dt)
    -- Mouse position is in screen pixels with the origin top-left; the scene
    -- is world-space with the origin at the centre, so convert.
    local mx, my = Input.get_mouse_position()
    local sw, sh = Camera.get_screen_size()
    local wx = mx - sw * 0.5
    local wy = sh * 0.5 - my

    if cursor then
        Transform.set_position(cursor, wx, wy)
    end
    if tip then
        Skeleton.set_ik_target(tip, wx, wy)
    end
    if mirror_tip then
        Skeleton.set_ik_target(mirror_tip, wx, wy)
    end
    if pupil then
        Skeleton.set_ik_target(pupil, wx, wy)
    end

    if Input.is_key_just_pressed("P") and arm ~= nil then
        -- Animation.play(entity, clip) / Animation.stop(entity) drive the
        -- SkeletalPlayer; both queue one SetSkeletalPlayer command.
        playing = not playing
        if playing then
            Animation.play(arm, CLIP)
        else
            Animation.stop(arm)
        end
    end

    if Input.is_key_just_pressed("Space") then
        -- flip_bend has no setter of its own; the elbow solution is part of
        -- the component, so this only reports the limitation rather than
        -- pretending to toggle it.
        flip = not flip
        log("[scene_34] flip_bend is authored on the component (no script setter yet)")
    end

    -- The spawned rig has no sprites: draw a line down each bone.
    local shoulder = spawned and Skeleton.find_bone(spawned, "shoulder") or nil
    local elbow = spawned and Skeleton.find_bone(spawned, "elbow") or nil
    local wrist = spawned and Skeleton.find_bone(spawned, "wrist") or nil
    if shoulder and elbow and wrist then
        local sx, sy = Transform.get_world_position(shoulder)
        local ex, ey = Transform.get_world_position(elbow)
        local wx2, wy2 = Transform.get_world_position(wrist)
        Debug.draw_line(sx, sy, ex, ey, 1, 0.85, 0.2)
        Debug.draw_line(ex, ey, wx2, wy2, 1, 0.85, 0.2)
        Debug.draw_line(wx2 - 6, wy2, wx2 + 6, wy2, 1, 0.85, 0.2)
        Debug.draw_line(wx2, wy2 - 6, wx2, wy2 + 6, 1, 0.85, 0.2)
    end

    if readout then
        -- find_skeleton / find_bone resolve rig entities by NAME rather than
        -- by entity id, which is how a script reaches into a rig it did not
        -- author; `spawn_rig` is the third of the trio.
        local rig = Skeleton.find("arm_rig")
        local lower = rig and Skeleton.find_bone(rig, "lower") or nil
        Text.set(readout, string.format(
            "ik target: %.0f, %.0f   |   find_skeleton(\"arm_rig\")=%s  find_bone(\"lower\")=%s   |   spawn_rig: %s",
            wx, wy, tostring(rig or "nil"), tostring(lower or "nil"),
            (shoulder and elbow and wrist) and "3 bones (yellow)" or "nothing"))
    end

    nav.tick(SCENE_PATH)
end
