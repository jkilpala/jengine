-- 03 Forest Shrine: a clearing in an autumn wood on a 3D stage. Sun shafts
-- through the canopy, leaves falling, lanterns along the path. The cleric
-- tends the shrine and the hunter walks the edge of the wood; the shafts
-- breathe as clouds pass over.

local nav = require("nav")
local explore = require("explore")

local SCENE = "scenes/03-forest-shrine.scene"
local walker, status
local post = explore.post_all_on()
local captions = true
local rays = {}
local t = 0

function init(entity)
    status = Entity.find_by_name("Status")
    walker = explore.new({
        hero = "Hero", x = 0, y = 0.5,
        tour = { { 0, 7 }, { -4, 9 }, { 4, 9 }, { 0, 7 }, { -6, -3 }, { 6, -3 }, { 0, 0.5 } },
        bounds = { -14, -14, 14, 16 },
        blocked = { { -4.5, 11, 4.5, 16 }, { -2.4, 3.6, -2.1, 3.9 }, { 2.1, 3.6, 2.4, 3.9 } },
    })
    explore.npc(walker, "Cleric", { { -0.8, 10.6 }, { 3.5, 10.0 }, { -3.5, 10.0 } }, 0.7, 3.5)
    explore.npc(walker, "Hunter", { { 6.0, 1.0 }, { 9.0, 8.0 }, { 7.0, -6.0 } }, 1.0, 2.0)
    for i = 0, 3 do rays[#rays + 1] = Entity.find_by_name("Ray" .. i) end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    explore.post_keys(post)
    explore.update(walker, dt)
    -- Clouds passing: each shaft brightens and fades on its own slow beat.
    for i, r in ipairs(rays) do
        local a = 0.14 + 0.1 * (0.5 + 0.5 * math.sin(t * 0.35 + i * 1.9))
        Sprite.set_color(r, 1.2, 1.0, 0.6, a)
    end
    if status then UI.set_text(status, explore.post_status(post)) end
end
