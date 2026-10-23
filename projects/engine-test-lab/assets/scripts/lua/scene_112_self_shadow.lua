-- Scene 112: an occluder does not shadow its own inside unless `self_shadow`
-- says so.
--
-- Two normal-mapped blocks, each with a box occluder its own size. Until Sep
-- 2026 every occluder shadowed its inside, so both drew black and the advice
-- was to draw them `unlit` (losing the normal map). The left one now takes
-- the lamp; the right one keeps the old dark inside by asking for it.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/112-occluder-self-shadow.scene"

local lamp, dot

function init(entity)
    lamp = Entity.find_by_name("Lamp")
    dot = Entity.find_by_name("LampDot")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local x, y = Input.get_mouse_world_position()
    if x then
        if lamp then Transform.set_position(lamp, x, y) end
        if dot then Transform.set_position(dot, x, y) end
    end
end
