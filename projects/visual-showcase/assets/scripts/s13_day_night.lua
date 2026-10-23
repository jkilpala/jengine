-- 13 Day & Night: a whole day in a minute. Everything here is driven from
-- the hour: the sun (a directional light - turned to swing its shadows,
-- lowered to lengthen them), the moon, the ambient light, the grade, the
-- pond that shows the sky, the lamps, the doorsteps and the fog.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/13-day-night.scene"

local DAY_SECONDS = 60.0            -- one day
local SUNRISE, SUNSET = 6.0, 18.5
local MAX_ELEVATION = math.rad(62)
local LAMPS = 8                     -- tools/gen_scenes.py DN_LAMPS
local HOUSES = 8                    -- DN_HOUSES

-- Linear colours through the day: { hour, r, g, b }.
local SKY = {
    { 0.0, 0.02, 0.03, 0.09 }, { 4.8, 0.03, 0.04, 0.12 }, { 5.6, 0.25, 0.16, 0.30 },
    { 6.3, 0.95, 0.45, 0.25 }, { 7.3, 0.50, 0.65, 0.95 }, { 12.0, 0.40, 0.62, 1.00 },
    { 17.2, 0.50, 0.62, 0.95 }, { 18.4, 1.00, 0.40, 0.18 }, { 19.2, 0.30, 0.16, 0.38 },
    { 20.3, 0.03, 0.04, 0.12 }, { 24.0, 0.02, 0.03, 0.09 },
}
-- { hour, r, g, b, intensity }
local AMBIENT = {
    { 0.0, 0.35, 0.45, 0.90, 0.10 }, { 5.0, 0.40, 0.45, 0.85, 0.12 }, { 6.3, 1.00, 0.70, 0.60, 0.30 },
    { 8.0, 0.80, 0.85, 1.00, 0.42 }, { 16.5, 0.80, 0.85, 1.00, 0.42 }, { 18.4, 1.00, 0.62, 0.48, 0.30 },
    { 19.4, 0.50, 0.45, 0.80, 0.16 }, { 21.0, 0.35, 0.45, 0.90, 0.10 }, { 24.0, 0.35, 0.45, 0.90, 0.10 },
}
-- The grade's tint: { hour, r, g, b, strength }.
local TINT = {
    { 0.0, 0.60, 0.70, 1.00, 0.22 }, { 5.2, 0.60, 0.70, 1.00, 0.20 }, { 6.4, 1.00, 0.75, 0.55, 0.18 },
    { 8.5, 1.00, 1.00, 1.00, 0.0 }, { 16.8, 1.00, 1.00, 1.00, 0.0 }, { 18.5, 1.00, 0.70, 0.50, 0.20 },
    { 19.6, 0.70, 0.60, 1.00, 0.18 }, { 21.0, 0.60, 0.70, 1.00, 0.22 }, { 24.0, 0.60, 0.70, 1.00, 0.22 },
}

local captions = true
local post_on = nav.post_all_on(SCENE)
local paused, fast = false, false
local hour = 5.4
local status, sun, moon, pond, stars, fog
local lamps, glows, doors = {}, {}, {}
local lamp_delay = {}

