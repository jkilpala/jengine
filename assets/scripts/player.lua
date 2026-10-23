-- Player movement script
-- Rotates and oscillates the entity

local time = 0

function init(entity)
    log("Lua player script initialized for entity " .. entity)
end

function update(entity, dt)
    time = time + dt

    -- Get current position
    local x, y = get_position(entity)

    -- Oscillate horizontally
    local new_x = math.sin(time * 2.0) * 100.0
    set_position(entity, new_x, y)

    -- Rotate over time
    set_rotation(entity, time * 1.5)
end
