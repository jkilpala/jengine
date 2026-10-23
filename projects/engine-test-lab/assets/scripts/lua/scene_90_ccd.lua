-- Scene 90 - a fast body does not tunnel
--
-- Two lanes, one ball each, the same speed, the same wall. The only
-- difference is `rigid_body.ccd`.
--
-- Rapier integrates a body's velocity and *then* looks for overlaps. A
-- ball 16 units across travelling 3000 units a second covers 50 units in
-- a 60 Hz step, so by the time anything is measured it is already on the
-- far side of a 20-unit wall, touching nothing. No contact is generated,
-- nothing is logged, and the ball simply leaves. Continuous collision
-- sweeps the shape along its path instead of testing where it landed.
--
-- The engine has always constructed a `CCDSolver` and handed it to every
-- step - but rapier only sweeps bodies that ask to be swept, and until
-- the `ccd` field existed nothing could ask. The solver ran on an empty
-- set for the life of the engine, and no game here had gone fast enough
-- to notice. The arcade's pinball table is what found it.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/90-ccd.scene"

local SPEED = 3000
local START_DX = -230
local LANES = {
    { name = "NoCcd", x = -320, label = "ccd OFF" },
    { name = "Ccd", x = 220, label = "ccd ON" },
}

local readout
local balls = {}
local t = 0
local fired_at = 0
local verdict = ""

local function fire()
    for _, lane in ipairs(LANES) do
        local b = balls[lane.name]
        if b ~= nil then
            Transform.set_position(b, lane.x + START_DX, 40)
            Physics.set_velocity(b, SPEED, 0)
        end
    end
    fired_at = t
    verdict = ""
end

function init(entity)
    readout = Entity.find_by_name("Readout")
    for _, lane in ipairs(LANES) do
        balls[lane.name] = Entity.find_by_name("Ball" .. lane.name)
    end
    fire()
    log("[scene_90] ready - the left ball should go through, the right one should not")
end

-- Where each ball ended up relative to its own wall. Past the wall by
-- more than its own radius means it went through.
local function report()
    local bits = {}
    local through = {}
    for _, lane in ipairs(LANES) do
        local b = balls[lane.name]
        if b ~= nil then
            local x = Transform.get_position(b)
            local past = x - lane.x
            through[lane.name] = past > 20
            bits[#bits + 1] = string.format("%s: %s (%+.0f from its wall)",
                lane.label,
                through[lane.name] and "WENT THROUGH" or "stopped",
                past)
        end
    end
    if through["NoCcd"] and not through["Ccd"] then
        verdict = "AS EXPECTED - " .. table.concat(bits, "   |   ")
    else
        verdict = "UNEXPECTED - " .. table.concat(bits, "   |   ")
    end
end

function update(entity, dt)
    t = t + dt
    if Input.is_key_just_pressed("Space") then
        fire()
    end
    -- Give them time to arrive, then read the result and hold it.
    if verdict == "" and t - fired_at > 0.6 then
        report()
    end
    -- And fire again on a loop, so the scene is watchable rather than a
    -- single frozen frame.
    if t - fired_at > 3.0 then
        fire()
    end
    if readout ~= nil then
        Text.set(readout, verdict)
    end
    nav.tick(SCENE_PATH)
end
