-- The starter player: moves with the move_* input actions (project.jengine).
--
-- Scripts call the engine through namespaces - Transform, Input, Entity,
-- Physics, Audio, ... - and every setter is deferred: it takes effect after
-- all scripts have run this frame. The Lua here must work on two dialects:
-- Luau natively, Lua 5.3 in the editor and on the web.

local SPEED = 260

function init(entity)
    log("player ready")
end

function update(entity, dt)
    local dx, dy = 0, 0
    if Input.is_action_pressed("move_left") then dx = dx - 1 end
    if Input.is_action_pressed("move_right") then dx = dx + 1 end
    if Input.is_action_pressed("move_up") then dy = dy + 1 end
    if Input.is_action_pressed("move_down") then dy = dy - 1 end

    if dx ~= 0 or dy ~= 0 then
        local len = math.sqrt(dx * dx + dy * dy)
        local x, y = Transform.get_position(entity)
        Transform.set_position(entity, x + dx / len * SPEED * dt, y + dy / len * SPEED * dt)
    end

    if Input.is_action_just_pressed("confirm") then
        Sprite.set_color(entity, math.random(), math.random(), math.random(), 1)
    end
end
