-- Scene 01: Transforms
-- Demonstrates set_position / set_rotation / set_scale.
-- NOTE: jEngine has no hierarchical transforms yet (BUGS.md #1),
-- so Planet and Moon orbits are computed manually in script.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/01-transforms.scene"

local t = 0.0
local sun, planet, moon, pulser, spinner

function init(entity)
    sun     = Entity.find_by_name("Sun")
    planet  = Entity.find_by_name("Planet")
    moon    = Entity.find_by_name("Moon")
    pulser  = Entity.find_by_name("Pulser")
    spinner = Entity.find_by_name("Spinner")
    log("[01-transforms] ready")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt

    -- Sun spins slowly in place.
    if sun then Transform.set_rotation(sun, t * 0.5) end

    -- Planet orbits Sun at radius 180, period ~6s.
    if planet then
        local ang = t * (2.0 * math.pi / 6.0)
        local px = math.cos(ang) * 180.0
        local py = math.sin(ang) * 180.0
        Transform.set_position(planet, px, py)
        Transform.set_rotation(planet, ang * 2.0)

        -- Moon orbits planet at radius 60, period ~1.5s.
        if moon then
            local mang = t * (2.0 * math.pi / 1.5)
            local mx = px + math.cos(mang) * 60.0
            local my = py + math.sin(mang) * 60.0
            Transform.set_position(moon, mx, my)
        end
    end

    -- Pulser scales with a sine wave.
    if pulser then
        local s = 1.0 + 0.5 * math.sin(t * 3.0)
        Transform.set_scale(pulser, s, s)
    end

    -- Spinner rotates and stretches non-uniformly (sx != sy).
    if spinner then
        Transform.set_rotation(spinner, -t * 1.5)
        Transform.set_scale(spinner, 2.0, 1.0 + 0.5 * math.sin(t * 2.0))
    end
end
