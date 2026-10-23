-- Scene 29 / collision-callback demo.
-- Attached to every scripted RigidBody2D in scene 29. The engine delivers
-- `on_collision_enter(self, data)` and `on_collision_exit(self, data)` to
-- both sides of each contact pair. `data.other` is the other entity's id.
--
-- This script:
--   * Remembers its initial sprite color in `init`.
--   * Counts how many other bodies it is currently touching.
--   * Flashes pure white while at least one contact is active.
--   * Restores the original color when the touch-count returns to zero.
--   * Writes each event to an on-screen status line - the text entity named
--     "<ball name>Status" - as well as the log.

script = { tick_rate = 30 }

local original_color = {} -- [self_id] -> {r,g,b,a}
local touch_count = {}    -- [self_id] -> int
local enters = {}         -- [self_id] -> int
local exits = {}          -- [self_id] -> int
local status_of = {}      -- [self_id] -> status text entity, or nil

local function name_of(eid)
    local n = Entity.get_name(eid)
    if n and n ~= "" then return n end
    return tostring(eid)
end

local function show(self, last)
    local id = status_of[self]
    if not id or id == 0 then return end
    Text.set(id, string.format("%s: touching %d   enters %d  exits %d   last: %s",
        name_of(self), touch_count[self] or 0, enters[self] or 0, exits[self] or 0, last))
end

function init(entity)
    local r, g, b, a = Sprite.get_color(entity)
    original_color[entity] = { r or 1.0, g or 1.0, b or 1.0, a or 1.0 }
    touch_count[entity] = 0
    enters[entity] = 0
    exits[entity] = 0
    status_of[entity] = Entity.find_by_name(name_of(entity) .. "Status")
    log(string.format("[collision_ball] ready on '%s'", name_of(entity)))
end

function on_collision_enter(self, data)
    local other = data.other
    touch_count[self] = (touch_count[self] or 0) + 1
    log(string.format("[collision] %s ENTER %s (touching=%d)",
        name_of(self), name_of(other), touch_count[self]))
    enters[self] = (enters[self] or 0) + 1
    show(self, "enter " .. name_of(other))
    Sprite.set_color(self, 1.0, 1.0, 1.0, 1.0)
end

function on_collision_exit(self, data)
    local other = data.other
    local n = (touch_count[self] or 1) - 1
    if n < 0 then n = 0 end
    touch_count[self] = n
    log(string.format("[collision] %s EXIT  %s (touching=%d)",
        name_of(self), name_of(other), n))
    exits[self] = (exits[self] or 0) + 1
    show(self, "exit " .. name_of(other))
    if n == 0 then
        local c = original_color[self] or { 1.0, 1.0, 1.0, 1.0 }
        Sprite.set_color(self, c[1], c[2], c[3], c[4])
    end
end
