-- 16 Festival Night: fireworks over a harbour. The script only launches
-- shells and says where they burst - everything that makes a burst look
-- like one (drag, gravity, stretch, colour over life, the smoke's
-- turbulence) is authored on the prefabs in tools/gen_scenes.py. It also
-- fades each burst's light, which a light cannot do by itself.
--
-- Click sends a shell to the mouse; Space fires a salvo; F the finale; A
-- turns the show's own launches off and on.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/16-festival-night.scene"

-- tools/gen_scenes.py: FW_Y, the FwShell prefab's gravity, FW_COLOURS, and
-- the bursts' point light.
local FW_Y = -30.0
local GRAVITY = 260.0
local COLOURS = { "red", "green", "blue", "violet", "gold", "cyan", "pink" }
local LIGHT_PEAK, LIGHT_FADE = 2.6, 0.9

local captions = true
local post_on = nav.post_all_on(SCENE)
local status
local auto = true
local next_launch = 0.6
local finale_left, finale_in = 0, 0.0
local shells = {}      -- { e, fuse, kind }
local lights = {}      -- { e, age }
local pending = {}     -- { at, name, x, y }: a crackle after a willow

local function refresh_status()
    if status then
        UI.set_text(status, "auto " .. (auto and "on" or "off") .. "   " .. nav.post_status(post_on))
    end
end

local function spawn_at(name, x, y)
    local e = Entity.spawn_prefab(name)
    if e then Transform.set_position(e, x, y) end
    return e
end

local function pick_kind()
    local r = math.random()
    if r < 0.55 then return "peony" elseif r < 0.8 then return "ring" end
    return "willow"
end

-- A shell from `x` on the quay that stops rising `height` above it - where
-- it bursts. `tx`, if given, is where it should be by then.
local function launch(x, height, tx)
    local e = spawn_at("FwShell", x, FW_Y + 6)
    if not e then return end
    local vy = math.sqrt(2.0 * GRAVITY * height)
    local fuse = vy / GRAVITY * 0.97
    local vx = tx and (tx - x) / fuse or (math.random() - 0.5) * 80.0
    Physics.set_velocity(e, vx, vy)
    shells[#shells + 1] = { e = e, fuse = fuse, kind = pick_kind() }
end

local function burst(x, y, kind)
    local colour = COLOURS[math.random(#COLOURS)]
    local name = "FwPeony_" .. colour
    if kind == "willow" then name = "FwWillow" elseif kind == "ring" then name = "FwRing_" .. colour end
    local e = spawn_at(name, x, y)
    if e then lights[#lights + 1] = { e = e, age = 0.0 } end
    spawn_at("FwFlash", x, y)
    spawn_at("FwSmoke", x, y)
    if kind == "willow" then
        pending[#pending + 1] = { at = 1.3, name = "FwCrackle", x = x, y = y - 110 }
    end
end

local function random_launch()
    launch((math.random() - 0.5) * 1000.0, 170.0 + math.random() * 170.0)
end

function init(entity)
    status = Entity.find_by_name("Status")
    refresh_status()
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("A") then auto = not auto; refresh_status() end
    if Input.is_key_just_pressed("Space") then
        for i = 1, 5 do launch(-480 + (i - 1) * 240, 220 + math.random() * 90) end
    end
    if Input.is_key_just_pressed("F") then finale_left, finale_in = 28, 0.0 end
    if Input.is_mouse_left_just_pressed() then
        local mx, my = Input.get_mouse_world_position()
        if my > FW_Y + 80 then launch(mx * 0.85, my - FW_Y, mx) end
    end

    if auto then
        next_launch = next_launch - dt
        if next_launch <= 0 then
            next_launch = 0.35 + math.random() * 0.9
            random_launch()
        end
    end
    if finale_left > 0 then
        finale_in = finale_in - dt
        if finale_in <= 0 then
            finale_in = 0.09
            finale_left = finale_left - 1
            random_launch()
        end
    end

    -- Shells: burst at the top of the climb, then let the trail fade.
    for i = #shells, 1, -1 do
        local s = shells[i]
        s.fuse = s.fuse - dt
        if s.fuse <= 0 then
            local x, y = Transform.get_position(s.e)
            if x then burst(x, y, s.kind) end
            Physics.set_velocity(s.e, 0, 0)
            Trail.set_emitting(s.e, false)
            Sprite.set_color(s.e, 0, 0, 0, 0)
            table.remove(shells, i)
        end
    end
    for i = #pending, 1, -1 do
        local p = pending[i]
        p.at = p.at - dt
        if p.at <= 0 then
            spawn_at(p.name, p.x, p.y)
            table.remove(pending, i)
        end
    end
    -- Each burst's light flares and dies away.
    for i = #lights, 1, -1 do
        local l = lights[i]
        l.age = l.age + dt
        local k = math.max(0.0, 1.0 - l.age / LIGHT_FADE)
        Lighting.set_intensity(l.e, LIGHT_PEAK * k * k)
        if l.age >= LIGHT_FADE then table.remove(lights, i) end
    end
end
