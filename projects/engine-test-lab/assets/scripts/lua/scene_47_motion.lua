-- Scene 47 - Motion Components & Prefab Instances
--
-- Three engine-side pieces, none of which needs a script to work:
--
--   Acceleration2D      `update_acceleration` does vel += a * dt every frame.
--                       MaxSpeed clamps the magnitude afterwards, and
--                       WrapBounds teleports the ball back to the top so the
--                       fall is continuous.
--   InputActionImpulse  `update_input_actions` watches the named action and
--                       *replaces* the velocity component on its rising edge -
--                       it is a set, not an add, which is why a held key does
--                       not stack.
--   PrefabInstance      expanded from assets/prefabs at scene load.
--   Gravity2D           a *vector* pull, unlike Velocity2D.gravity's scalar
--                       downward one. The purple ball drifts up-left.
--   AudioSource         auto_play starts the clip when the entity appears,
--                       with no script involved. Both of these components
--                       used to deserialize and then be read by nothing.
--
-- Leaving the scene stops the ambience: nav.lua no longer does that for us,
-- so this scene owns its own teardown like scenes 16 and 55 do.
--
-- The prefab is the parity case: only the native runtime expands
-- PrefabInstance markers (wgpu_scene_runner.rs calls
-- expand_prefab_instances_in_json). The editor viewport and the web runtime
-- do not, so the coin is missing there. See BUGS.md #12.

script = { tick_rate = 30, events = { "on_despawn" } }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/47-motion-components.scene"

local readout

function init(entity)
    readout = Entity.find_by_name("Readout")
    log("[scene_47] ready - Space/Enter jumps the green box")
end

function update(entity, dt)
    if readout then
        -- The coin only exists where PrefabInstance was expanded, so its
        -- presence is the runtime check this scene is here to make.
        local coin = Entity.find_by_name("CoinFromPrefab")
        local prefab_state = coin ~= nil
            and "PrefabInstance expanded (native)"
            or "PrefabInstance NOT expanded in this runtime (expected in editor/web)"
        Text.set(readout, prefab_state)
    end
    nav.tick(SCENE_PATH)
end

function on_despawn(entity)
    -- The AudioSource above loops; without this it would follow you out of
    -- the scene, which is exactly the trap BUGS.md #19 describes.
    Audio.stop_music()
    Audio.stop_all()
end
