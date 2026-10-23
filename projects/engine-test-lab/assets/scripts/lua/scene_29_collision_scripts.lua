-- Scene 29: Collision Callbacks (scripts)
-- Nav-only script. Each ball owns `collision_ball.lua`, which receives
-- `on_collision_enter(self, data)` / `on_collision_exit(self, data)`
-- from the engine. `data.other` is the other entity's id.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/29-collision-scripts.scene"

function init(entity)
    log("[29-collision-scripts] ready - 3 scripted balls, walls, floor, bumper")
    log("[29-collision-scripts] each ball's status line shows its enter/exit pairs")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    -- The balls settle and stop colliding; R drops them again.
    if Input.is_key_just_pressed("R") then
        Scene.load(SCENE_PATH)
    end
end
