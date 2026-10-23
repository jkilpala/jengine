-- 102 Firefight: two squads across a lot. The fighting is this script -
-- who aims at whom, when a burst or a beam starts, what a round hits - and
-- everything you see is the engine's: tracers are a pool of sprites with
-- trails and lights, beams are `line`s, impacts are prefabs of particles
-- that bounce off the cover and light the ground, and every hit is painted
-- into the asphalt with Sprite.paint, so the lot keeps the whole fight.
-- Cover is waist high: a soldier fires over their own and the target's
-- stops most rounds (some still get through). Rounds that find a soldier
-- are found by circle tests here; cover by Physics.raycast.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/102-firefight.scene"
local atan2 = math.atan2 or function(y, x) return math.atan(y, x) end

local HALF_X, HALF_Y = 900.000000, 520.000000
local RED_COVER = { { -625.0, 228.0 }, { -600.0, -210.0 }, { -348.0, 40.0 }, { -102.0, -250.0 }, { -132.0, 268.0 } }
local BLUE_COVER = { { 648.0, -230.0 }, { 600.0, 210.0 }, { 348.0, -40.0 }, { 88.0, -250.0 }, { 132.0, 230.0 } }
local RED_SPAWN = { -860.000000, 0.000000 }
local BLUE_SPAWN = { 860.000000, 0.000000 }
local BARREL_AT = { 60.000000, 20.000000 }
local TRACERS = 28
local SQUAD = 3
local FIRES = { "Car", "Barrel1", "Barrel2" }
local FIRE_BASE = { Car = 2.2, Barrel1 = 1.39, Barrel2 = 1.39 }

local RADIUS = 17          -- a soldier, to a round
local OWN_COVER = 80       -- a round starts this far out, over the shooter's cover
local OVER_THE_TOP = 0.35  -- how often a round gets past the target's cover
local BULLET = 2600        -- tracer speed, units a second
local BULLET_DAMAGE = 21
local BEAM_DPS = 85
local BLOOD = { 0.10, 0.003, 0.003 }

local e = {}
local soldiers = {}
local tracers = {}
local waves = {}
local booms = {}
local kills = { R = 0, B = 0 }
local captions = true
local t = 0
local rng = 102
local barrel = { up = true, hits = 0, heat = 0.0, quiet = 0.0, back = 0.0 }
local chroma = 0.0

local function rand()
    rng = (rng * 16807) % 2147483647
    return rng / 2147483647
end
local function between(a, b) return a + (b - a) * rand() end

local function paint(x, y, size, r, g, b, a, shape, rot)
    Sprite.paint(e.Floor, x, y, size, r, g, b, a, shape, rot or 0)
end

local function spawn(prefab, x, y)
    local h = Entity.spawn_prefab(prefab)
    Transform.set_position(h, x, y)
    return h
end

-- A burst of particles thrown along `angle`, spread by `spread`.
local function burst(prefab, x, y, angle, spread)
    local h = spawn(prefab, x, y)
    if angle then Particles.set(h, "angle", { angle - spread, angle + spread }) end
    return h
end

-- ── the squads ─────────────────────────────────────────────────────────

local function make_soldier(team, i)
    local name = team .. i
    local s = {
        team = team, name = name, e = Entity.find_by_name(name),
        muzzle = Entity.find_by_name(name .. "Muzzle"),
        hp = 100, alive = true, back = 0, flash = 0,
        cover = nil, relocate = between(2, 5), cooldown = between(0.8, 2.0),
        shots = 0, next_shot = 0, beam = 0, over = false, aim = team == "R" and 0 or math.pi,
        lx = 0, ly = 0, heading = team == "R" and 0 or math.pi, bleed = 0, pool = 0, pool_size = 0,
    }
    if team == "R" then
        s.casings = Entity.find_by_name(name .. "Casings")
    else
        s.beam_e = Entity.find_by_name(name .. "Beam")
        s.glow_e = Entity.find_by_name(name .. "Glow")
        s.hit_e = Entity.find_by_name(name .. "Hit")
    end
    s.lx, s.ly = Transform.get_position(s.e)
    return s
end

local function covers_of(team) return team == "R" and RED_COVER or BLUE_COVER end

