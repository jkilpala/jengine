-- Scene 138: Space drops a ball on each tower, R rebuilds. The breaking is
-- the engine's (`breakable`), and so is the lighting (`shadows_casters`);
-- this only lets the balls go.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/138-fracture-and-caster-shadows.scene"
local status
local dropped = false

function init(entity)
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("R") then Scene.load(SCENE_PATH) return end
    if Input.is_key_just_pressed("Space") and not dropped then
        dropped = true
        for _, n in ipairs({ "LBall", "RBall" }) do
            local b = Entity.find_by_name(n)
            if b then
                Physics.set_body_type(b, "Dynamic")
                Physics.set_velocity(b, 0, -900)
            end
        end
        if status then UI.set_prop(status, "text", "dropped - R rebuilds") end
    end
end
