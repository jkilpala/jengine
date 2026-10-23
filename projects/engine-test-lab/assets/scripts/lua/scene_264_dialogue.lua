-- Scene 264: `dialogue`. A conversation from `dialogue/lab_chat.data`,
-- played into the text above: Space advances (finishing a line still typing
-- first), 1-3 choose, R starts it again. Its own variables count the picks.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/264-dialogue.scene"
local line
local ended = false

function init(entity)
    line = Entity.find_by_name("Line0")
end

function on_finished(entity)
    ended = true
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then Dialogue.advance(entity) end
    for n = 1, 3 do
        if Input.is_key_just_pressed(tostring(n)) then Dialogue.choose(entity, n) end
    end
    if Input.is_key_just_pressed("R") then
        Entity.set_custom(entity, "DialogueVars", "picks", 0)
        ended = false
        Dialogue.start(entity)
    end
    local function f(k) return tostring(Entity.get_custom(entity, "Dialogue", k)) end
    UI.set_text(line, "node " .. f("node") .. "   playing " .. f("playing") .. "   typing " .. f("typing")
        .. "   choices " .. f("choices") .. (ended and "   finished" or ""))
end
