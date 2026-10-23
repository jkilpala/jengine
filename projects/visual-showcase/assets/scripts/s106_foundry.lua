-- 106 The Foundry: the crane's cycle. The metal is the engine's `fluid`:
-- this script moves the crucible over each mould, tips it, and turns the
-- pour on (`Fluid.set(melt, "pour_rate", ...)`) while the lip is down; the
-- liquid does the rest - falls, splashes, levels out, cools and sets.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/106-foundry.scene"

local MOULDS = { -150.0, 150.0, 450.0 }
local CRUCIBLE_Y = 20.000000
local RAIL_Y = 470.000000
local LIP = { 95.000000, 62.000000 }
local REST_X = -650
local TILT = -1.12
local POUR_RATE = 110

local e = {}
local t = 0
local x = REST_X
local tilt = 0
local phase = "rest"
local timer = 3
local mould = 1
local poured = 0
local hot = {}
local captions = true

local function lip()
    local c, s = math.cos(tilt), math.sin(tilt)
    return x + c * LIP[1] - s * LIP[2], CRUCIBLE_Y + s * LIP[1] + c * LIP[2]
end

local function pour(on)
    Fluid.set(e.Melt, "pour_rate", on and POUR_RATE or 0)
    Particles.set_active(e.SplashSparks, on)
    Particles.set_active(e.LipSparks, on)
    if on then
        Transform.set_position(e.SplashSparks, MOULDS[mould], -340)
        Particles.set_active(e["Steam" .. (mould - 1)], true)
        hot[mould] = 0
    end
end

local function approach(v, target, rate, dt)
    if v < target then return math.min(target, v + rate * dt) end
    return math.max(target, v - rate * dt)
end

function init(entity)
    for _, n in ipairs({ "Melt", "Crucible", "CrucibleGlow", "Trolley", "ChainL", "ChainR", "SplashSparks",
                         "LipSparks", "KnockOut", "FurnaceLight", "FurnaceFire", "Status" }) do
        e[n] = Entity.find_by_name(n)
    end
    for i = 0, 2 do
        e["Steam" .. i] = Entity.find_by_name("Steam" .. i)
        e["Haze" .. i] = Entity.find_by_name("Haze" .. i)
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("Space") and phase == "rest" then timer = 0 end
    timer = timer - dt
    -- The crane's cycle: travel, tip, pour, tip back - three times - then
    -- back to rest while the castings cool, then knock them out.
    if phase == "rest" then
        x = approach(x, REST_X, 220, dt)
        if timer <= 0 then
            phase, mould = "travel", 1
        end
    elseif phase == "travel" then
        local want = MOULDS[mould] - 122
        x = approach(x, want, 200, dt)
        if math.abs(x - want) < 1 then phase = "tip" end
    elseif phase == "tip" then
        tilt = approach(tilt, TILT, 0.9, dt)
        if tilt <= TILT then
            phase, timer = "pour", 2.0
            pour(true)
        end
    elseif phase == "pour" then
        poured = poured + dt
        if timer <= 0 then
            pour(false)
            phase = "untip"
        end
    elseif phase == "untip" then
        tilt = approach(tilt, 0, 1.0, dt)
        if tilt >= 0 then
            if mould < #MOULDS then
                mould = mould + 1
                phase = "travel"
            else
                phase, timer = "cool", 26
            end
        end
    elseif phase == "cool" then
        x = approach(x, REST_X, 220, dt)
        if timer <= 0 then
            -- Knocked out: the castings go, in a cloud of sand.
            for _, mx in ipairs(MOULDS) do
                Transform.set_position(e.KnockOut, mx, -320)
                Particles.emit_burst(e.KnockOut, 14)
            end
            Camera.shake(4, 0.3, 20)
            Fluid.set(e.Melt, "clear", true)
            poured = 0
            phase, timer = "rest", 4
        end
    end
    -- Place the crane, the crucible and the pour.
    Transform.set_position(e.Trolley, x, RAIL_Y - 10)
    Transform.set_position(e.Crucible, x, CRUCIBLE_Y)
    Transform.set_rotation(e.Crucible, tilt)
    local lx, ly = lip()
    Transform.set_position(e.Melt, lx + 4, ly - 2)
    Transform.set_rotation(e.Melt, tilt)
    Transform.set_position(e.LipSparks, lx, ly)
    Line.set(e.ChainL, "points", { { x - 30, RAIL_Y - 40 }, { x - 6, CRUCIBLE_Y + 8 } })
    Line.set(e.ChainR, "points", { { x + 30, RAIL_Y - 40 }, { x + 6, CRUCIBLE_Y + 8 } })
    -- The crucible's mouth dims as it empties.
    local full = math.max(0.15, 1 - poured / 6.5)
    Sprite.set_color(e.CrucibleGlow, 4.0 * full, 3.0 * full, 1.4 * full, 1)
    -- Steam and shimmer over each mould while it is hot.
    for i = 1, #MOULDS do
        if hot[i] then
            hot[i] = hot[i] + dt
            local k = math.max(0, 1 - hot[i] / 24)
            Sprite.set_color(e["Haze" .. (i - 1)], 1, 1, 1, 0.9 * k)
            if hot[i] > 20 then
                Particles.set_active(e["Steam" .. (i - 1)], false)
            end
            if hot[i] > 26 then hot[i] = nil end
        end
    end
    -- The furnace breathes.
    local f = 0.85 + 0.1 * math.sin(t * 7.3) + 0.06 * math.sin(t * 17.1)
    Lighting.set_intensity(e.FurnaceLight, 2.0 * f)
    UI.set_text(e.Status, phase == "pour" and string.format("pouring into mould %d", math.floor(mould))
        or (phase == "cool" and string.format("cooling   %d s", math.floor(math.max(0, timer)))) or phase)
end