local function pick_cover(s)
    local list = covers_of(s.team)
    local best, tries = nil, 0
    repeat
        best = 1 + math.floor(rand() * #list)
        tries = tries + 1
        local taken = false
        for _, o in ipairs(soldiers) do
            if o ~= s and o.team == s.team and o.cover == best then taken = true end
        end
        if not taken then break end
    until tries > 8
    s.cover = best
    local c = list[best]
    Nav.set_target(s.e, c[1] + between(-8, 8), c[2] + between(-8, 8))
end

local function enemies_of(s)
    local out = {}
    for _, o in ipairs(soldiers) do
        if o.team ~= s.team and o.alive then out[#out + 1] = o end
    end
    return out
end

local function nearest_enemy(s, x, y)
    local best, bd = nil, 1e9
    for _, o in ipairs(enemies_of(s)) do
        local ox, oy = Transform.get_position(o.e)
        local d = (ox - x) ^ 2 + (oy - y) ^ 2
        if d < bd then best, bd = o, d end
    end
    return best
end

-- Where a ray from (x, y) along (dx, dy) first meets a soldier's circle.
local function ray_circle(x, y, dx, dy, cx, cy, r)
    local fx, fy = x - cx, y - cy
    local b = fx * dx + fy * dy
    local c = fx * fx + fy * fy - r * r
    local disc = b * b - c
    if disc < 0 then return nil end
    local d = -b - math.sqrt(disc)
    if d < 0 then return nil end
    return d
end

-- What a shot along (dx, dy) from the muzzle finds first: a soldier (and
-- which), the barrel, cover (and its normal), or nothing within `reach`.
local function trace(s, x, y, dx, dy, reach, over)
    local hit = { kind = "none", d = reach, x = x + dx * reach, y = y + dy * reach }
    local best_s, best_d = nil, reach
    for _, o in ipairs(enemies_of(s)) do
        local ox, oy = Transform.get_position(o.e)
        local d = ray_circle(x, y, dx, dy, ox, oy, RADIUS)
        if d and d < best_d then best_s, best_d = o, d end
    end
    if barrel.up then
        local d = ray_circle(x, y, dx, dy, BARREL_AT[1], BARREL_AT[2], 19)
        if d and d < best_d then best_s, best_d = "barrel", d end
    end
    local ray = Physics.raycast(x + dx * OWN_COVER, y + dy * OWN_COVER, dx, dy, reach)
    if ray and ray.distance + OWN_COVER < best_d then
        local cd = ray.distance + OWN_COVER
        -- The target's own cover: some rounds still get over it.
        local pass = false
        if best_s and best_s ~= "barrel" and best_d - cd < 90 then pass = over end
        if not pass then
            return { kind = "cover", d = cd, x = ray.x, y = ray.y, nx = ray.normal_x, ny = ray.normal_y }
        end
    end
    if best_s == "barrel" then
        return { kind = "barrel", d = best_d, x = x + dx * best_d, y = y + dy * best_d }
    elseif best_s then
        return { kind = "soldier", who = best_s, d = best_d, x = x + dx * best_d, y = y + dy * best_d }
    end
    return hit
end

local function muzzle_of(s)
    local x, y = Transform.get_position(s.e)
    local c, sn = math.cos(s.heading), math.sin(s.heading)
    return x + c * 34 + sn * 4, y + sn * 34 - c * 4
end

-- ── what a hit leaves ──────────────────────────────────────────────────

local function bleed_at(x, y, angle, size)
    burst("FfBlood", x, y, angle, 0.45)
    burst("FfMist", x, y, angle, 0.6)
    paint(x + math.cos(angle) * 10, y + math.sin(angle) * 10, size, BLOOD[1], BLOOD[2], BLOOD[3], 0.9, "splat", angle)
end

local function die(o, angle)
    o.alive = false
    o.hp = 0
    o.back = between(6, 9)
    o.pool, o.pool_size = 2.4, 18
    o.beam = 0
    Nav.stop(o.e)
    Sprite.set_color(o.e, 0.42, 0.34, 0.34, 1)
    Lighting.set_intensity(o.e, 0)
    Transform.set_rotation(o.e, angle + between(-0.6, 0.6))
    kills[o.team == "R" and "B" or "R"] = kills[o.team == "R" and "B" or "R"] + 1
    local x, y = Transform.get_position(o.e)
    bleed_at(x, y, angle, 52)
    paint(x + math.cos(angle) * 30, y + math.sin(angle) * 30, 70, BLOOD[1], BLOOD[2], BLOOD[3], 0.8, "streak", angle)
    Camera.shake(3, 0.2, 25)
end

local function wound(o, amount, angle, x, y)
    if not o.alive then return end
    o.hp = o.hp - amount
    bleed_at(x, y, angle, between(22, 36))
    if o.hp <= 0 then die(o, angle) end
end

local function explode()
    if not barrel.up then return end
    barrel.up, barrel.hits, barrel.heat, barrel.quiet = false, 0, 0, 0
    barrel.back = 15
    local x, y = BARREL_AT[1], BARREL_AT[2]
    Sprite.set_color(e.Barrel, 1, 1, 1, 0)
    booms[#booms + 1] = { e = spawn("FfBoom", x, y), age = 0 }
    spawn("FfDebris", x, y)
    spawn("FfSmoke", x, y)
    waves[#waves + 1] = { e = spawn("FfWave", x, y), age = 0 }
    Camera.shake(16, 0.6, 28)
    Camera.flash(0.14, 1.0, 0.8, 0.55, 0.35)
    chroma = 1.0
    -- A scorch on the asphalt: a dark blot and its rays.
    paint(x, y, 150, 0.03, 0.025, 0.02, 0.6, "disc", 0)
    paint(x, y, 70, 0.02, 0.018, 0.015, 0.7, "splat", between(0, 6.28))
    for k = 0, 7 do
        local a = k * math.pi / 4 + between(-0.3, 0.3)
        paint(x + math.cos(a) * 80, y + math.sin(a) * 80, between(70, 130), 0.03, 0.025, 0.02, 0.4, "streak", a)
    end
    for _, o in ipairs(soldiers) do
        if o.alive then
            local ox, oy = Transform.get_position(o.e)
            local d = math.sqrt((ox - x) ^ 2 + (oy - y) ^ 2)
            local a = atan2(oy - y, ox - x)
            if d < 170 then
                wound(o, 200, a, ox, oy)
            elseif d < 300 then
                wound(o, 45, a, ox, oy)
            end
        end
    end
end

local function impact(s, hit, angle)
    if hit.kind == "soldier" then
        wound(hit.who, BULLET_DAMAGE, angle, hit.x, hit.y)
    elseif hit.kind == "barrel" then
        burst("FfSparks", hit.x, hit.y, angle + math.pi, 1.2)
        barrel.hits = barrel.hits + 1
        if barrel.hits >= 3 then explode() end
    elseif hit.kind == "cover" then
        local n = atan2(hit.ny, hit.nx)
        burst("FfSparks", hit.x, hit.y, n, 1.0)
        burst("FfDust", hit.x, hit.y, n, 0.9)
    else
        burst("FfDust", hit.x, hit.y, angle, 1.4)
        paint(hit.x, hit.y, between(6, 11), 0.05, 0.05, 0.05, 0.5, "disc", 0)
    end
end

-- ── rifles ─────────────────────────────────────────────────────────────

local function free_tracer()
    for _, tr in ipairs(tracers) do
        if not tr.live then return tr end
    end
    return nil
end

local function fire_round(s)
    local x, y = muzzle_of(s)
    local a = s.aim + between(-0.06, 0.06)
    local dx, dy = math.cos(a), math.sin(a)
    local hit = trace(s, x, y, dx, dy, 1500, rand() < OVER_THE_TOP)
    local tr = free_tracer()
    if tr then
        tr.live, tr.x, tr.y, tr.dx, tr.dy, tr.go, tr.len, tr.hit, tr.a, tr.from = true, x, y, dx, dy, 0, hit.d, hit, a, s
        Trail.clear(tr.e)
        Transform.set_position(tr.e, x, y)
        Transform.set_rotation(tr.e, a)
        Trail.set_emitting(tr.e, true)
        Sprite.set_color(tr.e, 5.0, 3.4, 1.8, 1)
        Lighting.set_intensity(tr.e, 1.3)
    else
        impact(s, hit, a)
    end
    s.flash = 0.05
    Particles.set(s.casings, "angle", { s.heading - 1.9, s.heading - 1.3 })
    Transform.set_position(s.casings, x - math.cos(s.heading) * 22, y - math.sin(s.heading) * 22)
    Particles.emit_burst(s.casings, 1)
end

local function update_tracers(dt)
    for _, tr in ipairs(tracers) do
        if tr.live then
            tr.go = tr.go + BULLET * dt
            local d = math.min(tr.go, tr.len)
            Transform.set_position(tr.e, tr.x + tr.dx * d, tr.y + tr.dy * d)
            if tr.go >= tr.len then
                tr.live = false
                impact(tr.from, tr.hit, tr.a)
                Trail.set_emitting(tr.e, false)
                Sprite.set_color(tr.e, 1, 1, 1, 0)
                Lighting.set_intensity(tr.e, 0)
            end
        end
    end
end

-- ── lasers ─────────────────────────────────────────────────────────────

local function beam_off(s)
    Line.set(s.beam_e, "points", { { 0, -5000 }, { 1, -5000 } })
    Line.set(s.glow_e, "points", { { 0, -5000 }, { 1, -5000 } })
    Particles.set_active(s.hit_e, false)
    Lighting.set_intensity(s.hit_e, 0)
end

local function update_beam(s, dt)
    local x, y = muzzle_of(s)
    local a = s.aim + math.sin(t * 7 + s.lx) * 0.025
    local dx, dy = math.cos(a), math.sin(a)
    local hit = trace(s, x, y, dx, dy, 1500, s.over)
    local w = 0.85 + 0.15 * math.sin(t * 60)
    Line.set(s.beam_e, "points", { { x, y }, { hit.x, hit.y } })
    Line.set(s.glow_e, "points", { { x, y }, { hit.x, hit.y } })
    Line.set(s.glow_e, "width", 14 + 4 * w)
    Transform.set_position(s.hit_e, hit.x, hit.y)
    Particles.set_active(s.hit_e, true)
    Lighting.set_intensity(s.hit_e, 2.2 * w)
    s.flash = 0.04
    s.burn = (s.burn or 0) - dt
    if hit.kind == "soldier" then
        hit.who.hp = hit.who.hp - BEAM_DPS * dt
        if s.burn <= 0 then
            bleed_at(hit.x, hit.y, a, between(14, 22))
            s.burn = 0.18
        end
        if hit.who.hp <= 0 and hit.who.alive then die(hit.who, a) end
    else
        if hit.kind == "barrel" then
            barrel.heat = barrel.heat + dt
            if barrel.heat > 0.7 then explode() end
        end
        -- A laser leaves a burn on the asphalt where it plays.
        if s.burn <= 0 then
            paint(hit.x, hit.y, between(8, 14), 0.06, 0.04, 0.03, 0.35, "disc", 0)
            s.burn = 0.05
        end
    end
end

-- ── a soldier's turn ───────────────────────────────────────────────────

local function respawn(s)
    local sp = s.team == "R" and RED_SPAWN or BLUE_SPAWN
    s.alive, s.hp, s.shots, s.beam, s.cooldown = true, 100, 0, 0, between(1.0, 2.0)
    Transform.set_position(s.e, sp[1], sp[2] + between(-200, 200))
    Sprite.set_color(s.e, 1, 1, 1, 1)
    Lighting.set_intensity(s.e, 0.45)
    s.lx, s.ly = Transform.get_position(s.e)
    pick_cover(s)
end

local function think(s, dt)
    if not s.alive then
        -- The blood pool spreads under the body, then they come back.
        if s.pool > 0 then
            s.pool = s.pool - dt
            s.pool_size = s.pool_size + dt * 22
            local x, y = Transform.get_position(s.e)
            if rand() < dt * 5 then
                paint(x + between(-6, 6), y + between(-6, 6), s.pool_size, BLOOD[1], BLOOD[2], BLOOD[3], 0.35, "disc", 0)
            end
        end
        s.back = s.back - dt
        if s.back <= 0 then respawn(s) end
        return
    end
    local x, y = Transform.get_position(s.e)
    local vx, vy = (x - s.lx) / math.max(dt, 1e-4), (y - s.ly) / math.max(dt, 1e-4)
    s.lx, s.ly = x, y
    local moving = vx * vx + vy * vy > 400
    local target = nearest_enemy(s, x, y)
    if moving then
        s.heading = atan2(vy, vx)
    elseif target then
        local tx, ty = Transform.get_position(target.e)
        local want = atan2(ty - y, tx - x)
        local d = (want - s.aim + math.pi) % (2 * math.pi) - math.pi
        s.aim = s.aim + d * math.min(1, dt * 8)
        s.heading = s.aim
    end
    Transform.set_rotation(s.e, s.heading)
    -- Wounded: a trail of drops.
    if s.hp < 55 and moving then
        s.bleed = s.bleed - dt
        if s.bleed <= 0 then
            paint(x, y, between(5, 9), BLOOD[1], BLOOD[2], BLOOD[3], 0.75, "disc", 0)
            s.bleed = 0.22
        end
    end
    -- Now and then, another piece of cover.
    s.relocate = s.relocate - dt
    if s.relocate <= 0 then
        pick_cover(s)
        s.relocate = between(5, 10)
    end
    if moving or not target then
        if s.beam > 0 then s.beam = 0; beam_off(s) end
        return
    end
    -- Fire: a burst of rounds, or a beam.
    if s.team == "R" then
        if s.shots > 0 then
            s.next_shot = s.next_shot - dt
            if s.next_shot <= 0 then
                fire_round(s)
                s.shots = s.shots - 1
                s.next_shot = 0.085
            end
        else
            s.cooldown = s.cooldown - dt
            if s.cooldown <= 0 then
                s.shots = 3 + math.floor(rand() * 4)
                s.cooldown = between(0.8, 1.9)
            end
        end
    else
        if s.beam > 0 then
            s.beam = s.beam - dt
            update_beam(s, dt)
            if s.beam <= 0 then beam_off(s) end
        else
            s.cooldown = s.cooldown - dt
            if s.cooldown <= 0 then
                s.beam = between(0.5, 0.9)
                s.over = rand() < OVER_THE_TOP
                s.cooldown = between(1.0, 2.2)
            end
        end
    end
end

-- ── the frame ──────────────────────────────────────────────────────────

function init(entity)
    for _, n in ipairs({ "Floor", "Barrel", "Searchlight", "Status" }) do e[n] = Entity.find_by_name(n) end
    for _, f in ipairs(FIRES) do e[f .. "Light"] = Entity.find_by_name(f .. "Light") end
    for i = 1, SQUAD do
        soldiers[#soldiers + 1] = make_soldier("R", i)
        soldiers[#soldiers + 1] = make_soldier("B", i)
    end
    for i = 0, TRACERS - 1 do tracers[#tracers + 1] = { e = Entity.find_by_name("Tracer" .. i), live = false } end
    for _, s in ipairs(soldiers) do pick_cover(s) end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("B") then explode() end

    for _, s in ipairs(soldiers) do
        think(s, dt)
        -- The muzzle flash: a star and a light, for a few hundredths.
        if s.flash > 0 then
            s.flash = s.flash - dt
            local k = s.flash > 0 and (0.7 + 0.3 * rand()) or 0
            Sprite.set_color(s.muzzle, 6.0, 4.5, 3.0, k)
            Lighting.set_intensity(s.muzzle, 2.6 * k)
            Transform.set_rotation(s.muzzle, rand() * 6.28)
        end
    end
    update_tracers(dt)

    -- The barrel: it goes up when shot, burnt, or after a quiet spell.
    if barrel.up then
        barrel.quiet = barrel.quiet + dt
        if barrel.quiet > 18 then explode() end
    else
        barrel.back = barrel.back - dt
        if barrel.back <= 0 then
            barrel.up = true
            Sprite.set_color(e.Barrel, 1, 1, 1, 1)
        end
    end
    for i = #booms, 1, -1 do
        local b = booms[i]
        b.age = b.age + dt
        if b.age > 1.4 then
            table.remove(booms, i)
        else
            Lighting.set_intensity(b.e, 4.0 * math.max(0, 1 - b.age / 1.3) ^ 2)
        end
    end
    for i = #waves, 1, -1 do
        local w = waves[i]
        w.age = w.age + dt
        local p = w.age / 0.6
        if p >= 1 then
            table.remove(waves, i)
        else
            Material.set_vec4(w.e, "progress", p, 0, 0, 0)
            Material.set_vec4(w.e, "tint", 1.0, 0.55, 0.2, 0)
        end
    end
    chroma = math.max(0, chroma - dt * 2.5)
    PostProcess.set_float("chromatic", "strength", 0.0015 + 0.012 * chroma)

    -- The fires flicker, the searchlight sweeps.
    for _, f in ipairs(FIRES) do
        local k = 0.85 + 0.1 * math.sin(t * 11 + #f) + 0.08 * math.sin(t * 23.7 + #f * 2)
        Lighting.set_intensity(e[f .. "Light"], FIRE_BASE[f] * k)
    end
    Transform.set_rotation(e.Searchlight, -1.5708 + math.sin(t * 0.35) * 0.85)

    local alive = { R = 0, B = 0 }
    for _, s in ipairs(soldiers) do if s.alive then alive[s.team] = alive[s.team] + 1 end end
    UI.set_text(e.Status, string.format("raiders %d up, %d kills     security %d up, %d kills",
        math.floor(alive.R), math.floor(kills.R), math.floor(alive.B), math.floor(kills.B)))
end
