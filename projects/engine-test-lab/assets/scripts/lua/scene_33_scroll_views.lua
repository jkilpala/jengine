-- Scene 33 - Scroll Views
--
-- Exercises the GUI overflow feature added Sep 2026:
--   v_scroll : `overflow: "Scroll"` column - 20 rows in a 300px box. The
--              engine handles the wheel, the pointer drag, and draws the
--              scrollbar; no script involvement is needed for any of it.
--
-- Pointer scrolling (Sep 2026): press inside a scrolling box and the content
-- follows the pointer; flick and release and it coasts to a stop; pull past
-- either end and it resists, then springs back. Touch events are mapped onto
-- the mouse fields by the web runtime, so this is the touch path too.
--
-- Two things about it are worth checking here rather than taking on trust:
--
--   * Dragging a list must not click the row it started on. Buttons fire on
--     *release* and a committed drag cancels that release, so the click
--     handler below should stay quiet while you scroll and fire when you tap.
--   * The stretch past an end is a separate field from `scroll_y`
--     (`overscroll_y`, never serialized). The readout below reads `scroll_y`,
--     so it pins at 0 or the maximum while the content is visibly stretched -
--     that is correct, not a stuck readout.
--   b_clip   : `overflow: "Hidden"` - an oversized child is clipped but does
--              not scroll (wheel over it does nothing).
--   h_scroll : `overflow: "Scroll"` row - horizontal scrolling. A container
--              whose content only overflows sideways takes the wheel on X.
--
-- What the script adds on top:
--   * `GUI.set(gui, node, "scroll_y", n)` - the Top / +-100 / End buttons.
--     The engine clamps, so writing 99999 lands exactly at the bottom.
--   * `GUI.get(gui, node, "scroll_y")` - the live readout.
--   * A click handler on every row, to prove that rows scrolled out of the
--     box are NOT clickable: the hit-test respects the clip rect.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local GUI_DOC = "ui/scene_33.gui"
local SCENE_PATH = "scenes/33-scroll-views.scene"

local ROW_COUNT = 20
local STEP = 100

local function scroll_y()
    return tonumber(GUI.get(GUI_DOC, "v_scroll", "scroll_y")) or 0
end

local function set_scroll(v)
    -- No clamping here on purpose: the layout engine clamps to the
    -- scrollable range, which is what "End" below relies on.
    GUI.set(GUI_DOC, "v_scroll", "scroll_y", v)
end

function init(entity)
    for i = 1, ROW_COUNT do
        GUI.on_click(entity, GUI_DOC, string.format("row_%02d", i), "on_gui_click")
    end
    for _, id in ipairs({ "btn_top", "btn_up", "btn_down", "btn_end" }) do
        GUI.on_click(entity, GUI_DOC, id, "on_gui_click")
    end
    for i = 1, 12 do
        GUI.on_click(entity, GUI_DOC, "tile_" .. i, "on_gui_click")
    end
    log("[scene_33] ready - wheel over each box; hidden rows must not click")
end

function update(entity, dt)
    GUI.set(GUI_DOC, "readout", "text", string.format("scroll_y = %d", math.floor(scroll_y() + 0.5)))
    nav.tick(SCENE_PATH)
end

function on_gui_click(self_entity, node_id)
    if node_id == "btn_top" then
        set_scroll(0)
    elseif node_id == "btn_up" then
        set_scroll(scroll_y() - STEP)
    elseif node_id == "btn_down" then
        set_scroll(scroll_y() + STEP)
    elseif node_id == "btn_end" then
        set_scroll(99999)
    elseif node_id then
        GUI.set(GUI_DOC, "click_log", "text", "clicked: " .. tostring(node_id))
        log("[scene_33] clicked " .. tostring(node_id))
    end
end
