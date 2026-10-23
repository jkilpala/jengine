-- 108 Orbital Station: raiders make passes at a shielded station and the
-- defenders chase them. The flying and the shooting are this script; what
-- you see is the engine's - every bolt that reaches the shield ripples it
-- where it lands (a ring and a shockwave clipped to the bubble by a
-- `sprite_mask`), lasers are `line`s, missiles trail particle smoke, and a
-- raider that dies goes up in prefabs of particles, a flash and a wave.
-- The sun and the flashes are bright enough to flare the lens (`lens_flare`);
-- L turns it off to compare.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/108-orbital-station.scene"
local atan2 = math.atan2 or function(y, x) return math.atan(y, x) end

local STX, STY = 260.000000, 40.000000
local SHW, SHH = 780.000000, 440.000000
local RAIDERS, DARTS, BOLTS, MISSILES, RIPPLES, ROCKS = 5, 4, 24, 6, 10, 7
local TURRETS = { { -30.0, 52.0 }, { 120.0, -40.0 }, { 390.0, 50.0 } }

local e = {}
local raiders, darts, bolts, missiles, ripples, rocks, turrets = {}, {}, {}, {}, {}, {}, {}
local booms = {}
local t = 0
local rng = 108
local captions = true
local flare_on = true
local flash = 0
local hits = 0
local downed = 0

local function rand()
    rng = (rng * 16807) % 2147483647
    return rng / 2147483647
end
local function between(a, b) return a + (b - a) * rand() end
local function wrap(a)
    while a > math.pi do a = a - 2 * math.pi end
    while a < -math.pi do a = a + 2 * math.pi end
    return a
end

-- Is (x, y) inside the shield? (1 at its rim.)
local function shield_at(x, y)
    local dx, dy = (x - STX) / (SHW * 0.5), (y - STY) / (SHH * 0.5)
    return math.sqrt(dx * dx + dy * dy)
end

local function spawn(prefab, x, y)
    local h = Entity.spawn_prefab(prefab)
    Transform.set_position(h, x, y)
    return h
end

-- Turn a ship toward (tx, ty) at `rate`, and fly it on at its speed.
local function steer(s, tx, ty, rate, dt)
    local want = atan2(ty - s.y, tx - s.x)
    local d = wrap(want - s.a)
    s.a = s.a + math.max(-rate * dt, math.min(rate * dt, d))
    s.x = s.x + math.cos(s.a) * s.speed * dt
    s.y = s.y + math.sin(s.a) * s.speed * dt
    return math.abs(d)
end

local function ripple(x, y, strength)
    for _, r in ipairs(ripples) do
        if r.age >= r.life then
            r.x, r.y, r.age, r.life, r.k = x, y, 0, 0.75, strength
            hits = hits + 1
            flash = math.min(1, flash + 0.35 * strength)
            return
        end
    end
end

