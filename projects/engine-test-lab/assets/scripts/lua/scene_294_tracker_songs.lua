-- Scene 294: tracker songs. A .song is chip music as notes, which
-- jengine-tracker renders to PCM when the scene loads - after that it is an
-- ordinary clip, so the music director's calls work on it unchanged. Each
-- looping song here has an intro, so its playhead must go back to its loop
-- point, not to 0.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/294-tracker-songs.scene"
local SUNNY = "sounds/lab294_sunny_fields.song"
local CAVERN = "sounds/lab294_deep_cavern.song"
local JINGLE = "sounds/lab294_victory.song"
local LOOPS = { [SUNNY] = { 48.0, 6.86 }, [CAVERN] = { 50.0, 10.0 } }
local lines = {}
local last = "-"
local laps = 0
local prev = 0

function init(entity)
    for k = 0, 3 do lines[k + 1] = Entity.find_by_name("Line" .. k) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local key = Input.is_key_just_pressed
    if key("1") then Audio.play_music(SUNNY); last = "play Sunny Fields"; laps = 0 end
    if key("2") then Audio.crossfade_music(CAVERN, 2); last = "crossfade to Deep Cavern"; laps = 0 end
    if key("3") then Audio.play_sound(JINGLE, 0.8); last = "victory jingle" end
    if key("S") then Audio.stop_all(); last = "stop all" end

    local clip = Audio.get_music_clip()
    local at = Audio.get_music_position()
    -- The playhead jumping back is a loop; where it lands is the loop point.
    if at + 1 < prev then laps = laps + 1 end
    prev = at
    local loop = LOOPS[clip]

    UI.set_text(lines[1], "last: " .. last .. "    music: " .. (clip ~= "" and clip or "none")
        .. string.format("    at %.2f s", at))
    UI.set_text(lines[2], loop and string.format("this song is %.2f s long and loops back to %.2f s", loop[1], loop[2])
        or "no song playing")
    UI.set_text(lines[3], string.format("loops heard: %d", laps))
end
