-- 05 Snowbound: a village under snow at night on a 3D stage, snow falling
-- across the whole view. The lamplighter does the rounds; a child plays by
-- the well; the lamps breathe.

local nav = require("nav")
local explore = require("explore")

local SCENE = "scenes/05-snowbound.scene"
local walker, status
local post = explore.post_all_on()
local captions = true
local lamps = {}
local t = 0

function init(entity)
    status = Entity.find_by_name("Status")
    walker = explore.new({
        hero = "Hero", x = 0, y = -2,
        tour = { { -2.5, 4 }, { -3, 11 }, { 3, 11 }, { 2.5, 4 }, { 8, -4 }, { -8, -4 }, { 0, -2 } },
        bounds = { -22, -10, 22, 22 },
        blocked = {
            { -13, 3, -6, 8 }, { 6, 2, 12.5, 7 }, { -9, 14, -2, 19 }, { 3, 15, 10, 20 }, { -20, 10, -14, 15 },
            { 15, 12, 21, 17 }, { -1.2, 5.0, 1.2, 7.4 },
        },
    })
    explore.npc(walker, "Lamplighter", { { -4.5, -2.5 }, { 3.5, -2.5 }, { 3.5, 10.5 }, { -3.5, 10.5 } }, 0.9, 2.5)
    explore.npc(walker, "Child", { { 2.0, 9.0 }, { -2.0, 8.6 }, { -2.2, 4.0 }, { 2.2, 4.2 } }, 2.0, 0.6)
    explore.npc(walker, "Guard", { { 5.0, 10.5 }, { 5.0, 13.5 } }, 0.6, 5.0)
    for i = 0, 5 do lamps[#lamps + 1] = Entity.find_by_name("Lamp" .. i .. "Light") end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    explore.post_keys(post)
    explore.update(walker, dt)
    for i, l in ipairs(lamps) do
        Lighting.set_intensity(l, 2.0 + 0.12 * math.sin(t * 6.1 + i * 1.3))
    end
    if status then UI.set_text(status, explore.post_status(post)) end
end
