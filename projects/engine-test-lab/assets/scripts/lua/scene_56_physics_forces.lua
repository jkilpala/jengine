-- Scene 56 - Physics Forces & Contacts
--
-- Scene 17 builds bodies and lets gravity do the work. This one drives them
-- from script and reads the simulation back:
--
--   Physics.apply_force(e, fx, fy)          accumulate a force on a dynamic body
--   Physics.set_angular_velocity(e, omega)  spin it directly
--   Physics.get_angular_velocity(e)         read the spin back
--   Physics.contact_count()                 how many contacts this frame
--   Physics.get_contact(i)                  -> entity_a, entity_b, started   (1-based)
--
-- `started` is true only on the first frame of a touch, which is how a script
-- distinguishes "just hit" from "still resting on". `Physics.get_contacts()` is the
-- array-returning alternative and is listed as not-yet-on-bridge natively, so
-- this scene uses the count + index pair, which is available everywhere.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/56-physics-forces.scene"

-- Force, so acceleration is PUSH / mass. The ball and crate take the default
-- `mass` of 1.0, which since Sep 2026 is their actual mass rather than a field
-- nothing read (BUGS #50) -- they used to weigh ~1200, derived from collider
-- density x area, which is why this constant used to be five figures. At 1500
-- the sideways push is a bit under 1.5g and the `PUSH * 26` launch on Space
-- clears about 200 units.
local PUSH = 1500.0
local ball, crate
local started_log = "-"

function init(entity)
    ball = Entity.find_by_name("Ball")
    crate = Entity.find_by_name("Crate")
    log("[scene_56] ready - arrows push, Space launches, S spins, X stops the spin, R reloads")
end

function update(entity, dt)
    if Input.is_key_just_pressed("R") then
        Scene.load(SCENE_PATH)
        return
    end
    if ball ~= nil then
        -- A force pushes for the frame it is applied in, so holding a key is a
        -- steady push (constant acceleration) and letting go stops it. Until
        -- BUGS #156 every frame's force stayed on the body for good, so a held
        -- arrow ramped the push without bound and the ball tunnelled out.
        if Input.is_key_pressed("Left") then Physics.apply_force(ball, -PUSH, 0.0) end
        if Input.is_key_pressed("Right") then Physics.apply_force(ball, PUSH, 0.0) end
        if Input.is_key_pressed("Up") then Physics.apply_force(ball, 0.0, PUSH) end
        if Input.is_key_just_pressed("Space") then Physics.apply_force(ball, 0.0, PUSH * 26.0) end
        if Input.is_key_just_pressed("S") then Physics.set_angular_velocity(ball, 12.0) end
        if Input.is_key_just_pressed("X") then Physics.set_angular_velocity(ball, 0.0) end
    end

    local n = Physics.contact_count() or 0
    local pairs_seen = {}
    local fresh = {}
    for i = 1, n do
        local a, b, started = Physics.get_contact(i)
        if a ~= nil then
            pairs_seen[#pairs_seen + 1] = string.format("%s-%s", tostring(a), tostring(b))
            if started then
                fresh[#fresh + 1] = string.format("%s+%s", tostring(a), tostring(b))
            end
        end
    end
    if #fresh > 0 then
        started_log = table.concat(fresh, "  ")
    end

    local sl = Entity.find_by_name("SpinLabel")
    if sl and ball ~= nil then
        Text.set(sl, string.format("get_angular_velocity(ball) = %.2f rad/s",
            Physics.get_angular_velocity(ball) or 0))
    end
    local cl = Entity.find_by_name("ContactLabel")
    if cl then
        Text.set(cl, string.format("contact_count() = %d   [%s]", n,
            #pairs_seen > 0 and table.concat(pairs_seen, " ") or "none"))
    end
    local stl = Entity.find_by_name("StartedLabel")
    if stl then
        Text.set(stl, "last contact start (started=true): " .. started_log)
    end
    local r = Entity.find_by_name("Readout")
    if r then
        Text.set(r, "arrows push (a steady force while held), Space launches, S/X spin/stop, R reload")
    end

    nav.tick(SCENE_PATH)
end
