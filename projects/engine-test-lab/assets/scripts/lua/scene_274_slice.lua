-- Scene 274: `sliceable` and `Physics.slice`. Dragging the mouse cuts
-- along the stretch it moved each frame; R reloads. Left alone, it makes a
-- cut of its own every couple of seconds, at each shape in turn, a
-- different angle each time.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/274-slice.scene"
local line
local last_x, last_y
local idle, t, auto_n = 0, 0, 0
local cuts = 0

function init(entity)
    line = Entity.find_by_name("Line0")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if Input.is_key_just_pressed("R") then Scene.load(SCENE_PATH) return end
    local mx, my = Input.get_mouse_world_position()
    if last_x ~= nil and (math.abs(mx - last_x) > 1 or math.abs(my - last_y) > 1) then
        idle = 0
        if Input.is_mouse_left_pressed() then
            Physics.slice(last_x, last_y, mx, my)
            cuts = cuts + 1
        end
    else
        idle = idle + dt
    end
    last_x, last_y = mx, my
    if idle > 2 and t > 1.5 then
        t = 0
        auto_n = auto_n + 1
        local a = 0.4 + auto_n * 1.1
        local dx, dy = math.cos(a) * 200, math.sin(a) * 200
        local cx, cy = ({ -330, 0, 330 })[auto_n % 3 + 1], -170
        Physics.slice(cx - dx, cy - dy, cx + dx, cy + dy)
        cuts = cuts + 1
    end
    UI.set_text(line, "swipes: " .. cuts .. (idle > 2 and "   (automatic)" or ""))
end
