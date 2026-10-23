-- Scene 95 - frame pacing, or why a good FPS number can still judder
--
-- A runtime measures `dt` as the wall time between two frame starts. That
-- is correct on average and wrong every single frame: the gap absorbs
-- swapchain acquire latency, compositor scheduling, and whatever else the
-- OS did in between. A display holding a rock-steady 165 Hz hands the
-- simulation deltas like 5.8 / 6.3 / 5.9 / 6.4 ms rather than a flat 6.06.
--
-- Integrate that directly and things move by a different amount each frame
-- while the display samples them at a fixed cadence. The frame *rate* is
-- perfect and the motion is not, which is the single most confusing class
-- of performance complaint: the counter says everything is fine.
--
-- The engine's `FramePacer` snaps a delta that is nearly a whole number of
-- refresh periods onto that grid, and leaves a genuine hitch alone. This
-- scene is the eyeball test for it - the bars below move at a constant
-- speed, so any unevenness in them is unevenness in `dt`.
--
-- Worth knowing while reading the numbers:
--
--  * A **multi-monitor setup with mismatched refresh rates** is the worst
--    case, and on a hybrid-GPU laptop it is worse still: if the window is
--    on a panel driven by the iGPU while wgpu picked the discrete card,
--    every frame is copied across adapters before it can be shown, on a
--    clock unrelated to the one the swapchain blocks against. Drag this
--    window between displays and watch the spread change.
--  * **An unattended run measures nothing.** Windows throttles
--    presentation when the display sleeps, so leave the mouse alone for
--    thirty seconds and the drift row below will show the game clock
--    falling behind the wall. That is the machine idling, not a bug.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/95-frame-pacing.scene"

-- Constant speeds, in units a second. Judder is easiest to see on the
-- fast row and easiest to miss on the slow one, so there are three.
local LANES = {
    { name = "LaneSlow", speed = 120 },
    { name = "LaneMid", speed = 420 },
    { name = "LaneFast", speed = 900 },
}
local TRACK_HALF = 560

-- How many frames the spread readout looks back over. Two seconds or so
-- at a typical refresh: long enough to catch an occasional hiccup, short
-- enough to still respond when you drag the window to another display.
local WINDOW = 180

local lanes = {}
local readout_dt, readout_drift, readout_hint

local samples = {}
local next_sample = 1
local filled = 0

-- Game clock and wall clock, started together. Comparing them is the only
-- honest way to tell a genuinely slow frame from a throttled one.
local sim_time = 0
local wall_start = nil

function init(entity)
    readout_dt = Entity.find_by_name("ReadoutDt")
    readout_drift = Entity.find_by_name("ReadoutDrift")
    readout_hint = Entity.find_by_name("ReadoutHint")
    for _, lane in ipairs(LANES) do
        lanes[lane.name] = Entity.find_by_name(lane.name)
    end
    if Time.get_unix ~= nil then
        wall_start = Time.get_unix()
    end
    log("[scene_95] ready - the bars should glide, not stutter")
end

-- Min / max / mean over the ring buffer, in milliseconds. The *spread*
-- is the number that matters: a tight spread is a smooth frame, and a
-- wide one is judder whatever the average says.
local function stats()
    if filled == 0 then
        return 0, 0, 0
    end
    local lo, hi, sum = math.huge, -math.huge, 0
    for i = 1, filled do
        local v = samples[i]
        if v < lo then lo = v end
        if v > hi then hi = v end
        sum = sum + v
    end
    return lo * 1000, hi * 1000, (sum / filled) * 1000
end

function update(entity, dt)
    sim_time = sim_time + dt

    samples[next_sample] = dt
    next_sample = next_sample + 1
    if next_sample > WINDOW then next_sample = 1 end
    if filled < WINDOW then filled = filled + 1 end

    -- Constant velocity, wrapped. Nothing here is frame-rate dependent,
    -- so every wobble you see came out of `dt`.
    for _, lane in ipairs(LANES) do
        local e = lanes[lane.name]
        if e ~= nil then
            local x, y = Transform.get_position(e)
            x = x + lane.speed * dt
            if x > TRACK_HALF then
                x = x - TRACK_HALF * 2
            end
            Transform.set_position(e, x, y)
        end
    end

    local lo, hi, mean = stats()
    if readout_dt ~= nil then
        Text.set(readout_dt, string.format(
            "dt over the last %d frames:   mean %.2f ms   min %.2f   max %.2f   spread %.2f ms",
            filled, mean, lo, hi, hi - lo))
    end

    if readout_drift ~= nil then
        if wall_start ~= nil then
            local wall = Time.get_unix() - wall_start
            if wall > 0.5 then
                local ratio = sim_time / wall
                local verdict
                if ratio > 0.98 and ratio < 1.02 then
                    verdict = "tracking the wall clock"
                elseif ratio < 0.98 then
                    verdict = "BEHIND the wall clock - frames are being dropped or the display is throttled"
                else
                    verdict = "AHEAD of the wall clock - dt is being over-reported"
                end
                Text.set(readout_drift, string.format(
                    "game clock %.1fs vs wall %.1fs  (%.3fx) - %s",
                    sim_time, wall, ratio, verdict))
            end
        else
            Text.set(readout_drift, "get_unix_time() unavailable in this runtime")
        end
    end

    if readout_hint ~= nil then
        local fps = 0
        if Debug.get_fps ~= nil then fps = Debug.get_fps() end
        Text.set(readout_hint, string.format(
            "reported FPS %.0f - note it can look perfect while the spread above is wide; that is the whole point of this scene",
            fps))
    end

    nav.tick(SCENE_PATH)
end
