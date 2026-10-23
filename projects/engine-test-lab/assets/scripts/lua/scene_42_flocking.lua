-- Scene 42 - Flocking (Separation + Obstacles)
--
-- Scene 24 covers SeekTarget on its own. The two behaviours here are what turn
-- a set of seekers into a crowd, and both are engine systems that run after
-- seeking in every runtime:
--
--   Separation  apply_separation - overlapping agents push apart each frame
--   Obstacle    resolve_obstacle_collisions - static keep-out circles
--
-- Both swarms below have identical SeekTarget components, each chasing its own
-- target on its own side, round its own rock. The only difference is that the
-- left one also has Separation, so the comparison is direct: the right swarm
-- converges into a single stacked sprite, the left keeps its spacing while
-- still reaching the target. (They used to share one target, so the two
-- swarms merged and the comparison with it.)
--
-- Neither swarm has Health. The rocks kept only Health-carrying bodies out,
-- so both flocks walked through them (BUGS #155); anything with a Velocity2D
-- is a mover now.
--
-- No script drives the motion.

script = { tick_rate = 10 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/42-flocking.scene"

local readout

function init(entity)
    readout = Entity.find_by_name("Readout")
    if readout then
        Text.set(readout, "left: Separation + Obstacles   |   right: Obstacles only - watch it stack up")
    end
    log("[scene_42] ready - compare the two swarms")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
