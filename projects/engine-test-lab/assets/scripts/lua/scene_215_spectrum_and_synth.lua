-- Scene 215: the mix's spectrum as 24 bars and a level meter
-- (Audio.get_spectrum, Audio.get_level), and notes played by Audio.synth.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/215-spectrum-and-synth.scene"
local BARS = 24
local KEYS = { "A", "S", "D", "F", "G", "H", "J", "K" }
local NOTES = { 60, 62, 64, 65, 67, 69, 71, 72 }
local WAVES = { "sine", "square", "saw", "triangle", "noise" }
local e = {}
local wave = 1
local music = false

function init(entity)
    for i = 0, BARS - 1 do e[i] = Entity.find_by_name("Bar" .. i) end
    e.Level = Entity.find_by_name("Level")
    e.Line0 = Entity.find_by_name("Line0")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local key = Input.is_key_just_pressed
    for i = 1, 5 do
        if key(tostring(i)) then wave = i end
    end
    for i, k in ipairs(KEYS) do
        if key(k) then
            Audio.synth({ wave = WAVES[wave], note = NOTES[i], duration = 0.35, attack = 0.01,
                decay = 0.1, sustain = 0.6, release = 0.4, volume = 0.5 })
        end
    end
    if key("M") then
        music = not music
        if music then Audio.play_music("sounds/lab208_loop.wav", 0.7, true) else Audio.stop_music() end
    end
    local s = Audio.get_spectrum(BARS)
    for i = 0, BARS - 1 do
        Transform.set_scale(e[i], 1, 1 + (s[i + 1] or 0) * 160)
    end
    local level = Audio.get_level()
    Transform.set_scale(e.Level, 1, 1 + level * 300)
    UI.set_text(e.Line0, "wave: " .. WAVES[wave] .. "   music: " .. (music and "on" or "off")
        .. "   level " .. string.format("%.2f", level))
end
