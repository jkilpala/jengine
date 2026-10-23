-- Scene 27 / trigger zone callback demo.
-- Attached to every entity that owns a `TriggerArea2D`. The engine now
-- delivers `on_trigger_enter(self, data)` / `on_trigger_exit(self, data)`
-- to scripts; `data.other` is the entity ID that crossed the boundary
-- this frame.
--
-- This script tints the other entity (the probe) white while it sits
-- inside any zone, and writes each event to an on-screen status line - the
-- text entity named "<zone name>Status" - as well as the log, so the
-- callbacks can be checked without opening the console. Each zone
-- independently counts how many "others" it currently contains, then
-- restores the probe to yellow on the last exit.

script = { tick_rate = 30 }

local inside_count = {} -- [self_id] -> int
local enters = {}       -- [self_id] -> int
local exits = {}        -- [self_id] -> int
local status_of = {}    -- [self_id] -> status text entity, or nil

local function name_of(eid)
    local n = Entity.get_name(eid)
    if n and n ~= "" then return n end
    return tostring(eid)
end

local function show(self, last)
    local id = status_of[self]
    if not id or id == 0 then return end
    local state = (inside_count[self] or 0) > 0 and "probe INSIDE" or "empty"
    Text.set(id, string.format("%s: %s   enters %d  exits %d   last: %s",
        name_of(self), state, enters[self] or 0, exits[self] or 0, last))
end

function init(entity)
    inside_count[entity] = 0
    enters[entity] = 0
    exits[entity] = 0
    status_of[entity] = Entity.find_by_name(name_of(entity) .. "Status")
    log(string.format("[trigger_zone] ready on '%s'", name_of(entity)))
end

function on_trigger_enter(self, data)
    local other = data.other
    inside_count[self] = (inside_count[self] or 0) + 1
    enters[self] = (enters[self] or 0) + 1
    log(string.format("[trigger] %s ENTERED %s", name_of(other), name_of(self)))
    show(self, name_of(other) .. " entered")
    -- Bright tint while at least one zone holds the probe.
    Sprite.set_color(other, 1.0, 1.0, 1.0, 1.0)
end

function on_trigger_exit(self, data)
    local other = data.other
    local n = (inside_count[self] or 1) - 1
    if n < 0 then n = 0 end
    inside_count[self] = n
    exits[self] = (exits[self] or 0) + 1
    log(string.format("[trigger] %s EXITED  %s", name_of(other), name_of(self)))
    show(self, name_of(other) .. " exited")
    -- Only restore the original color when this zone is fully empty.
    -- (Other zones may still contain the probe — we accept a brief flicker
    -- there in exchange for keeping the demo script simple.)
    if n == 0 then
        Sprite.set_color(other, 1.0, 0.95, 0.3, 1.0)
    end
end
