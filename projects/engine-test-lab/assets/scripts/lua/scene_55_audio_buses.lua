-- Scene 55 - Audio Buses
--
-- The mixer is a tree of named buses. "master" is the root, one-shots default
-- to "sfx" and music to "music", and a bus's effective volume is its own
-- multiplied down the chain to master.
--
--   Audio.create_bus(name, parent, volume)   add a child bus
--   Audio.remove_bus(name)                   drop it
--   Audio.set_bus_volume(name, v)            set gain
--   Audio.set_bus_muted(name, bool)          mute
--   Audio.set_bus_solo(name, bool)           solo (silences siblings)
--   Audio.get_bus_volume(name) / Audio.is_bus_muted(name)
--
-- Scene 16 covers playback and the master volume; this one is the mixer above
-- it, plus stopping sounds.
--
-- VOLUME CHECK (BUGS #54): a sound plays at its own volume x its bus. Press M
-- (music at 0.5), then 3 a few times: the hum must get QUIETER each press, in
-- every runtime. Natively it used to jump to the bare bus volume - louder - and
-- in the editor and on web it did not change at all until the next play_music.
-- Then W (mute music) and V (set_music_volume): the hum must stay silent until
-- W unmutes it, and come back at V's volume.
--
-- NOTE ON HANDLES: `play_sound` is a *deferred* command - it is queued and runs
-- after the script returns - so it cannot hand back a handle, and the per-handle
-- calls (`stop_sound`, `set_sound_volume`) have nothing real to address. Stop a
-- one-shot with `Audio.stop_sound_by_clip(clip_id)` instead, which names the clip and
-- stops every live instance of it. C does that below.

script = { tick_rate = 30, events = { "on_despawn" } }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/55-audio-buses.scene"

local BLIP = "sounds/blip.wav"
local HUM = "sounds/hum.wav"

local sfx_vol, music_vol = 1.0, 1.0
local track_vol = 0.5
local sfx_muted, music_muted, sfx_solo = false, false, false
local custom_bus = false
local last_handle = nil
local last = "-"

local function fmt(v)
    return string.format("%.2f", tonumber(v) or 0)
end

function init(entity)
    log("[scene_55] ready - Space blip, M music, V music volume, C stop blips, 1/2 sfx vol, 3/4 music vol, Q/W mute, E solo, N bus")
end

function update(entity, dt)
    if Input.is_key_just_pressed("Space") then
        -- The return value is not a usable handle (see the note above); it is
        -- kept only so the X / Z keys can show what happens when you try.
        last_handle = Audio.play_sound(BLIP, 0.9, 1.0, 0.0)
        last = "play_sound(blip)"
    elseif Input.is_key_just_pressed("M") then
        Audio.play_music(HUM, 0.5, true)
        track_vol = 0.5
        last = "play_music(hum, looping)"
    elseif Input.is_key_just_pressed("V") then
        -- The music's own volume; the bus still applies on top of it.
        track_vol = (track_vol == 0.5) and 0.2 or 0.5
        Audio.set_music_volume(track_vol)
        last = "set_music_volume(" .. fmt(track_vol) .. ") - still under the music bus"
    elseif Input.is_key_just_pressed("1") then
        sfx_vol = math.max(0.0, sfx_vol - 0.2)
        Audio.set_bus_volume("sfx", sfx_vol); last = "sfx bus volume " .. fmt(sfx_vol)
    elseif Input.is_key_just_pressed("2") then
        sfx_vol = math.min(1.0, sfx_vol + 0.2)
        Audio.set_bus_volume("sfx", sfx_vol); last = "sfx bus volume " .. fmt(sfx_vol)
    elseif Input.is_key_just_pressed("3") then
        music_vol = math.max(0.0, music_vol - 0.2)
        Audio.set_bus_volume("music", music_vol); last = "music bus volume " .. fmt(music_vol)
    elseif Input.is_key_just_pressed("4") then
        music_vol = math.min(1.0, music_vol + 0.2)
        Audio.set_bus_volume("music", music_vol); last = "music bus volume " .. fmt(music_vol)
    elseif Input.is_key_just_pressed("Q") then
        sfx_muted = not sfx_muted
        Audio.set_bus_muted("sfx", sfx_muted); last = "sfx muted = " .. tostring(sfx_muted)
    elseif Input.is_key_just_pressed("W") then
        music_muted = not music_muted
        Audio.set_bus_muted("music", music_muted); last = "music muted = " .. tostring(music_muted)
    elseif Input.is_key_just_pressed("E") then
        sfx_solo = not sfx_solo
        Audio.set_bus_solo("sfx", sfx_solo); last = "sfx solo = " .. tostring(sfx_solo)
    elseif Input.is_key_just_pressed("N") then
        if custom_bus then
            Audio.remove_bus("ui")
            custom_bus = false
            last = "audio_remove_bus(\"ui\")"
        else
            Audio.create_bus("ui", "master", 0.6)
            custom_bus = true
            last = "audio_create_bus(\"ui\", parent=master, 0.6)"
        end
    elseif Input.is_key_just_pressed("C") then
        -- The one that actually works: stops every live instance of the clip.
        Audio.stop_sound_by_clip(BLIP)
        last = "stop_sound_by_clip(blip)"
    elseif Input.is_key_just_pressed("X") then
        if last_handle ~= nil then
            Audio.stop_sound(last_handle)
            last = "stop_sound(handle) - no-op, play_sound returns no handle"
        end
    elseif Input.is_key_just_pressed("Z") then
        if last_handle ~= nil then
            Audio.set_sound_volume(last_handle, 0.2)
            last = "set_sound_volume(handle, 0.2)"
        end
    end

    local sl = Entity.find_by_name("SfxLabel")
    if sl then
        Text.set(sl, string.format("sfx    volume %s  muted %s  solo %s",
            fmt(Audio.get_bus_volume("sfx")), tostring(Audio.is_bus_muted("sfx")),
            tostring(sfx_solo)))
    end
    local ml = Entity.find_by_name("MusicLabel")
    if ml then
        Text.set(ml, string.format("music  volume %s  muted %s",
            fmt(Audio.get_bus_volume("music")), tostring(Audio.is_bus_muted("music"))))
    end
    local mal = Entity.find_by_name("MasterLabel")
    if mal then
        Text.set(mal, string.format("master volume %s  muted %s",
            fmt(Audio.get_bus_volume("master")), tostring(Audio.is_bus_muted("master"))))
    end
    local el = Entity.find_by_name("ExtraLabel")
    if el then
        Text.set(el, custom_bus
            and string.format("custom bus 'ui' -> master, volume %s", fmt(Audio.get_bus_volume("ui")))
            or "custom bus: none  (N creates 'ui')")
    end
    local r = Entity.find_by_name("Readout")
    if r then Text.set(r, "last: " .. last) end

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
