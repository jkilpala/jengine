-- 148 Portal Lab: two `portal`s, Blue in the floor and Orange in the right
-- wall, each naming the other. The engine draws each as the view out of the
-- other and carries what goes in out of the other with its speed turned -
-- the script only drops crates and fires them. Left alone, it alternates.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/148-portal-lab.scene"
local DROP_X, DROP_Y = -300.0, 150.0
local FIRE_X, FIRE_Y = -480.0, -170.0
local status
local captions = true
local t, auto_in, n = 0, 0.8, 0
local last = ""

local function drop()
    local c = Entity.spawn_prefab("PlCrate")
    Transform.set_position(c, DROP_X + math.random(-20, 20), DROP_Y)
    Physics.set_velocity(c, 0, -60)
    last = "dropped into the floor - out of the wall"
end

local function fire()
    local c = Entity.spawn_prefab("PlCrate")
    Transform.set_position(c, FIRE_X, FIRE_Y)
    Physics.set_velocity(c, 1100, 520)
    last = "fired at the wall - up out of the floor"
end

function init(entity)
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    if Input.is_key_just_pressed("Space") then drop(); auto_in = 6 end
    if Input.is_key_just_pressed("F") then fire(); auto_in = 6 end
    auto_in = auto_in - dt
    if auto_in <= 0 then
        n = n + 1
        if n % 2 == 1 then drop() else fire() end
        auto_in = 2.6
    end
    UI.set_text(status, last)
end