local function explode(x, y)
    spawn("OsBoom", x, y)
    spawn("OsFlash", x, y)
    spawn("OsSmoke", x, y)
    booms[#booms + 1] = { e = spawn("OsWave", x, y), age = 0 }
    downed = downed + 1
end

local function park(id) Transform.set_position(id, 0, -5000) end
local function beam_off(name)
    Line.set(e[name .. "Beam"], "points", { { 0, -5000 }, { 1, -5000 } })
    Line.set(e[name .. "Glow"], "points", { { 0, -5000 }, { 1, -5000 } })
end
local function beam_on(name, x0, y0, x1, y1, flicker)
    local w = 1 + 0.25 * math.sin(t * 90 + x0)
    Line.set(e[name .. "Beam"], "points", { { x0, y0 }, { x1, y1 } })
    Line.set(e[name .. "Glow"], "points", { { x0, y0 }, { x1, y1 } })
    Line.set(e[name .. "Beam"], "width", (flicker or 3) * w)
end

local function respawn(r)
    r.x = between(1050, 1250)
    r.y = between(-450, 450)
    r.a = math.pi + between(-0.3, 0.3)
    r.hp = 5
    r.dead = false
    r.respawn = 0
    r.cool = between(0.5, 1.5)
    r.orbit = between(0, 2 * math.pi)
end

function init(entity)
    for _, n in ipairs({ "Status", "ShieldFlash", "Shield" }) do e[n] = Entity.find_by_name(n) end
    for i = 0, RAIDERS - 1 do
        local r = { e = Entity.find_by_name("Raider" .. i), speed = between(330, 380) }
        respawn(r)
        r.x = r.x + i * 260
        raiders[#raiders + 1] = r
    end
    for i = 0, DARTS - 1 do
        local name = "Dart" .. i
        e[name .. "Beam"] = Entity.find_by_name(name .. "Beam")
        e[name .. "Glow"] = Entity.find_by_name(name .. "Glow")
        darts[#darts + 1] = { e = Entity.find_by_name(name), name = name, x = STX - 200 + i * 120, y = STY - 300,
                              a = 1.4, speed = between(400, 440), cool = between(0.5, 2), firing = 0, missile = between(2, 5) }
    end
    for i, p in ipairs(TURRETS) do
        local name = "Turret" .. (i - 1)
        e[name .. "Beam"] = Entity.find_by_name(name .. "Beam")
        e[name .. "Glow"] = Entity.find_by_name(name .. "Glow")
        turrets[#turrets + 1] = { name = name, x = STX + p[1], y = STY + p[2], cool = between(1, 3), firing = 0 }
    end
    for i = 0, BOLTS - 1 do
        local id = Entity.find_by_name("Bolt" .. i)
        bolts[#bolts + 1] = { e = id, live = false }
    end
    for i = 0, MISSILES - 1 do
        missiles[#missiles + 1] = { e = Entity.find_by_name("Missile" .. i), smoke = Entity.find_by_name("Missile" .. i .. "Smoke"), live = false }
    end
    for i = 0, RIPPLES - 1 do
        ripples[#ripples + 1] = { e = Entity.find_by_name("Ripple" .. i), wave = Entity.find_by_name("RippleWave" .. i), age = 1, life = 0 }
    end
    for i = 0, ROCKS - 1 do
        local id = Entity.find_by_name("Rock" .. i)
        local x, y = Transform.get_position(id)
        rocks[#rocks + 1] = { e = id, x = x, y = y, vx = between(-40, -14), vy = between(-8, 8), a = between(0, 6), w = between(-0.5, 0.5) }
    end
    for i = 0, 3 do e["Beacon" .. i] = Entity.find_by_name("Beacon" .. i) end
end

local function fire_bolt(r)
    for _, b in ipairs(bolts) do
        if not b.live then
            -- Aimed at the station, a little off.
            local tx, ty = STX + between(-120, 120), STY + between(-60, 60)
            local a = atan2(ty - r.y, tx - r.x)
            b.live, b.x, b.y, b.vx, b.vy, b.age = true, r.x + math.cos(a) * 30, r.y + math.sin(a) * 30,
                math.cos(a) * 1300, math.sin(a) * 1300, 0
            Transform.set_position(b.e, b.x, b.y)
            Transform.set_rotation(b.e, a)
            Trail.clear(b.e)
            Trail.set_emitting(b.e, true)
            Sprite.set_color(b.e, 6, 2.1, 1.2, 1)
            Lighting.set_intensity(b.e, 1.2)
            return
        end
    end
end

local function fire_missile(d, target)
    for _, m in ipairs(missiles) do
        if not m.live then
            m.live, m.x, m.y, m.a, m.speed, m.target, m.age = true, d.x, d.y, d.a + between(-0.6, 0.6), 380, target, 0
            Particles.set(m.smoke, "active", true)
            return
        end
    end
end

local function nearest_raider(x, y, reach)
    local best, bd = nil, reach
    for _, r in ipairs(raiders) do
        if not r.dead then
            local d = math.sqrt((r.x - x) ^ 2 + (r.y - y) ^ 2)
            if d < bd then best, bd = r, d end
        end
    end
    return best, bd
end

local function hurt(r, n)
    if r.dead then return end
    r.hp = r.hp - n
    if r.hp <= 0 then
        r.dead = true
        r.respawn = between(2.5, 4.5)
        explode(r.x, r.y)
        Trail.set_emitting(r.e, false)
        Trail.clear(r.e)
        park(r.e)
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("L") then
        flare_on = not flare_on
        PostProcess.set_enabled("lens_flare", flare_on)
    end

    -- Raiders: in from the right, a pass round the shield firing as they
    -- go, out and round again.
    for _, r in ipairs(raiders) do
        if r.dead then
            r.respawn = r.respawn - dt
            if r.respawn <= 0 then
                respawn(r)
                Trail.clear(r.e)
                Trail.set_emitting(r.e, true)
            end
        else
            r.orbit = r.orbit + dt * 0.45
            local tx = STX + math.cos(r.orbit) * SHW * 0.72
            local ty = STY + math.sin(r.orbit) * SHH * 0.85
            steer(r, tx, ty, 2.2, dt)
            r.cool = r.cool - dt
            local rim = shield_at(r.x, r.y)
            if r.cool <= 0 and rim < 2.0 and rim > 1.05 then
                fire_bolt(r)
                r.cool = between(0.35, 0.9)
            end
            Transform.set_position(r.e, r.x, r.y)
            Transform.set_rotation(r.e, r.a)
        end
    end

    -- Bolts fly until the shield takes them.
    for _, b in ipairs(bolts) do
        if b.live then
            b.x, b.y, b.age = b.x + b.vx * dt, b.y + b.vy * dt, b.age + dt
            if shield_at(b.x, b.y) <= 1.0 then
                ripple(b.x, b.y, 1.0)
                b.live = false
            elseif b.age > 1.5 then
                b.live = false
            end
            if b.live then
                Transform.set_position(b.e, b.x, b.y)
            else
                Trail.set_emitting(b.e, false)
                Trail.clear(b.e)
                Sprite.set_color(b.e, 6, 2.1, 1.2, 0)
                Lighting.set_intensity(b.e, 0)
                park(b.e)
            end
        end
    end

    -- Darts chase the nearest raider, firing their lasers when it is in
    -- their sights, and now and then a missile.
    for _, d in ipairs(darts) do
        local r, dist = nearest_raider(d.x, d.y, 4000)
        local off = 4
        if r then
            off = steer(d, r.x + math.cos(r.a) * 120, r.y + math.sin(r.a) * 120, 2.6, dt)
        else
            d.orbit = (d.orbit or 0) + dt * 0.5
            steer(d, STX + math.cos(d.orbit) * 500, STY + math.sin(d.orbit) * 320, 2.0, dt)
        end
        d.cool = d.cool - dt
        d.missile = d.missile - dt
        if r and d.cool <= 0 and off < 0.2 and dist < 520 then
            d.firing, d.cool = 0.18, between(0.8, 1.6)
        end
        if d.firing > 0 then
            d.firing = d.firing - dt
            if r then
                beam_on(d.name, d.x + math.cos(d.a) * 30, d.y + math.sin(d.a) * 30, r.x, r.y)
                if d.firing <= 0 then hurt(r, 1) end
            end
        else
            beam_off(d.name)
        end
        if r and d.missile <= 0 and dist < 900 then
            fire_missile(d, r)
            d.missile = between(3, 6)
        end
        Transform.set_position(d.e, d.x, d.y)
        Transform.set_rotation(d.e, d.a)
    end

    -- Missiles home and leave smoke.
    for _, m in ipairs(missiles) do
        if m.live then
            m.age = m.age + dt
            m.speed = math.min(900, m.speed + 700 * dt)
            local r = m.target
            if r.dead then r = nearest_raider(m.x, m.y, 4000) or r; m.target = r end
            steer(m, r.x, r.y, 3.2, dt)
            local hit = not r.dead and (r.x - m.x) ^ 2 + (r.y - m.y) ^ 2 < 34 * 34
            if hit then hurt(r, 3) end
            if hit or m.age > 4 then
                if not hit then explode(m.x, m.y); downed = downed - 1 end
                m.live = false
                Particles.set(m.smoke, "active", false)
                park(m.e)
            else
                Transform.set_position(m.e, m.x, m.y)
                Transform.set_rotation(m.e, m.a)
                Transform.set_position(m.smoke, m.x - math.cos(m.a) * 18, m.y - math.sin(m.a) * 18)
            end
        end
    end

    -- The station's turrets fire through the shield at raiders in reach.
    for _, tu in ipairs(turrets) do
        tu.cool = tu.cool - dt
        local r, dist = nearest_raider(tu.x, tu.y, 700)
        if r and tu.cool <= 0 then
            tu.firing, tu.cool, tu.target = 0.22, between(1.8, 3.2), r
        end
        if tu.firing > 0 and tu.target and not tu.target.dead then
            tu.firing = tu.firing - dt
            beam_on(tu.name, tu.x, tu.y, tu.target.x, tu.target.y, 4)
            if tu.firing <= 0 then hurt(tu.target, 1) end
        else
            tu.firing = 0
            beam_off(tu.name)
        end
    end

    -- Ripples spread over the shield and fade; the shield flashes with hits.
    for _, r in ipairs(ripples) do
        if r.age < r.life then
            r.age = r.age + dt
            local p = math.min(1, r.age / r.life)
            local s = 40 + 360 * p
            Transform.set_position(r.e, r.x, r.y)
            Transform.set_scale(r.e, s / 40, s / 40)
            local a = (1 - p) ^ 1.5 * r.k
            Sprite.set_color(r.e, 2.4, 4.5, 5.0, a)
            Lighting.set_intensity(r.e, 2.2 * a)
            Transform.set_position(r.wave, r.x, r.y)
            Material.set_vec4(r.wave, "progress", p, 0, 0, 0)
            Material.set_vec4(r.wave, "tint", 0.3 * (1 - p), 0.7 * (1 - p), 1.0 * (1 - p), 0)
            if r.age >= r.life then
                Lighting.set_intensity(r.e, 0)
                park(r.e)
                park(r.wave)
            end
        end
    end
    flash = math.max(0, flash - dt * 1.8)
    Sprite.set_color(e.ShieldFlash, 1.75, 2.4, 3.0, flash * 0.6)
    local hum = 0.5 + 0.06 * math.sin(t * 2.3)
    Sprite.set_color(e.Shield, 0.35, 0.95, 1.4, hum)

    -- Explosion waves.
    for i = #booms, 1, -1 do
        local w = booms[i]
        w.age = w.age + dt
        if w.age > 0.65 then
            table.remove(booms, i)
        else
            Material.set_vec4(w.e, "progress", w.age / 0.65, 0, 0, 0)
            Material.set_vec4(w.e, "tint", 1.0, 0.55, 0.25, 0)
        end
    end

    -- Rocks drift by and come round again.
    for _, k in ipairs(rocks) do
        k.x, k.y, k.a = k.x + k.vx * dt, k.y + k.vy * dt, k.a + k.w * dt
        if k.x < -1150 then k.x = 1150 end
        Transform.set_position(k.e, k.x, k.y)
        Transform.set_rotation(k.e, k.a)
    end

    -- The beacons blink.
    for i = 0, 3 do
        local on = math.floor(t * 1.2 + i * 0.5) % 2 == 0
        Lighting.set_intensity(e["Beacon" .. i], on and 1.4 or 0)
        Sprite.set_color(e["Beacon" .. i], i % 2 == 0 and 6 or 1.2, i % 2 == 0 and 0.9 or 6, i % 2 == 0 and 0.6 or 2.1, on and 1 or 0.1)
    end

    UI.set_text(e.Status, string.format("shield hits %d   raiders down %d   lens flare %s", hits, downed, flare_on and "on" or "off"))
end
