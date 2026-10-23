-- Scene 151: both maps hold the same cells - 0 none, 1 grass, 2 stone. The
-- left one carries an `autotile` component, so the engine draws its edges
-- and corners on a dual grid; the right one draws each cell as its own
-- tile. A click cycles the cell under it, in both maps.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/151-autotile.scene"
local COLS, ROWS = 13, 10
local maps = {}

function init(entity)
    maps = { Entity.find_by_name("Autotiled"), Entity.find_by_name("Cells") }
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if not Input.is_mouse_left_just_pressed() then return end
    local wx, wy = Input.get_mouse_world_position()
    for _, m in ipairs(maps) do
        local x, y = Tilemap.world_to_tile(m, wx, wy)
        if x ~= nil and x >= 0 and y >= 0 and x < COLS and y < ROWS then
            local v = (Tilemap.get_tile(m, "ground", x, y) + 1) % 3
            for _, other in ipairs(maps) do
                Tilemap.set_tile(other, "ground", x, y, v)
            end
            return
        end
    end
end
