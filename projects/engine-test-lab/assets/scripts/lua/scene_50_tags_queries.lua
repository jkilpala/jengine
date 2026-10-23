-- Scene 50 - Tags & Spatial Queries
--
-- Tags are free-form runtime labels. They can be authored in the scene under
-- "tags" or added at runtime, and they are the cheapest way to ask "which
-- entities are these?" without a component of your own.
--
--   Entity.add_tag(e, tag) / Entity.remove_tag(e, tag) / Entity.has_tag(e, tag)
--   Entity.find_by_tag(tag)              -> array of entity ids
--   Entity.find_by_tag(tag)      -> legacy alias for the same thing
--   Entity.find_in_radius(x, y, r[, tag]) -> array of entity ids inside the circle,
--                                     optionally filtered by tag
--
-- The optional tag filter on find_in_radius is the interesting part: it is one
-- call instead of a query plus a manual has_tag loop.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/50-tags-queries.scene"

local RADIUS = 150.0
local SPEED = 320.0

local IN_COLOR = { 1.0, 0.85, 0.3 }
local OUT_COLOR = { 0.35, 0.45, 0.6 }
local MARKED_COLOR = { 0.4, 1.0, 0.55 }

local probe

local function count(t)
    if type(t) ~= "table" then return 0 end
    return #t
end

function init(entity)
    probe = Entity.find_by_name("Probe")
    log("[scene_50] ready - WASD moves the probe, Space marks what is inside")
end

function update(entity, dt)
    if probe == nil then probe = Entity.find_by_name("Probe") end
    if probe == nil then return end

    local dx = (Input.is_action_pressed("move_right") and 1 or 0) - (Input.is_action_pressed("move_left") and 1 or 0)
    local dy = (Input.is_action_pressed("move_up") and 1 or 0) - (Input.is_action_pressed("move_down") and 1 or 0)
    local px, py = Transform.get_position(probe)
    if dx ~= 0 or dy ~= 0 then
        local len = math.sqrt(dx * dx + dy * dy)
        Transform.set_position(probe, px + dx / len * SPEED * dt, py + dy / len * SPEED * dt)
        px, py = px + dx / len * SPEED * dt, py + dy / len * SPEED * dt
    end

    -- Everything tagged "orb" within the circle. The tag filter keeps the
    -- probe itself out of the result even though it is inside its own radius.
    local inside = Entity.find_in_radius(px, py, RADIUS, "orb") or {}
    local inside_set = {}
    for _, e in ipairs(inside) do inside_set[e] = true end

    local all = Entity.find_by_tag("orb") or {}
    for _, e in ipairs(all) do
        local c
        if Entity.has_tag(e, "marked") then
            c = MARKED_COLOR
        elseif inside_set[e] then
            c = IN_COLOR
        else
            c = OUT_COLOR
        end
        Sprite.set_color(e, c[1], c[2], c[3], 1.0)
    end

    if Input.is_key_just_pressed("Space") then
        for _, e in ipairs(inside) do
            if Entity.has_tag(e, "marked") then
                Entity.remove_tag(e, "marked")
            else
                Entity.add_tag(e, "marked")
            end
        end
    end

    local cl = Entity.find_by_name("CountLabel")
    if cl then
        Text.set(cl, string.format("find_in_radius(probe, %d, \"orb\") = %d",
            math.floor(RADIUS), count(inside)))
    end
    local tl = Entity.find_by_name("TagLabel")
    if tl then
        Text.set(tl, string.format("find_by_tag: orb=%d  even=%d  odd=%d  marked=%d",
            count(all), count(Entity.find_by_tag("even")), count(Entity.find_by_tag("odd")),
            count(Entity.find_by_tag("marked"))))
    end
    local r = Entity.find_by_name("Readout")
    if r then Text.set(r, "Space toggles the 'marked' tag on everything inside the radius") end

    nav.tick(SCENE_PATH)
end
