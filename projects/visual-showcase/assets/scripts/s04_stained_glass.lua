-- 04 Stained Glass: the sun crosses the sky beyond the north wall.
--
-- The coloured light on the floor is the sun's light through tinted
-- occluders (the panes), so moving the one light moves every band. What the
-- script adds is the air: each pane's beam is a sprite aimed away from the
-- sun, and the sun warms toward the ends of its arc, as a low sun does.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/04-stained-glass.scene"

local SUN_Y = 1500.0          -- tools/gen_scenes.py GLASS_SUN_Y
local SUN_REACH = 1150.0      -- how far either side the sun travels
local SUN_PERIOD = 60.0       -- seconds for one crossing and back
local SUN_INTENSITY = 4.0
local BEAM_ALPHA = 0.05
local WINDOWS, PANES = 4, 5
local SURFACES = { "flagstones", "carpet", "pew" }

local t = SUN_PERIOD * 0.3
local paused = false
local beams_on = true
local normals_on = true
local captions = true
local post_on = nav.post_all_on(SCENE)

local sun, status
local beams = {}   -- { entity, x, y, r, g, b }

local function refresh_status()
    if status then
        Text.set(status, (paused and "sun paused   " or "")
            .. (normals_on and "" or "normal maps OFF   ")
            .. nav.post_status(post_on))
    end
end

local function set_normals(on)
    normals_on = on
    for _, stem in ipairs(SURFACES) do
        local normal = on and ("sprites/env/" .. stem .. "_n.png") or nil
        for _, e in ipairs(Entity.find_by_tag("bump_" .. stem)) do
            Material.set_normal_map(e, normal)
        end
    end
end

-- atan2 differs between Luau and Fengari (CLAUDE.md, "Parity").
local function atan2(y, x)
    if math.atan2 then return math.atan2(y, x) end
    return math.atan(y, x)
end

function init(entity)
    sun = Entity.find_by_name("Sun")
    status = Entity.find_by_name("Status")
    for w = 1, WINDOWS do
        for p = 1, PANES do
            local e = Entity.find_by_name("Beam" .. w .. "_" .. p)
            if e then
                local x, y = Transform.get_position(e)
                local r, g, b = Sprite.get_color(e)
                beams[#beams + 1] = { entity = e, x = x, y = y, r = r, g = g, b = b }
            end
        end
    end
    refresh_status()
end

local function handle_keys(dt)
    local changed = nav.post_keys(post_on)
    if Input.is_key_just_pressed("Space") then paused = not paused; changed = true end
    if Input.is_key_just_pressed("N") then set_normals(not normals_on); changed = true end
    if Input.is_key_just_pressed("B") then beams_on = not beams_on end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    -- The arrows steer the sun by scrubbing its clock.
    if Input.is_key_pressed("Left") then t = t - dt * 6.0 end
    if Input.is_key_pressed("Right") then t = t + dt * 6.0 end
    if changed then refresh_status() end
end

function update(entity, dt)
    nav.tick(SCENE)
    handle_keys(dt)
    if not paused then t = t + dt end

    -- A slow sweep, eased at the ends as a sun seems to hang there.
    local s = math.sin(t / SUN_PERIOD * math.pi * 2.0)
    local sx = s * SUN_REACH
    local low = math.abs(s)                    -- 0 at noon, 1 at the ends
    if sun then
        Transform.set_position(sun, sx, SUN_Y)
        Lighting.set_color(sun, 1.0, 0.86 - 0.28 * low, 0.64 - 0.44 * low)
        Lighting.set_intensity(sun, SUN_INTENSITY * (1.0 - 0.25 * low))
    end

    -- Every beam points from the sun through its pane.
    for _, b in ipairs(beams) do
        Transform.set_rotation(b.entity, atan2(b.y - SUN_Y, b.x - sx))
        Sprite.set_color(b.entity, b.r, b.g, b.b, beams_on and BEAM_ALPHA or 0.0)
    end
end
