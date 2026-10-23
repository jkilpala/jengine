-- Scene 12 - MaterialComponent + normal-map showcase.
--
-- Two rows of identical bumps128.png diffuses, both tagged with
-- material_id == "lit_sprite" so they route through the forward
-- WgpuLitSpritePipeline (not the deferred lighting pass). Top row
-- uses the flat-normal fallback; bottom row binds bumps128_n.png so
-- each fragment shades against the procedural hemisphere normals.
-- Three orbiting point lights + a dim ambient make the difference
-- between flat shading and per-pixel normal-map lighting obvious.
--
-- This scene looked "subtle" for a long time, and the cause was not what it
-- appeared to be. In order of how much it mattered:
--
--   1. The editor and the web never drew `lit_sprite` materials at all. The
--      regular sprite pass skips them expecting a lit pass, and only native
--      had one - so both rows were *absent*, and what read as a faint effect
--      was the point lights glowing on an empty background (BUGS #89).
--   2. Native, the one runtime that did draw them, uploaded the normal map as
--      sRGB colour, so every normal was gamma-decoded and leaned ~40 degrees
--      down-left. A flat normal map differed from no normal map by a channel
--      delta of 106/255.
--   3. The texture pair. The old normal map was four sparse bumps with 57%
--      flat area, and flat area shades exactly like the row it is compared
--      against. `tools/gen_bump_textures.py` makes a dense 5x5 grid instead,
--      and a near-uniform diffuse - painting the bumps into the albedo would
--      make the *flat* row look bumpy too, since both rows share it.
--
-- The first fix tried was 3, on the theory that the bumps were too weak. It
-- helped "a bit", which is what a fix to the least important cause does.
--
-- Controls:
--   SPACE   - pause / resume light orbit
--   R       - toggle all three point lights on/off (intensity 0 <-> 2.0)
--   N       - flip bottom-row material between lit_sprite (normal map
--             stays bound at the component level) and default_sprite
--             (drops out of the lit pass entirely - shows the diffuse
--             with no shading at all for a clear before/after).
--   1/2/3   - ambient 0.15 / 0.4 / 0.8 (cool tint preserved; 2 is the default)
--   H / Esc - hub
--   [ / ]   - prev / next scene in tour

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/12-materials.scene"

local LIGHT_INTENSITY = 2.0
-- Brighter than it was: at the old (0.4, 0.5, 0.8) x 0.15 everything outside
-- a light pool - both rows and every label - sat at 6-12% and read as black.
-- The lights still carry all the directional shading; ambient only has to make
-- the sprites and hints visible between them. `1` keeps the dark version for
-- anyone who wants the lights to dominate.
local AMBIENT_COLOR   = { 0.75, 0.8, 1.0 }

local t = 0.0
local orbit_running = true
local lights_on     = true
local bump_lit      = true   -- false => bottom row switched to default_sprite

local prev_space, prev_r, prev_n = false, false, false
local prev_1, prev_2, prev_3 = false, false, false

local ids = nil
local function resolve_ids()
    ids = {
        light_r = Entity.find_by_name("Light_R"),
        light_g = Entity.find_by_name("Light_G"),
        light_b = Entity.find_by_name("Light_B"),
        bumps   = {
            Entity.find_by_name("BumpA"),
            Entity.find_by_name("BumpB"),
            Entity.find_by_name("BumpC"),
            Entity.find_by_name("BumpD"),
            Entity.find_by_name("BumpE"),
        },
    }
end

local function set_all_lights(intensity)
    if not ids then return end
    for _, id in ipairs({ ids.light_r, ids.light_g, ids.light_b }) do
        if id and id ~= 0 then Lighting.set_intensity(id, intensity) end
    end
end

local function set_bumps_material(mid)
    if not ids then return end
    for _, id in ipairs(ids.bumps) do
        if id and id ~= 0 then Material.set(id, mid) end
    end
end

function init(entity)
    log("[scene_12] ready - SPACE pause orbit, R toggle lights, N toggle bump material, 1/2/3 ambient")
end

-- Material uniforms are settable per entity at runtime: set_material_uniform_float
-- for a scalar and _vec4 for a colour or a packed 4-tuple. get_material(e)
-- reports which material an entity resolved to, which is the quickest way to
-- tell "the override did not apply" from "it applied and looks the same".
local mat_t = 0.0
local mat_probe = nil

function update(entity, dt)
    mat_t = mat_t + dt
    if mat_probe == nil then
        mat_probe = Entity.find_by_name("BumpA") or Entity.find_by_name("FlatA")
    end
    if mat_probe ~= nil then
        local pulse = 0.5 + 0.5 * math.sin(mat_t * 1.5)
        Material.set_float(mat_probe, "u_strength", pulse)
        Material.set_vec4(mat_probe, "u_tint", pulse, 0.6, 1.0 - pulse, 1.0)
    end
    if not ids then resolve_ids() end

    -- Edge-trigger keys.
    local sp = Input.is_key_pressed("Space")
    local rk = Input.is_key_pressed("R")
    local nk = Input.is_key_pressed("N")
    local k1 = Input.is_key_pressed("1")
    local k2 = Input.is_key_pressed("2")
    local k3 = Input.is_key_pressed("3")

    if sp and not prev_space then orbit_running = not orbit_running end
    if rk and not prev_r then
        lights_on = not lights_on
        set_all_lights(lights_on and LIGHT_INTENSITY or 0.0)
    end
    if nk and not prev_n then
        bump_lit = not bump_lit
        set_bumps_material(bump_lit and "lit_sprite" or "default_sprite")
    end
    if k1 and not prev_1 then
        Lighting.set_ambient(AMBIENT_COLOR[1], AMBIENT_COLOR[2], AMBIENT_COLOR[3], 0.15)
    end
    if k2 and not prev_2 then
        Lighting.set_ambient(AMBIENT_COLOR[1], AMBIENT_COLOR[2], AMBIENT_COLOR[3], 0.4)
    end
    if k3 and not prev_3 then
        Lighting.set_ambient(AMBIENT_COLOR[1], AMBIENT_COLOR[2], AMBIENT_COLOR[3], 0.8)
    end

    prev_space, prev_r, prev_n = sp, rk, nk
    prev_1, prev_2, prev_3 = k1, k2, k3

    if orbit_running then
        t = t + dt
    end

    -- Three lights orbit a horizontal track between the two rows
    -- (rows are at y=+30 and y=-170, midline y=-70). Each light
    -- traces an ellipse so they pass over both rows.
    local cx, cy = 0.0, -70.0
    local rx, ry = 320.0, 90.0
    local phase_r = 0.0
    local phase_g = 2.0 * math.pi / 3.0
    local phase_b = 4.0 * math.pi / 3.0
    local omega = 0.9
    if ids.light_r and ids.light_r ~= 0 then
        Transform.set_position(ids.light_r, cx + math.cos(t * omega + phase_r) * rx,
                                   cy + math.sin(t * omega + phase_r) * ry)
    end
    if ids.light_g and ids.light_g ~= 0 then
        Transform.set_position(ids.light_g, cx + math.cos(t * omega + phase_g) * rx,
                                   cy + math.sin(t * omega + phase_g) * ry)
    end
    if ids.light_b and ids.light_b ~= 0 then
        Transform.set_position(ids.light_b, cx + math.cos(t * omega + phase_b) * rx,
                                   cy + math.sin(t * omega + phase_b) * ry)
    end

    nav.tick(SCENE_PATH)
end
