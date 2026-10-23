-- 107 Night Pursuit: the camera rides with the getaway car, so the street
-- does the moving. The static things stream past on a `velocity` each and
-- are put back ahead when they leave; the traffic, the police and the car's
-- weaving are moved here. `motion_blur` (with its velocity pass) smears each
-- by its own motion across the screen - M switches it off to compare.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/107-night-pursuit.scene"

local SPEED = 1400.000000
local PERIOD = 3600.000000
local LANES = { -65.000000, -195.000000, 65.000000, 195.000000 }   -- two east, two west

local e = {}
local streaming = {}
local traffic = {}
local t = 0
local lane = 1
local car_y = LANES[1]
local blur_on = true
local captions = true
local rng = 107

local function rand()
    rng = (rng * 16807) % 2147483647
    return rng / 2147483647
end

local function track(name)
    local id = Entity.find_by_name(name)
    if id then streaming[#streaming + 1] = id end
end

-- Is a lane clear around x?
local function clear(l, x)
    for _, c in ipairs(traffic) do
        if c.lane == l and math.abs(c.x - x) < 520 then return false end
    end
    return true
end

function init(entity)
    for _, n in ipairs({ "Coupe", "Searchlight", "Rotor", "Camera", "Status" }) do e[n] = Entity.find_by_name(n) end
    for i = 0, 2 do track("Street" .. i) end
    for i = 0, 40 do
        track("Roof" .. i .. "N")
        track("Roof" .. i .. "S")
        track("Neon" .. i)
    end
    for i = 0, 9 do track("Lamp" .. i) end
    for i = 0, 5 do track("Parked" .. i) end
    for k = 0, 5 do
        local east = k < 3
        traffic[#traffic + 1] = {
            e = Entity.find_by_name("Traffic" .. k), east = east, lane = east and (k % 2 + 1) or (k % 2 + 3),
            x = 600 + k * 500, speed = east and (650 + rand() * 250) or (800 + rand() * 300),
        }
    end
    for k = 0, 1 do
        e["Police" .. k] = Entity.find_by_name("Police" .. k)
        e["Police" .. k .. "Red"] = Entity.find_by_name("Police" .. k .. "Red")
        e["Police" .. k .. "Blue"] = Entity.find_by_name("Police" .. k .. "Blue")
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("M") then
        blur_on = not blur_on
        PostProcess.set_enabled("motion_blur", blur_on)
    end
    -- What streams past comes round again ahead.
    for _, id in ipairs(streaming) do
        local x, y = Transform.get_position(id)
        if x < -PERIOD / 2 then Transform.set_position(id, x + PERIOD, y) end
    end
    -- Traffic: east-bound slower than us, west-bound towards us.
    for _, c in ipairs(traffic) do
        local rel = c.east and (c.speed - SPEED) or (-c.speed - SPEED)
        c.x = c.x + rel * dt
        if c.x < -1500 then
            c.x = 1500 + rand() * 900
            c.speed = c.east and (650 + rand() * 250) or (800 + rand() * 300)
            for _ = 1, 4 do
                local l = c.east and (1 + math.floor(rand() * 2)) or (3 + math.floor(rand() * 2))
                if clear(l, c.x) then c.lane = l break end
            end
        end
        Transform.set_position(c.e, c.x, LANES[c.lane])
    end
    -- The getaway car weaves: if its lane is blocked ahead, it takes another
    -- - the oncoming ones too, when it must.
    local blocked = false
    for _, c in ipairs(traffic) do
        if c.lane == lane and c.x > -200 and c.x < 500 then blocked = true end
    end
    if blocked then
        for _, l in ipairs({ 1, 2, 3, 4 }) do
            if l ~= lane and clear(l, 100) then lane = l break end
        end
    end
    local target = LANES[lane]
    local dy = (target - car_y)
    car_y = car_y + dy * math.min(1, dt * 3.5)
    local x = -200 + math.sin(t * 0.7) * 60
    Transform.set_position(e.Coupe, x, car_y)
    Transform.set_rotation(e.Coupe, math.max(-0.35, math.min(0.35, dy * 0.004)))
    -- The police keep on its tail, closing and falling back.
    for k = 0, 1 do
        local px = x - 420 - 220 * k + math.sin(t * 0.9 + k * 2.1) * 140
        local py = car_y + (k == 0 and -1 or 1) * 70 + math.sin(t * 1.3 + k) * 30
        Transform.set_position(e["Police" .. k], px, py)
        Transform.set_rotation(e["Police" .. k], math.sin(t * 1.3 + k) * 0.06)
        local red = math.floor(t * 8 + k) % 2 == 0
        Lighting.set_intensity(e["Police" .. k .. "Red"], red and 2.4 or 0)
        Lighting.set_intensity(e["Police" .. k .. "Blue"], red and 0 or 2.4)
        Sprite.set_color(e["Police" .. k .. "Red"], 6, 0.6, 0.6, red and 1 or 0.15)
        Sprite.set_color(e["Police" .. k .. "Blue"], 0.9, 2.1, 6, red and 0.15 or 1)
    end
    -- The helicopter's light hunts after the car, its rotor turning above.
    local sx = x + 40 * math.sin(t * 1.7)
    local sy = car_y + 30 * math.cos(t * 1.3)
    Transform.set_position(e.Searchlight, sx, sy)
    Transform.set_position(e.Rotor, sx + 90, sy + 60)
    Transform.set_rotation(e.Rotor, t * 26)
    -- The camera sways with the car.
    Transform.set_position(e.Camera, 0, car_y * 0.25)
    UI.set_text(e.Status, blur_on and "motion blur on" or "motion blur off")
end
