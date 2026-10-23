-- Scene 208: the music director and sound ids. Crossfades, stems, a
-- playlist, pause, seek and pitch (Audio.*), and a looping sound reached by
-- the id Audio.play_sound returned.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/208-music-director.scene"
local STEMS = { "sounds/lab208_stem1.wav", "sounds/lab208_stem2.wav", "sounds/lab208_stem3.wav" }
local lines = {}
local stems = { 1, 1, 1 }
local loop = nil
local loop_pitch = 1.0
local music_pitch = 1.0
local paused = false
local last = "-"

function init(entity)
    for k = 0, 7 do lines[k + 1] = Entity.find_by_name("Line" .. k) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local key = Input.is_key_just_pressed
    if key("1") then Audio.play_music("sounds/lab208_a.wav"); last = "play A" end
    if key("2") then Audio.crossfade_music("sounds/lab208_b.wav", 2); last = "crossfade to B" end
    if key("3") then
        stems = { 1, 1, 1 }
        Audio.play_music_layers(STEMS, stems, 1); last = "stems"
    end
    for i, k in ipairs({ "4", "5", "6" }) do
        if key(k) then
            stems[i] = stems[i] > 0 and 0 or 1
            Audio.set_music_layer(i, stems[i], 0.75); last = "stem " .. i
        end
    end
    if key("7") then
        Audio.set_playlist({ "sounds/lab208_a.wav", "sounds/lab208_b.wav" }, { crossfade = 1.5, loop = true })
        last = "playlist"
    end
    if key("N") then Audio.next_track(); last = "next track" end
    if key("P") then
        paused = not paused
        if paused then Audio.pause_music() else Audio.resume_music() end
        last = paused and "pause" or "resume"
    end
    if key("M") then Audio.seek_music(2); last = "seek 2 s" end
    if key("Minus") then music_pitch = math.max(0.5, music_pitch - 0.1); Audio.set_music_pitch(music_pitch) end
    if key("Equal") then music_pitch = math.min(2.0, music_pitch + 0.1); Audio.set_music_pitch(music_pitch) end
    if key("L") then
        if loop then Audio.stop_sound(loop); loop = nil else loop = Audio.play_sound("sounds/lab208_loop.wav", 0.4, loop_pitch, 0, true) end
    end
    if loop and key("Up") then loop_pitch = math.min(3, loop_pitch + 0.1); Audio.set_sound_pitch(loop, loop_pitch) end
    if loop and key("Down") then loop_pitch = math.max(0.3, loop_pitch - 0.1); Audio.set_sound_pitch(loop, loop_pitch) end
    if key("S") then Audio.stop_all(); loop = nil; last = "stop all" end

    UI.set_text(lines[1], "last: " .. last .. "    music: " .. (Audio.get_music_clip() ~= "" and Audio.get_music_clip() or "none")
        .. string.format("    at %.2f s", Audio.get_music_position()))
    UI.set_text(lines[2], string.format("stems heading to  %.1f  %.1f  %.1f", Audio.get_music_layer(1),
        Audio.get_music_layer(2), Audio.get_music_layer(3)))
    UI.set_text(lines[3], string.format("music pitch %.1f    paused %s", music_pitch, tostring(paused)))
    UI.set_text(lines[4], loop and string.format("loop sound id %d at pitch %.1f", loop, loop_pitch) or "no loop sound")
end
