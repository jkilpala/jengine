-- Scene 153: the slab is a sprite with a `carvable`. `Sprite.carve` cuts a
-- hole from its pixels and the engine rebuilds its collider from what is
-- left; this only turns clicks into holes.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/153-carvable-sprite.scene"
local slab

function init(entity)
    slab = Entity.find_by_name("Slab")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("R") then
        Scene.load(SCENE_PATH)
        return
    end
    if slab and Input.is_mouse_left_just_pressed() then
        local x, y = Input.get_mouse_world_position()
        Sprite.carve(slab, x, y, 46)
    end
end
