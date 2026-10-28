-- 01 Lantern Square: walk the traveller round a market square at dusk.
-- Everything is a 3D stage (`stage`, `stage_quad`, `stage_billboard`): the
-- houses are quads projected in perspective, the people billboards in it,
-- the lanterns lights whose pools shrink with distance and lie flat on the
-- cobbles. The camera follows the traveller; Q / E orbit it. The square's
-- people go about their evening, and the lamps flicker.

local nav = require("nav")
local explore = require("explore")

local SCENE = "scenes/01-lantern-square.scene"
local walker
local status
local post = explore.post_all_on()
local captions = true
local lamps = {}
local t = 0

function init(entity)
    status = Entity.find_by_name("Status")
    walker = explore.new({
        hero = "Hero", x = 0, y = -3,
        tour = { { -3, 1 }, { 0, 9 }, { 0, 15 }, { -6, 14.8 }, { 6, 14.8 }, { 0, 9 }, { 3, 1 }, { 8, -2 }, { -8, -2 }, { 0, -3 } },
        bounds = { -14, -9, 14, 21.5 },
        blocked = {
            { -12.5, 2.0, -5.5, 7.5 }, { 5.5, 2.5, 12.0, 7.5 }, { -10.0, 15.5, -2.5, 20.5 }, { 2.5, 15.5, 9.0, 20.0 },
            { -1.3, -0.3, 1.3, 2.3 },
            -- The river, but for the bridge.
            { -40, 10.2, -1.6, 13.6 }, { 1.6, 10.2, 40, 13.6 },
        },
    })
    explore.npc(walker, "Merchant", { { 6.0, 0.0 }, { 3.0, -1.5 } }, 0.9, 3.0)
    explore.npc(walker, "Villager", { { -6.0, -1.0 }, { -3.0, -6.0 }, { -8.0, -6.5 } }, 1.1, 2.0)
    explore.npc(walker, "Lamplighter", { { 3.0, 8.0 }, { -3.0, 8.0 }, { -3.5, -3.5 }, { 3.5, -3.5 } }, 1.0, 2.5)
    explore.npc(walker, "Guard", { { 0.6, 12.0 }, { 0.6, 10.4 } }, 0.7, 4.0)
    explore.npc(walker, "Child", { { -2.0, 4.0 }, { 2.5, 3.5 }, { 2.5, -1.5 }, { -2.5, -1.8 } }, 2.2, 0.4)
    explore.npc(walker, "Cleric", { { -5.0, 17.0 }, { 0.0, 16.5 }, { -1.0, 21.0 } }, 0.8, 2.5)
    for i = 0, 5 do lamps[#lamps + 1] = Entity.find_by_name("Lamp" .. i .. "Light") end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    explore.post_keys(post)
    explore.update(walker, dt)
    -- Lamps breathe, each at its own pace.
    for i, l in ipairs(lamps) do
        local f = 1.8 + 0.12 * math.sin(t * 7.3 + i * 1.7) + 0.08 * math.sin(t * 13.1 + i)
        Lighting.set_intensity(l, f)
    end
    if status then UI.set_text(status, explore.post_status(post)) end
end
