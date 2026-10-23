-- 127 The Glass House: stones lobbed at the glasshouse from the right.
-- Each pane is a `breakable` with `pattern = "voronoi"`: a pane fixed in its
-- frame breaks on the stone's change in velocity, into shards crowded round
-- where it struck, which fall loose onto the ledge and the lawn. The script
-- only throws the stones - aimed by the lob's own arithmetic at a pane still
-- whole - and reglazes when most of the glass is gone.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/127-glass-house.scene"
local PANES = {
    { "Pane0_0", -420.0, -120.0 },
    { "Pane0_1", -300.0, -120.0 },
    { "Pane0_2", -180.0, -120.0 },
    { "Pane0_3", -60.0, -120.0 },
    { "Pane0_4", 60.0, -120.0 },
    { "Pane0_5", 180.0, -120.0 },
    { "Pane0_6", 300.0, -120.0 },
    { "Pane0_7", 420.0, -120.0 },
    { "Pane1_0", -420.0, -20.0 },
    { "Pane1_1", -300.0, -20.0 },
    { "Pane1_2", -180.0, -20.0 },
    { "Pane1_3", -60.0, -20.0 },
    { "Pane1_4", 60.0, -20.0 },
    { "Pane1_5", 180.0, -20.0 },
    { "Pane1_6", 300.0, -20.0 },
    { "Pane1_7", 420.0, -20.0 },
    { "Pane2_0", -420.0, 80.0 },
    { "Pane2_1", -300.0, 80.0 },
    { "Pane2_2", -180.0, 80.0 },
    { "Pane2_3", -60.0, 80.0 },
    { "Pane2_4", 60.0, 80.0 },
    { "Pane2_5", 180.0, 80.0 },
    { "Pane2_6", 300.0, 80.0 },
    { "Pane2_7", 420.0, 80.0 },
    { "Gable0", -420.0, 141.9 },
    { "Gable1", -300.0, 165.6 },
    { "Gable2", -180.0, 189.4 },
    { "Gable3", -60.0, 213.1 },
    { "Gable4", 60.0, 213.1 },
    { "Gable5", 180.0, 189.4 },
    { "Gable6", 300.0, 165.6 },
    { "Gable7", 420.0, 141.9 }
}
local FROM_X, FROM_Y = 760, -240

local status
local captions = true
local t, next_throw, gone_for = 0, 1.0, 0
local seed = 7

local function rand()
    seed = (seed * 1103515245 + 12345) % 2147483648
    return seed / 2147483648
end

local function whole()
    local list = {}
    for _, p in ipairs(PANES) do
        if Entity.find_by_name(p[1]) ~= nil then list[#list + 1] = p end
    end
    return list
end

local function throw(list)
    if #list == 0 then return end
    local p = list[1 + math.floor(rand() * #list) % #list]
    local tx, ty = p[2] + (rand() - 0.5) * 50, p[3] + (rand() - 0.5) * 40
    -- get_gravity answers in m/s^2; the project's length unit is 100.
    local gx, gy = Physics.get_gravity()
    gy = gy * 100
    local dx, dy = tx - FROM_X, ty - FROM_Y
    local T = 0.7 + math.abs(dx) / 2600
    local vx, vy = dx / T, dy / T - 0.5 * gy * T
    local s = Entity.spawn_prefab("GhStone")
    Transform.set_position(s, FROM_X, FROM_Y)
    Physics.set_velocity(s, vx, vy)
end

function init(entity)
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("R") then Scene.load(SCENE) return end
    t = t + dt
    local list = whole()
    if Input.is_key_just_pressed("Space") then throw(list) end
    if t > next_throw then
        throw(list)
        next_throw = t + 1.1 + rand() * 0.9
    end
    if #list <= math.floor(#PANES / 4) then
        gone_for = gone_for + dt
        if gone_for > 4 then Scene.load(SCENE) return end
    end
    UI.set_text(status, #list .. " of " .. #PANES .. " panes whole")
end
