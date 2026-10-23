-- Scene 269: `silhouette` / `silhouette_occluder`. A disc walks behind a
-- brick wall with windows: outlined over the brick, itself in the windows.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/269-silhouette.scene"
local line, walker
local auto, t = true, 0

function init(entity)
    line = Entity.find_by_name("Line0")
    walker = Entity.find_by_name("Walker")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("A") then auto = not auto end
    local x, y
    if auto then
        t = t + dt
        x, y = math.sin(t * 0.5) * 420, math.sin(t * 1.1) * 60
    else
        x, y = Input.get_mouse_world_position()
    end
    Transform.set_position(walker, x, y)
    UI.set_text(line, "disc at (" .. math.floor(x + 0.5) .. ", " .. math.floor(y + 0.5) .. ")" .. (auto and "   (walking)" or ""))
end
