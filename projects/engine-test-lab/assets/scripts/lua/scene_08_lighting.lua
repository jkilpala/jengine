-- Scene 08: 2D Lighting
--
-- Exercises the engine's deferred-style lighting pass: three PointLight2D
-- entities (R/G/B) orbit a dim checker floor lit by a low ambient. The
-- per-frame pipeline runs `gather_lights` + `WgpuLightingPass` whenever
-- any light component is present, so this scene also stress-tests the
-- "modify-via-script-command" path for the lighting API.
--
-- Controls (all use the scripting/api `Set*Light*` commands):
--   SPACE = toggle all three point lights on / off
--   R     = toggle the orbit animation
--   1     = ambient intensity 0.05  (near-black)
--   2     = ambient intensity 0.20  (baseline)
--   3     = ambient intensity 0.60  (overcast)
--   C     = cycle the colour palette: classic RGB -> warm sunset -> cool dawn
--   Equal / NumpadAdd      = grow each light's radius by 20
--   Minus / NumpadSubtract = shrink each light's radius by 20 (floor at 60)

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/08-lighting.scene"

local LIGHT_NAMES = { "Light_R", "Light_G", "Light_B" }
local BASE_OFFSETS = {
    Light_R = { angle = math.pi,         radius = 220.0, y = -20.0 },
    Light_G = { angle = 0.0,             radius = 220.0, y = -20.0 },
    Light_B = { angle = math.pi * 0.5,   radius = 220.0, y = -20.0 },
}
local BASE_INTENSITY = 1.8
local BASE_RADIUS = 260.0

local PALETTES = {
    {
        name = "classic-rgb",
        Light_R = { 1.0, 0.25, 0.2 },
        Light_G = { 0.25, 1.0, 0.35 },
        Light_B = { 0.3, 0.45, 1.0 },
    },
    {
        name = "warm-sunset",
        Light_R = { 1.0, 0.5, 0.15 },
        Light_G = { 0.95, 0.2, 0.55 },
        Light_B = { 1.0, 0.85, 0.3 },
    },
    {
        name = "cool-dawn",
        Light_R = { 0.3, 0.95, 1.0 },
        Light_G = { 0.6, 0.4, 1.0 },
        Light_B = { 0.95, 0.95, 1.0 },
    },
}

local cached_ids = nil
local lights_on = true
local orbiting = true
local elapsed = 0.0
local current_radius = BASE_RADIUS
local palette_index = 1

local prev_space, prev_r, prev_1, prev_2, prev_3, prev_c = false, false, false, false, false, false
local prev_plus, prev_minus = false, false

local function ids()
    if cached_ids then return cached_ids end
    local out = {}
    local ok = true
    for _, name in ipairs(LIGHT_NAMES) do
        local id = Entity.find_by_name(name)
        if not id then ok = false; break end
        out[name] = id
    end
    if ok then cached_ids = out end
    return out
end

local function for_each(fn)
    local map = ids()
    for _, name in ipairs(LIGHT_NAMES) do
        local id = map[name]
        if id then fn(id, name) end
    end
end

local function toggle_lights()
    lights_on = not lights_on
    local intensity = lights_on and BASE_INTENSITY or 0.0
    for_each(function(id) Lighting.set_intensity(id, intensity) end)
    log(lights_on and "[08-lighting] lights ON" or "[08-lighting] lights OFF")
end

local function set_ambient(level)
    Lighting.set_ambient(0.4, 0.5, 0.8, level)
    log(string.format("[08-lighting] ambient intensity = %.2f", level))
end

local function cycle_palette()
    palette_index = (palette_index % #PALETTES) + 1
    local palette = PALETTES[palette_index]
    for_each(function(id, name)
        local c = palette[name]
        if c then Lighting.set_color(id, c[1], c[2], c[3]) end
    end)
    log("[08-lighting] palette = " .. palette.name)
end

local function nudge_radius(delta)
    current_radius = math.max(60.0, current_radius + delta)
    for_each(function(id) Lighting.set_radius(id, current_radius) end)
    log(string.format("[08-lighting] radius = %.0f", current_radius))
end

function init(entity)
    log("[08-lighting] ready - SPACE toggle, R orbit, 1/2/3 ambient, C palette, +/- radius")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)

    if orbiting then
        elapsed = elapsed + dt
        local map = ids()
        for _, name in ipairs(LIGHT_NAMES) do
            local id = map[name]
            local cfg = BASE_OFFSETS[name]
            if id and cfg then
                local a = cfg.angle + elapsed * 0.6
                local x = math.cos(a) * cfg.radius
                local y = cfg.y + math.sin(a) * cfg.radius * 0.45
                Transform.set_position(id, x, y)
            end
        end
    end

    local k_space = Input.is_key_pressed("Space")
    local k_r     = Input.is_key_pressed("R")
    local k_1     = Input.is_key_pressed("1")
    local k_2     = Input.is_key_pressed("2")
    local k_3     = Input.is_key_pressed("3")
    local k_c     = Input.is_key_pressed("C")
    -- Key names are the canonical `KeyCode::name()` ones, which are physical
    -- keys rather than the characters a layout prints on them: the `+` key is
    -- `Equal` and the `-` key is `Minus`. "=", "+", "-" and "_" are not names
    -- at all, so these two checks were false in every runtime, always.
    local k_plus  = Input.is_key_pressed("Equal") or Input.is_key_pressed("NumpadAdd")
    local k_minus = Input.is_key_pressed("Minus") or Input.is_key_pressed("NumpadSubtract")

    if k_space and not prev_space then toggle_lights() end
    if k_r     and not prev_r     then
        orbiting = not orbiting
        log(orbiting and "[08-lighting] orbit ON" or "[08-lighting] orbit OFF")
    end
    if k_1     and not prev_1     then set_ambient(0.05) end
    if k_2     and not prev_2     then set_ambient(0.20) end
    if k_3     and not prev_3     then set_ambient(0.60) end
    if k_c     and not prev_c     then cycle_palette() end
    if k_plus  and not prev_plus  then nudge_radius(20.0) end
    if k_minus and not prev_minus then nudge_radius(-20.0) end

    prev_space, prev_r = k_space, k_r
    prev_1, prev_2, prev_3, prev_c = k_1, k_2, k_3, k_c
    prev_plus, prev_minus = k_plus, k_minus
end
