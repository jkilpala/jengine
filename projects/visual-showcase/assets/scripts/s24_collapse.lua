-- 24 Collapse: throw the boulder, flicker the fire, rebuild. The breaking
-- is the engine's: every block is `breakable`, and dust bursts where one
-- breaks (`debris`). This throws, shakes the camera when something lands
-- hard, and reloads the scene to stand the tower up again.
-- Space throws now; R rebuilds.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/24-collapse.scene"
local TOWER_X = 170                 -- tools/gen_scenes.py: CL_TOWER_X

local post_on = nav.post_all_on(SCENE)
local captions = true
local status, boulder
local lights = {}
local t, throws = 0.0, 0
local last_vx, last_vy = 0.0, 0.0
local rng = 2424

local function rand()
    rng = (rng * 16807) % 2147483647
    return rng / 2147483647
end

local function refresh_status()
    if status then
        Text.set(status, "throws " .. throws .. "   " .. nav.post_status(post_on))
    end
end

-- At the tower from wherever the boulder is: hard along, and up enough to arc.
local function throw()
    if not boulder then return end
    local x = Transform.get_position(boulder)
    local dir = (x < TOWER_X) and 1 or -1
    Physics.set_velocity(boulder, dir * (1350 + rand() * 250), 420 + rand() * 220)
    throws = throws + 1
    refresh_status()
end

function init(entity)
    status = Entity.find_by_name("Status")
    boulder = Entity.find_by_name("Boulder")
    for _, n in ipairs({ "BrazierLLight", "BrazierRLight" }) do
        local l = Entity.find_by_name(n)
        if l then lights[#lights + 1] = l end
    end
    refresh_status()
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    t = t + dt

    if Input.is_key_just_pressed("Space") then throw() end
    if Input.is_key_just_pressed("R") then Scene.load(SCENE) return end
    -- Left alone: throw, throw again, rebuild.
    if (t > 1.5 and throws == 0) or (t > 7.0 and throws == 1) then throw() end
    if t > 16.0 then Scene.load(SCENE) return end

    -- The fire breathes: two flickers that do not keep step.
    for i, l in ipairs(lights) do
        local f = 1.55 + 0.18 * math.sin(t * 9.0 + i * 2.1) + 0.12 * math.sin(t * 23.0 + i) + 0.08 * (rand() - 0.5)
        Lighting.set_intensity(l, f)
    end

    -- A hard stop of the boulder shakes the ground.
    if boulder then
        local vx, vy = Physics.get_velocity(boulder)
        local dv = math.sqrt((vx - last_vx) ^ 2 + (vy - last_vy) ^ 2)
        if dv > 500 then Camera.shake(math.min(14, dv / 90), 0.25, 30) end
        last_vx, last_vy = vx, vy
    end
end
