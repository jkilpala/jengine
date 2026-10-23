-- Scene 26: Joints
-- Three joint flavors:
--   * Revolute pendulum chain (kick-started)
--   * Rope chain (5 segments)
--   * Spring chain (rest=120/80, stiffness=200, damping=6) - it rings and
--     settles within a few seconds. At damping 2 it never visibly settled.
-- Each group uses a unique collision layer pair so segments only
-- collide with the floor - no group-vs-group contact noise.
--
-- R reloads to replay the kick-off. F4 (every runtime) outlines the
-- colliders and draws each joint from its anchor to its target.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/26-joints.scene"

function init(entity)
    log("[26-joints] ready - pendulum / rope / spring (rapier2d)")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("R") then
        Scene.load(SCENE_PATH)
    end
end
