-- Scene 62 - Editor Stress (long entity list)
--
-- 384 ordinary authored entities. Not a rendering test - the engine handles
-- these trivially - but a test of the *editor*, which draws one row per entity
-- in the SCENE panel (left dock).
--
-- WHY IT EXISTS. The panel used to render every row as a `<div>` with six
-- event handlers and no virtualisation: at 16,000 entities that was ~68 ms a
-- frame of browser layout against 24 ms of engine work, and it is invisible to
-- the engine's profiler because it is the DOM, not the frame loop. The panel
-- now windows its rows - only the ones in view are rendered, with spacers
-- holding the scrollbar - and below 150 entities it renders everything exactly
-- as it always did.
--
-- Nothing else in the project could exercise that: scene 61's flock is
-- excluded from the panel by `BulkEntity`, and the next largest scene has 34
-- entities. So the feature had no way to be checked until this scene.
--
-- WHAT TO TRY (all in the SCENE panel, not the viewport):
--   * scroll the list fast - no blank gaps, scrollbar length looks right
--   * click a row, ctrl-click another - selection follows the right entities
--   * drag one row onto another - reparenting still works while windowed
--   * type in the panel's search - filtering still narrows the list
--
-- These entities are deliberately NOT marked `BulkEntity`: the point is a long
-- list the editor has to stay responsive with, not one it can hide.

script = { tick_rate = 10 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/62-editor-stress.scene"

local readout
local t = 0.0

function init(entity)
    readout = Entity.find_by_name("Readout")
    log("[scene_62] ready - open the SCENE panel (left dock) and scroll it")
end

function update(entity, dt)
    t = t + dt
    if readout ~= nil then
        local live = -1
        if Debug.get_entity_count ~= nil then
            live = Debug.get_entity_count()
        end
        local fps = 0.0
        if Debug.get_fps ~= nil then
            fps = Debug.get_fps() or 0.0
        end
        Text.set(readout, string.format(
            "%d entities in the scene panel   %.0f fps   (windowing kicks in above 150 rows)",
            live, fps))
    end
    nav.tick(SCENE_PATH)
end
