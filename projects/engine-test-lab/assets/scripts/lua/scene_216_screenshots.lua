-- Scene 216: Screenshot.take into four photo slots (textures the slot
-- sprites already name), and to a file with F.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/216-screenshots.scene"
local e = {}
local slot = 0
local shots = 0
local last = nil
local t = 0

function init(entity)
    for i = 0, 2 do e[i] = Entity.find_by_name("Orb" .. i) end
    e.Line0 = Entity.find_by_name("Line0")
end

local function take(file)
    local opts = { texture = "photos/lab216_" .. slot, scale = 0.5 }
    if file then opts.file = "lab216_" .. shots end
    last = Screenshot.take(opts)
    slot = (slot + 1) % 4
    shots = shots + 1
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    for i = 0, 2 do
        Transform.set_position(e[i], -200 + i * 200 + math.sin(t * (1 + i * 0.4)) * 80, 60 + math.cos(t * 1.3 + i) * 70)
    end
    if Input.is_key_just_pressed("P") then take(false) end
    if Input.is_key_just_pressed("F") then take(true) end
    local state = "no photo yet"
    if last then state = "photo " .. shots .. (Screenshot.ready(last) and " ready" or " developing...") end
    UI.set_text(e.Line0, state)
end
