-- 139 Toy Town: every house, tree, lamp and cart is a `sprite_stack` - a
-- voxel model cut into slices, drawn bottom to top and raised up the screen.
-- The camera turns over the board; the models turn with the world and keep
-- standing up, sorted by where their feet are on the screen. Left / Right
-- turn it yourself, Up / Down zoom.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/139-toy-town.scene"
local cam, status
local captions = true
local turn, zoom = 0, 0.9
local idle = 3

function init(entity)
    cam = Entity.find_by_name("Camera")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    local spin = 0
    if Input.is_key_pressed("Left") then spin = spin + 1 end
    if Input.is_key_pressed("Right") then spin = spin - 1 end
    local z = 0
    if Input.is_key_pressed("Up") then z = z + 1 end
    if Input.is_key_pressed("Down") then z = z - 1 end
    if spin ~= 0 or z ~= 0 then idle = 0 else idle = idle + dt end
    if idle > 3 then spin = 0.08 end
    turn = turn + spin * dt * (idle > 3 and 1 or 1.2)
    zoom = math.max(0.55, math.min(1.6, zoom * (1 + z * dt)))
    Transform.set_rotation(cam, turn)
    Camera.set_zoom(zoom)
    local deg = math.floor(((turn * 180 / math.pi) % 360) + 0.5)
    UI.set_text(status, "the town has turned " .. deg .. " degrees - and still stands up")
end
