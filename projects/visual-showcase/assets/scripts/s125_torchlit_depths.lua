-- 125 Torchlit Depths: the cave is lit by its `tile_light` alone - the
-- torches are `tile_light_source` entities, the lava, mushrooms and crystals
-- tiles the map names as emitters - flooded through the open cells. This
-- walks the miner (whose lantern is a source too) along the tunnel, follows
-- him with the camera, and flickers the torches, which the flood rebuilds
-- for at most `max_rate` times a second.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/125-torchlit-depths.scene"
local ROAD = { { -1776.0, -192.0 }, { -1744.0, -192.0 }, { -1712.0, -192.0 }, { -1680.0, -192.0 }, { -1648.0, -192.0 }, { -1616.0, -192.0 }, { -1584.0, -192.0 }, { -1552.0, -192.0 }, { -1520.0, -192.0 }, { -1488.0, -192.0 }, { -1456.0, -192.0 }, { -1424.0, -160.0 }, { -1392.0, -160.0 }, { -1360.0, -160.0 }, { -1328.0, -160.0 }, { -1296.0, -160.0 }, { -1264.0, -160.0 }, { -1232.0, -160.0 }, { -1200.0, -160.0 }, { -1168.0, -160.0 }, { -1136.0, -160.0 }, { -1104.0, -160.0 }, { -1072.0, -160.0 }, { -1040.0, -160.0 }, { -1008.0, -160.0 }, { -976.0, -160.0 }, { -944.0, -160.0 }, { -912.0, -160.0 }, { -880.0, -160.0 }, { -848.0, -160.0 }, { -816.0, -192.0 }, { -784.0, -192.0 }, { -752.0, -192.0 }, { -720.0, -192.0 }, { -688.0, -192.0 }, { -656.0, -192.0 }, { -624.0, -192.0 }, { -592.0, -192.0 }, { -560.0, -192.0 }, { -528.0, -224.0 }, { -496.0, -224.0 }, { -464.0, -224.0 }, { -432.0, -192.0 }, { -400.0, -192.0 }, { -368.0, -192.0 }, { -336.0, -192.0 }, { -304.0, -192.0 }, { -272.0, -192.0 }, { -240.0, -160.0 }, { -208.0, -160.0 }, { -176.0, -160.0 }, { -144.0, -160.0 }, { -112.0, -128.0 }, { -80.0, -128.0 }, { -48.0, -96.0 }, { -16.0, -96.0 }, { 16.0, -96.0 }, { 48.0, -64.0 }, { 80.0, -64.0 }, { 112.0, -32.0 }, { 144.0, -32.0 }, { 176.0, -32.0 }, { 208.0, 0.0 }, { 240.0, 0.0 }, { 272.0, 0.0 }, { 304.0, 32.0 }, { 336.0, 32.0 }, { 368.0, 32.0 }, { 400.0, 32.0 }, { 432.0, 32.0 }, { 464.0, 32.0 }, { 496.0, 64.0 }, { 528.0, 64.0 }, { 560.0, 64.0 }, { 592.0, 32.0 }, { 624.0, 32.0 }, { 656.0, 32.0 }, { 688.0, 32.0 }, { 720.0, 32.0 }, { 752.0, 32.0 }, { 784.0, 32.0 }, { 816.0, 0.0 }, { 848.0, 0.0 }, { 880.0, 0.0 }, { 912.0, 0.0 }, { 944.0, 0.0 }, { 976.0, -32.0 }, { 1008.0, -32.0 }, { 1040.0, -32.0 }, { 1072.0, -32.0 }, { 1104.0, -32.0 }, { 1136.0, -32.0 }, { 1168.0, -64.0 }, { 1200.0, -64.0 }, { 1232.0, -64.0 }, { 1264.0, -64.0 }, { 1296.0, -64.0 }, { 1328.0, -64.0 }, { 1360.0, -64.0 }, { 1392.0, -64.0 }, { 1424.0, -64.0 }, { 1456.0, -64.0 }, { 1488.0, -64.0 }, { 1520.0, -32.0 }, { 1552.0, -32.0 }, { 1584.0, -32.0 }, { 1616.0, -32.0 }, { 1648.0, -32.0 }, { 1680.0, -32.0 }, { 1712.0, -32.0 }, { 1744.0, -32.0 }, { 1776.0, -32.0 } }
local TORCHES = 86
local VIEW_W, VIEW_H = 1280, 720
local MAP_W, MAP_H = 3840, 1280

local cave, miner, lantern, glow, camera, status
local torches, flames = {}, {}
local captions, lit, walking = true, true, true
local t, s, dir = 0, 0, 1
local cx, cy

local function road_at(d)
    -- `d` in road points; linear between them.
    local i = math.floor(d)
    local f = d - i
    local a = ROAD[math.max(1, math.min(#ROAD, i))]
    local b = ROAD[math.max(1, math.min(#ROAD, i + 1))]
    return a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f
end

function init(entity)
    cave = Entity.find_by_name("Cave")
    miner = Entity.find_by_name("Miner")
    lantern = Entity.find_by_name("Lantern")
    glow = Entity.find_by_name("LanternGlow")
    camera = Entity.find_by_name("Camera")
    status = Entity.find_by_name("Status")
    for i = 0, TORCHES - 1 do
        torches[#torches + 1] = Entity.find_by_name("Torch" .. i)
        flames[#flames + 1] = Entity.find_by_name("Flame" .. i)
    end
    s = 1
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("Space") then walking = not walking end
    if Input.is_key_just_pressed("L") then
        lit = not lit
        TileLight.set(cave, "intensity", lit and 2.2 or 0.0)
    end
    t = t + dt
    if walking then
        s = s + dir * dt * 2.2
        if s >= #ROAD then s, dir = #ROAD, -1 end
        if s <= 1 then s, dir = 1, 1 end
    end
    local x, y = road_at(s)
    local bob = walking and math.abs(math.sin(t * 7)) * 3 or 0
    Transform.set_position(miner, x, y + bob)
    Transform.set_scale(miner, dir, 1)
    Transform.set_position(lantern, x + 22 * dir, y + 26 + bob)
    Transform.set_position(glow, x + 22 * dir, y + 26 + bob)
    -- The torches flicker, each on its own phase.
    for i, e in ipairs(torches) do
        local f = 0.85 + 0.08 * math.sin(t * 9 + i * 1.7) + 0.05 * math.sin(t * 23 + i * 3.1)
        TileLight.set(e, "intensity", f)
        Transform.set_scale(flames[i], 0.9 + f * 0.15, 0.85 + f * 0.25)
    end
    -- The camera follows, eased, inside the map.
    local hx, hy = MAP_W / 2 - VIEW_W / 2, MAP_H / 2 - VIEW_H / 2
    local tx = math.max(-hx, math.min(hx, x))
    local ty = math.max(-hy, math.min(hy, y + 40))
    if cx == nil then cx, cy = tx, ty end
    local k = 1 - math.exp(-dt * 2.5)
    cx, cy = cx + (tx - cx) * k, cy + (ty - cy) * k
    Transform.set_position(camera, cx, cy)
    UI.set_text(status, lit and (TORCHES .. " torches, lava, mushrooms, crystals - one flood") or "the tile light off")
end
