-- 115 Rewind Ruins: a boulder brings the temple down, then time runs
-- back. Everything that moves is `rewindable`; the `rewind` clock on "Clock"
-- records and rewinds them. This script only throws the boulder, lights the
-- rubble and turns the clock - and keeps its own time in step with it, since
-- a script's state is its own to rewind.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/115-rewind-ruins.scene"
local SPEED = 1.6

local clock, boulder, pilgrim, fire, dust, status
local captions = true
local t = 0            -- the world's time, as the clock has it
local mode = "record"
local hold = 0
local thrown, landed = false, false
local throws = 0

local function set_mode(m)
    mode = m
    Rewind.set(clock, "mode", m)
    -- Rewinding looks like rewinding: cold, faded, the colours split.
    local back = m ~= "record"
    PostProcess.set_float("color_grade", "saturation", back and 0.35 or 1.05)
    PostProcess.set_vec4("color_grade", "tint_color", back and 0.6 or 1.0, back and 0.75 or 0.9, back and 1.0 or 0.78, back and 0.25 or 0.06)
    PostProcess.set_enabled("chromatic", back)
end

local function throw()
    throws = throws + 1
    local k = throws % 3
    Physics.set_velocity(boulder, 820 + k * 140, 300 + k * 120)
    thrown = true
end

function init(entity)
    clock = Entity.find_by_name("Clock")
    boulder = Entity.find_by_name("Boulder")
    pilgrim = Entity.find_by_name("Pilgrim")
    fire = Entity.find_by_name("RubbleFire")
    dust = Entity.find_by_name("Dust")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if mode == "record" then
        t = t + dt
        -- The pilgrim walks on (the clock walks her back).
        local x, y = Transform.get_position(pilgrim)
        Transform.set_position(pilgrim, x + 62 * dt, y)
        if (not thrown and t > 1.6) or Input.is_key_just_pressed("Space") then throw() end
        local bx = Transform.get_position(boulder)
        if thrown and not landed and bx > -40 then
            landed = true
            Particles.emit_burst(dust, 120)
        end
        -- The rubble catches from the braziers after the fall.
        if landed then
            local rate = math.min(90, (t - 2.6) * 60)
            if rate > 0 then
                Particles.set_rate(fire, rate)
                Lighting.set_intensity(fire, rate / 90 * 2.2)
            end
        end
        if t > 6.5 or Input.is_key_just_pressed("R") then set_mode("rewind") end
    elseif mode == "rewind" then
        t = math.max(0, t - dt * SPEED)
        -- The fire's light goes back with its rate.
        Lighting.set_intensity(fire, math.max(0, math.min(90, (t - 2.6) * 60)) / 90 * 2.2)
        if t <= 0.05 then
            set_mode("hold")
            hold = 1.0
        end
    else
        hold = hold - dt
        if hold <= 0 then
            t = 0
            thrown, landed = false, false
            set_mode("record")
        end
    end
    UI.set_text(status, mode == "record" and string.format("%.1f s", t)
        or mode == "rewind" and string.format("<< %.1f s", t) or "...")
end
