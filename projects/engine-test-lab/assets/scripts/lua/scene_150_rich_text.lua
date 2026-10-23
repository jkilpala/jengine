-- Scene 150: the markup and the typewriter are the engine's (`"rich": true`,
-- `reveal_speed`). This restarts the typewriter every few seconds by
-- setting the other of two strings - setting the same text again would
-- not, since the reveal starts over only when the text changes.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/150-rich-text.scene"
local LINES = {
    "The typewriter waits here,[pause=0.8] and here,[pause=0.8] then [b]finishes[/b].",
    "The typewriter waits here,[pause=0.8] and here,[pause=0.8] then [b]finishes[/b]. ",
}
local typer
local t, which = 0, 1

function init(entity)
    typer = Entity.find_by_name("Typewriter")
    if typer then Text.set(typer, LINES[which]) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if t > 7 and typer then
        t = 0
        which = which % 2 + 1
        Text.set(typer, LINES[which])
    end
end
