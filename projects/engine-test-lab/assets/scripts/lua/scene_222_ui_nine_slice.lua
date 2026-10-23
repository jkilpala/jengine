-- Scene 222 - UI nine-slice
--
-- A `ui_image` with `nine_slice` keeps its corners at `scale` times their
-- texture pixels and stretches only its edges and middle, so one small kit
-- panel (jSuite's "Fantasy wood & gold UI", copied into ui/ninekit/) frames a
-- window of any size. The left panel is the same image stretched whole, for
-- comparison. Everything here is authored; the script only navigates.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/222-ui-nine-slice.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
