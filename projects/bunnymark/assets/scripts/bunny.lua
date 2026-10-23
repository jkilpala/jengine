-- Bunny Movement Script
-- Applied to each bunny entity via the Bunny prefab.
-- Simulates gravity, bouncing off walls and floor.

local GRAVITY = 500        -- pixels/sec^2, pulling downward
local HALF_WIDTH = 640     -- screen half-width
local HALF_HEIGHT = 360    -- screen half-height
local BOUNCE_DAMPING = 0.9 -- energy loss on bounce

function init(entity)
    -- Nothing to do — velocity is set by the spawner
end

function update(entity, dt)
    -- Read velocity from custom component
    local vx = Entity.get_custom(entity, "BunnyData", "velocity_x") or 0
    local vy = Entity.get_custom(entity, "BunnyData", "velocity_y") or 0

    -- Apply gravity (downward in +Y up coordinate system)
    vy = vy - GRAVITY * dt

    -- Get current position
    local x, y = Transform.get_position(entity)

    -- Move
    x = x + vx * dt
    y = y + vy * dt

    -- Bounce off horizontal walls
    if x < -HALF_WIDTH then
        x = -HALF_WIDTH
        vx = math.abs(vx) * BOUNCE_DAMPING
    elseif x > HALF_WIDTH then
        x = HALF_WIDTH
        vx = -math.abs(vx) * BOUNCE_DAMPING
    end

    -- Bounce off floor
    if y < -HALF_HEIGHT then
        y = -HALF_HEIGHT
        vy = math.abs(vy) * BOUNCE_DAMPING
        -- Add random horizontal perturbation
        vx = vx + (math.random() - 0.5) * 60
    end

    -- Bounce off ceiling
    if y > HALF_HEIGHT then
        y = HALF_HEIGHT
        vy = -math.abs(vy) * BOUNCE_DAMPING
    end

    -- Write back
    Transform.set_position(entity, x, y)
    Entity.set_custom(entity, "BunnyData", "velocity_x", vx)
    Entity.set_custom(entity, "BunnyData", "velocity_y", vy)
end
