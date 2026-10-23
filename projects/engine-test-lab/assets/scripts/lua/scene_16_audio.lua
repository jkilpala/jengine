-- Scene 16 - Audio.
--
-- The scene file declares `audio_refs: ["sounds/blip.wav",
-- "sounds/hum.wav"]` at its top level. WgpuSceneRunner reads each
-- referenced file and inserts the raw bytes into the AssetManager
-- audio cache before this script first ticks; the kira backend
-- decodes lazily on first play.
--
-- Two clips:
--   sounds/blip.wav - 0.12s 880Hz sine with attack/release envelope
--                     (one-shot SFX feel)
--   sounds/hum.wav  - 1.0s 220Hz sine, looped as background music
--
-- Controls:
--   SPACE     play one-shot blip (no overlap dedupe - spam away)
--   M         start hum as looping music
--   P / R     pause / resume music
--   S         stop music
--   1..9      master volume = N/10
--   0         master volume = 1.0
--   , / .     pan the next blip left / right (4th arg of play_sound)
--   /         re-centre the pan
--   H / Esc   hub
--   [ / ]     prev / next scene

script = { tick_rate = 60, events = { "on_despawn" } }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/16-audio.scene"

local BLIP = "sounds/blip.wav"
local HUM  = "sounds/hum.wav"

local status_id, status2_id, status3_id
local last_action  = "(none)"
local music_state  = "stopped"
local master_vol   = 1.0
local blip_count   = 0
-- Stereo pan for one-shots: -1.0 hard left, 0.0 centre, +1.0 hard right.
-- `pan` is the optional 4th argument of play_sound and defaults to centre, so
-- every older call site keeps its behaviour.
local blip_pan     = 0.0

local prev = {}
local function edge(key)
    local now = Input.is_key_pressed(key)
    local pressed = now and not prev[key]
    prev[key] = now
    return pressed
end

local function refresh()
    if status_id and status_id ~= 0 then
        Text.set(status_id, "last action: " .. last_action)
    end
    if status2_id and status2_id ~= 0 then
        Text.set(status2_id, string.format(
            "music: %s    master volume: %.2f", music_state, master_vol))
    end
    if status3_id and status3_id ~= 0 then
        Text.set(status3_id, string.format(
            "blip plays: %d    pan: %+.2f  (%s)",
            blip_count, blip_pan,
            blip_pan < -0.05 and "left" or (blip_pan > 0.05 and "right" or "centre")))
    end
end

local function play_blip()
    Audio.play_sound(BLIP, 0.9, 1.0, blip_pan)
    blip_count = blip_count + 1
    last_action = string.format("play_sound(blip, 0.9, 1.0, pan=%+.2f)", blip_pan)
end

local function nudge_pan(delta)
    blip_pan = math.max(-1.0, math.min(1.0, blip_pan + delta))
    last_action = string.format("pan = %+.2f", blip_pan)
    play_blip()
end

local function start_music()
    Audio.play_music(HUM, 0.4, true)
    music_state = "playing"
    last_action = "play_music(hum, looping)"
end

local function set_master(v)
    master_vol = v
    Audio.set_master_volume(v)
    last_action = string.format("set_master_volume(%.2f)", v)
end

local ready_logged = false

function update(entity, dt)
    if status_id == nil then
        status_id  = Entity.find_by_name("Status")
        status2_id = Entity.find_by_name("Status2")
        status3_id = Entity.find_by_name("Status3")
        refresh()
    end

    if not ready_logged then
        log("[scene_16] ready - SPACE blip, M hum, P/R/S music control, 0-9 volume")
        ready_logged = true
    end

    local dirty = false

    if edge("Space") then play_blip(); dirty = true end
    if edge("M")     then start_music(); dirty = true end
    if edge("P")     then Audio.pause_music();  music_state = "paused";  last_action = "pause_music()";  dirty = true end
    if edge("R")     then Audio.resume_music(); music_state = "playing"; last_action = "resume_music()"; dirty = true end
    if edge("S")     then Audio.stop_music();   music_state = "stopped"; last_action = "stop_music()";   dirty = true end

    for i = 1, 9 do
        if edge(tostring(i)) then set_master(i / 10.0); dirty = true end
    end
    if edge("0") then set_master(1.0); dirty = true end

    -- The two remaining global controls: music gain independent of master,
    -- and a hard stop for every one-shot currently playing.
    if edge("N") then
        Audio.set_music_volume(0.15)
        last_action = "set_music_volume(0.15)"
        dirty = true
    end
    if edge("B") then
        Audio.stop_all()
        last_action = "stop_all_sounds()"
        dirty = true
    end

    -- Panning: each press moves the pan and immediately plays a blip there,
    -- so the change is audible rather than just a number on screen.
    if edge("Comma")  then nudge_pan(-0.25); dirty = true end
    if edge("Period") then nudge_pan(0.25);  dirty = true end
    if edge("Slash")  then blip_pan = 0.0; play_blip(); dirty = true end

    if dirty then refresh() end

    nav.tick(SCENE_PATH)
end

-- The teardown hook. Music is owned by the audio backend, not the world, so it
-- survives the scene that started it — leaving here without this is how a hum
-- follows you through every other scene. `on_despawn` fires on entity removal
-- and on a scene transition, before the world is replaced.
function on_despawn(entity)
    Audio.stop_music()
    Audio.stop_all()
end
