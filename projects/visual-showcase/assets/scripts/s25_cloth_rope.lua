-- 25 Cloth & Rope: a traveller crosses the bridge and back, and W changes
-- the wind. The cloth, the ropes and the bridge are the engine's (`cloth`,
-- `rope`, joints and a character with `push_mass`); this walks the
-- traveller and turns the wind.
-- W: calm / breeze / gale. Space: hurry.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/25-cloth-and-rope.scene"
local TURN_X = 560                 -- where the traveller turns round
local GRAVITY = 1200

local post_on = nav.post_all_on(SCENE)
local captions = true
local status, walker
local cloths = {}
local dir, vy = 1, 0.0
local hurry = false
local winds = {
    { name = "calm",   flag = { 160, 20 },   banner = 50,  flutter = 120, wave = 1200 },
    { name = "breeze", flag = { 900, 60 },   banner = 320, flutter = 300, wave = 5000 },
    { name = "gale",   flag = { 2200, 140 }, banner = 900, flutter = 600, wave = 9000 },
}
local wind = 2

local function refresh_status()
    if status then
        Text.set(status, "wind " .. winds[wind].name .. (hurry and "   hurrying" or "") .. "   " .. nav.post_status(post_on))
    end
end

local function apply_wind()
    local w = winds[wind]
    for _, c in ipairs(cloths) do
        if c.kind == "flag" then
            Cloth.set(c.e, "wind", { w.flag[1], w.flag[2] })
            Cloth.set(c.e, "flutter", w.flutter)
            Cloth.set(c.e, "wave", w.wave)
        else
            Cloth.set(c.e, "wind", { w.banner, 0 })
            Cloth.set(c.e, "flutter", w.flutter * 0.5)
            Cloth.set(c.e, "wave", w.wave * 0.16)
        end
    end
    refresh_status()
end

function init(entity)
    status = Entity.find_by_name("Status")
    walker = Entity.find_by_name("Walker")
    for _, n in ipairs({ "FlagL", "FlagR" }) do
        local e = Entity.find_by_name(n)
        if e then cloths[#cloths + 1] = { e = e, kind = "flag" } end
    end
    for _, n in ipairs({ "BannerL", "BannerR" }) do
        local e = Entity.find_by_name(n)
        if e then cloths[#cloths + 1] = { e = e, kind = "banner" } end
    end
    apply_wind()
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    if Input.is_key_just_pressed("W") then
        wind = wind % #winds + 1
        apply_wind()
    end
    if Input.is_key_just_pressed("Space") then
        hurry = not hurry
        refresh_status()
    end

    if walker then
        local x = Transform.get_position(walker)
        if x > TURN_X then dir = -1 elseif x < -TURN_X then dir = 1 end
        Transform.set_scale(walker, dir, 1)
        local c = Physics.get_character(walker)
        if c and c.grounded then vy = -40 else vy = vy - GRAVITY * dt end
        Physics.move_character(walker, dir * (hurry and 180 or 80), vy)
    end
end
