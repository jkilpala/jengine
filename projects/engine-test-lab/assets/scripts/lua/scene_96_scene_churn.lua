-- Scene 96 - what survives a scene change.
--
-- Replacing the world does not touch state the world does not own, and this
-- repo has lost that argument three times: the post-process chain and looping
-- music both followed the player into the next scene (BUGS #84), and script
-- **timers, event listeners and GUI click handlers** survived every scene
-- change in every runtime for the life of the engine (BUGS #149).
--
-- None of it was noticed, because until a microgame compilation every game here loaded
-- one scene and stayed in it - a strand had nowhere to land. This scene gives
-- it somewhere: it reloads *itself*, twenty-five times, and counts what the
-- last copy left behind.
--
-- # The probe, and why it counts rather than detects
--
-- Every copy of this scene does exactly two things: registers ONE listener for
-- `churn_ping`, and sends ONE ping. So a correct runtime delivers **exactly
-- one** callback, in every generation, forever.
--
-- If listeners strand, generation N hears N of them - because all N-1 previous
-- registrations are still in the map, pointing at entity ids this scene has
-- since reused. That turns a yes/no into a number, and the number is the
-- accumulation: it is what "a hundred swaps left two hundred listeners" looks
-- like from inside.
--
-- The timer probe is the simpler half. Each copy arms a three-second timer and
-- reloads after 0.6s, so `on_stray_timer` can only ever fire if a *previous*
-- copy's timer outlived it. One firing is a bug; the count says how bad.
--
-- # Where the counters live
--
-- The save store, deliberately - it is the one thing that is *supposed* to
-- survive a swap, and this scene would have nowhere to keep a tally otherwise.
-- That is the same reason a compilation keeps its run there.
--
-- # What you should see
--
--   native, editor and web alike:  pings 1, strays 0.
--
-- All three were wrong when this scene was written, and in different ways -
-- which is the argument for the scene existing. Native stranded both (BUGS
-- #149). The editor stranded both *and* heard nothing at all, because the
-- browser hosts called a differently-named handler (BUGS #150) - so its first
-- run read "pings 0, strays 48", two bugs stacked into one reading that looked
-- like neither.
--
-- A divergence between the three columns here is the bug, not a broken scene.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/96-scene-churn.scene"

local SLOT = "testlab"
local TARGET = 25
-- Long enough for the probe to complete, short enough that twenty-five of them
-- is a quarter of a minute rather than a coffee break.
local DWELL = 0.6
-- Well past DWELL, so a timer that fires at all proves it outlived its scene.
local STRAY_AFTER = 3.0

-- Everything a fresh measurement must start from zero. One list, used by both
-- [C] and [R], so the two cannot drift apart - which is exactly how [C] came
-- to reset the loads and not the accumulators.
local RESET_KEYS = {
    "churn_loads", "churn_worst_pings", "churn_strays", "churn_gap_total",
    "churn_gap_worst", "churn_last_pings", "churn_at", "churn_left_at",
}

local runner, ui = nil, {}
local clock = 0
local pings = 0
local registered, pinged, reloaded = false, false, false

local function get(key, fallback)
    local v = Save.get(SLOT, key)
    if v == nil then
        return fallback
    end
    return v
end

local function put(key, value)
    Save.set(SLOT, key, value)
end

-- The listener's callback. Named at the top level, which is where the engine
-- looks for it.
function on_churn_ping(entity)
    pings = pings + 1
end

-- The timer's. Reaching this at all means a previous copy's timer survived,
-- because no copy lives long enough to fire its own.
function on_stray_timer(entity)
    put("churn_strays", get("churn_strays", 0) + 1)
    Save.commit(SLOT)
end

function init(entity)
    runner = Entity.find_by_name("Runner")
    for _, n in ipairs({ "Readout", "Verdict", "Detail", "Cost" }) do
        ui[n] = Entity.find_by_name(n)
    end
    -- Wall-clock, because frames do not run during a swap and a frame counter
    -- therefore cannot see it. `churn_left_at` is stamped immediately before
    -- `load_scene`, so this measures the swap *itself* - the load, the asset
    -- resolution and the first tick - rather than the swap plus however long
    -- this copy chose to sit there first.
    local now = Time.get_unix()
    local last = get("churn_left_at", 0)
    if get("churn_loads", 0) > 0 and last > 0 then
        local gap = now - last
        put("churn_gap_total", get("churn_gap_total", 0) + gap)
        if gap > get("churn_gap_worst", 0) then
            put("churn_gap_worst", gap)
        end
    end
    put("churn_at", now)
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    clock = clock + dt

    -- The probe, in order. Registering and pinging on separate frames because
    -- both are deferred: sending in the same frame as registering would race
    -- the command queue and measure that instead.
    local churning_now = get("churn_active", 0) > 0
    if not registered and clock > 0.08 then
        registered = true
        Event.on("churn_ping", runner)
        -- Only while churning. The *last* copy is never replaced, so it lives
        -- long enough to fire its own timer - which would be counted as a
        -- strand and is nothing of the sort. The probe only means anything
        -- for a copy that is about to be destroyed.
        if churning_now then
            Timer.after(runner, STRAY_AFTER, "on_stray_timer")
        end
    elseif registered and not pinged and clock > 0.20 then
        pinged = true
        Event.send("churn_ping", runner, "")
    end

    local loads = get("churn_loads", 0)
    local worst = get("churn_worst_pings", 0)
    local strays = get("churn_strays", 0)
    local churning = get("churn_active", 0) > 0

    -- Record this generation's result once the ping has had time to land.
    if pinged and clock > 0.35 and not reloaded then
        if pings > worst then
            worst = pings
            put("churn_worst_pings", worst)
        end
        put("churn_last_pings", pings)
    end

    if ui.Readout ~= nil then
        Text.set(ui.Readout, string.format(
            "LOADS %d   PINGS HEARD THIS COPY %d   WORST %d   STRAY TIMERS %d",
            loads, pings, worst, strays))
    end
    if ui.Verdict ~= nil then
        if loads == 0 then
            Text.set(ui.Verdict, "PRESS [C] TO CHURN")
            Text.set_color(ui.Verdict, 0.8, 0.84, 0.92, 1)
        elseif worst <= 1 and strays == 0 then
            Text.set(ui.Verdict, "NOTHING SURVIVED THE SWAP")
            Text.set_color(ui.Verdict, 0.6, 1.0, 0.6, 1)
        else
            Text.set(ui.Verdict, "STATE IS OUTLIVING ITS SCENE")
            Text.set_color(ui.Verdict, 1.0, 0.55, 0.5, 1)
        end
    end
    if ui.Detail ~= nil then
        if loads == 0 then
            Text.set(ui.Detail, "")
        elseif worst <= 1 and strays == 0 then
            Text.set(ui.Detail, string.format(
                "One listener registered per copy, one ping sent, one heard - across %d loads.", loads))
        else
            Text.set(ui.Detail, string.format(
                "One copy heard %d pings. Every one is a dead registration from a scene that no longer exists.",
                worst))
        end
    end
    if ui.Cost ~= nil and loads > 1 then
        -- The other half of the archetype's claim: a hitch between microgames
        -- is fatal to a format whose appeal is rhythm.
        local avg = get("churn_gap_total", 0) / math.max(loads - 1, 1)
        Text.set(ui.Cost, string.format(
            "SWAP COST  avg %.0f ms   worst %.0f ms   (a microgame is ~4000 ms)",
            avg * 1000.0, get("churn_gap_worst", 0) * 1000.0))
    end

    if Input.is_key_just_pressed("R") then
        for _, k in ipairs(RESET_KEYS) do
            put(k, 0)
        end
        put("churn_active", 0)
        Save.commit(SLOT)
        Scene.load(SCENE_PATH)
        return
    end
    if Input.is_key_just_pressed("C") and not churning then
        -- **Every** counter, not just the loads. Resetting the divisor while
        -- leaving the accumulators is how a run reported an average swap cost
        -- of 60 ms against a worst of 43 - an impossibility that said plainly
        -- the numbers were from two different runs added together.
        for _, k in ipairs(RESET_KEYS) do
            put(k, 0)
        end
        put("churn_active", 1)
        Save.commit(SLOT)
    end

    -- Churn. Stops at TARGET so the scene can be read afterwards - a scene
    -- that reloaded forever could not be left.
    if churning and clock > DWELL and not reloaded then
        reloaded = true
        put("churn_left_at", Time.get_unix())
        if loads + 1 >= TARGET then
            put("churn_active", 0)
        end
        put("churn_loads", loads + 1)
        Save.commit(SLOT)
        Scene.load(SCENE_PATH)
    end
end
