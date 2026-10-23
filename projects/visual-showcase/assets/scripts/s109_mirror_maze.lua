-- 109 Mirror Maze: four mirrors wall a small room, and everything in it is
-- shown again in each - and in each mirror's reflection of the others. This
-- script moves what is in the room: the laser turning and bouncing, the
-- bulbs chasing, the lights breathing, the visitors wandering - and now and
-- then one of them stepping into the portal and out into the garden it
-- looks down on. The mirrors themselves are the engine's (`mirror`).

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/109-mirror-maze.scene"

local RX, RY = 280.000000, 150.000000
local LX, LY = -130.000000, 0.000000
local PX, PY = 150.000000, 0.000000
local GX, GY = 8000.000000, 0.000000
local VISITORS = 5
local MIRRORS = { "MirrorN", "MirrorS", "MirrorE", "MirrorW" }

local e = {}
local bulbs = {}
local visitors = {}
local t = 0
local rng = 109
local captions = true
local mirrors_on = true

local function rand()
    rng = (rng * 16807) % 2147483647
    return rng / 2147483647
end
local function between(a, b) return a + (b - a) * rand() end

-- A place on the floor clear of the laser and the portal.
local function spot()
    for _ = 1, 20 do
        local x, y = between(-RX + 30, RX - 30), between(-RY + 30, RY - 30)
        if (x - LX) ^ 2 + (y - LY) ^ 2 > 60 ^ 2 and (x - PX) ^ 2 + (y - PY) ^ 2 > 90 ^ 2 then return x, y end
    end
    return 0, -RY + 40
end

function init(entity)
    for _, n in ipairs({ "Laser", "Beam", "BeamGlow", "Status", "PortalRunes", "PortalGlow" }) do e[n] = Entity.find_by_name(n) end
    for _, n in ipairs(MIRRORS) do e[n] = Entity.find_by_name(n) end
    for i = 0, 3 do e["Corner" .. i] = Entity.find_by_name("Corner" .. i) end
    for i = 0, 7 do e["Hit" .. i] = Entity.find_by_name("Hit" .. i) end
    local i = 0
    while true do
        local b = Entity.find_by_name("Bulb" .. i)
        if not b then break end
        bulbs[#bulbs + 1] = b
        i = i + 1
    end
    for k = 0, VISITORS - 1 do
        local id = Entity.find_by_name("Visitor" .. k)
        local x, y = Transform.get_position(id)
        local tx, ty = spot()
        visitors[#visitors + 1] = { e = id, x = x, y = y, tx = tx, ty = ty, a = 0, speed = between(45, 70),
                                    state = "walk", wait = between(0, 2), scale = 1 }
    end
    visitors[1].state = "portal"
end

-- The beam from the laser, bounced off the four walls.
local function beam_path(angle)
    local pts = { { LX, LY } }
    local x, y = LX, LY
    local dx, dy = math.cos(angle), math.sin(angle)
    local hits = {}
    for _ = 1, 6 do
        local tx = dx > 0 and (RX - 2 - x) / dx or (dx < 0 and (-RX + 2 - x) / dx or 1e9)
        local ty = dy > 0 and (RY - 2 - y) / dy or (dy < 0 and (-RY + 2 - y) / dy or 1e9)
        local s = math.min(tx, ty)
        x, y = x + dx * s, y + dy * s
        pts[#pts + 1] = { x, y }
        hits[#hits + 1] = { x, y }
        if tx < ty then dx = -dx else dy = -dy end
    end
    return pts, hits
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("M") then
        mirrors_on = not mirrors_on
        for _, n in ipairs(MIRRORS) do Mirror.set(e[n], "enabled", mirrors_on) end
    end

    -- The laser turns slowly; its beam bounces round the room.
    local angle = t * 0.23 + 0.4
    Transform.set_rotation(e.Laser, angle)
    local pts, hits = beam_path(angle)
    local flicker = 1 + 0.15 * math.sin(t * 61)
    Line.set(e.Beam, "points", pts)
    Line.set(e.BeamGlow, "points", pts)
    Line.set(e.BeamGlow, "width", 14 * flicker)
    for i = 0, 7 do
        local h = hits[i + 1]
        if h then Transform.set_position(e["Hit" .. i], h[1], h[2]) else Transform.set_position(e["Hit" .. i], 0, -5000) end
    end

    -- Bulbs chase round the frames.
    local n = #bulbs
    for i, b in ipairs(bulbs) do
        local on = math.floor(t * 9 - i) % 4 == 0
        local k = on and 6 or 1.2
        Sprite.set_color(b, k, k * 0.82, k * 0.45, on and 1 or 0.6)
    end

    -- Corner lights breathe, out of step.
    for i = 0, 3 do
        Lighting.set_intensity(e["Corner" .. i], 0.8 + 0.3 * math.sin(t * 0.9 + i * 1.6))
    end
    Transform.set_rotation(e.PortalRunes, t * 0.4)
    Lighting.set_intensity(e.PortalGlow, 1.1 + 0.3 * math.sin(t * 2.3))

    -- Visitors wander; the first goes through the portal, and back.
    for _, v in ipairs(visitors) do
        if v.state == "walk" or v.state == "portal" or v.state == "return" then
            local tx, ty = v.tx, v.ty
            if v.state == "portal" then tx, ty = PX, PY end
            local dx, dy = tx - v.x, ty - v.y
            local d = math.sqrt(dx * dx + dy * dy)
            if d < 4 then
                if v.state == "portal" then
                    v.state = "sink"
                else
                    v.wait = between(0.5, 2.5)
                    v.state = "wait"
                end
            else
                local step = math.min(d, v.speed * dt)
                v.x, v.y = v.x + dx / d * step, v.y + dy / d * step
                v.a = (math.atan2 or math.atan)(dy, dx)
            end
        elseif v.state == "wait" then
            v.wait = v.wait - dt
            if v.wait <= 0 then
                v.tx, v.ty = spot()
                v.state = (v == visitors[1] and rand() < 0.5) and "portal" or "walk"
            end
        elseif v.state == "sink" then
            v.scale = v.scale - dt * 1.2
            v.a = v.a + dt * 8
            if v.scale <= 0 then
                v.scale = 0
                -- Out in the garden, walking a while.
                v.x, v.y = GX + between(-60, 60), GY - 40
                v.state = "garden"
                v.wait = 6
                v.tx, v.ty = GX + between(-120, 120), GY + between(-120, 120)
            end
        elseif v.state == "garden" then
            v.scale = math.min(1, v.scale + dt * 1.5)
            v.wait = v.wait - dt
            local dx, dy = v.tx - v.x, v.ty - v.y
            local d = math.sqrt(dx * dx + dy * dy)
            if d > 4 then
                v.x, v.y = v.x + dx / d * v.speed * dt, v.y + dy / d * v.speed * dt
                v.a = (math.atan2 or math.atan)(dy, dx)
            else
                v.tx, v.ty = GX + between(-120, 120), GY + between(-120, 120)
            end
            if v.wait <= 0 then
                v.x, v.y = PX, PY
                v.scale = 0
                v.state = "rise"
            end
        elseif v.state == "rise" then
            v.scale = math.min(1, v.scale + dt * 1.2)
            v.a = v.a - dt * 8
            if v.scale >= 1 then
                v.tx, v.ty = spot()
                v.state = "walk"
            end
        end
        Transform.set_position(v.e, v.x, v.y)
        Transform.set_rotation(v.e, v.a)
        Transform.set_scale(v.e, v.scale, v.scale)
    end

    UI.set_text(e.Status, mirrors_on and "mirrors on" or "mirrors off")
end
