-- Scene 67 - UI scaling
--
-- Both UI systems lay out in pixels, and until Sep 2026 nothing scaled them:
-- a `.gui` authored against the project's 1280x720 simply ran off the edges of
-- a smaller window, and scene 33's third column was cut in half in the editor.
-- `settings.ui_scale_mode` (Project Settings -> Display) now scales the whole
-- UI by `min(w / 1280, h / 720)`, so a UI authored once fits any window.
--
-- What to check, in every runtime, while resizing the window:
--
--   * the green bar stays flush with the left and right edges,
--   * the four boxes stay inside their bordered row - they span 1240 of the
--     1280 the project declares, so nothing but scaling keeps them in,
--   * the four corner panels stay in their corners and fully on screen
--     (entity UI, anchored - BUGS #60 had these half off the edge),
--   * both buttons still respond where they are drawn. The hit test is
--     scaled by the same factor as the drawing; scale one and not the other
--     and a button answers clicks somewhere it is not.
--
-- `Camera.get_screen_size()` still reports **physical** pixels here, not virtual
-- ones: scripts use it to convert the cursor into world space (scene 34) and
-- that has nothing to do with how the UI is laid out.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/67-ui-scaling.scene"
local GUI_DOC = "ui/scene_67_scaling.gui"

local gui_clicks = 0
local entity_clicks = 0
local readout, entity_button

function init(entity)
    readout = Entity.find_by_name("Readout")
    entity_button = Entity.find_by_name("EntityButton")
    Event.on("ui_click", entity)
    -- The GUI button reports through a callback, the entity-UI one through
    -- the `ui_click` event. Two UI systems, one scale factor between them.
    -- `gui_on_click` takes the entity first: the callback is resolved on that
    -- entity's script at click time.
    GUI.on_click(entity, GUI_DOC, "click_me", "on_gui_click")
    GUI.set(GUI_DOC, "clicks", "text", "clicks: 0")
    log("[scene_67] resize the window - the UI must scale, not spill")
end

function on_gui_click(entity, node_id)
    gui_clicks = gui_clicks + 1
    GUI.set(GUI_DOC, "clicks", "text", "clicks: " .. gui_clicks)
end

-- An entity-UI click is the `ui_click` event, hit-tested in the same scaled
-- space as the `.gui` click above - the two counters should agree.
function on_ui_click(entity, data)
    if data ~= nil and data.button == entity_button then
        entity_clicks = entity_clicks + 1
    end
end

function update(entity, dt)
    if readout then
        local sw, sh = Camera.get_screen_size()
        Text.set(readout, string.format(
            "window %dx%d (physical)   gui clicks %d   entity clicks %d",
            math.floor(sw or 0), math.floor(sh or 0), gui_clicks, entity_clicks))
    end

    nav.tick(SCENE_PATH)
end
