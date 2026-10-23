-- 128 Quicksave Workshop: the whole workshop is saved and loaded with
-- `State.save` / `State.load` - every domino, marble and the pendulum put
-- back where they were, moving and spinning as they were. The run's clock
-- and marble count live in the Director's custom `Run` component, so a load
-- rewinds them with everything else; `passes` is a plain local, which a load
-- leaves alone - that is the difference between the two.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/128-quicksave-workshop.scene"
local PEG_X, PEG_Y = -430.0, 150.0
local HOPPER_X, HOPPER_Y = 168.0, 320.0
local SAVE_AT, LOAD_AT, RESET_AT = 4.0, 8.5, 14.0

local me, bob, rope, photo, flash, status
local captions = true
local passes, flash_a, shot = 0, 0, nil
local started = false
local message, message_t = "", 0

local function run_get(field) return Entity.get_custom(me, "Run", field) or 0 end
local function run_set(field, v) Entity.set_custom(me, "Run", field, v) end

local function say(text, flash)
    message, message_t = text, 2.5
    -- A save is photographed in the next frame, so it flashes not at all.
    if flash then flash_a = 0.8 end
end

local function quicksave()
    State.save("quick")
    shot = Screenshot.take({ texture = "__save/quick", scale = 0.2 })
    say(string.format("quicksaved at %.1f s", run_get("t")))
end

local function quickload()
    if State.has("quick") then
        State.load("quick")
        passes = passes + 1
        say("loaded - the same run again", true)
    end
end

function init(entity)
    me = entity
    bob = Entity.find_by_name("Bob")
    rope = Entity.find_by_name("String")
    photo = Entity.find_by_name("Photo")
    flash = Entity.find_by_name("Flash")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if not started then
        -- The whole workshop as it began, once its bodies exist.
        State.save("start")
        started = true
    end
    if Input.is_key_just_pressed("F5") then quicksave() end
    if Input.is_key_just_pressed("F9") then quickload() end
    if Input.is_key_just_pressed("R") then State.load("start"); passes = 0; say("back to the start", true) end

    local t = run_get("t") + dt
    run_set("t", t)
    -- A marble from the hopper every second, eight in all.
    local n = run_get("marbles")
    if n < 8 and t > 0.8 + n * 1.1 then
        local m = Entity.spawn_prefab("WkMarble" .. (n % 4))
        Transform.set_position(m, HOPPER_X, HOPPER_Y)
        Physics.set_velocity(m, 40, 0)
        run_set("marbles", n + 1)
    end
    -- The demonstration: save, load it once, then start over.
    if passes == 0 and t >= SAVE_AT and t - dt < SAVE_AT then quicksave() end
    if passes == 0 and t >= LOAD_AT then quickload() end
    if t >= RESET_AT then
        State.load("start")
        passes = 0
        say("back to the start", true)
    end

    -- The string from the peg to the bob.
    local bx, by = Transform.get_position(bob)
    Line.set(rope, "points", { { PEG_X, PEG_Y }, { bx, by } })
    -- The polaroid shows once its photograph is in (a load puts the photo's
    -- colour back with everything else, so this says it again).
    if shot ~= nil and Screenshot.ready(shot) then Sprite.set_color(photo, 1, 1, 1, 1) end
    flash_a = math.max(0, flash_a - dt * 2.5)
    Sprite.set_color(flash, 1, 1, 1, flash_a)
    message_t = message_t - dt
    UI.set_text(status, message_t > 0 and message or string.format("run %.1f s", t))
end
