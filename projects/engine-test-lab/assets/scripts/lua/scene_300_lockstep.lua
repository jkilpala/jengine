-- Scene 300 - lockstep: choose to play or to watch.
--
-- The match itself is `lab300/play.scene` (or `watch.scene`): a scene whose
-- `lockstep_session` joins a lockstep room on a `jengine-server` and holds
-- the scene's fixed steps until every player's input for a step is in
-- (`jengine-online`'s `lockstep`; docs/determinism-proposal.md, phase 5). A
-- session gates from its scene's load, so the choice is made here, in a
-- scene before it.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/300-lockstep.scene"

function init(entity)
end

function update(entity, dt)
    if Input.is_key_just_pressed("1") then
        Scene.load("scenes/lab300/play.scene")
        return
    end
    if Input.is_key_just_pressed("2") then
        Scene.load("scenes/lab300/watch.scene")
        return
    end
    nav.tick(SCENE_PATH)
end
