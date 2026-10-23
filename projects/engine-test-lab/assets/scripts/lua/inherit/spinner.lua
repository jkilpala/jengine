-- Scene 103's base module. Everything a script inherits has to be on the
-- table this returns: a plain `function update` here would belong to this
-- module alone. `.` rather than `:` - the engine calls these with the entity.
local M = {}

function M.init(entity)
    Event.on("ping", entity)
end

function M.update(entity, dt)
    Transform.set_rotation(entity, Transform.get_rotation(entity) + 1.5 * dt)
    -- Ease back from the pulse `on_ping` gives.
    local s = Transform.get_scale(entity)
    local k = math.min(1, dt * 6)
    local next_s = s + (1 - s) * k
    Transform.set_scale(entity, next_s, next_s)
end

-- An inherited event handler: the scene's controller pings every second.
function M.on_ping(entity, data)
    Transform.set_scale(entity, 1.5, 1.5)
end

return M
