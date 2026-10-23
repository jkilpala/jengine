-- Scene 262: `smooth_terrain`. The ground is a density grid drawn and
-- collided as its contour; the mouse digs and builds with `Terrain.edit`,
-- and an automatic digger sweeps a pit back and forth under the balls.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/262-smooth-terrain.scene"
local line, ground
local auto, t = true, 0

function init(entity)
    line = Entity.find_by_name("Line0")
    ground = Entity.find_by_name("Ground")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("A") then auto = not auto end
    local mx, my = Input.get_mouse_world_position()
    if Input.is_mouse_left_pressed() then
        Terrain.edit(ground, mx, my, 40, 3 * dt)
    elseif Input.is_mouse_right_pressed() then
        Terrain.edit(ground, mx, my, 40, -3 * dt)
    end
    if auto then
        t = t + dt
        Terrain.edit(ground, math.sin(t * 0.5) * 380, -70, 50, 1.5 * dt)
    end
    UI.set_text(line, auto and "automatic digger on" or "automatic digger off")
end
