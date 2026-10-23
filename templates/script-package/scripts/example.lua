-- {{PACKAGE_NAME}} — example script
-- Runs on entities whose `script` component points at this file, installed as
-- packages/{{PACKAGE_NAME}}/scripts/example.lua.
--
-- Call the engine through its namespaces (Transform.get_position, Input.is_action_pressed,
-- Entity.spawn_prefab, ...): see the Scripting API reference. Keep to the Lua both
-- dialects share - Luau natively, Lua 5.3 in the editor and on the web.

-- tick_rate is updates per second (the base is 60). A script with no header runs
-- every frame and logs a reminder; 10 suits logic that need not run every frame.
-- Continuous motion belongs in native components (velocity, path_follow, ...).
script = { tick_rate = 10 }

function init(entity)
    log("{{PACKAGE_NAME}} script initialized for entity " .. entity)
end

function update(entity, dt)
    -- Your update logic here. dt is the time since this script last ran.
end
