-- 02 Crystal Hollow: a cave on a 3D stage, lit by torches (their lights
-- flicker) and by crystals, with a shaft of daylight from far above. The
-- party explores; the traveller walks, the hunter and the scholar wander.

local nav = require("nav")
local explore = require("explore")

local SCENE = "scenes/02-crystal-hollow.scene"
local walker, status
local post = explore.post_all_on()
local captions = true
local torches = {}
local t = 0

function init(entity)
    status = Entity.find_by_name("Status")
    walker = explore.new({
        hero = "Hero", x = 0, y = -2,
        tour = { { -4, 1.5 }, { -3, 10 }, { 4, 11 }, { 12, 8 }, { 11, -2 }, { 4, -6 }, { 0, -2 } },
        bounds = { -15, -11, 15, 17 },
        blocked = { { 2.5, 2.0, 9.0, 7.0 }, { -10.1, 6.9, -7.9, 9.1 }, { 9.7, 9.7, 12.3, 12.3 }, { -11.9, -2.9, -10.1, -1.1 } },
    })
    explore.npc(walker, "Hunter", { { -3.5, 0.5 }, { -6.0, 12.0 }, { 0.0, 14.0 } }, 1.1, 2.5)
    explore.npc(walker, "Scholar", { { 3.0, -5.0 }, { 10.0, 0.0 }, { 12.5, -6.0 } }, 0.8, 3.0)
    for i = 0, 3 do torches[#torches + 1] = Entity.find_by_name("Torch" .. i .. "Light") end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    explore.post_keys(post)
    explore.update(walker, dt)
    for i, l in ipairs(torches) do
        local f = 2.2 + 0.35 * math.sin(t * 9.1 + i * 2.3) + 0.2 * math.sin(t * 17.3 + i * 0.7)
        Lighting.set_intensity(l, f)
    end
    if status then UI.set_text(status, explore.post_status(post)) end
end
