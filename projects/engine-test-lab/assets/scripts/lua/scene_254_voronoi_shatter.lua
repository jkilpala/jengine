-- Scene 254: `breakable` with `pattern = "voronoi"`. A ball is thrown at
-- each pane; the left breaks in a grid, the right into Voronoi shards round
-- the point struck. The scene reloads to put the panes back.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/254-voronoi-shatter.scene"
local line
local t, thrown, heights = 0, false, { 40, -100, 60 }
local k = 1

function init(entity)
    line = Entity.find_by_name("Line0")
end

local function throw()
    local h = heights[k]
    k = k % #heights + 1
    for _, x in ipairs({ -200, 260 }) do
        local b = Entity.spawn_prefab("Lab254Ball")
        Transform.set_position(b, x - 300, h - 40)
        Physics.set_velocity(b, 1400, 0)
    end
    thrown = true
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if Input.is_key_just_pressed("Space") or (not thrown and t > 1.5) then throw() end
    if thrown and t > 6 then Scene.load(SCENE_PATH) return end
    UI.set_text(line, thrown and "struck" or "about to throw")
end
