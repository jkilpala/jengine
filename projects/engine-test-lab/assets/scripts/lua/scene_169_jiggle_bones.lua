-- Scene 169: the swinging is the engine's (`jiggle` on each bone); this moves
-- the bar after the mouse, and shakes it.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/169-jiggle-bones.scene"
local block
local shake = 0
local t = 0

function init(entity)
    block = Entity.find_by_name("Block")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if Input.is_key_just_pressed("Space") then shake = 0.6 end
    local x, y = Input.get_mouse_world_position()
    y = math.max(-120, math.min(200, y))
    if shake > 0 then
        shake = shake - dt
        x = x + math.sin(t * 60) * 30
    end
    Transform.set_position(block, x, y)
end
