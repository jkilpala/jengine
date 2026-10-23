-- 135 Smooth Caves: rock that is a `smooth_terrain` - a density grid drawn
-- and collided as its marching-squares contour, so dug walls are slopes, not
-- stairs. The mouse digs (left) and builds (right) through `Terrain.edit`;
-- left alone, a drill tunnels along a winding path and the boulders roll into
-- what it opens. The collider is rebuilt on the same body each edit, so
-- what rested on dug ground falls.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/135-smooth-caves.scene"
local rock, drill, lantern, status
local captions = true
local idle, t, dug = 3, 0, 0
local opened = false
local GALLERY = 285.0
local mx0, my0 = 0, 0

function init(entity)
    rock = Entity.find_by_name("Rock")
    drill = Entity.find_by_name("Drill")
    lantern = Entity.find_by_name("Lantern")
    status = Entity.find_by_name("Status")
end

local function drill_at(s)
    -- A winding tunnel from the left wall to the right, dipping and rising.
    local x = -700 + (s % 18) / 18 * 1400
    -- Up into the gallery now and then, so the boulders drop in.
    local y = 20 + math.sin(s * 0.9) * 210 + math.sin(s * 0.37) * 80
    return x, y
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    local mx, my = Input.get_mouse_world_position()
    local moved = math.abs(mx - mx0) + math.abs(my - my0) > 0.5
    mx0, my0 = mx, my
    local left = Input.is_mouse_left_pressed()
    local right = Input.is_mouse_right_pressed()
    if moved or left or right then idle = 0 else idle = idle + dt end
    if not opened then
        -- A gallery along the top for the boulders to start in.
        for x = -680, 680, 30 do Terrain.edit(rock, x, GALLERY, 52, 2) end
        opened = true
    end
    if left then
        Terrain.edit(rock, mx, my, 46, 3.0 * dt)
        dug = dug + dt
    elseif right then
        Terrain.edit(rock, mx, my, 40, -2.5 * dt)
    end
    if idle > 2 then
        -- The drill: it digs where it goes, turned along its way.
        t = t + dt
        local x, y = drill_at(t)
        local x2, y2 = drill_at(t + 0.05)
        Transform.set_position(drill, x, y)
        Transform.set_rotation(drill, math.atan2 ~= nil and math.atan2(y2 - y, x2 - x) or math.atan(y2 - y, x2 - x))
        Sprite.set_color(drill, 1, 1, 1, 1)
        Terrain.edit(rock, x, y, 40, 4.0 * dt)
        Transform.set_position(lantern, x, y)
        UI.set_text(status, "left alone, a drill tunnels through - the boulders follow it down")
    else
        Sprite.set_color(drill, 1, 1, 1, 0)
        Transform.set_position(lantern, mx, my)
        UI.set_text(status, left and "digging" or (right and "building" or "the rock is a density grid; its walls are its contour"))
    end
end
