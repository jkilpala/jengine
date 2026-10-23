-- 142 Behind the Walls: the roofs and tree crowns are a
-- `silhouette_occluder`; the thief and the guards carry a `silhouette`, so
-- wherever a roof hides one of them the engine draws its outline over the
-- roof, clipped to the roof's own pixels: one outline per roof or crown
-- that covers him, and none where he is in the open.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/142-behind-the-walls.scene"
local ROOFS = { { -580.0, 300.0, -120.0, 110.0 }, { -580.0, 110.0, -390.0, -160.0 }, { 120.0, 280.0, 580.0, 60.0 }, { 340.0, 60.0, 580.0, -280.0 }, { -220.0, -110.0, 120.0, -300.0 } }
local TREES = { { 0.0, 200.0, 70.0 }, { -310.0, -20.0, 60.0 }, { 180.0, -60.0, 66.0 }, { -80.0, 30.0, 48.0 }, { 520.0, 300.0, 50.0 } }
local thief, status
local captions = true

local function covered(x, y)
    for _, r in ipairs(ROOFS) do
        if x >= r[1] and x <= r[3] and y >= r[4] and y <= r[2] then return "under a roof" end
    end
    for _, t in ipairs(TREES) do
        if (x - t[1]) ^ 2 + (y - t[2]) ^ 2 <= t[3] ^ 2 then return "under the trees" end
    end
    return nil
end

function init(entity)
    thief = Entity.find_by_name("Thief")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    local x, y = Transform.get_position(thief)
    local where = covered(x, y)
    if where == nil then
        UI.set_text(status, "the thief is in the open")
    else
        UI.set_text(status, "the thief is " .. where .. " - you see only his outline")
    end
end
