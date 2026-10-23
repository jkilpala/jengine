-- Scene 261: a displacement field. The orange pusher is a `displacer`;
-- every blade is a `field_bend` leaning away from the push where it stands.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/261-displacement-field.scene"
local line, pusher
local follow, t = false, 0

function init(entity)
    line = Entity.find_by_name("Line0")
    pusher = Entity.find_by_name("Pusher")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("M") then follow = not follow end
    t = t + dt
    if follow then
        local mx, my = Input.get_mouse_world_position()
        Transform.set_position(pusher, mx, my)
    else
        Transform.set_position(pusher, math.sin(t * 0.6) * 600, -40 + math.sin(t * 1.7) * 90)
    end
    UI.set_text(line, follow and "pusher on the mouse" or "pusher sweeping")
end
