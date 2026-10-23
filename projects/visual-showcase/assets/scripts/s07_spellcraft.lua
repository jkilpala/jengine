-- 07 Spellcraft: every light in the courtyard is a spell.
--
-- Fireballs and blasts are spawned prefabs that carry their own light and
-- emitters; this script only flies them, fades what they leave and drives
-- each shockwave's `progress`. Lightning and the frost nova are pools of
-- sprites authored in the scene and laid out here. Left alone, the mage
-- casts on its own (A turns that off); any click takes over for a while.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/07-spellcraft.scene"

-- Where things are - tools/gen_scenes.py (SC_TIP, SC_MAGE, SC_WARDS).
local TIP_X, TIP_Y = -355.5, 112.5
local MAGE_X, MAGE_Y = -420.0, -150.0
local TARGETS = { { 90.0, 45.0, -60.0 }, { 330.0, -95.0, -200.0 }, { 560.0, 65.0, -40.0 } }  -- x, y, floor y
local BOLT_SEGMENTS = 20
local SHARDS = 18
local FIRE_SPEED = 1050.0
local BOLT_TIME = 0.42
local NOVA_TIME = 0.9
local WAVE_TIME = 0.7

-- Linear colours, already past white where they glow (see gen_scenes.py).
local RING_COLOR = { 1.029, 0.441, 3.0, 0.9 }
local FIRE_TINT = { 3.2, 1.2, 0.3 }
local FROST_TINT = { 0.8, 1.8, 3.0 }

-- Luau has math.atan2; Fengari (Lua 5.3) has only math.atan(y, x).
local function atan2(y, x)
    if math.atan2 then return math.atan2(y, x) end
    return math.atan(y, x)
end

local t = 0.0
local captions = true
local post_on = nav.post_all_on(SCENE)
local auto = true
local idle = 10.0
local auto_timer = 0.6
local auto_step = 0
local kick = 0.0

local status, ring, seal, nova_ring, staff_light, circle_light, bolt_light, nova_light
local orbs = {}
local bolt_cores, bolt_glows = {}, {}
local shards = {}

local fireballs = {}   -- { e, x, y, tx, ty, fy, dead }
local blasts = {}      -- { e, age, life, base }
local waves = {}       -- { e, age, tint }
local bolt = nil       -- { tx, ty, age, next }
local nova = nil       -- { age, angles }

local function refresh_status()
    if status then
        Text.set(status, (auto and "auto-cast on   " or "auto-cast off   ") .. nav.post_status(post_on))
    end
end

function init(entity)
    status = Entity.find_by_name("Status")
    ring = Entity.find_by_name("RuneRing")
    seal = Entity.find_by_name("RuneSeal")
    nova_ring = Entity.find_by_name("NovaRing")
    staff_light = Entity.find_by_name("StaffLight")
    circle_light = Entity.find_by_name("CircleLight")
    bolt_light = Entity.find_by_name("BoltLight")
    nova_light = Entity.find_by_name("NovaLight")
    for i = 1, 3 do orbs[i] = Entity.find_by_name("Orb" .. i) end
    for i = 1, BOLT_SEGMENTS do
        bolt_cores[i] = Entity.find_by_name("Bolt" .. i)
        bolt_glows[i] = Entity.find_by_name("BoltGlow" .. i)
    end
    for i = 1, SHARDS do shards[i] = Entity.find_by_name("Shard" .. i) end
    refresh_status()
end

-- ── what a hit leaves behind ────────────────────────────────────────────

local function spawn_at(prefab, x, y)
    local e = Entity.spawn_prefab(prefab)
    Transform.set_position(e, x, y)
    return e
end

