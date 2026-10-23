-- 113 Falling Sands: one `pixel_sim` is the whole cave - sand, water, oil,
-- fire, smoke, lava, steam, stone and wood, each a cell moved by its rules.
-- This script only starts things: a spark on the oil (F, and every 12 s),
-- a lava bomb onto the bridge (L, and every 17 s), crates tipped onto the
-- dunes - and it sets the cave again after a while.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/113-falling-sands.scene"
local OIL = { 350.000000, -3.300000 }
local BRIDGE = { 0.000000, 50.000000 }
local DROPS = { { -566.700000, 316.700000 }, { -400.000000, 316.700000 } }

local cave, oil_fire, status
local t = 0
local captions = true
local next_fire, next_lava, next_crate = 6, 10, 3
local burning = 0
local crates = 0

local function spark()
    PixelSim.paint(cave, OIL[1], OIL[2], 10, "fire")
    burning = 9
end

local function lava_bomb()
    PixelSim.paint(cave, BRIDGE[1] + math.sin(t) * 60, 330, 16, "lava")
end

function init(entity)
    cave = Entity.find_by_name("Cave")
    oil_fire = Entity.find_by_name("OilFire")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    next_fire = next_fire - dt
    next_lava = next_lava - dt
    next_crate = next_crate - dt
    if Input.is_key_just_pressed("F") or next_fire <= 0 then
        spark()
        next_fire = 12
    end
    if Input.is_key_just_pressed("L") or next_lava <= 0 then
        lava_bomb()
        next_lava = 17
    end
    if next_crate <= 0 and crates < 6 then
        crates = crates + 1
        local d = DROPS[crates % 2 + 1]
        local c = Entity.spawn_prefab("FsCrate")
        Transform.set_position(c, d[1] + (crates * 37) % 60 - 30, d[2])
        next_crate = 5
    end
    -- The oil fire's light while it burns.
    burning = math.max(0, burning - dt)
    Lighting.set_intensity(oil_fire, burning > 0 and (1.8 + 0.5 * math.sin(t * 13)) * math.min(1, burning) or 0)
    if t > 80 then Scene.load(SCENE) end
    UI.set_text(status, string.format("the cave is set again in %d s", math.floor(80 - t + 0.5)))
end
