-- Scene 14 - WgpuPostProcessStack showcase.
--
-- The scene file authors a starting effect chain in its `post_process`
-- block: bloom + vignette + color_grade enabled, chromatic disabled.
-- WgpuSceneRunner picks those up via `parse_scene_post_process` and
-- pushes them into the stack on frame 0, so the post-process pass is
-- already running when this script first ticks.
--
-- Controls:
--   1/2/3/4    toggle bloom / vignette / color_grade / chromatic
--   Q / W      bloom.threshold  -0.05 / +0.05  (0.05..2.0)
--   E / R      bloom.intensity  -0.1  / +0.1   (0..3)
--   A / S      color_grade.saturation -0.1 / +0.1 (0..2.5)
--   Z / X      chromatic.strength -0.002 / +0.002 (0..0.04)
--   SPACE      reset all params + enabled flags to scene defaults
--   H / Esc    hub
--   [ / ]      prev / next scene

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/14-post.scene"

local DEFAULTS = {
    bloom_enabled       = true,
    vignette_enabled    = true,
    color_grade_enabled = true,
    chromatic_enabled   = false,
    bloom_threshold     = 0.65,
    bloom_intensity     = 1.0,
    saturation          = 1.2,
    chromatic_strength  = 0.004,
}

local s = {}
local status_id, status2_id

local function fmt_on(b) if b then return "ON " else return "OFF" end end

local function apply_all()
    PostProcess.set_enabled("bloom",       s.bloom_enabled)
    PostProcess.set_enabled("vignette",    s.vignette_enabled)
    PostProcess.set_enabled("color_grade", s.color_grade_enabled)
    PostProcess.set_enabled("chromatic",   s.chromatic_enabled)
    PostProcess.set_float("bloom", "threshold",        s.bloom_threshold)
    PostProcess.set_float("bloom", "intensity",        s.bloom_intensity)
    PostProcess.set_float("color_grade", "saturation", s.saturation)
    PostProcess.set_float("chromatic", "strength",     s.chromatic_strength)
end

local function reset()
    for k, v in pairs(DEFAULTS) do s[k] = v end
    apply_all()
end

local function refresh_status()
    if status_id and status_id ~= 0 then
        Text.set(status_id, string.format(
            "bloom: %s  vignette: %s  color_grade: %s  chromatic: %s",
            fmt_on(s.bloom_enabled), fmt_on(s.vignette_enabled),
            fmt_on(s.color_grade_enabled), fmt_on(s.chromatic_enabled)))
    end
    if status2_id and status2_id ~= 0 then
        Text.set(status2_id, string.format(
            "threshold %.2f  intensity %.2f  saturation %.2f  chromatic %.4f",
            s.bloom_threshold, s.bloom_intensity, s.saturation, s.chromatic_strength))
    end
end

local function clamp(v, lo, hi)
    if v < lo then return lo end
    if v > hi then return hi end
    return v
end

local prev = {}
local function edge(key)
    local now = Input.is_key_pressed(key)
    local pressed = now and not prev[key]
    prev[key] = now
    return pressed
end

local ready_logged = false

-- set_post_process_vec4 is the 4-component sibling of the float setter; both
-- address an effect by name plus a parameter name.
local pp_t = 0.0

function update(entity, dt)
    pp_t = pp_t + dt
    PostProcess.set_vec4("vignette", "color", 0.0, 0.0, 0.0,
        0.5 + 0.3 * math.sin(pp_t))
    if status_id == nil then
        status_id  = Entity.find_by_name("Status")
        status2_id = Entity.find_by_name("Status2")
        reset()
        refresh_status()
    end

    if not ready_logged then
        log("[scene_14] ready - 1/2/3/4 toggle effects, Q/W threshold, E/R intensity, A/S saturation, Z/X chromatic, SPACE reset")
        ready_logged = true
    end

    local dirty = false

    if edge("1") then s.bloom_enabled       = not s.bloom_enabled;       PostProcess.set_enabled("bloom",       s.bloom_enabled);       dirty = true end
    if edge("2") then s.vignette_enabled    = not s.vignette_enabled;    PostProcess.set_enabled("vignette",    s.vignette_enabled);    dirty = true end
    if edge("3") then s.color_grade_enabled = not s.color_grade_enabled; PostProcess.set_enabled("color_grade", s.color_grade_enabled); dirty = true end
    if edge("4") then s.chromatic_enabled   = not s.chromatic_enabled;   PostProcess.set_enabled("chromatic",   s.chromatic_enabled);   dirty = true end

    if edge("Q") then s.bloom_threshold = clamp(s.bloom_threshold - 0.05, 0.05, 2.0); PostProcess.set_float("bloom", "threshold", s.bloom_threshold); dirty = true end
    if edge("W") then s.bloom_threshold = clamp(s.bloom_threshold + 0.05, 0.05, 2.0); PostProcess.set_float("bloom", "threshold", s.bloom_threshold); dirty = true end
    if edge("E") then s.bloom_intensity = clamp(s.bloom_intensity - 0.1,  0.0,  3.0); PostProcess.set_float("bloom", "intensity", s.bloom_intensity); dirty = true end
    if edge("R") then s.bloom_intensity = clamp(s.bloom_intensity + 0.1,  0.0,  3.0); PostProcess.set_float("bloom", "intensity", s.bloom_intensity); dirty = true end
    if edge("A") then s.saturation      = clamp(s.saturation      - 0.1,  0.0,  2.5); PostProcess.set_float("color_grade", "saturation", s.saturation); dirty = true end
    if edge("S") then s.saturation      = clamp(s.saturation      + 0.1,  0.0,  2.5); PostProcess.set_float("color_grade", "saturation", s.saturation); dirty = true end
    if edge("Z") then s.chromatic_strength = clamp(s.chromatic_strength - 0.002, 0.0, 0.04); PostProcess.set_float("chromatic", "strength", s.chromatic_strength); dirty = true end
    if edge("X") then s.chromatic_strength = clamp(s.chromatic_strength + 0.002, 0.0, 0.04); PostProcess.set_float("chromatic", "strength", s.chromatic_strength); dirty = true end

    if edge("Space") then reset(); dirty = true end

    if dirty then refresh_status() end

    nav.tick(SCENE_PATH)
end
