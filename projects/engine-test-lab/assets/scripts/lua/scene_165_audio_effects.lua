-- Scene 165: the effects are the engine's - the boxes are `audio_zone`s, and
-- L / R set the sfx bus's own with Audio.set_bus_effect. This moves the
-- listener, toggles, and plays a blip where you click.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/165-audio-effects.scene"
local listener, readout, cave, water
local lowpass, reverb, zones = false, false, true

function init(entity)
    listener = Entity.find_by_name("Listener")
    readout = Entity.find_by_name("Readout")
    cave = Entity.find_by_name("Cave")
    water = Entity.find_by_name("Water")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local x, y = Input.get_mouse_world_position()
    if listener then Transform.set_position(listener, x, y) end
    if Input.is_key_just_pressed("L") then
        lowpass = not lowpass
        Audio.set_bus_effect("sfx", "lowpass", lowpass and 700 or 0)
    end
    if Input.is_key_just_pressed("R") then
        reverb = not reverb
        Audio.set_bus_effect("sfx", "reverb", reverb and 0.6 or 0)
        Audio.set_bus_effect("sfx", "room", 0.8)
    end
    if Input.is_key_just_pressed("E") then
        zones = not zones
        Transform.set_position(cave, zones and -280 or -5000, -10)
        Transform.set_position(water, zones and 280 or 5000, -10)
    end
    if Input.is_mouse_left_just_pressed() then
        Audio.play_sound_at("sounds/blip.wav", x, y)
    end
    Text.set(readout, "script low-pass " .. (lowpass and "700 Hz" or "off") .. "   script reverb " .. (reverb and "0.6" or "off")
        .. "   zones " .. (zones and "on" or "off"))
end
