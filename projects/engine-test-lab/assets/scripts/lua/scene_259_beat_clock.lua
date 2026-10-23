-- Scene 259: `beat_clock`. A metronome: the clock on this entity calls
-- on_beat and on_bar, and its custom BeatClock fields say where it is.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/259-beat-clock.scene"
local boxes, pulse, line, me
local bpm, per_bar, running = 90, 4, true
local beats, bars, flash = 0, 0, 0

function on_beat(entity)
    beats = beats + 1
end

function on_bar(entity)
    bars = bars + 1
    flash = 1
end

function init(entity)
    me = entity
    boxes = {}
    for k = 0, 3 do boxes[#boxes + 1] = Entity.find_by_name("Box" .. k) end
    pulse = Entity.find_by_name("Pulse")
    line = Entity.find_by_name("Line0")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Up") then bpm = math.min(240, bpm + 10); BeatClock.set(me, "bpm", bpm) end
    if Input.is_key_just_pressed("Down") then bpm = math.max(30, bpm - 10); BeatClock.set(me, "bpm", bpm) end
    if Input.is_key_just_pressed("3") then per_bar = 3; BeatClock.set(me, "beats_per_bar", 3) end
    if Input.is_key_just_pressed("4") then per_bar = 4; BeatClock.set(me, "beats_per_bar", 4) end
    if Input.is_key_just_pressed("P") then running = not running; BeatClock.set(me, "running", running) end
    local phase = Entity.get_custom(me, "BeatClock", "phase") or 0
    local in_bar = Entity.get_custom(me, "BeatClock", "beat_in_bar") or 0
    flash = math.max(0, flash - dt * 2)
    for k, b in ipairs(boxes) do
        local on = (k - 1) == in_bar and (k - 1) < per_bar
        if k > per_bar then
            Sprite.set_color(b, 0.08, 0.08, 0.1, 1)
        elseif on and k == 1 then
            Sprite.set_color(b, 1.0, 0.75 + 0.2 * flash, 0.2, 1)
        elseif on then
            Sprite.set_color(b, 0.3, 0.75, 1.0, 1)
        else
            Sprite.set_color(b, 0.2, 0.22, 0.3, 1)
        end
    end
    Transform.set_scale(pulse, 1 - phase, 1)
    UI.set_text(line, string.format("%d bpm   %d to the bar   on_beat %d   on_bar %d   %s", bpm, per_bar, beats, bars,
        running and "running" or "stopped"))
end
