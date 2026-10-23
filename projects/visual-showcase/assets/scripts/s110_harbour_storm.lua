-- 110 Harbour Storm: the sea is a `water_surface` and everything on it
-- floats on its moving surface. This script is the weather: the lighthouse
-- turning, the clouds drifting, lightning now and then (a bolt, a flash,
-- and a splash where it strikes the sea), gusts that throw the pier's
-- crates in (the engine splashes them), spray where the swell breaks on the
-- rock. S calms the storm and lets it rise again.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/110-harbour-storm.scene"
local SEA = -120.000000
local PIER_Y = -40.000000

local e = {}
local t = 0
local rng = 110
local captions = true
local storm = true
local strength = 1.0
local next_bolt = 2.5
local bolt_age = 9
local next_gust = 6
local next_spray = 1.0
local crates = {}

local function rand()
    rng = (rng * 16807) % 2147483647
    return rng / 2147483647
end
local function between(a, b) return a + (b - a) * rand() end

function init(entity)
    for _, n in ipairs({ "Sea", "Lamp", "LampGlow", "Clouds0", "Clouds1", "Sky", "Bolt", "BoltGlow", "Flash", "Ambient",
                         "Rain", "Spray", "Status" }) do
        e[n] = Entity.find_by_name(n)
    end
    for i = 0, 3 do
        local id = Entity.find_by_name("Crate" .. i)
        local x, y = Transform.get_position(id)
        crates[#crates + 1] = { e = id, x = x, y = y, out = 0 }
    end
end

-- A jagged bolt from the clouds to the sea at x.
local function bolt(x)
    local pts = {}
    local px, py = x + between(-160, 160), 360
    pts[#pts + 1] = { px, py }
    local steps = 11
    for i = 1, steps do
        local f = i / steps
        local ty = 360 + (SEA - 360) * f
        px = px + (x - px) / (steps - i + 1) + between(-34, 34)
        pts[#pts + 1] = { px, ty }
    end
    pts[#pts] = { x, SEA }
    return pts
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("S") then storm = not storm end
    strength = strength + ((storm and 1 or 0) - strength) * math.min(1, dt * 0.4)

    -- The sea gets up and lies down with the storm.
    Water.set(e.Sea, "chop", 4 + 20 * strength)
    Water.set(e.Sea, "swell_height", 5 + 13 * strength)
    Particles.set(e.Rain, "emission_rate", 120 + 800 * strength)

    -- The lighthouse turns, a slow sweep round.
    local a = math.pi + math.sin(t * 0.35) * 1.1 - 0.25
    Transform.set_rotation(e.Lamp, a)

    -- Clouds drift.
    Transform.set_position(e.Clouds0, ((t * 14) % 400) - 200, 330)
    Transform.set_position(e.Clouds1, ((t * -22) % 500) - 250, 260)

    -- Lightning: a bolt, the clouds and the sea flashed, a splash where it
    -- strikes - and it fades over a third of a second, with an after-flash.
    next_bolt = next_bolt - dt * strength
    if next_bolt <= 0 then
        local x = between(-700, 300)
        local pts = bolt(x)
        Line.set(e.Bolt, "points", pts)
        Line.set(e.BoltGlow, "points", pts)
        Transform.set_position(e.Flash, x, 200)
        Water.splash(e.Sea, x, -18)
        bolt_age = 0
        next_bolt = between(3.5, 8.0)
    end
    bolt_age = bolt_age + dt
    local flash = 0
    if bolt_age < 0.45 then
        flash = math.max(0, 1 - bolt_age / 0.12) + (bolt_age > 0.18 and bolt_age < 0.26 and 0.7 or 0)
    end
    if bolt_age > 0.35 then
        Line.set(e.Bolt, "points", { { 0, -5000 }, { 1, -5000 } })
        Line.set(e.BoltGlow, "points", { { 0, -5000 }, { 1, -5000 } })
    end
    Lighting.set_intensity(e.Flash, flash * 1.3)
    local c = 0.32 + flash * 0.8
    Sprite.set_color(e.Clouds0, c * 0.95, c, c * 1.25, 0.85)
    Sprite.set_color(e.Clouds1, c * 0.9, c * 0.95, c * 1.2, 0.7)
    local s = 0.45 + flash * 0.7
    Sprite.set_color(e.Sky, s * 0.21, s * 0.24, s * 0.34, 1)

    -- Gusts throw a crate off the pier now and then; it splashes in. Once
    -- every crate is in, they are put back.
    next_gust = next_gust - dt * strength
    if next_gust <= 0 then
        local all_in = true
        for _, c in ipairs(crates) do
            if c.out == 0 then
                all_in = false
                c.out = 1
                Physics.apply_impulse(c.e, between(900, 1500), between(500, 900))
                break
            end
        end
        if all_in then
            for _, c in ipairs(crates) do
                c.out = 0
                Transform.set_position(c.e, c.x, c.y)
                Transform.set_rotation(c.e, 0)
                Physics.set_velocity(c.e, 0, 0)
            end
        end
        next_gust = between(4, 7)
    end

    -- Spray where the swell breaks on the rock.
    next_spray = next_spray - dt
    if next_spray <= 0 then
        Particles.emit_burst(e.Spray, 60)
        Water.splash(e.Sea, 360, 6 * strength)
        next_spray = between(1.2, 2.6) / math.max(0.3, strength)
    end

    UI.set_text(e.Status, storm and "storm" or "calming")
end
