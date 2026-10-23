-- Scene 255: `State.save` / `State.load`. The whole world - balls, paddle,
-- velocities and spin - written down and put back. This script's own clock
-- is a local, so a load leaves it running.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/255-save-states.scene"
local line
local t, cycle, saved, loads = 0, 0, false, 0

function init(entity)
    line = Entity.find_by_name("Line0")
end

local function save()
    State.save("lab")
    saved = true
    -- Dropped in after the save, so the next load takes it away.
    local late = Entity.spawn_prefab("Lab255Late")
    Transform.set_position(late, 0, 200)
end

local function load()
    if State.has("lab") then
        State.load("lab")
        loads = loads + 1
    end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    cycle = cycle + dt
    if Input.is_key_just_pressed("S") then save() end
    if Input.is_key_just_pressed("L") then load() end
    if cycle >= 2.0 and cycle - dt < 2.0 then save() end
    if cycle >= 5.0 then
        load()
        cycle = 2.0
    end
    UI.set_text(line, (saved and "saved" or "not saved yet") .. "   loads " .. loads .. "   t " .. math.floor(t * 10 + 0.5) / 10)
end
