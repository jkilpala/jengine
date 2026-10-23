-- Scene 154: the outlines and shadows are the engine's (`outline` and
-- `shadow` on `text` and `ui_text`); this only navigates.

script = { tick_rate = 60 }

local nav = require("lua/nav")

function update(entity, dt)
    nav.tick("scenes/154-text-outlines.scene")
end
