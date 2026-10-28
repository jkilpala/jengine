-- 04 The Tavern: a room seen as a dollhouse on a 3D stage. The hearth and
-- the candles flicker; the regulars go between the bar, the tables and the
-- fire. The camera turns only a little, since the room has no south wall.

local nav = require("nav")
local explore = require("explore")

local SCENE = "scenes/04-the-tavern.scene"
local walker, status
local post = explore.post_all_on()
local captions = true
local fires = {}
local t = 0

function init(entity)
    status = Entity.find_by_name("Status")
    walker = explore.new({
        hero = "Hero", x = 0, y = -2, yaw_limit = 0.45, turn = 0.8,
        tour = { { -4, 1 }, { -4, 7.5 }, { 3, 7.5 }, { 4, 0 }, { 8.5, -2 }, { 0, -2 } },
        bounds = { -9.4, -3.5, 9.4, 9.2 },
        blocked = { { -2.0, 8.8, 2.0, 10.0 }, { 5.5, 1.0, 7.0, 8.0 } },
    })
    explore.npc(walker, "Barkeep", { { 7.8, 4.0 }, { 7.8, 7.0 }, { 7.8, 1.6 } }, 0.8, 3.0)
    explore.npc(walker, "Villager", { { -2.5, 2.8 }, { 4.0, 6.5 }, { -1.0, 7.5 } }, 1.0, 3.5)
    explore.npc(walker, "Farmer", { { 3.4, -0.6 }, { 4.6, 5.0 } }, 0.9, 5.0)
    explore.npc(walker, "Guard", { { -5.5, 6.0 }, { -2.5, 7.8 } }, 0.7, 6.0)
    fires[#fires + 1] = { Entity.find_by_name("FireLight"), 3.0, 0.5 }
    for i = 0, 3 do fires[#fires + 1] = { Entity.find_by_name("Candle" .. i .. "Light"), 1.2, 0.15 } end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    explore.post_keys(post)
    explore.update(walker, dt)
    for i, f in ipairs(fires) do
        if f[1] then
            local v = f[2] + f[3] * (math.sin(t * 8.7 + i * 2.1) + 0.6 * math.sin(t * 15.3 + i))
            Lighting.set_intensity(f[1], v)
        end
    end
    if status then UI.set_text(status, explore.post_status(post)) end
end
