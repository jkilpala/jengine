-- 30 Spell Storm: how many sparks, and whether time runs. The sparks are the
-- engine's (`gpu_particles`, every one computed on the GPU); this sets their
-- counts with `Particles.set` and flickers the lights.
-- Up / Down: 50k to 400k.  Space: stop time.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/30-spell-storm.scene"
-- vortex, column, knot: the share of the total each one has
local SHARES = { { "StormVortex", 0.72 }, { "StormColumn", 0.22 }, { "StormKnot", 0.06 } }
local TIERS = { 50000, 100000, 200000, 300000, 400000 }

local post_on = nav.post_all_on(SCENE)
local captions = true
local status
local storms = {}
local tier = 3
local frozen = false
local t = 0.0
local lights = {}

local function total()
    return TIERS[tier]
end

local function refresh_status()
    if status then
        UI.set_text(status, math.floor(total() / 1000 + 0.5) .. "k sparks" .. (frozen and "   time stopped" or "")
            .. "   " .. nav.post_status(post_on))
    end
end

local function apply()
    for _, s in ipairs(storms) do
        Particles.set(s.e, "count", math.floor(total() * s.share + 0.5))
        Particles.set(s.e, "playing", not frozen)
    end
    refresh_status()
end

function init(entity)
    status = Entity.find_by_name("Status")
    for _, s in ipairs(SHARES) do
        local e = Entity.find_by_name(s[1])
        if e then storms[#storms + 1] = { e = e, share = s[2] } end
    end
    for _, n in ipairs({ "CircleLight", "StaffLight" }) do
        local e = Entity.find_by_name(n)
        if e then lights[#lights + 1] = e end
    end
    apply()
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("Up") and tier < #TIERS then tier = tier + 1; apply() end
    if Input.is_key_just_pressed("Down") and tier > 1 then tier = tier - 1; apply() end
    if Input.is_key_just_pressed("Space") then frozen = not frozen; apply() end
    if not frozen then t = t + dt end
    for i, l in ipairs(lights) do
        Lighting.set_intensity(l, 1.5 + 0.25 * math.sin(t * 7.0 + i * 1.9) + 0.15 * math.sin(t * 17.0 + i))
    end
end
