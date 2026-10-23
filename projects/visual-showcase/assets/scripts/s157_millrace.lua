-- 157 Millrace: the race is three `flow_path`s on one curve - a bank,
-- running water and faster foam. The water carries every leaf and the paper
-- boat round its bends; this script only brings what reaches the end back
-- to the pond, drops a leaf where you click, and turns the millwheel at the
-- water's speed (`Flow.set` changes it).

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/157-millrace.scene"
local START_X, START_Y = -560.0, 250.0
local status
local captions = true
local floats = {}
local water, foam, wheel
local speed = 110
local spin = 0
local next_drop = 1

local function to_pond(e)
    Transform.set_position(e, START_X + (math.random() - 0.5) * 30, START_Y + (math.random() - 0.5) * 30)
end

local function set_speed(v)
    speed = math.max(0, math.min(300, v))
    Flow.set(water, "speed", speed)
    Flow.set(foam, "speed", speed * 1.35)
end

function init(entity)
    status = Entity.find_by_name("Status")
    water = Entity.find_by_name("Water")
    foam = Entity.find_by_name("Foam")
    wheel = Entity.find_by_name("Wheel")
    for i = 0, 13 do table.insert(floats, Entity.find_by_name("Leaf" .. i)) end
    table.insert(floats, Entity.find_by_name("Boat"))
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("Up") then set_speed(speed + 30) end
    if Input.is_key_just_pressed("Down") then set_speed(speed - 30) end
    if Input.is_mouse_left_just_pressed() then
        local mx, my = Input.get_mouse_world_position()
        Transform.set_position(floats[next_drop], mx, my)
        next_drop = next_drop % 14 + 1
    end
    for _, e in ipairs(floats) do
        local x, y = Transform.get_position(e)
        if x > 600 or y < -330 then to_pond(e) end
    end
    spin = spin + speed * dt / 60
    Transform.set_rotation(wheel, -spin)
    UI.set_text(status, "race " .. math.floor(speed + 0.5) .. " a second")
end
