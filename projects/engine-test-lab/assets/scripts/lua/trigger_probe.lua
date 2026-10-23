-- Scene 27 / symmetric trigger callback demo.
-- Attached to the wandering Probe entity. Demonstrates the Phase B
-- "moving entity" handlers: `on_entered_trigger(self, data)` /
-- `on_exited_trigger(self, data)` fire on the entity that crosses
-- into / out of a `TriggerArea2D`, with `data.trigger = <zone_eid>`.
--
-- This is the counterpart to `trigger_zone.lua`, which uses the
-- zone-side `on_trigger_enter` / `on_trigger_exit` callbacks.

local function name_of(eid)
    local n = Entity.get_name(eid)
    if n and n ~= "" then return n end
    return tostring(eid)
end

function init(entity)
    log(string.format("[trigger_probe] ready on '%s'", name_of(entity)))
end

function on_entered_trigger(self, data)
    log(string.format("[trigger] (probe side) %s ENTERED zone %s",
        name_of(self), name_of(data.trigger)))
end

function on_exited_trigger(self, data)
    log(string.format("[trigger] (probe side) %s EXITED  zone %s",
        name_of(self), name_of(data.trigger)))
end
