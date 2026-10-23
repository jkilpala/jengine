-- 15 Still Water: the camera drifts and breathes in and out over a lake, and
-- the water keeps mirroring what stands above it. A click drops a stone - a
-- ripple ring the water graph bends the reflection with - and the lake drops
-- one of its own now and then, a fish. W raises the wind.
--
-- The graph reads four parameters: `lake` (surface y, moon x, wind, unused)
-- and three ripples (x, y, age in seconds, strength). The age is counted
-- here rather than read off the shader's clock, so a ripple starts when it is
-- dropped in every runtime.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/15-still-water.scene"

-- tools/gen_scenes.py: LAKE_Y, LAKE_MOON, LAKE_WIND.
local LAKE_Y = -40.0
local MOON_X = 300.0
local CALM, WINDY = 0.12, 0.85
local RIPPLE_LIFE = 5.0
-- How far each far layer follows the camera: 1 is pinned to the sky.
local PARALLAX = {
    Sky = 0.95, Stars = 0.95, MoonHalo = 0.92, Moon = 0.92, HorizonGlow = 0.95,
    Mountains = 0.6, Pines = 0.2,
}

local captions = true
local post_on = nav.post_all_on(SCENE)
local windy, drifting = false, true
local wind = CALM
local t = 0.0
local status, water, cam
local layers = {}            -- name -> { entity, x, y }
local ripples = {            -- x, y, age, strength
    { 0, 0, 99, 0 }, { 0, 0, 99, 0 }, { 0, 0, 99, 0 },
}
local next_slot = 1
local fish_in = 3.0

local function refresh_status()
    if status then
        UI.set_text(status, "wind " .. (windy and "up" or "calm") .. "   drift " .. (drifting and "on" or "off")
            .. "   " .. nav.post_status(post_on))
    end
end

local function drop(x, y, strength)
    ripples[next_slot] = { x, y, 0.0, strength }
    next_slot = next_slot % #ripples + 1
end

function init(entity)
    status = Entity.find_by_name("Status")
    water = Entity.find_by_name("Water")
    cam = Entity.find_by_name("Camera")
    for name, _ in pairs(PARALLAX) do
        local e = Entity.find_by_name(name)
        if e then
            local x, y = Transform.get_position(e)
            layers[name] = { e, x, y }
        end
    end
    refresh_status()
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("W") then windy = not windy; refresh_status() end
    if Input.is_key_just_pressed("C") then drifting = not drifting; refresh_status() end

    if drifting then t = t + dt end
    -- The camera: a slow drift sideways, breathing in and out.
    local cx = math.sin(t * 0.07) * 240.0
    if cam then Transform.set_position(cam, cx, 0) end
    Camera.set_zoom(1.0 + 0.07 * math.sin(t * 0.05))
    for name, l in pairs(layers) do
        Transform.set_position(l[1], l[2] + cx * PARALLAX[name], l[3])
    end
    local moon_x = MOON_X + cx * PARALLAX.Moon

    -- Wind eases toward what W asks for.
    local goal = windy and WINDY or CALM
    wind = wind + (goal - wind) * math.min(1.0, dt * 1.5)

    if Input.is_mouse_left_just_pressed() then
        local mx, my = Input.get_mouse_world_position()
        if my < LAKE_Y - 4 then drop(mx, my, 1.0) end
    end
    fish_in = fish_in - dt
    if fish_in <= 0 then
        fish_in = 2.5 + math.random() * 3.5
        drop(cx + (math.random() - 0.5) * 1000, LAKE_Y - 30 - math.random() * 220, 0.45)
    end

    if water then
        Material.set_vec4(water, "lake", LAKE_Y, moon_x, wind, 0)
        local names = { "ripple_a", "ripple_b", "ripple_c" }
        for i, r in ipairs(ripples) do
            r[3] = r[3] + dt
            local live = r[3] < RIPPLE_LIFE
            Material.set_vec4(water, names[i], r[1], r[2], r[3], live and r[4] or 0)
        end
    end
end
