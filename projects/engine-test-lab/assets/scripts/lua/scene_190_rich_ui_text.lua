-- Scene 190: entity-UI text and .gui Text nodes read the same markup as
-- world text when "rich" is on; a ui_text with reveal_speed is a typewriter.
-- Setting its text again (here, a space appended and taken off) restarts it.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/190-rich-ui-text.scene"
local LETTER = "A typewriter[pause=0.6] - it waits at each [color=gold]pause[/color],\nthen writes on.[pause=0.4] [icon=sprites/shape_crown.png] Space starts it again."
local letter, status
local flip, restarts = false, 0

function init(entity)
    letter = Entity.find_by_name("Letter")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") and letter then
        -- A different string restarts the typewriter.
        flip = not flip
        restarts = restarts + 1
        UI.set_text(letter, flip and (LETTER .. " ") or LETTER)
    end
    if status then
        UI.set_text(status, "typewriter restarts: " .. restarts)
    end
end