-- The keyframes either side of `h`, blended.
local function sample(frames, h)
    for i = 1, #frames - 1 do
        local a, b = frames[i], frames[i + 1]
        if h >= a[1] and h <= b[1] then
            local f = (h - a[1]) / math.max(b[1] - a[1], 1e-4)
            f = f * f * (3 - 2 * f)
            local out = {}
            for k = 2, #a do out[k - 1] = a[k] + (b[k] - a[k]) * f end
            return out
        end
    end
    local last = frames[#frames]
    local out = {}
    for k = 2, #last do out[k - 1] = last[k] end
    return out
end

local function smooth(e0, e1, x)
    local t = math.max(0, math.min(1, (x - e0) / (e1 - e0)))
    return t * t * (3 - 2 * t)
end

local function clock()
    local h = math.floor(hour)
    local m = math.floor((hour - h) * 60)
    return string.format("%02d:%02d", h, m)
end

local fps_avg = 0.0

local function refresh_status()
    if status then
        local fps = math.floor(fps_avg + 0.5)
        Text.set(status, clock() .. "   " .. fps .. " fps" .. (paused and "   paused" or "") .. (fast and "   x4" or "")
            .. "   " .. nav.post_status(post_on))
    end
end

function init(entity)
    status = Entity.find_by_name("Status")
    sun = Entity.find_by_name("Sun")
    moon = Entity.find_by_name("Moon")
    pond = Entity.find_by_name("Pond")
    stars = Entity.find_by_name("PondStars")
    fog = Entity.find_by_name("Fog")
    for i = 1, LAMPS do
        lamps[i] = Entity.find_by_name("Lamp" .. i)
        glows[i] = Entity.find_by_name("LampGlow" .. i)
        -- Each lamp comes on a little after the last: 0 .. 0.6 hours.
        lamp_delay[i] = ((i * 37) % 11) / 11 * 0.6
    end
    for i = 1, HOUSES do doors[i] = Entity.find_by_name("Door" .. i) end
end

function update(entity, dt)
    nav.tick(SCENE)
    nav.post_keys(post_on)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    if Input.is_key_just_pressed("Space") then paused = not paused end
    if Input.is_key_just_pressed("F") then fast = not fast end
    if not paused then
        hour = (hour + dt * 24.0 / DAY_SECONDS * (fast and 4 or 1)) % 24.0
    end

    -- The sun: across the sky from east to west. Its light travels west at
    -- sunrise (direction PI, rotation 0) and east at sunset (rotation -PI).
    local p = (hour - SUNRISE) / (SUNSET - SUNRISE)
    local elevation = (p > 0 and p < 1) and MAX_ELEVATION * math.sin(math.pi * p) or 0
    local up = smooth(0.0, math.rad(8), elevation)
    if sun then
        Transform.set_rotation(sun, -math.pi * math.max(0, math.min(1, p)))
        Lighting.set_height(sun, math.max(0.06, math.tan(elevation)))
        local warm = smooth(math.rad(4), math.rad(30), elevation)
        Lighting.set_color(sun, 1.0, 0.55 + 0.42 * warm, 0.3 + 0.6 * warm)
        Lighting.set_intensity(sun, 1.15 * up)
    end
    -- The night: 1 from an hour after sunset to an hour before sunrise.
    local night = math.max(smooth(SUNSET + 0.3, SUNSET + 1.6, hour), 1 - smooth(SUNRISE - 1.2, SUNRISE, hour))
    if moon then
        Transform.set_rotation(moon, -math.pi * (((hour + 6) % 24) / 12))
        Lighting.set_intensity(moon, 0.22 * night)
    end

    local sky = sample(SKY, hour)
    local amb = sample(AMBIENT, hour)
    local tint = sample(TINT, hour)
    Lighting.set_ambient(amb[1], amb[2], amb[3], amb[4])
    PostProcess.set_vec4("color_grade", "tint", tint[1], tint[2], tint[3], tint[4])
    if pond then Sprite.set_color(pond, sky[1] * 1.3, sky[2] * 1.3, sky[3] * 1.3, 1.0) end
    if stars then Sprite.set_color(stars, 1.0, 1.0, 1.0, night) end

    -- Lamps come on through dusk, one after another, and go off at dawn.
    local dusk = 1 - smooth(math.rad(2), math.rad(10), elevation)
    for i = 1, LAMPS do
        local on = smooth(SUNSET - 0.4 + lamp_delay[i], SUNSET + 0.1 + lamp_delay[i], hour)
        on = math.max(on, 1 - smooth(SUNRISE - 0.2 + lamp_delay[i] * 0.5, SUNRISE + 0.4 + lamp_delay[i] * 0.5, hour))
        on = on * dusk
        if lamps[i] then Lighting.set_intensity(lamps[i], 1.6 * on) end
        if glows[i] then Sprite.set_color(glows[i], 4.0, 2.4, 1.1, 0.8 * on) end
    end
    for i = 1, HOUSES do
        if doors[i] then Lighting.set_intensity(doors[i], 0.9 * math.max(night, dusk * 0.6)) end
    end
    -- Fog gathers after midnight and burns off in the morning.
    if fog then
        local dense = math.max(smooth(0.5, 4.0, hour) * (1 - smooth(6.8, 8.3, hour)), smooth(21.5, 24.0, hour))
        Material.set_vec4(fog, "fog", 0.32 * dense, 0, 0, 0)
    end
    local fps = Debug.get_fps and Debug.get_fps() or 0
    fps_avg = fps_avg == 0 and fps or fps_avg + (fps - fps_avg) * 0.05
    refresh_status()
end
