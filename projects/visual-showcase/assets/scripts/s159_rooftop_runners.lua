-- 159 Rooftop Runners: each roof is a `nav_region` and `nav_link`s join
-- them - jumps, a plank, a footbridge, a one-way zipline. The engine plans
-- every courier's route across them; this script only picks where each
-- goes next, and lifts one up while it jumps (`NavCrossing`).

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/159-rooftop-runners.scene"
local SPOTS = { { -560, 300 }, { -330, 90 }, { -150, 300 }, { 100, 290 }, { 560, 300 }, { 260, 130 }, { -580, -300 }, { -200, -120 }, { 0, -300 }, { 560, -120 }, { 250, -300 } }
local status
local captions = true
local runners, shadows = {}, {}
local deliveries = 0
local t = 0

local function send(i)
    local s = SPOTS[math.random(#SPOTS)]
    Nav.set_target(runners[i], s[1] + (math.random() - 0.5) * 30, s[2] + (math.random() - 0.5) * 30)
end

function init(entity)
    status = Entity.find_by_name("Status")
    for i = 0, 4 do
        table.insert(runners, Entity.find_by_name("Runner" .. i))
        table.insert(shadows, Entity.find_by_name("Shadow" .. i))
    end
    for i = 1, #runners do send(i) end
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    if Input.is_mouse_left_just_pressed() then
        local mx, my = Input.get_mouse_world_position()
        for i = 1, #runners do
            Nav.set_target(runners[i], mx + (i - 3) * 26, my)
        end
    end
    for i, e in ipairs(runners) do
        if not Nav.is_moving(e) then
            deliveries = deliveries + 1
            send(i)
        end
        local x, y = Transform.get_position(e)
        -- In the air over a gap: bigger as it rises, its shadow left below.
        local kind = Entity.get_custom(e, "NavCrossing", "kind")
        local lift = 0
        if kind == "jump" then
            lift = math.sin(math.pi * (Entity.get_custom(e, "NavCrossing", "t") or 0))
        elseif kind == "zip" then
            lift = 0.35
        end
        Transform.set_scale(e, 1 + 0.45 * lift, 1 + 0.45 * lift)
        Transform.set_position(shadows[i], x + 4 + 18 * lift, y - 4 - 18 * lift)
    end
    UI.set_text(status, deliveries .. " deliveries")
end
