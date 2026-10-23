-- Scene 162: the fading and panning are the engine's (`spatial` on the
-- audio sources, the square's `audio_listener`); this moves the listener and
-- plays a blip where you click.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/162-positional-audio.scene"
local listener

function init(entity)
    listener = Entity.find_by_name("Listener")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local x, y = Input.get_mouse_world_position()
    if listener then Transform.set_position(listener, x, y) end
    if Input.is_mouse_left_just_pressed() then
        Audio.play_sound_at("sounds/blip.wav", x, y)
    end
end
