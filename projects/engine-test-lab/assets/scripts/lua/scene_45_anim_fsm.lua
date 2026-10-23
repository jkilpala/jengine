-- Scene 45 - Animation State Machine
--
-- AnimStateMachine is a component that sits next to SpriteAnimator: the FSM
-- picks the clip, the animator plays it. `tick_animation_state_machines` runs
-- it in all three runtimes, before the animator tick.
--
-- States and transitions are authored in the .scene file. Transitions are
-- evaluated in declaration order and the first whose conditions ALL hold wins,
-- so ordering is how priority is expressed - `wave` is listed before `walk` in
-- the idle state precisely so a wave interrupts starting to move.
--
-- The four condition types:
--   BoolEquals        { param, value }
--   FloatGreaterThan  { param, threshold }
--   FloatLessThan     { param, threshold }
--   ClipFinished      (non-looping clips only - how a one-shot returns)
--
-- The FSM reads no input of its own; scripts push parameters into it, and read
-- the result back with get_anim_state / get_current_animation /
-- is_animation_playing. start_state_machine(e, name) forces a state directly,
-- bypassing the transition table - useful for a reset.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/45-anim-state-machine.scene"

local villager, state_lbl, speed_lbl, readout
local speed = 0.0
local waving = false
local wave_hold = 0.0

function init(entity)
    villager = Entity.find_by_name("Villager")
    state_lbl = Entity.find_by_name("StateLabel")
    speed_lbl = Entity.find_by_name("SpeedLabel")
    readout = Entity.find_by_name("Readout")
    log("[scene_45] ready - Left/Right speed up, W wave, R force idle")
end

function update(entity, dt)
    -- Build a speed parameter from held keys rather than reading it off a
    -- velocity, so the thresholds are easy to drive by hand.
    local moving = Input.is_key_pressed("Left") or Input.is_key_pressed("Right")
        or Input.is_action_pressed("move_left") or Input.is_action_pressed("move_right")
    if moving then
        speed = math.min(1.0, speed + dt * 0.9)
    else
        speed = math.max(0.0, speed - dt * 1.6)
    end

    if Input.is_key_just_pressed("W") then
        waving = true
        wave_hold = 0.25
        -- Triggers are the third parameter type (Bool / Float / Trigger): a
        -- one-shot that the FSM consumes when a transition fires.
        Animation.set_trigger(villager, "wave_once")
    end
    if Input.is_key_just_pressed("R") then
        -- Force a state without waiting for a transition to qualify.
        Animation.start_state_machine(villager, "idle")
        speed, waving, wave_hold = 0.0, false, 0.0
    end
    if wave_hold > 0.0 then
        wave_hold = wave_hold - dt
        if wave_hold <= 0.0 then
            -- Clear the bool so the FSM does not immediately re-enter `wave`
            -- after ClipFinished sends it home.
            waving = false
        end
    end

    if villager ~= nil then
        Animation.set_float(villager, "speed", speed)
        Animation.set_bool(villager, "wave", waving)
    end

    if speed_lbl then
        Text.set(speed_lbl, string.format("speed = %.2f    wave = %s", speed, tostring(waving)))
    end
    if state_lbl then
        -- Read the live state back out of the FSM and the animator, and put
        -- the state the authored thresholds *imply* next to it. Agreement is
        -- the assertion: if they diverge, the transition table and the engine
        -- disagree about what should be playing.
        local expected
        if waving then
            expected = "wave"
        elseif speed > 0.65 then
            expected = "run"
        elseif speed > 0.1 then
            expected = "walk"
        else
            expected = "idle"
        end
        local actual = Animation.get_state(villager)
        local clip = Animation.get_current(villager)
        local playing = Animation.is_playing(villager)
        Text.set(state_lbl, string.format(
            "state: %s   expected: %s   clip: %s%s",
            tostring(actual or "?"), expected, tostring(clip or "?"),
            playing and "" or "  (stopped)"))
    end
    if readout then
        Text.set(readout, "idle -> walk at 0.1, walk -> run at 0.65, wave returns on ClipFinished")
    end

    nav.tick(SCENE_PATH)
end
