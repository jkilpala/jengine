-- Scene 48 - Timers & Events
--
-- Two script services with no component of their own:
--
--   Timer.after(e, delay, "cb")     one-shot, returns an id
--   Timer.every(e, period, "cb") repeating, returns an id
--   Timer.cancel(id)              stops either kind
--
--   Event.on(name, e)                subscribe this entity
--   Event.send(name, sender, data)   broadcast; `data` is an optional string
--   Event.off(name, e)   unsubscribe
--
-- Callbacks are plain globals in the script's own environment (each script
-- gets its own env, so the names do not collide between scenes) and receive
-- the entity that registered them.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/48-timers-events.scene"

local self_entity
local timer_id, interval_id
local one_shot_state = "idle"
local ticks = 0
local events = 0
local last_payload = "-"
local pulse_t = 0.0
local pulse_flash = 0.0

-- Callbacks must be global for the timer/event dispatcher to find them.
function on_one_shot(entity)
    one_shot_state = "fired"
    timer_id = nil
    pulse_flash = 0.35
end

function on_interval_tick(entity)
    ticks = ticks + 1
    pulse_flash = 0.2
end

function on_demo_event(entity, data)
    events = events + 1
    last_payload = tostring(data or "(no payload)")
    pulse_flash = 0.5
end

function init(entity)
    self_entity = entity
    -- The handler for `demo_event` is looked up by convention: on_<event_name>.
    Event.on("demo_event", entity)
    -- register_event_listener is the longhand of on_event; both end up in the
    -- same listener table, which is why re-registering is harmless.
    Event.on("demo_event", entity)
    log("[scene_48] ready - T one-shot, I interval, E broadcast, D direct, R reset")
end

function update(entity, dt)
    pulse_t = pulse_t + dt
    if pulse_flash > 0.0 then pulse_flash = math.max(0.0, pulse_flash - dt) end

    if Input.is_key_just_pressed("T") then
        one_shot_state = "armed (1.5s)"
        timer_id = Timer.after(self_entity, 1.5, "on_one_shot")
    elseif Input.is_key_just_pressed("I") then
        if interval_id ~= nil then
            Timer.cancel(interval_id)
            interval_id = nil
        else
            interval_id = Timer.every(self_entity, 0.5, "on_interval_tick")
        end
    elseif Input.is_key_just_pressed("E") then
        Event.send("demo_event", self_entity, "payload #" .. (events + 1))
    elseif Input.is_key_just_pressed("D") then
        -- send_event_to targets one entity instead of broadcasting, so a
        -- listener on a different entity does not see it.
        Event.send_to(self_entity, "demo_event", self_entity, "direct #" .. (events + 1))
    elseif Input.is_key_just_pressed("R") then
        if timer_id ~= nil then Timer.cancel(timer_id); timer_id = nil end
        if interval_id ~= nil then Timer.cancel(interval_id); interval_id = nil end
        one_shot_state, ticks, events, last_payload = "idle", 0, 0, "-"
    end

    local t = Entity.find_by_name("TimerLabel")
    if t then Text.set(t, "one-shot (T): " .. one_shot_state) end
    local i = Entity.find_by_name("IntervalLabel")
    if i then
        Text.set(i, string.format("interval (I): %s   ticks: %d",
            interval_id and "running @0.5s" or "stopped", ticks))
    end
    local ev = Entity.find_by_name("EventLabel")
    if ev then Text.set(ev, "events received (E): " .. events) end
    local pl = Entity.find_by_name("PayloadLabel")
    if pl then Text.set(pl, "last payload: " .. last_payload) end

    -- The pulse reacts to callbacks so a fired timer is visible, not just counted.
    local pulse = Entity.find_by_name("Pulse")
    if pulse then
        local base = 0.35 + 0.15 * math.sin(pulse_t * 2.0)
        local lit = math.min(1.0, base + pulse_flash * 2.0)
        Sprite.set_color(pulse, lit * 0.5, lit, lit, 1.0)
    end

    local r = Entity.find_by_name("Readout")
    if r then
        Text.set(r, "callbacks are named globals in this script's env, called with (entity)")
    end

    nav.tick(SCENE_PATH)
end
