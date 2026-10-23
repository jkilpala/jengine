-- Scene 161: the shape queries are the engine's (`Physics.overlap_*`,
-- `Physics.circle_cast`); this colours what they answer.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/161-shape-queries.scene"
local NAMES = { "Wall", "Crate1", "Crate2", "Ball", "Post", "Slab" }
local things, probe, sweep, readout = {}, nil, nil, nil
local mode, turn = 1, 0
local R, BW, BH, SR = 80, 220, 60, 20

function init(entity)
    for _, n in ipairs(NAMES) do
        local e = Entity.find_by_name(n)
        if e then things[#things + 1] = { e = e, name = n } end
    end
    probe = Entity.find_by_name("Probe")
    sweep = Entity.find_by_name("Sweep")
    readout = Entity.find_by_name("Readout")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("1") then mode = 1 end
    if Input.is_key_just_pressed("2") then mode = 2 end
    if Input.is_key_just_pressed("3") then mode = 3 end
    if Input.is_key_pressed("Q") then turn = turn + dt end
    if Input.is_key_pressed("E") then turn = turn - dt end
    local x, y = Input.get_mouse_world_position()
    local found
    if mode == 1 then
        found = Physics.overlap_circle(x, y, R)
        Transform.set_scale(probe, 1, 1)
        Transform.set_rotation(probe, 0)
        Sprite.set_color(probe, 1.0, 0.85, 0.3, 0.9)
    elseif mode == 2 then
        found = Physics.overlap_box(x, y, BW, BH, turn)
        Transform.set_scale(probe, BW / 160, BH / 160)
        Transform.set_rotation(probe, turn)
        Sprite.set_color(probe, 1.0, 0.85, 0.3, 0.9)
    else
        found = Physics.overlap_point(x, y)
        Transform.set_scale(probe, 0.06, 0.06)
        Sprite.set_color(probe, 1.0, 0.85, 0.3, 1.0)
    end
    Transform.set_position(probe, x, y)
    local hit = {}
    for _, id in ipairs(found) do hit[id] = true end
    local list = {}
    for _, t in ipairs(things) do
        if hit[t.e] then
            Sprite.set_color(t.e, 1.0, 0.78, 0.25, 1)
            list[#list + 1] = t.name
        else
            Sprite.set_color(t.e, 0.55, 0.58, 0.66, 1)
        end
    end
    -- A circle swept straight down from the top of the screen above the mouse.
    local cast = Physics.circle_cast(x, 340, 0, -1, SR, 1000)
    if cast then
        Transform.set_position(sweep, x, 340 - cast.distance)
    else
        Transform.set_position(sweep, x, -340)
    end
    if readout then
        local shape = (mode == 1 and "circle") or (mode == 2 and "box") or "point"
        UI.set_text(readout, shape .. " touches: " .. (#list > 0 and table.concat(list, ", ") or "nothing"))
    end
end
