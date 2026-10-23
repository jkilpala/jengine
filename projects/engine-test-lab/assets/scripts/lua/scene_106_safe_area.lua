-- Scene 106: the safe area - where a notch, camera hole or rounded corner
-- covers the screen (`ui::scaling::SafeInsets`).
--
-- Nothing here is scripted. The four corner panels are entity UI anchored to
-- the screen's corners, the stick and buttons are a `touch_controls` layout,
-- and the orange bars are world sprites at the authored edges (x = -640 and
-- +640) on a backdrop wider than any screen.
--
-- In the editor's Game view, pick a phone in the device menu: the corner
-- panels and the controls must move clear of the notch and the rounded
-- corners (and turn with the phone), while the backdrop still runs under
-- them - the world is not moved, only the UI. On an Android phone or the
-- emulator with a simulated cut-out, the same. On a desktop nothing moves.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/106-safe-area.scene"

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
