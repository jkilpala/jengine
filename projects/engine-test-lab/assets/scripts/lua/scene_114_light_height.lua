-- Scene 114: a light's height decides how normal maps take it.
--
-- Two identical normal-mapped spheres, each circled by its own light. The
-- left light sits at the default height (0.5, about 27 degrees up), so its
-- sphere is lit nearly all over; the right one is low (0.1), so its sphere
-- has a night side. Before `height`, every light sat at 0.5.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/114-light-height.scene"
local ORBIT = 230

local t = 0
local pairs_ = {}

function init(entity)
    for i = 1, 2 do
        local s = Entity.find_by_name("Sphere" .. i)
        local sx, sy = Transform.get_position(s)
        pairs_[i] = { x = sx, y = sy, light = Entity.find_by_name("Light" .. i), dot = Entity.find_by_name("Light" .. i .. "Dot") }
    end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    local a = math.pi + t * 0.5
    for _, p in ipairs(pairs_) do
        local x, y = p.x + math.cos(a) * ORBIT, p.y + math.sin(a) * ORBIT * 0.5
        if p.light then Transform.set_position(p.light, x, y) end
        if p.dot then Transform.set_position(p.dot, x, y) end
    end
end
