-- Scene 07: Particles
--
-- Exercises the ParticleEmitter component with four contrasting setups:
--   * Fountain — narrow upward cone, full gravity, blue→fade
--   * Radial   — 360° spray, no gravity, magenta→yellow
--   * Fire     — narrow upward, light negative gravity (rises), orange→red
--   * Spiral   — high angular velocity + omnidirectional, green→blue
--
-- Scripts wire two controls via the scripting API
-- (`set_emitter_active`, `set_emitter_rate`):
--   SPACE = toggle all emitters on/off
--   1     = rate x0.25  (slow drip)
--   2     = rate x1.0   (baseline)
--   3     = rate x4.0   (heavy)

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/07-particles.scene"

local TARGETS = { "Fountain", "Radial", "Fire", "Spiral" }

-- Baseline rates must match `emission_rate` authored in the .scene.
local BASE_RATE = {
    Fountain = 60.0,
    Radial   = 50.0,
    Fire     = 80.0,
    Spiral   = 30.0,
}

local cached_ids = nil
local active = true
local prev_space, prev_1, prev_2, prev_3 = false, false, false, false

local function ids()
    if cached_ids then return cached_ids end
    local ok = true
    local out = {}
    for _, name in ipairs(TARGETS) do
        local id = Entity.find_by_name(name)
        if not id then ok = false; break end
        out[name] = id
    end
    if ok then cached_ids = out end
    return out
end

local function for_each(fn)
    local map = ids()
    for _, name in ipairs(TARGETS) do
        local id = map[name]
        if id then fn(id, name) end
    end
end

local function toggle_active()
    active = not active
    for_each(function(id) Particles.set_active(id, active) end)
    log(active and "[07-particles] all emitters ON" or "[07-particles] all emitters OFF")
end

local function set_rate_mult(m)
    for_each(function(id, name) Particles.set_rate(id, BASE_RATE[name] * m) end)
    log(string.format("[07-particles] emission rate x%.2f", m))
end

function init(entity)
    log("[07-particles] ready - SPACE toggle, 1/2/3 rate")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)

    local k_space = Input.is_key_pressed("Space")
    local k1 = Input.is_key_pressed("1")
    local k2 = Input.is_key_pressed("2")
    local k3 = Input.is_key_pressed("3")

    if k_space and not prev_space then toggle_active() end
    if k1 and not prev_1 then set_rate_mult(0.25) end
    if k2 and not prev_2 then set_rate_mult(1.0) end
    if k3 and not prev_3 then set_rate_mult(4.0) end

    prev_space, prev_1, prev_2, prev_3 = k_space, k1, k2, k3
end
