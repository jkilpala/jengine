-- Scene 131: jengine-swarm steered through its `Swarm` custom fields, which
-- every runtime reads - a target the flock is drawn to (the mouse, or a slow
-- circle when the mouse is still) and a threat it flees (an orbiting disc).

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/131-swarm-steering.scene"

local swarm, target, threat
local t, still = 0.0, 0.0
local last_x, last_y = 0.0, 0.0

function init(entity)
    swarm = Entity.find_by_name("Swarm")
    target = Entity.find_by_name("Target")
    threat = Entity.find_by_name("Threat")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    local mx, my = Input.get_mouse_world_position()
    if math.abs(mx - last_x) + math.abs(my - last_y) > 0.5 then still = 0.0 else still = still + dt end
    last_x, last_y = mx, my
    local tx, ty = mx, my
    if still > 2.0 then tx, ty = math.cos(t * 0.4) * 380.0, math.sin(t * 0.4) * 160.0 end
    local hx, hy = math.cos(-t * 0.7) * 220.0, math.sin(-t * 0.7) * 120.0
    if target then Transform.set_position(target, tx, ty) end
    if threat then Transform.set_position(threat, hx, hy) end
    if swarm then
        Entity.set_custom(swarm, "Swarm", "target_x", tx)
        Entity.set_custom(swarm, "Swarm", "target_y", ty)
        Entity.set_custom(swarm, "Swarm", "threat_x", hx)
        Entity.set_custom(swarm, "Swarm", "threat_y", hy)
    end
end