local function shockwave(x, y, tint, scale)
    local w = spawn_at("SpWave", x, y)
    if scale then Transform.set_scale(w, scale, scale) end
    waves[#waves + 1] = { e = w, age = 0.0, tint = tint }
end

local function scorch(x, y)
    local s = spawn_at("SpScorch", x, y)
    -- Multiply by white is no change: fade the mark out by fading to white.
    Tween.color(s, 1.0, 1.0, 1.0, 1.0, 5.5, "ease_in")
end

local function explode(x, y, floor_y)
    local b = spawn_at("SpBlast", x, y)
    blasts[#blasts + 1] = { e = b, age = 0.0, life = 0.9, base = 7.0 }
    spawn_at("SpSmoke", x, y)
    shockwave(x, y, FIRE_TINT)
    scorch(x, floor_y)
    Camera.shake(7, 0.25)
end

-- ── the spells ──────────────────────────────────────────────────────────

local function cast_fireball(tx, ty, fy)
    local e = spawn_at("SpFireball", TIP_X, TIP_Y)
    fireballs[#fireballs + 1] = { e = e, x = TIP_X, y = TIP_Y, tx = tx, ty = ty, fy = fy }
    kick = 1.0
end

local function cast_bolt(tx, ty, fy)
    bolt = { tx = tx, ty = ty, age = 0.0, next = 0.0 }
    spawn_at("SpSparks", tx, ty)
    Transform.set_position(bolt_light, tx, ty)
    shockwave(tx, ty, FROST_TINT, 0.6)
    scorch(tx, fy)
    Camera.shake(4, 0.15)
    kick = 1.5
end

local function cast_nova()
    local angles = {}
    for i = 1, SHARDS do
        angles[i] = (i - 1) / SHARDS * math.pi * 2 + (math.random() - 0.5) * 0.25
    end
    nova = { age = 0.0, angles = angles }
    shockwave(MAGE_X, MAGE_Y + 10, FROST_TINT, 1.7)
    Camera.shake(5, 0.3)
    kick = 1.0
end

-- A fork from (ax, ay) to (bx, by): midpoint displacement, four rounds.
local function fork(ax, ay, bx, by, rounds, rough)
    local pts = { { ax, ay }, { bx, by } }
    for _ = 1, rounds do
        local out = { pts[1] }
        for i = 1, #pts - 1 do
            local p, q = pts[i], pts[i + 1]
            local dx, dy = q[1] - p[1], q[2] - p[2]
            local len = math.sqrt(dx * dx + dy * dy)
            local off = (math.random() * 2 - 1) * len * rough
            out[#out + 1] = { (p[1] + q[1]) / 2 - dy / len * off, (p[2] + q[2]) / 2 + dx / len * off }
            out[#out + 1] = q
        end
        pts = out
    end
    return pts
end

local function lay_segment(i, p, q, a)
    local dx, dy = q[1] - p[1], q[2] - p[2]
    local len = math.sqrt(dx * dx + dy * dy)
    local ang = atan2(dy, dx)
    for _, e in ipairs({ bolt_cores[i], bolt_glows[i] }) do
        Transform.set_position(e, p[1], p[2])
        Transform.set_rotation(e, ang)
        Transform.set_scale(e, (len + 2) / 16, 1)
    end
    Sprite.set_color(bolt_cores[i], 6.8, 7.4, 8.0, a)
    Sprite.set_color(bolt_glows[i], 1.2, 1.6, 2.5, a * 0.8)
end

local function hide_bolt()
    for i = 1, BOLT_SEGMENTS do
        Sprite.set_color(bolt_cores[i], 0, 0, 0, 0)
        Sprite.set_color(bolt_glows[i], 0, 0, 0, 0)
    end
    Lighting.set_intensity(bolt_light, 0)
end

local function update_bolt(dt)
    if not bolt then return end
    bolt.age = bolt.age + dt
    if bolt.age >= BOLT_TIME then
        bolt = nil
        hide_bolt()
        return
    end
    local fade = 1.0 - bolt.age / BOLT_TIME
    Lighting.set_intensity(bolt_light, 7.0 * fade * (0.6 + 0.4 * math.random()))
    if bolt.age < bolt.next then return end
    bolt.next = bolt.age + 0.05
    -- The main fork: 16 segments. A branch off it: the other 4.
    local main = fork(TIP_X, TIP_Y, bolt.tx, bolt.ty, 4, 0.22)
    local a = fade * (0.7 + 0.3 * math.random())
    for i = 1, 16 do lay_segment(i, main[i], main[i + 1], a) end
    local from = main[math.random(5, 10)]
    local bx = from[1] + (bolt.tx - from[1]) * 0.5 + (math.random() - 0.5) * 160
    local by = from[2] + (bolt.ty - from[2]) * 0.5 - 60 - math.random() * 80
    local branch = fork(from[1], from[2], bx, by, 2, 0.3)
    for i = 1, 4 do lay_segment(16 + i, branch[i], branch[i + 1], a * 0.7) end
end

local function update_nova(dt)
    if not nova then return end
    nova.age = nova.age + dt
    local u = nova.age / NOVA_TIME
    if u >= 1.0 then
        nova = nil
        for i = 1, SHARDS do Sprite.set_color(shards[i], 0, 0, 0, 0) end
        Sprite.set_color(nova_ring, 0, 0, 0, 0)
        Lighting.set_intensity(nova_light, 0)
        return
    end
    local r = 560 * (1 - (1 - u) * (1 - u))
    local a = 1.0 - u * u
    for i = 1, SHARDS do
        local ang = nova.angles[i]
        local cx, cy = math.cos(ang), math.sin(ang) * 0.36
        Transform.set_position(shards[i], MAGE_X + cx * r, MAGE_Y + cy * r)
        Transform.set_rotation(shards[i], atan2(cy, cx))
        Sprite.set_color(shards[i], 1.4, 2.9, 4.0, a)
    end
    -- The ring is a child of the squashed circle, so it grows on the floor.
    local s = 1.0 + u * 5.6
    Transform.set_scale(nova_ring, s, s)
    Sprite.set_color(nova_ring, 1.2, 2.6, 4.0, a * 0.9)
    Lighting.set_intensity(nova_light, 5.0 * a)
end

local function update_fireballs(dt)
    for i = #fireballs, 1, -1 do
        local f = fireballs[i]
        if f.dead then
            f.dead = f.dead + dt
            -- Kept a moment after the hit, emitter off, so its trail can
            -- finish fading instead of vanishing with it.
            if f.dead > 0.7 then
                Entity.destroy(f.e)
                table.remove(fireballs, i)
            end
        else
            local dx, dy = f.tx - f.x, f.ty - f.y
            local d = math.sqrt(dx * dx + dy * dy)
            local step = FIRE_SPEED * dt
            if d <= step then
                f.dead = 0.0
                Particles.set_active(f.e, false)
                Sprite.set_color(f.e, 0, 0, 0, 0)
                Lighting.set_intensity(f.e, 0)
                explode(f.tx, f.ty, f.fy)
            else
                f.x = f.x + dx / d * step
                f.y = f.y + dy / d * step + math.sin(t * 30 + i) * 0.6
                Transform.set_position(f.e, f.x, f.y)
            end
        end
    end
end

local function update_blasts(dt)
    for i = #blasts, 1, -1 do
        local b = blasts[i]
        b.age = b.age + dt
        local k = 1.0 - b.age / b.life
        if k <= 0 then
            table.remove(blasts, i)      -- the prefab's lifetime removes it
        else
            Lighting.set_intensity(b.e, b.base * k * k)
        end
    end
    for i = #waves, 1, -1 do
        local w = waves[i]
        w.age = w.age + dt
        local p = w.age / WAVE_TIME
        if p >= 1.0 then
            table.remove(waves, i)
        else
            -- `w` = 0: a tint's alpha would add to the output's.
            Material.set_vec4(w.e, "progress", p, 0, 0, 0)
            Material.set_vec4(w.e, "tint", w.tint[1], w.tint[2], w.tint[3], 0)
        end
    end
end

-- ── the idle show ───────────────────────────────────────────────────────

local function auto_cast()
    auto_step = auto_step % 7 + 1
    local target = TARGETS[(auto_step - 1) % 3 + 1]
    if auto_step == 4 then
        cast_nova()
    elseif auto_step % 2 == 0 then
        cast_bolt(target[1], target[2], target[3])
    else
        cast_fireball(target[1], target[2], target[3])
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    if Input.is_key_just_pressed("A") then auto = not auto; refresh_status() end

    -- The player's casts, at the mouse; the floor under it is where it is.
    local mx, my = Input.get_mouse_world_position()
    idle = idle + dt
    if Input.is_mouse_left_just_pressed() then cast_fireball(mx, my, my - 20); idle = 0 end
    if Input.is_mouse_right_just_pressed() then cast_bolt(mx, my, my - 20); idle = 0 end
    if Input.is_key_just_pressed("F") then cast_nova(); idle = 0 end
    if auto and idle > 4.0 then
        auto_timer = auto_timer - dt
        if auto_timer <= 0 then
            auto_timer = 1.25
            auto_cast()
        end
    end

    -- The circle: two rings turning opposite ways on the floor, breathing.
    Transform.set_rotation(ring, t * 0.22)
    Transform.set_rotation(seal, -t * 0.37)
    local breath = 0.85 + 0.15 * math.sin(t * 2.1)
    Sprite.set_color(ring, RING_COLOR[1] * breath, RING_COLOR[2] * breath, RING_COLOR[3] * breath, RING_COLOR[4])
    Lighting.set_intensity(circle_light, 1.3 + 0.3 * math.sin(t * 2.1))

    -- The staff: orbs circling the crystal, passing behind it, and a light
    -- that flares with every cast.
    kick = math.max(0.0, kick - dt * 3.0)
    Lighting.set_intensity(staff_light, 1.6 + 0.15 * math.sin(t * 9.0) + kick * 3.0)
    for i = 1, 3 do
        local a = t * 2.3 + i * 2.094
        local c, s = math.cos(a), math.sin(a)
        Transform.set_position(orbs[i], TIP_X + c * 44, TIP_Y + s * 14 + math.sin(t * 3 + i) * 5)
        Sprite.set_z_order(orbs[i], s > 0 and 18 or 22)
    end

    update_fireballs(dt)
    update_blasts(dt)
    update_bolt(dt)
    update_nova(dt)
end
