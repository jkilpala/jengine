-- Scene 113: an occluder's shadow can end.
--
-- Four identical posts under a far "sun"; their `shadow_length` is 0
-- (unbounded), 60, 160 and 320. Lighting is flat land, so an occluder is
-- infinitely tall and its shadow runs to the edge of the light unless it is
-- told how long it is - which a short thing under a far light needs.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/113-shadow-length.scene"
local SUN_Y = 1100

local sun

function init(entity)
    sun = Entity.find_by_name("Sun")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local x = Input.get_mouse_world_position()
    if sun and x then Transform.set_position(sun, x * 3.0, SUN_Y) end
end
