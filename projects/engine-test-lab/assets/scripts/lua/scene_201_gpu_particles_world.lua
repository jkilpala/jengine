-- Scene 201: GPU particles meeting the world. The stream collides with the
-- disc and the slab (`collide`); its attractor follows the pointer.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/201-gpu-particles-world.scene"
local stream, pointer, status
local collide, attract = true, false

local function show()
    if status then
        UI.set_text(status, "collide " .. (collide and "on" or "off") .. "   attract " .. (attract and "on" or "off"))
    end
end

function init(entity)
    stream = Entity.find_by_name("Stream")
    pointer = Entity.find_by_name("Pointer")
    status = Entity.find_by_name("Status")
    show()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local mx, my = Input.get_mouse_world_position()
    Transform.set_position(pointer, mx, my)
    if Input.is_key_just_pressed("C") then
        collide = not collide
        Particles.set(stream, "collide", collide)
        show()
    end
    if Input.is_key_just_pressed("A") then
        attract = not attract
        Particles.set(stream, "attractors", { { follow = "Pointer", strength = attract and 1.5 or 0,
                                                swirl = attract and 5 or 0, radius = 120 } })
        show()
    end
end
