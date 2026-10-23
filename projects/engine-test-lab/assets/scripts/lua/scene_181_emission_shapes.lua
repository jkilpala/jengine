-- Scene 181: emission shapes. The fuse emits along its own vector path
-- (`emit_shape` "path"), the star over its own sprite (`emit_shape`
-- "sprite"); this turns the star and switches its fire between outline and
-- whole with Particles.set.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/181-emission-shapes.scene"
local star, mist, status
local edge, turning, angle = true, true, 0

function init(entity)
    star = Entity.find_by_name("Star")
    mist = Entity.find_by_name("StarMist")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("E") and star then
        edge = not edge
        Particles.set(star, "emit_shape", { kind = "sprite", edge = edge, cutoff = 0.5 })
    end
    if Input.is_key_just_pressed("Space") then turning = not turning end
    if turning then angle = angle + dt * 0.4 end
    if star then Transform.set_rotation(star, angle) end
    if mist then Transform.set_rotation(mist, angle) end
    if status then UI.set_text(status, edge and "the star burns its outline" or "the star burns all over") end
end
